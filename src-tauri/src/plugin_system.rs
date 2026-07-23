use crate::kugou::{KugouProviderBridge, KugouSourceProvider, KUGOU_PLUGIN_ID, KUGOU_PROVIDER_ID};
use crate::netease::{
    NeteaseProviderBridge, NeteaseSourceProvider, NETEASE_PLUGIN_ID, NETEASE_PROVIDER_ID,
};
use crate::source_runtime::{
    self, DiagnosticLevel, SourceCapability, SourceInfo, SourceProvider, SourceRequest,
    SourceRequestOutcome, SourceRuntime, SourceRuntimeApiVersion, SourceRuntimeError,
};
#[cfg(test)]
use crate::source_runtime::{
    LyricResponse, SourceAction, SourceResponse, SourceRuntimeContext, SourceSearchResponse,
    SourceSearchResult,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PLUGIN_MANIFEST_FILE: &str = "plugin.json";
pub const PLUGIN_MANIFEST_VERSION: u32 = 1;
pub const PLUGIN_COMPATIBILITY_TARGET: &str = "fika-music";
pub const PLUGIN_RUNTIME_API_VERSION: SourceRuntimeApiVersion =
    source_runtime::SOURCE_RUNTIME_API_VERSION;
const IMPORTED_LX_ENTRYPOINT_PREFIX: &str = "builtin:lx-js:";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PluginManifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(
        alias = "providers",
        alias = "sourceProviders",
        alias = "sourceProviderEntrypoints"
    )]
    pub provider_entrypoints: Vec<PluginProviderEntrypoint>,
    #[serde(default)]
    pub capabilities: BTreeSet<SourceCapability>,
    pub compatibility_target: String,
    #[serde(alias = "sourceRuntimeApiVersion", alias = "supportedApiVersion")]
    pub supported_api_version: SourceRuntimeApiVersion,
    #[serde(default, alias = "hostBridges")]
    pub required_host_bridges: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PluginProviderEntrypoint {
    pub id: String,
    pub entrypoint: String,
    #[serde(default)]
    pub capabilities: BTreeSet<SourceCapability>,
    #[serde(default, alias = "sources")]
    pub source_catalog: BTreeMap<String, SourceInfo>,
}

impl PluginManifest {
    pub fn declared_capabilities(&self) -> BTreeSet<SourceCapability> {
        let mut capabilities = self.capabilities.clone();
        for provider in &self.provider_entrypoints {
            capabilities.extend(provider.capabilities.iter().copied());
        }
        capabilities
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.manifest_version != PLUGIN_MANIFEST_VERSION {
            errors.push(format!(
                "manifest version {} is not supported (expected {})",
                self.manifest_version, PLUGIN_MANIFEST_VERSION
            ));
        }
        if !valid_identifier(&self.id) {
            errors.push("id must contain only letters, numbers, '.', '_' or '-'".to_owned());
        }
        if self.name.trim().is_empty() {
            errors.push("name must not be empty".to_owned());
        }
        if !valid_semver(&self.version) {
            errors.push(format!("version is not valid semver: {}", self.version));
        }
        if self.compatibility_target.trim().is_empty() {
            errors.push("compatibilityTarget must not be empty".to_owned());
        }
        if self.provider_entrypoints.is_empty() {
            errors.push("providerEntrypoints must contain at least one entrypoint".to_owned());
        }

        let mut provider_ids = BTreeSet::new();
        let mut provider_routes = BTreeMap::new();
        for provider in &self.provider_entrypoints {
            if !valid_identifier(&provider.id) {
                errors.push(format!(
                    "provider entrypoint id is invalid: {}",
                    provider.id
                ));
            }
            if !provider_ids.insert(provider.id.clone()) {
                errors.push(format!(
                    "provider entrypoint ids must be unique: {}",
                    provider.id
                ));
            }
            if !valid_entrypoint(&provider.entrypoint) {
                errors.push(format!(
                    "provider entrypoint is invalid for {}: {}",
                    provider.id, provider.entrypoint
                ));
            }
            if provider
                .entrypoint
                .starts_with(IMPORTED_LX_ENTRYPOINT_PREFIX)
            {
                errors.push(
                    "LX JavaScript sources must be imported through Audio Sources, not Plugin System"
                        .to_owned(),
                );
            }
            if provider.entrypoint == "builtin:qishui" {
                errors.push(
                    "builtin:qishui is not available; import playback sources through Audio Sources"
                        .to_owned(),
                );
            }
            #[cfg(not(test))]
            if matches!(
                provider.entrypoint.as_str(),
                "builtin:runtime-demo" | "builtin:catalog" | "catalog"
            ) {
                errors.push(format!(
                    "test-only provider entrypoint is not available: {}",
                    provider.entrypoint
                ));
            }

            if matches!(provider.entrypoint.as_str(), "catalog" | "builtin:catalog")
                && provider.source_catalog.is_empty()
            {
                errors.push(format!(
                    "catalog provider {} must declare at least one source",
                    provider.id
                ));
            }

            for (source_id, source) in &provider.source_catalog {
                if source_id != &source.id {
                    errors.push(format!(
                        "provider {} catalog key {} does not match source id {}",
                        provider.id, source_id, source.id
                    ));
                }
                if source.name.trim().is_empty() {
                    errors.push(format!(
                        "provider {} source {} must have a name",
                        provider.id, source_id
                    ));
                }
                if source.actions.is_empty() {
                    errors.push(format!(
                        "provider {} source {} must declare an action",
                        provider.id, source_id
                    ));
                }
                for action in &source.actions {
                    let route = (source.id.clone(), *action);
                    if let Some(existing_provider) =
                        provider_routes.insert(route, provider.id.clone())
                    {
                        if existing_provider != provider.id {
                            errors.push(format!(
                                "source {} action {action:?} is exposed by both providers {} and {}",
                                source.id, existing_provider, provider.id
                            ));
                        }
                    }
                }
            }
        }

        for bridge in &self.required_host_bridges {
            if bridge.trim().is_empty() || bridge.chars().any(char::is_whitespace) {
                errors.push("requiredHostBridges must contain non-empty identifiers".to_owned());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn compatibility_diagnostics(
        &self,
        runtime_api_version: SourceRuntimeApiVersion,
        available_host_bridges: &BTreeSet<String>,
    ) -> Vec<PluginDiagnostic> {
        let mut diagnostics = Vec::new();
        if self.compatibility_target != PLUGIN_COMPATIBILITY_TARGET
            && self.compatibility_target != "*"
        {
            diagnostics.push(PluginDiagnostic::error(
                "compatibility",
                None,
                format!(
                    "plugin targets {}, but this app targets {}",
                    self.compatibility_target, PLUGIN_COMPATIBILITY_TARGET
                ),
            ));
        }
        if !self
            .supported_api_version
            .is_compatible_with(runtime_api_version)
        {
            diagnostics.push(PluginDiagnostic::error(
                "compatibility",
                None,
                format!(
                    "plugin supports Source Runtime API {}, but the runtime is {}",
                    self.supported_api_version, runtime_api_version
                ),
            ));
        }
        for bridge in &self.required_host_bridges {
            if !available_host_bridges.contains(bridge) {
                diagnostics.push(PluginDiagnostic::error(
                    "bridge-compatibility",
                    None,
                    format!("required host bridge is unavailable: {bridge}"),
                ));
            }
        }
        diagnostics
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export_to = "bindings.ts")]
pub enum PluginOrigin {
    Bundled,
    User,
}

impl PluginOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export_to = "bindings.ts")]
pub enum PluginState {
    Disabled,
    NeedsReview,
    Enabled,
    Incompatible,
    Error,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PluginDiagnostic {
    pub code: String,
    pub level: DiagnosticLevel,
    pub source_id: Option<String>,
    pub message: String,
    pub timestamp: i64,
}

impl PluginDiagnostic {
    pub fn info(
        code: impl Into<String>,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(code, DiagnosticLevel::Info, source_id, message)
    }

    pub fn warning(
        code: impl Into<String>,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(code, DiagnosticLevel::Warn, source_id, message)
    }

    pub fn error(
        code: impl Into<String>,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(code, DiagnosticLevel::Error, source_id, message)
    }

    pub fn security(source_id: Option<String>, message: impl Into<String>) -> Self {
        Self::new(
            "security-denial",
            DiagnosticLevel::Security,
            source_id,
            message,
        )
    }

    fn new(
        code: impl Into<String>,
        level: DiagnosticLevel,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            level,
            source_id,
            message: message.into(),
            timestamp: now_timestamp(),
        }
    }

    fn from_source(diagnostic: &source_runtime::SourceDiagnostic) -> Self {
        let code = match diagnostic.level {
            DiagnosticLevel::Security => "security-denial",
            DiagnosticLevel::Error => "runtime-error",
            DiagnosticLevel::Warn => "runtime-warning",
            DiagnosticLevel::Info => "runtime-log",
        };
        Self::new(
            code,
            diagnostic.level,
            Some(diagnostic.source_id.clone()),
            diagnostic.message.clone(),
        )
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PluginProviderState {
    pub id: String,
    pub entrypoint: String,
    pub initialized: bool,
    pub sources: Vec<SourceInfo>,
    pub runtime_report: Option<source_runtime::SourceRuntimeReport>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PluginRecord {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub path: String,
    pub origin: PluginOrigin,
    pub state: PluginState,
    pub enabled: bool,
    pub permissions_reviewed: bool,
    pub declared_capabilities: BTreeSet<SourceCapability>,
    pub granted_capabilities: BTreeSet<SourceCapability>,
    pub required_host_bridges: BTreeSet<String>,
    pub providers: Vec<PluginProviderState>,
    pub diagnostics: Vec<PluginDiagnostic>,
    pub can_remove: bool,
    pub can_enable: bool,
    pub manifest: Option<PluginManifest>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginSystemError {
    #[error("plugin filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("plugin manifest error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("plugin database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),
    #[error("plugin {0} was not found")]
    NotFound(String),
    #[error("plugin {0} cannot be removed")]
    NotRemovable(String),
    #[error("plugin {0} has an invalid lifecycle state: {1}")]
    InvalidState(String, String),
    #[error("plugin {plugin_id} does not support capability {capability}")]
    InvalidCapability {
        plugin_id: String,
        capability: String,
    },
    #[error("plugin {plugin_id} cannot load provider {entrypoint}: {message}")]
    ProviderLoad {
        plugin_id: String,
        entrypoint: String,
        message: String,
    },
    #[error("plugin {plugin_id} runtime error: {message}")]
    Runtime {
        plugin_id: String,
        message: String,
        diagnostics: Vec<PluginDiagnostic>,
    },
    #[error("plugin package is invalid: {0}")]
    Package(String),
}

impl PluginSystemError {
    pub fn diagnostics(&self) -> Vec<PluginDiagnostic> {
        match self {
            Self::Runtime { diagnostics, .. } => diagnostics.clone(),
            Self::InvalidManifest(message) | Self::Package(message) => {
                vec![PluginDiagnostic::error("manifest", None, message.clone())]
            }
            Self::ProviderLoad {
                plugin_id,
                entrypoint,
                message,
            } => vec![PluginDiagnostic::error(
                "load-error",
                Some(plugin_id.clone()),
                format!("{entrypoint}: {message}"),
            )],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct PersistedPluginState {
    manifest_fingerprint: String,
    enabled: bool,
    permissions_reviewed: bool,
    granted_capabilities: BTreeSet<SourceCapability>,
    diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Clone)]
struct PluginEntryRuntime {
    record: PluginRecord,
    providers: BTreeMap<String, Arc<dyn SourceProvider>>,
}

pub(crate) struct PreparedPluginRequest {
    plugin_id: String,
    provider_id: String,
    provider: Arc<dyn SourceProvider>,
    runtime: Arc<SourceRuntime>,
}

impl PreparedPluginRequest {
    pub(crate) fn execute(
        &self,
        request: SourceRequest,
        cancellation: source_runtime::SourceCancellationToken,
    ) -> Result<SourceRequestOutcome, PluginSystemError> {
        self.runtime
            .dispatch_request_with_cancellation(self.provider.as_ref(), request, cancellation)
            .map_err(|error| {
                let message = error.to_string();
                let diagnostics = error
                    .diagnostics()
                    .iter()
                    .map(PluginDiagnostic::from_source)
                    .collect();
                PluginSystemError::Runtime {
                    plugin_id: self.plugin_id.clone(),
                    message,
                    diagnostics,
                }
            })
    }
}

struct PackageReplacement {
    temporary: PathBuf,
    destination: PathBuf,
    backup: Option<PathBuf>,
    staging_root: PathBuf,
    restore_on_drop: bool,
}

impl PackageReplacement {
    fn apply(
        temporary: PathBuf,
        destination: PathBuf,
        backup: PathBuf,
        staging_root: PathBuf,
    ) -> Result<Self, PluginSystemError> {
        let backup = if destination.exists() {
            fs::rename(&destination, &backup)?;
            Some(backup)
        } else {
            None
        };

        if let Err(promote_error) = fs::rename(&temporary, &destination) {
            let restore_error = backup
                .as_ref()
                .map(|backup| fs::rename(backup, &destination))
                .transpose()
                .err();
            let _ = remove_path(&temporary);
            let _ = remove_dir_if_empty(&staging_root);
            return match restore_error {
                Some(restore_error) => Err(PluginSystemError::Package(format!(
                    "could not promote staged Plugin package: {promote_error}; \
                     restoring the previous package also failed: {restore_error}"
                ))),
                None => Err(PluginSystemError::Io(promote_error)),
            };
        }

        Ok(Self {
            temporary,
            destination,
            backup,
            staging_root,
            restore_on_drop: true,
        })
    }

    fn rollback(mut self) -> Result<(), PluginSystemError> {
        self.restore().map_err(Into::into)
    }

    fn keep(mut self) {
        self.restore_on_drop = false;
        if let Some(backup) = self.backup.take() {
            let _ = remove_path(&backup);
        }
        let _ = remove_path(&self.temporary);
        let _ = remove_dir_if_empty(&self.staging_root);
    }

    fn restore(&mut self) -> Result<(), std::io::Error> {
        if !self.restore_on_drop {
            return Ok(());
        }
        remove_path(&self.destination)?;
        if let Some(backup) = self.backup.as_ref() {
            fs::rename(backup, &self.destination)?;
        }
        let _ = remove_path(&self.temporary);
        self.restore_on_drop = false;
        let _ = remove_dir_if_empty(&self.staging_root);
        Ok(())
    }
}

impl Drop for PackageReplacement {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct PackageRemoval {
    original: PathBuf,
    staged: PathBuf,
    staging_root: PathBuf,
    restore_on_drop: bool,
}

impl PackageRemoval {
    fn apply(
        original: PathBuf,
        staged: PathBuf,
        staging_root: PathBuf,
    ) -> Result<Self, PluginSystemError> {
        fs::create_dir_all(&staging_root)?;
        fs::rename(&original, &staged)?;
        Ok(Self {
            original,
            staged,
            staging_root,
            restore_on_drop: true,
        })
    }

    fn rollback(mut self) -> Result<(), PluginSystemError> {
        self.restore().map_err(Into::into)
    }

    fn delete_with(
        mut self,
        delete: impl FnOnce(&Path) -> Result<(), std::io::Error>,
    ) -> Result<(), PluginSystemError> {
        if let Err(delete_error) = delete(&self.staged) {
            return match self.restore() {
                Ok(()) => Err(PluginSystemError::Io(delete_error)),
                Err(restore_error) => Err(PluginSystemError::Package(format!(
                    "deleting the quarantined Plugin package failed: {delete_error}; \
                     restoring the package also failed: {restore_error}"
                ))),
            };
        }

        self.restore_on_drop = false;
        let _ = remove_dir_if_empty(&self.staging_root);
        Ok(())
    }

    fn restore(&mut self) -> Result<(), std::io::Error> {
        if !self.restore_on_drop {
            return Ok(());
        }
        if self.original.exists() {
            remove_path(&self.original)?;
        }
        fs::rename(&self.staged, &self.original)?;
        self.restore_on_drop = false;
        let _ = remove_dir_if_empty(&self.staging_root);
        Ok(())
    }
}

impl Drop for PackageRemoval {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub struct PluginRegistry {
    user_plugins_dir: PathBuf,
    bundled_plugins_dir: PathBuf,
    runtime: Arc<SourceRuntime>,
    available_host_bridges: BTreeSet<String>,
    netease_bridge: Option<Arc<dyn NeteaseProviderBridge>>,
    kugou_bridge: Option<Arc<dyn KugouProviderBridge>>,
    plugins: BTreeMap<String, PluginEntryRuntime>,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginRegistry")
            .field("user_plugins_dir", &self.user_plugins_dir)
            .field("bundled_plugins_dir", &self.bundled_plugins_dir)
            .field("available_host_bridges", &self.available_host_bridges)
            .field("has_netease_bridge", &self.netease_bridge.is_some())
            .field("has_kugou_bridge", &self.kugou_bridge.is_some())
            .field("plugin_count", &self.plugins.len())
            .finish_non_exhaustive()
    }
}

impl PluginRegistry {
    pub fn new(
        user_plugins_dir: impl Into<PathBuf>,
        bundled_plugins_dir: impl Into<PathBuf>,
        runtime: Arc<SourceRuntime>,
    ) -> Self {
        Self {
            user_plugins_dir: user_plugins_dir.into(),
            bundled_plugins_dir: bundled_plugins_dir.into(),
            runtime,
            available_host_bridges: BTreeSet::new(),
            netease_bridge: None,
            kugou_bridge: None,
            plugins: BTreeMap::new(),
        }
    }

    pub fn with_available_host_bridges(
        mut self,
        bridges: impl IntoIterator<Item = String>,
    ) -> Self {
        self.available_host_bridges = bridges.into_iter().collect();
        self
    }

    pub fn with_netease_bridge(mut self, bridge: Arc<dyn NeteaseProviderBridge>) -> Self {
        self.netease_bridge = Some(bridge);
        self
    }

    pub fn with_kugou_bridge(mut self, bridge: Arc<dyn KugouProviderBridge>) -> Self {
        self.kugou_bridge = Some(bridge);
        self
    }

    pub fn user_plugins_dir(&self) -> &Path {
        &self.user_plugins_dir
    }

    pub fn records(&self) -> Vec<PluginRecord> {
        self.plugins
            .values()
            .map(|plugin| plugin.record.clone())
            .collect()
    }

    pub fn record(&self, plugin_id: &str) -> Option<PluginRecord> {
        self.plugins
            .get(plugin_id)
            .map(|plugin| plugin.record.clone())
    }

    pub fn refresh(
        &mut self,
        connection: &Connection,
    ) -> Result<Vec<PluginRecord>, PluginSystemError> {
        const SAVEPOINT: &str = "fika_plugin_refresh";

        connection.execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))?;
        let previous = std::mem::take(&mut self.plugins);
        if let Err(error) = clear_runtime_entries(&self.runtime, &previous) {
            return Err(self.restore_failed_refresh(connection, SAVEPOINT, previous, error));
        }

        let result = self.refresh_in_place(connection);
        match result {
            Ok(records) => {
                if let Err(error) =
                    connection.execute_batch(&format!("RELEASE SAVEPOINT {SAVEPOINT}"))
                {
                    return Err(self.restore_failed_refresh(
                        connection,
                        SAVEPOINT,
                        previous,
                        PluginSystemError::Database(error),
                    ));
                }
                Ok(records)
            }
            Err(error) => Err(self.restore_failed_refresh(connection, SAVEPOINT, previous, error)),
        }
    }

    fn refresh_in_place(
        &mut self,
        connection: &Connection,
    ) -> Result<Vec<PluginRecord>, PluginSystemError> {
        let persisted = load_persisted_states(connection)?;
        let mut candidates = Vec::new();
        candidates.extend(
            discover_package_paths(&self.bundled_plugins_dir)?
                .into_iter()
                .map(|path| (PluginOrigin::Bundled, path)),
        );
        candidates.extend(
            discover_package_paths(&self.user_plugins_dir)?
                .into_iter()
                .map(|path| (PluginOrigin::User, path)),
        );

        let mut seen_ids = BTreeSet::new();
        let mut seen_provider_ids = BTreeSet::new();
        for (origin, path) in candidates {
            let manifest = match read_manifest(&path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    self.insert_invalid_record(origin, path, error.to_string());
                    continue;
                }
            };

            if is_legacy_lx_audio_source(&manifest) {
                continue;
            }

            if let Err(errors) = manifest.validate() {
                self.insert_invalid_record(origin, path, errors.join("; "));
                continue;
            }
            if !seen_ids.insert(manifest.id.clone()) {
                self.insert_invalid_record(
                    origin,
                    path,
                    format!("duplicate plugin id discovered: {}", manifest.id),
                );
                continue;
            }
            if let Some(provider_id) = manifest
                .provider_entrypoints
                .iter()
                .map(|provider| provider.id.as_str())
                .find(|provider_id| seen_provider_ids.contains(*provider_id))
            {
                self.insert_invalid_record(
                    origin,
                    path,
                    format!("duplicate Source Provider id discovered: {provider_id}"),
                );
                continue;
            }
            seen_provider_ids.extend(
                manifest
                    .provider_entrypoints
                    .iter()
                    .map(|provider| provider.id.clone()),
            );

            let saved = persisted.get(&manifest.id);
            let compatibility = manifest.compatibility_diagnostics(
                self.runtime.api_version(),
                &self.available_host_bridges,
            );
            let mut record = record_for_manifest(&manifest, &path, origin, saved, compatibility)?;
            if record.state == PluginState::Incompatible {
                record.enabled = false;
            }
            self.plugins.insert(
                manifest.id.clone(),
                PluginEntryRuntime {
                    record,
                    providers: BTreeMap::new(),
                },
            );
        }

        let requested_enabled = self
            .plugins
            .iter()
            .filter_map(|(id, plugin)| plugin.record.enabled.then_some(id.clone()))
            .collect::<Vec<_>>();
        for plugin_id in requested_enabled {
            if let Err(error) = self.activate_plugin(connection, &plugin_id) {
                if matches!(
                    error,
                    PluginSystemError::Database(_)
                        | PluginSystemError::Io(_)
                        | PluginSystemError::Json(_)
                ) {
                    return Err(error);
                }
            }
        }

        for plugin in self.plugins.values() {
            if plugin.record.manifest.is_some() {
                persist_plugin(connection, &plugin.record)?;
            }
        }

        Ok(self.records())
    }

    fn restore_failed_refresh(
        &mut self,
        connection: &Connection,
        savepoint: &str,
        previous: BTreeMap<String, PluginEntryRuntime>,
        refresh_error: PluginSystemError,
    ) -> PluginSystemError {
        let cleanup_error = clear_runtime_entries(&self.runtime, &self.plugins).err();
        let database_error = connection
            .execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {savepoint}; RELEASE SAVEPOINT {savepoint}"
            ))
            .err();
        let runtime_error = restore_runtime_entries(&self.runtime, &previous).err();
        self.plugins = previous;

        if cleanup_error.is_none() && database_error.is_none() && runtime_error.is_none() {
            return refresh_error;
        }

        let mut failures = Vec::new();
        if let Some(error) = cleanup_error {
            failures.push(format!("candidate runtime cleanup failed: {error}"));
        }
        if let Some(error) = database_error {
            failures.push(format!("database rollback failed: {error}"));
        }
        if let Some(error) = runtime_error {
            failures.push(format!("runtime restore failed: {error}"));
        }
        PluginSystemError::Package(format!(
            "refresh failed: {refresh_error}; {}",
            failures.join("; ")
        ))
    }

    pub fn set_capabilities(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        capabilities: impl IntoIterator<Item = SourceCapability>,
        reviewed: bool,
    ) -> Result<PluginRecord, PluginSystemError> {
        let requested = capabilities.into_iter().collect::<BTreeSet<_>>();
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        let declared = plugin.record.declared_capabilities.clone();
        if let Some(capability) = requested.difference(&declared).next() {
            return Err(PluginSystemError::InvalidCapability {
                plugin_id: plugin_id.to_owned(),
                capability: capability.as_str().to_owned(),
            });
        }
        if plugin.record.enabled && !reviewed {
            return Err(PluginSystemError::InvalidState(
                plugin_id.to_owned(),
                "an enabled plugin must remain permission-reviewed".to_owned(),
            ));
        }

        let previous = plugin.record.clone();
        let manifest = previous.manifest.clone();
        let active_provider_ids = plugin.providers.keys().cloned().collect::<BTreeSet<_>>();
        let mut candidate = previous.clone();
        candidate.granted_capabilities = requested.clone();
        candidate.permissions_reviewed = reviewed;
        if candidate.enabled {
            for capability in previous
                .granted_capabilities
                .difference(&requested)
                .copied()
            {
                append_diagnostic(
                    &mut candidate.diagnostics,
                    PluginDiagnostic::security(
                        Some(plugin_id.to_owned()),
                        format!("capability revoked: {}", capability.as_str()),
                    ),
                );
            }
        } else if candidate.manifest.is_some() && candidate.state != PluginState::Incompatible {
            candidate.state = if !candidate.declared_capabilities.is_empty() && !reviewed {
                PluginState::NeedsReview
            } else {
                PluginState::Disabled
            };
        }
        update_action_flags(&mut candidate);

        if let Some(manifest) = manifest.as_ref() {
            if let Err(error) = replace_runtime_provider_grants(
                &self.runtime,
                manifest,
                &active_provider_ids,
                &requested,
            ) {
                if let Err(rollback_error) = replace_runtime_provider_grants(
                    &self.runtime,
                    manifest,
                    &active_provider_ids,
                    &previous.granted_capabilities,
                ) {
                    return Err(PluginSystemError::Package(format!(
                        "updating runtime grants failed: {error}; restoring runtime grants also failed: {rollback_error}"
                    )));
                }
                return Err(runtime_error(plugin_id, error));
            }
        }

        if let Err(error) = persist_plugin(connection, &candidate) {
            if let Some(manifest) = manifest.as_ref() {
                if let Err(rollback_error) = replace_runtime_provider_grants(
                    &self.runtime,
                    manifest,
                    &active_provider_ids,
                    &previous.granted_capabilities,
                ) {
                    return Err(PluginSystemError::Package(format!(
                        "capability update failed: {error}; restoring runtime grants also failed: {rollback_error}"
                    )));
                }
            }
            return Err(error);
        }

        self.plugins
            .get_mut(plugin_id)
            .expect("plugin checked above")
            .record = candidate.clone();
        Ok(candidate)
    }

    pub fn set_enabled(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<PluginRecord, PluginSystemError> {
        if enabled {
            self.activate_plugin(connection, plugin_id)
        } else {
            self.deactivate_plugin(connection, plugin_id)
        }
    }

    pub fn remove(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
    ) -> Result<Vec<PluginRecord>, PluginSystemError> {
        self.remove_with_package_cleanup(connection, plugin_id, remove_path)
    }

    fn remove_with_package_cleanup(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        cleanup: impl FnOnce(&Path) -> Result<(), std::io::Error>,
    ) -> Result<Vec<PluginRecord>, PluginSystemError> {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        if !plugin.record.can_remove || plugin.record.origin != PluginOrigin::User {
            return Err(PluginSystemError::NotRemovable(plugin_id.to_owned()));
        }
        let previous = plugin.clone();
        let path = PathBuf::from(&plugin.record.path);
        fs::create_dir_all(&self.user_plugins_dir)?;
        let user_plugins_dir = fs::canonicalize(&self.user_plugins_dir)?;
        let path = if path.exists() {
            let path = fs::canonicalize(path)?;
            if path == user_plugins_dir || !is_within(&path, &user_plugins_dir) || !path.is_dir() {
                return Err(PluginSystemError::NotRemovable(plugin_id.to_owned()));
            }
            Some(path)
        } else {
            None
        };

        let transaction = connection.unchecked_transaction()?;
        if let Err(error) = self.deactivate_plugin(&transaction, plugin_id) {
            drop(transaction);
            return Err(self.restore_failed_removal(None, previous, error));
        }
        let staged_removal = if let Some(path) = path {
            let staging_root = user_plugins_dir.join(".remove-staging");
            let staged = removal_staging_path(&staging_root, plugin_id);
            match PackageRemoval::apply(path, staged, staging_root) {
                Ok(staged_removal) => Some(staged_removal),
                Err(error) => {
                    drop(transaction);
                    return Err(self.restore_failed_removal(None, previous, error));
                }
            }
        } else {
            None
        };
        if let Err(error) = delete_persisted_plugin(&transaction, plugin_id) {
            drop(transaction);
            return Err(self.restore_failed_removal(staged_removal, previous, error));
        }

        if let Err(error) = transaction.commit() {
            return Err(self.restore_failed_removal(
                staged_removal,
                previous,
                PluginSystemError::Database(error),
            ));
        }
        if let Some(staged_removal) = staged_removal {
            if let Err(error) = staged_removal.delete_with(cleanup) {
                return Err(self.restore_committed_removal(connection, previous, error));
            }
        }
        self.plugins.remove(plugin_id);
        Ok(self.records())
    }

    pub fn clear_diagnostics(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
    ) -> Result<PluginRecord, PluginSystemError> {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        let mut candidate = plugin.record.clone();
        candidate.diagnostics.clear();
        for provider in &mut candidate.providers {
            provider.diagnostics.clear();
        }
        persist_plugin(connection, &candidate)?;
        self.plugins
            .get_mut(plugin_id)
            .expect("plugin checked above")
            .record = candidate.clone();
        Ok(candidate)
    }

    pub fn install(
        &mut self,
        connection: &Connection,
        package_path: &Path,
    ) -> Result<PluginRecord, PluginSystemError> {
        let package_path = normalize_package_path(package_path)?;
        let manifest = read_manifest(&package_path)?;
        if let Err(errors) = manifest.validate() {
            return Err(PluginSystemError::InvalidManifest(errors.join("; ")));
        }
        if self
            .plugins
            .get(&manifest.id)
            .is_some_and(|plugin| plugin.record.origin == PluginOrigin::Bundled)
        {
            return Err(PluginSystemError::Package(format!(
                "cannot replace bundled plugin {}",
                manifest.id
            )));
        }
        self.validate_install_conflicts(&manifest)?;

        fs::create_dir_all(&self.user_plugins_dir)?;
        let user_plugins_dir = fs::canonicalize(&self.user_plugins_dir)?;
        let destination = user_plugins_dir.join(&manifest.id);
        let staging_root = user_plugins_dir.join(".install-staging");
        if paths_overlap(&package_path, &staging_root) {
            return Err(PluginSystemError::Package(
                "installation workspace cannot be inside the source package".to_owned(),
            ));
        }
        if paths_overlap(&package_path, &destination) {
            return Err(PluginSystemError::Package(
                "source package and installation directory must not overlap".to_owned(),
            ));
        }
        if is_within(&package_path, &user_plugins_dir) {
            return Err(PluginSystemError::Package(
                "source package cannot be inside the managed Plugin directory".to_owned(),
            ));
        }

        fs::create_dir_all(&staging_root)?;
        let (temporary, backup) = install_staging_paths(&staging_root, &manifest.id);
        if let Err(error) = copy_package_tree(&package_path, &temporary) {
            let _ = fs::remove_dir_all(&temporary);
            let _ = remove_dir_if_empty(&staging_root);
            return Err(error);
        }
        let copied_manifest = match read_manifest(&temporary) {
            Ok(copied_manifest) => copied_manifest,
            Err(error) => {
                let _ = fs::remove_dir_all(&temporary);
                let _ = remove_dir_if_empty(&staging_root);
                return Err(error);
            }
        };
        if copied_manifest != manifest {
            let _ = fs::remove_dir_all(&temporary);
            let _ = remove_dir_if_empty(&staging_root);
            return Err(PluginSystemError::Package(
                "plugin manifest changed while the package was being copied".to_owned(),
            ));
        }

        let transaction = connection.unchecked_transaction()?;
        let replacement = PackageReplacement::apply(temporary, destination, backup, staging_root)?;
        let installed = match self.refresh(&transaction).and_then(|_| {
            self.record(&manifest.id)
                .ok_or_else(|| PluginSystemError::NotFound(manifest.id.clone()))
        }) {
            Ok(installed) => installed,
            Err(error) => {
                drop(transaction);
                return Err(self.restore_failed_install(connection, replacement, error));
            }
        };

        if let Err(error) = transaction.commit() {
            return Err(self.restore_failed_install(
                connection,
                replacement,
                PluginSystemError::Database(error),
            ));
        }
        replacement.keep();
        Ok(installed)
    }

    pub fn dispatch_request(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        request: SourceRequest,
        cancellation: source_runtime::SourceCancellationToken,
    ) -> Result<SourceRequestOutcome, PluginSystemError> {
        let dispatch = self.prepare_dispatch(plugin_id, &request)?;
        let result = dispatch.execute(request, cancellation);
        self.complete_dispatch_best_effort(connection, &dispatch, &result);
        result
    }

    pub(crate) fn prepare_dispatch(
        &self,
        plugin_id: &str,
        request: &SourceRequest,
    ) -> Result<PreparedPluginRequest, PluginSystemError> {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        if !plugin.record.enabled {
            return Err(PluginSystemError::InvalidState(
                plugin_id.to_owned(),
                "plugin must be enabled before dispatch".to_owned(),
            ));
        }
        let action = request.action();
        let mut matching_providers = plugin.record.providers.iter().filter(|provider| {
            provider
                .sources
                .iter()
                .any(|source| source.id == request.source() && source.actions.contains(&action))
        });
        let provider_id = match (matching_providers.next(), matching_providers.next()) {
            (Some(provider), None) => provider.id.clone(),
            (Some(first), Some(second)) => {
                return Err(PluginSystemError::Package(format!(
                    "plugin {plugin_id} has an ambiguous route for source {} action {action:?}: providers {} and {}",
                    request.source(),
                    first.id,
                    second.id
                )));
            }
            (None, _) if plugin.providers.len() == 1 => plugin
                .providers
                .keys()
                .next()
                .expect("single provider checked above")
                .clone(),
            (None, _) => {
                return Err(PluginSystemError::Package(format!(
                    "no provider in plugin {plugin_id} exposes source {} action {action:?}",
                    request.source()
                )));
            }
        };
        let provider = plugin.providers.get(&provider_id).cloned().ok_or_else(|| {
            PluginSystemError::InvalidState(
                plugin_id.to_owned(),
                "provider handle is not initialized".to_owned(),
            )
        })?;

        Ok(PreparedPluginRequest {
            plugin_id: plugin_id.to_owned(),
            provider_id,
            provider,
            runtime: Arc::clone(&self.runtime),
        })
    }

    pub(crate) fn complete_dispatch(
        &mut self,
        connection: &Connection,
        dispatch: &PreparedPluginRequest,
        result: &Result<SourceRequestOutcome, PluginSystemError>,
    ) -> Result<(), PluginSystemError> {
        let Some(plugin) = self.plugins.get(&dispatch.plugin_id) else {
            return Ok(());
        };
        let is_current_provider = plugin
            .providers
            .get(&dispatch.provider_id)
            .is_some_and(|provider| Arc::ptr_eq(provider, &dispatch.provider));
        if !is_current_provider {
            return Ok(());
        }

        let diagnostics = match result {
            Ok(outcome) => outcome
                .diagnostics
                .iter()
                .map(PluginDiagnostic::from_source)
                .collect(),
            Err(error) => error.diagnostics(),
        };
        self.append_runtime_diagnostics(
            connection,
            &dispatch.plugin_id,
            &dispatch.provider_id,
            diagnostics,
        )
    }

    pub(crate) fn complete_dispatch_best_effort(
        &mut self,
        connection: &Connection,
        dispatch: &PreparedPluginRequest,
        result: &Result<SourceRequestOutcome, PluginSystemError>,
    ) {
        if let Err(error) = self.complete_dispatch(connection, dispatch, result) {
            self.append_unpersisted_completion_warning(dispatch, &error);
        }
    }

    fn append_unpersisted_completion_warning(
        &mut self,
        dispatch: &PreparedPluginRequest,
        error: &PluginSystemError,
    ) {
        let Some(plugin) = self.plugins.get_mut(&dispatch.plugin_id) else {
            return;
        };
        let is_current_provider = plugin
            .providers
            .get(&dispatch.provider_id)
            .is_some_and(|provider| Arc::ptr_eq(provider, &dispatch.provider));
        if !is_current_provider {
            return;
        }

        let diagnostic = PluginDiagnostic::warning(
            "diagnostic-persistence",
            Some(dispatch.provider_id.clone()),
            format!("request completed, but diagnostics could not be persisted: {error}"),
        );
        if let Some(provider) = plugin
            .record
            .providers
            .iter_mut()
            .find(|provider| provider.id == dispatch.provider_id)
        {
            append_diagnostic(&mut provider.diagnostics, diagnostic.clone());
        }
        append_diagnostic(&mut plugin.record.diagnostics, diagnostic);
    }

    fn activate_plugin(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
    ) -> Result<PluginRecord, PluginSystemError> {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        let provider_handles_are_active = plugin.record.enabled
            && plugin.providers.len() == plugin.record.providers.len()
            && plugin.record.providers.iter().all(|provider| {
                provider.initialized && plugin.providers.contains_key(&provider.id)
            });
        if provider_handles_are_active {
            return Ok(plugin.record.clone());
        }
        let Some(manifest) = plugin.record.manifest.clone() else {
            return Err(PluginSystemError::InvalidState(
                plugin_id.to_owned(),
                "manifest is invalid".to_owned(),
            ));
        };
        if plugin.record.state == PluginState::Incompatible {
            return Err(PluginSystemError::InvalidState(
                plugin_id.to_owned(),
                "plugin is incompatible with the current Source Runtime".to_owned(),
            ));
        }
        if !plugin.record.declared_capabilities.is_empty() && !plugin.record.permissions_reviewed {
            let plugin = self
                .plugins
                .get_mut(plugin_id)
                .expect("plugin checked above");
            plugin.record.state = PluginState::NeedsReview;
            plugin.record.enabled = false;
            update_action_flags(&mut plugin.record);
            persist_plugin(connection, &plugin.record)?;
            return Err(PluginSystemError::InvalidState(
                plugin_id.to_owned(),
                "capabilities must be reviewed before enabling the plugin".to_owned(),
            ));
        }

        let granted = plugin.record.granted_capabilities.clone();
        let entries = manifest.provider_entrypoints.clone();
        let package_path = PathBuf::from(&plugin.record.path);
        let runtime = Arc::clone(&self.runtime);
        let mut providers = BTreeMap::new();
        let mut provider_states = Vec::new();
        let mut configured_provider_ids: Vec<String> = Vec::new();

        for entry in entries {
            let provider = match build_provider(
                &manifest,
                &entry,
                &package_path,
                self.netease_bridge.clone(),
                self.kugou_bridge.clone(),
            ) {
                Ok(provider) => provider,
                Err(error) => {
                    clear_runtime_provider_state(&runtime, configured_provider_ids);
                    return self.activation_failed(connection, plugin_id, provider_states, error);
                }
            };
            let provider_id = provider.id().to_owned();
            let provider_grants = granted
                .intersection(&provider_declared_capabilities(&manifest, &entry))
                .copied()
                .collect::<BTreeSet<_>>();
            if let Err(error) =
                runtime.replace_provider_granted_capabilities(provider_id.clone(), provider_grants)
            {
                clear_runtime_provider_state(&runtime, configured_provider_ids);
                return self.activation_failed(
                    connection,
                    plugin_id,
                    provider_states,
                    runtime_error(plugin_id, error),
                );
            }
            configured_provider_ids.push(provider_id);

            match runtime.initialize_provider(provider.as_ref()) {
                Ok(report) => {
                    let diagnostics = report
                        .diagnostics
                        .iter()
                        .map(PluginDiagnostic::from_source)
                        .collect::<Vec<_>>();
                    provider_states.push(PluginProviderState {
                        id: entry.id.clone(),
                        entrypoint: entry.entrypoint.clone(),
                        initialized: true,
                        sources: report.sources.values().cloned().collect(),
                        runtime_report: Some(report),
                        diagnostics,
                    });
                    providers.insert(entry.id, provider);
                }
                Err(error) => {
                    clear_runtime_provider_state(&runtime, configured_provider_ids);
                    let error = runtime_error(plugin_id, error);
                    return self.activation_failed(connection, plugin_id, provider_states, error);
                }
            }
        }

        if let Err(error) = validate_provider_routes(plugin_id, &provider_states) {
            clear_runtime_provider_state(&runtime, configured_provider_ids);
            return self.activation_failed(connection, plugin_id, provider_states, error);
        }

        let mut candidate = self
            .plugins
            .get(plugin_id)
            .expect("plugin checked above")
            .record
            .clone();
        candidate.providers = provider_states;
        candidate.enabled = true;
        candidate.state = PluginState::Enabled;
        let runtime_diagnostics = candidate
            .providers
            .iter()
            .flat_map(|provider| provider.diagnostics.iter().cloned())
            .collect::<Vec<_>>();
        for diagnostic in runtime_diagnostics {
            append_diagnostic(&mut candidate.diagnostics, diagnostic);
        }
        append_diagnostic(
            &mut candidate.diagnostics,
            PluginDiagnostic::info(
                "lifecycle",
                Some(plugin_id.to_owned()),
                "plugin providers initialized",
            ),
        );
        update_action_flags(&mut candidate);
        if let Err(error) = persist_plugin(connection, &candidate) {
            clear_runtime_provider_state(&runtime, configured_provider_ids);
            return Err(error);
        }

        let plugin = self
            .plugins
            .get_mut(plugin_id)
            .expect("plugin checked above");
        plugin.providers = providers;
        plugin.record = candidate.clone();
        Ok(candidate)
    }

    fn activation_failed(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        mut provider_states: Vec<PluginProviderState>,
        error: PluginSystemError,
    ) -> Result<PluginRecord, PluginSystemError> {
        let diagnostics = error.diagnostics();
        for provider in &mut provider_states {
            provider.initialized = false;
            provider.runtime_report = None;
        }
        if let Some(plugin) = self.plugins.get(plugin_id) {
            let mut candidate = plugin.record.clone();
            let runtime_diagnostics = provider_states
                .iter()
                .flat_map(|provider| provider.diagnostics.iter().cloned())
                .collect::<Vec<_>>();
            if provider_states.is_empty() {
                for provider in &mut candidate.providers {
                    provider.initialized = false;
                    provider.runtime_report = None;
                }
            } else {
                for provider_state in provider_states {
                    if let Some(existing) = candidate
                        .providers
                        .iter_mut()
                        .find(|provider| provider.id == provider_state.id)
                    {
                        *existing = provider_state;
                    } else {
                        candidate.providers.push(provider_state);
                    }
                }
            }
            candidate.enabled = false;
            candidate.state = PluginState::Error;
            for diagnostic in runtime_diagnostics {
                append_diagnostic(&mut candidate.diagnostics, diagnostic);
            }
            for diagnostic in diagnostics {
                append_diagnostic(&mut candidate.diagnostics, diagnostic);
            }
            update_action_flags(&mut candidate);
            persist_plugin(connection, &candidate)?;
            let plugin = self
                .plugins
                .get_mut(plugin_id)
                .expect("plugin checked above");
            plugin.providers.clear();
            plugin.record = candidate;
        }
        Err(error)
    }

    fn deactivate_plugin(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
    ) -> Result<PluginRecord, PluginSystemError> {
        self.deactivate_plugin_with(connection, plugin_id, clear_runtime_entry)
    }

    fn deactivate_plugin_with(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        cleanup_runtime: impl FnOnce(
            &SourceRuntime,
            &PluginEntryRuntime,
        ) -> Result<(), PluginSystemError>,
    ) -> Result<PluginRecord, PluginSystemError> {
        let Some(previous) = self.plugins.get(plugin_id).cloned() else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        if !previous.record.enabled
            && matches!(
                previous.record.state,
                PluginState::Invalid | PluginState::Incompatible
            )
        {
            return Ok(previous.record);
        }
        let mut candidate = previous.record.clone();
        for provider in &mut candidate.providers {
            provider.initialized = false;
            provider.runtime_report = None;
        }
        candidate.enabled = false;
        candidate.state =
            if !candidate.declared_capabilities.is_empty() && !candidate.permissions_reviewed {
                PluginState::NeedsReview
            } else {
                PluginState::Disabled
            };
        append_diagnostic(
            &mut candidate.diagnostics,
            PluginDiagnostic::info(
                "lifecycle",
                Some(plugin_id.to_owned()),
                "plugin providers disabled",
            ),
        );
        update_action_flags(&mut candidate);

        if let Err(error) = cleanup_runtime(&self.runtime, &previous) {
            return Err(restore_after_failed_deactivation(
                &self.runtime,
                &previous,
                error,
            ));
        }
        if let Err(error) = persist_plugin(connection, &candidate) {
            return Err(restore_after_failed_deactivation(
                &self.runtime,
                &previous,
                error,
            ));
        }
        let plugin = self
            .plugins
            .get_mut(plugin_id)
            .expect("plugin checked above");
        plugin.providers.clear();
        plugin.record = candidate.clone();
        Ok(candidate)
    }

    fn append_runtime_diagnostics(
        &mut self,
        connection: &Connection,
        plugin_id: &str,
        provider_id: &str,
        diagnostics: Vec<PluginDiagnostic>,
    ) -> Result<(), PluginSystemError> {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return Err(PluginSystemError::NotFound(plugin_id.to_owned()));
        };
        let mut candidate = plugin.record.clone();
        if let Some(provider) = candidate
            .providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
        {
            for diagnostic in &diagnostics {
                append_diagnostic(&mut provider.diagnostics, diagnostic.clone());
            }
        }
        for diagnostic in diagnostics {
            append_diagnostic(&mut candidate.diagnostics, diagnostic);
        }
        persist_plugin(connection, &candidate)?;
        self.plugins
            .get_mut(plugin_id)
            .expect("plugin checked above")
            .record = candidate;
        Ok(())
    }

    fn insert_invalid_record(&mut self, origin: PluginOrigin, path: PathBuf, message: String) {
        let id = invalid_record_id(&path, origin);
        let diagnostic = PluginDiagnostic::error("manifest", None, message);
        let record = PluginRecord {
            id: id.clone(),
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Invalid Plugin")
                .to_owned(),
            version: None,
            description: None,
            author: None,
            path: path.to_string_lossy().into_owned(),
            origin,
            state: PluginState::Invalid,
            enabled: false,
            permissions_reviewed: false,
            declared_capabilities: BTreeSet::new(),
            granted_capabilities: BTreeSet::new(),
            required_host_bridges: BTreeSet::new(),
            providers: Vec::new(),
            diagnostics: vec![diagnostic],
            can_remove: origin == PluginOrigin::User,
            can_enable: false,
            manifest: None,
        };
        self.plugins.insert(
            id,
            PluginEntryRuntime {
                record,
                providers: BTreeMap::new(),
            },
        );
    }

    fn validate_install_conflicts(
        &self,
        manifest: &PluginManifest,
    ) -> Result<(), PluginSystemError> {
        let installing_provider_ids = manifest
            .provider_entrypoints
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<BTreeSet<_>>();
        for plugin in self.plugins.values() {
            let Some(existing_manifest) = plugin.record.manifest.as_ref() else {
                continue;
            };
            if existing_manifest.id == manifest.id {
                continue;
            }
            if let Some(provider_id) = existing_manifest
                .provider_entrypoints
                .iter()
                .map(|provider| provider.id.as_str())
                .find(|provider_id| installing_provider_ids.contains(provider_id))
            {
                return Err(PluginSystemError::Package(format!(
                    "Source Provider id {provider_id} is already owned by plugin {}",
                    existing_manifest.id
                )));
            }
        }
        Ok(())
    }

    fn restore_failed_install(
        &mut self,
        connection: &Connection,
        replacement: PackageReplacement,
        install_error: PluginSystemError,
    ) -> PluginSystemError {
        let filesystem_error = replacement.rollback().err();
        let registry_error = self.refresh(connection).err();
        if filesystem_error.is_none() && registry_error.is_none() {
            return install_error;
        }

        let mut failures = Vec::new();
        if let Some(error) = filesystem_error {
            failures.push(format!("filesystem rollback failed: {error}"));
        }
        if let Some(error) = registry_error {
            failures.push(format!("registry restore failed: {error}"));
        }
        PluginSystemError::Package(format!(
            "installation failed: {install_error}; {}",
            failures.join("; ")
        ))
    }

    fn restore_failed_removal(
        &mut self,
        staged_removal: Option<PackageRemoval>,
        previous: PluginEntryRuntime,
        removal_error: PluginSystemError,
    ) -> PluginSystemError {
        let filesystem_error = staged_removal.and_then(|removal| removal.rollback().err());
        let runtime_error = restore_runtime_entry(&self.runtime, &previous).err();
        let plugin_id = previous.record.id.clone();
        self.plugins.insert(plugin_id, previous);

        if filesystem_error.is_none() && runtime_error.is_none() {
            return removal_error;
        }

        let mut failures = Vec::new();
        if let Some(error) = filesystem_error {
            failures.push(format!("filesystem rollback failed: {error}"));
        }
        if let Some(error) = runtime_error {
            failures.push(format!("runtime restore failed: {error}"));
        }
        PluginSystemError::Package(format!(
            "removal failed: {removal_error}; {}",
            failures.join("; ")
        ))
    }

    fn restore_committed_removal(
        &mut self,
        connection: &Connection,
        previous: PluginEntryRuntime,
        removal_error: PluginSystemError,
    ) -> PluginSystemError {
        let database_error = persist_plugin(connection, &previous.record).err();
        let runtime_error = restore_runtime_entry(&self.runtime, &previous).err();
        let plugin_id = previous.record.id.clone();
        self.plugins.insert(plugin_id, previous);

        if database_error.is_none() && runtime_error.is_none() {
            return removal_error;
        }

        let mut failures = Vec::new();
        if let Some(error) = database_error {
            failures.push(format!("database restore failed: {error}"));
        }
        if let Some(error) = runtime_error {
            failures.push(format!("runtime restore failed: {error}"));
        }
        PluginSystemError::Package(format!(
            "removal cleanup failed: {removal_error}; {}",
            failures.join("; ")
        ))
    }
}

fn load_persisted_states(
    connection: &Connection,
) -> Result<BTreeMap<String, PersistedPluginState>, PluginSystemError> {
    let mut states = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT plugin_id, manifest_fingerprint, enabled, permissions_reviewed, granted_capabilities
         FROM plugin_states",
    )?;
    let rows = statement.query_map([], |row| {
        let plugin_id: String = row.get(0)?;
        let grants_json: String = row.get(4)?;
        let grants = serde_json::from_str::<Vec<String>>(&grants_json).unwrap_or_default();
        let granted_capabilities = grants
            .iter()
            .filter_map(|capability| capability_from_str(capability))
            .collect();
        Ok((
            plugin_id,
            PersistedPluginState {
                manifest_fingerprint: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                permissions_reviewed: row.get::<_, i64>(3)? != 0,
                granted_capabilities,
                diagnostics: Vec::new(),
            },
        ))
    })?;
    for row in rows {
        let (plugin_id, state) = row?;
        states.insert(plugin_id, state);
    }

    let mut statement = connection.prepare(
        "SELECT plugin_id, code, level, source_id, message, timestamp
         FROM plugin_diagnostics ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        let level = match row.get::<_, String>(2)?.as_str() {
            "info" => DiagnosticLevel::Info,
            "warn" | "warning" => DiagnosticLevel::Warn,
            "security" => DiagnosticLevel::Security,
            _ => DiagnosticLevel::Error,
        };
        Ok((
            row.get::<_, String>(0)?,
            PluginDiagnostic {
                code: row.get(1)?,
                level,
                source_id: row.get(3)?,
                message: row.get(4)?,
                timestamp: row.get(5)?,
            },
        ))
    })?;
    for row in rows {
        let (plugin_id, diagnostic) = row?;
        states
            .entry(plugin_id)
            .or_insert_with(|| PersistedPluginState {
                manifest_fingerprint: String::new(),
                enabled: false,
                permissions_reviewed: false,
                granted_capabilities: BTreeSet::new(),
                diagnostics: Vec::new(),
            })
            .diagnostics
            .push(diagnostic);
    }
    for state in states.values_mut() {
        trim_diagnostics(&mut state.diagnostics);
    }
    Ok(states)
}

fn persist_plugin(connection: &Connection, record: &PluginRecord) -> Result<(), PluginSystemError> {
    const SAVEPOINT: &str = "fika_plugin_persist";

    connection.execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))?;
    let result = persist_plugin_rows(connection, record);
    match result {
        Ok(()) => {
            if let Err(error) = connection.execute_batch(&format!("RELEASE SAVEPOINT {SAVEPOINT}"))
            {
                return match connection.execute_batch(&format!(
                    "ROLLBACK TO SAVEPOINT {SAVEPOINT}; RELEASE SAVEPOINT {SAVEPOINT}"
                )) {
                    Ok(()) => Err(PluginSystemError::Database(error)),
                    Err(rollback_error) => Err(PluginSystemError::Package(format!(
                        "committing Plugin state failed: {error}; rolling back the database savepoint also failed: {rollback_error}"
                    ))),
                };
            }
            Ok(())
        }
        Err(error) => {
            if let Err(rollback_error) = connection.execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {SAVEPOINT}; RELEASE SAVEPOINT {SAVEPOINT}"
            )) {
                return Err(PluginSystemError::Package(format!(
                    "persisting Plugin state failed: {error}; rolling back the database savepoint also failed: {rollback_error}"
                )));
            }
            Err(error)
        }
    }
}

fn persist_plugin_rows(
    connection: &Connection,
    record: &PluginRecord,
) -> Result<(), PluginSystemError> {
    let grants = record
        .granted_capabilities
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>();
    let grants_json = serde_json::to_string(&grants)?;
    let fingerprint = record
        .manifest
        .as_ref()
        .map(manifest_fingerprint)
        .transpose()?
        .unwrap_or_default();
    let timestamp = now_timestamp();
    connection.execute(
        "INSERT INTO plugin_states (
             plugin_id, package_path, origin, manifest_fingerprint, enabled,
             permissions_reviewed, granted_capabilities, installed_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(plugin_id) DO UPDATE SET
             package_path = excluded.package_path,
             origin = excluded.origin,
             manifest_fingerprint = excluded.manifest_fingerprint,
             enabled = excluded.enabled,
             permissions_reviewed = excluded.permissions_reviewed,
             granted_capabilities = excluded.granted_capabilities,
             updated_at = excluded.updated_at",
        params![
            record.id,
            record.path,
            record.origin.as_str(),
            fingerprint,
            i64::from(record.enabled),
            i64::from(record.permissions_reviewed),
            grants_json,
            timestamp,
        ],
    )?;
    connection.execute(
        "DELETE FROM plugin_diagnostics WHERE plugin_id = ?1",
        params![record.id],
    )?;
    for diagnostic in &record.diagnostics {
        connection.execute(
            "INSERT INTO plugin_diagnostics (
                 plugin_id, code, level, source_id, message, timestamp
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                diagnostic.code,
                diagnostic_level_as_str(diagnostic.level),
                diagnostic.source_id,
                diagnostic.message,
                diagnostic.timestamp,
            ],
        )?;
    }
    Ok(())
}

fn delete_persisted_plugin(
    connection: &Connection,
    plugin_id: &str,
) -> Result<(), PluginSystemError> {
    connection.execute(
        "DELETE FROM plugin_diagnostics WHERE plugin_id = ?1",
        params![plugin_id],
    )?;
    connection.execute(
        "DELETE FROM plugin_states WHERE plugin_id = ?1",
        params![plugin_id],
    )?;
    Ok(())
}

fn record_for_manifest(
    manifest: &PluginManifest,
    path: &Path,
    origin: PluginOrigin,
    persisted: Option<&PersistedPluginState>,
    compatibility: Vec<PluginDiagnostic>,
) -> Result<PluginRecord, PluginSystemError> {
    let declared_capabilities = manifest.declared_capabilities();
    let current_fingerprint = manifest_fingerprint(manifest)?;
    let state_matches_manifest = persisted
        .map(|state| state.manifest_fingerprint == current_fingerprint)
        .unwrap_or(false);
    let granted_capabilities = persisted
        .filter(|_| state_matches_manifest)
        .map(|state| {
            state
                .granted_capabilities
                .intersection(&declared_capabilities)
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let permissions_reviewed = persisted
        .filter(|_| state_matches_manifest)
        .map(|state| state.permissions_reviewed)
        .unwrap_or(false);
    let mut diagnostics = persisted
        .filter(|_| state_matches_manifest)
        .map(|state| state.diagnostics.clone())
        .unwrap_or_default();
    diagnostics.retain(|diagnostic| {
        diagnostic.code != "compatibility" && diagnostic.code != "bridge-compatibility"
    });
    diagnostics.extend(compatibility.iter().cloned());
    let compatible = compatibility.is_empty();
    let requested_enabled = persisted
        .filter(|_| state_matches_manifest)
        .map(|state| state.enabled)
        .unwrap_or(false);
    let enabled = requested_enabled
        && compatible
        && (declared_capabilities.is_empty() || permissions_reviewed);
    let state = if !compatible {
        PluginState::Incompatible
    } else if enabled {
        PluginState::Enabled
    } else if !declared_capabilities.is_empty() && !permissions_reviewed {
        PluginState::NeedsReview
    } else {
        PluginState::Disabled
    };
    let providers = manifest
        .provider_entrypoints
        .iter()
        .map(|provider| PluginProviderState {
            id: provider.id.clone(),
            entrypoint: provider.entrypoint.clone(),
            initialized: false,
            sources: provider.source_catalog.values().cloned().collect(),
            runtime_report: None,
            diagnostics: Vec::new(),
        })
        .collect();
    let mut record = PluginRecord {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: Some(manifest.version.clone()),
        description: manifest.description.clone(),
        author: manifest.author.clone(),
        path: path.to_string_lossy().into_owned(),
        origin,
        state,
        enabled,
        permissions_reviewed,
        declared_capabilities,
        granted_capabilities,
        required_host_bridges: manifest.required_host_bridges.clone(),
        providers,
        diagnostics,
        can_remove: origin == PluginOrigin::User,
        can_enable: false,
        manifest: Some(manifest.clone()),
    };
    update_action_flags(&mut record);
    Ok(record)
}

fn update_action_flags(record: &mut PluginRecord) {
    record.can_enable = record.manifest.is_some()
        && record.state != PluginState::Invalid
        && record.state != PluginState::Incompatible
        && (record.declared_capabilities.is_empty() || record.permissions_reviewed);
}

fn build_provider(
    manifest: &PluginManifest,
    entrypoint: &PluginProviderEntrypoint,
    _package_path: &Path,
    netease_bridge: Option<Arc<dyn NeteaseProviderBridge>>,
    kugou_bridge: Option<Arc<dyn KugouProviderBridge>>,
) -> Result<Arc<dyn SourceProvider>, PluginSystemError> {
    let capabilities = provider_declared_capabilities(manifest, entrypoint);
    match entrypoint.entrypoint.as_str() {
        #[cfg(test)]
        "builtin:runtime-demo" => Ok(Arc::new(DemoSourceProvider::new(
            entrypoint.id.clone(),
            capabilities,
            entrypoint.source_catalog.clone(),
        ))),
        #[cfg(test)]
        "catalog" | "builtin:catalog" => Ok(Arc::new(CatalogSourceProvider::new(
            entrypoint.id.clone(),
            capabilities,
            entrypoint.source_catalog.clone(),
        ))),
        "builtin:netease"
            if manifest.id == NETEASE_PLUGIN_ID && entrypoint.id == NETEASE_PROVIDER_ID =>
        {
            netease_bridge
                .map(|bridge| {
                    Arc::new(NeteaseSourceProvider::new(
                        entrypoint.id.clone(),
                        capabilities,
                        bridge,
                    )) as Arc<dyn SourceProvider>
                })
                .ok_or_else(|| PluginSystemError::ProviderLoad {
                    plugin_id: manifest.id.clone(),
                    entrypoint: entrypoint.entrypoint.clone(),
                    message: "the NetEase Service Bridge is unavailable".to_owned(),
                })
        }
        "builtin:kugou" if manifest.id == KUGOU_PLUGIN_ID && entrypoint.id == KUGOU_PROVIDER_ID => {
            kugou_bridge
                .map(|bridge| {
                    Arc::new(KugouSourceProvider::new(
                        entrypoint.id.clone(),
                        capabilities,
                        bridge,
                    )) as Arc<dyn SourceProvider>
                })
                .ok_or_else(|| PluginSystemError::ProviderLoad {
                    plugin_id: manifest.id.clone(),
                    entrypoint: entrypoint.entrypoint.clone(),
                    message: "the KuGou Service Bridge is unavailable".to_owned(),
                })
        }
        _ => Err(PluginSystemError::ProviderLoad {
            plugin_id: manifest.id.clone(),
            entrypoint: entrypoint.entrypoint.clone(),
            message: "only built-in provider entrypoints are loadable in this runtime".to_owned(),
        }),
    }
}

#[cfg(test)]
#[derive(Debug)]
struct DemoSourceProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    sources: BTreeMap<String, SourceInfo>,
}

#[cfg(test)]
impl DemoSourceProvider {
    fn new(
        id: String,
        capabilities: BTreeSet<SourceCapability>,
        sources: BTreeMap<String, SourceInfo>,
    ) -> Self {
        let sources = if sources.is_empty() {
            BTreeMap::from([(
                source_runtime::LX_SOURCE_WY.to_owned(),
                source_runtime::lx_music_source(
                    source_runtime::LX_SOURCE_WY,
                    "Runtime Demo",
                    standard_actions(),
                    source_runtime::standard_lx_qualities(),
                ),
            )])
        } else {
            sources
        };
        Self {
            id,
            capabilities,
            sources,
        }
    }
}

#[cfg(test)]
impl SourceProvider for DemoSourceProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info("initialized bundled Plugin demo Source Provider");
        Ok(self.sources.clone())
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        match request {
            SourceRequest::MusicSearch {
                source, keyword, ..
            } => Ok(SourceResponse::MusicSearch(SourceSearchResponse {
                is_end: true,
                total: Some(1),
                list: vec![SourceSearchResult {
                    id: "runtime-demo-track".to_owned(),
                    source,
                    title: format!("Demo result for {keyword}"),
                    artist: "Fika Runtime".to_owned(),
                    album: Some("Plugin System MVP".to_owned()),
                    duration_seconds: Some(180),
                    cover_url: Some("https://example.invalid/runtime-demo-cover.jpg".to_owned()),
                    raw_info: json!({ "id": "runtime-demo-track" }),
                }],
            })),
            SourceRequest::MusicUrl {
                source, music_info, ..
            } => {
                context
                    .require_capability(SourceCapability::NetworkAny, "resolve demo musicUrl")?;
                let id = music_info
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("runtime-demo-track");
                Ok(SourceResponse::MusicUrl(format!(
                    "https://example.invalid/{source}/{id}.mp3"
                )))
            }
            SourceRequest::Lyric { .. } => Ok(SourceResponse::Lyric(LyricResponse {
                lyric: Some("[00:00.00]Runtime demo lyric".to_owned()),
                tlyric: None,
                rlyric: None,
                lxlyric: None,
            })),
            SourceRequest::Pic { source, .. } => Ok(SourceResponse::Pic(format!(
                "https://example.invalid/{source}/cover.jpg"
            ))),
            request => Err(context.unsupported_action(request.source(), request.action())),
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
struct CatalogSourceProvider {
    id: String,
    capabilities: BTreeSet<SourceCapability>,
    sources: BTreeMap<String, SourceInfo>,
}

#[cfg(test)]
impl CatalogSourceProvider {
    fn new(
        id: String,
        capabilities: BTreeSet<SourceCapability>,
        sources: BTreeMap<String, SourceInfo>,
    ) -> Self {
        Self {
            id,
            capabilities,
            sources,
        }
    }
}

#[cfg(test)]
impl SourceProvider for CatalogSourceProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info("initialized catalog-only Plugin Source Provider");
        Ok(self.sources.clone())
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        _request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        Err(context.provider_error(
            "catalog-only provider has no executable Source Provider implementation",
        ))
    }
}

#[cfg(test)]
fn standard_actions() -> Vec<SourceAction> {
    vec![
        SourceAction::MusicSearch,
        SourceAction::MusicUrl,
        SourceAction::Lyric,
        SourceAction::Pic,
    ]
}

fn validate_provider_routes(
    plugin_id: &str,
    providers: &[PluginProviderState],
) -> Result<(), PluginSystemError> {
    let mut routes = BTreeMap::new();
    for provider in providers {
        for source in &provider.sources {
            for action in &source.actions {
                let route = (source.id.clone(), *action);
                if let Some(existing_provider) = routes.insert(route, provider.id.clone()) {
                    return Err(PluginSystemError::Package(format!(
                        "plugin {plugin_id} has an ambiguous route for source {} action {action:?}: providers {} and {}",
                        source.id, existing_provider, provider.id
                    )));
                }
            }
        }
    }
    Ok(())
}

fn clear_runtime_provider_state(
    runtime: &SourceRuntime,
    provider_ids: impl IntoIterator<Item = String>,
) {
    for provider_id in provider_ids {
        let _ = runtime.uninitialize_provider(&provider_id);
        let _ = runtime.clear_provider_granted_capabilities(&provider_id);
    }
}

fn clear_runtime_provider_ids<'a>(
    runtime: &SourceRuntime,
    provider_ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), PluginSystemError> {
    let mut failures = Vec::new();
    for provider_id in provider_ids {
        if let Err(error) = runtime.uninitialize_provider(provider_id) {
            failures.push(format!("uninitializing {provider_id} failed: {error}"));
        }
        if let Err(error) = runtime.clear_provider_granted_capabilities(provider_id) {
            failures.push(format!("clearing grants for {provider_id} failed: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(PluginSystemError::Package(failures.join("; ")))
    }
}

fn clear_runtime_entry(
    runtime: &SourceRuntime,
    entry: &PluginEntryRuntime,
) -> Result<(), PluginSystemError> {
    clear_runtime_provider_ids(runtime, entry.providers.keys().map(String::as_str))
}

fn clear_runtime_entries(
    runtime: &SourceRuntime,
    entries: &BTreeMap<String, PluginEntryRuntime>,
) -> Result<(), PluginSystemError> {
    let provider_ids = entries
        .values()
        .flat_map(|plugin| plugin.providers.keys().cloned())
        .collect::<BTreeSet<_>>();
    clear_runtime_provider_ids(runtime, provider_ids.iter().map(String::as_str))
}

fn restore_runtime_entries(
    runtime: &SourceRuntime,
    entries: &BTreeMap<String, PluginEntryRuntime>,
) -> Result<(), PluginSystemError> {
    let mut restored_provider_ids = Vec::new();
    for entry in entries.values() {
        if let Err(error) = restore_runtime_entry(runtime, entry) {
            clear_runtime_provider_state(runtime, restored_provider_ids);
            return Err(error);
        }
        restored_provider_ids.extend(entry.providers.keys().cloned());
    }
    Ok(())
}

fn restore_runtime_entry(
    runtime: &SourceRuntime,
    entry: &PluginEntryRuntime,
) -> Result<(), PluginSystemError> {
    if !entry.record.enabled || entry.providers.is_empty() {
        return Ok(());
    }

    let mut restored_provider_ids = Vec::new();
    for (provider_id, provider) in &entry.providers {
        let grants = entry
            .record
            .granted_capabilities
            .intersection(&provider.required_capabilities())
            .copied()
            .collect::<BTreeSet<_>>();
        if let Err(error) =
            runtime.replace_provider_granted_capabilities(provider_id.clone(), grants)
        {
            clear_runtime_provider_state(runtime, restored_provider_ids);
            return Err(runtime_error(&entry.record.id, error));
        }
        restored_provider_ids.push(provider_id.clone());
        if let Err(error) = runtime.initialize_provider(provider.as_ref()) {
            clear_runtime_provider_state(runtime, restored_provider_ids);
            return Err(runtime_error(&entry.record.id, error));
        }
    }
    Ok(())
}

fn restore_after_failed_deactivation(
    runtime: &SourceRuntime,
    previous: &PluginEntryRuntime,
    deactivation_error: PluginSystemError,
) -> PluginSystemError {
    match restore_runtime_entry(runtime, previous) {
        Ok(()) => deactivation_error,
        Err(restore_error) => PluginSystemError::Package(format!(
            "deactivating plugin {} failed: {deactivation_error}; restoring its runtime state also failed: {restore_error}",
            previous.record.id
        )),
    }
}

fn provider_declared_capabilities(
    manifest: &PluginManifest,
    entrypoint: &PluginProviderEntrypoint,
) -> BTreeSet<SourceCapability> {
    manifest
        .capabilities
        .union(&entrypoint.capabilities)
        .copied()
        .collect()
}

fn replace_runtime_provider_grants(
    runtime: &SourceRuntime,
    manifest: &PluginManifest,
    active_provider_ids: &BTreeSet<String>,
    grants: &BTreeSet<SourceCapability>,
) -> Result<(), SourceRuntimeError> {
    for entrypoint in &manifest.provider_entrypoints {
        if !active_provider_ids.contains(&entrypoint.id) {
            continue;
        }
        let declared = provider_declared_capabilities(manifest, entrypoint);
        let provider_grants = grants.intersection(&declared).copied();
        runtime.replace_provider_granted_capabilities(entrypoint.id.clone(), provider_grants)?;
    }
    Ok(())
}

fn runtime_error(plugin_id: &str, error: SourceRuntimeError) -> PluginSystemError {
    let diagnostics = error
        .diagnostics()
        .iter()
        .map(PluginDiagnostic::from_source)
        .collect();
    PluginSystemError::Runtime {
        plugin_id: plugin_id.to_owned(),
        message: error.to_string(),
        diagnostics,
    }
}

fn read_manifest(package_path: &Path) -> Result<PluginManifest, PluginSystemError> {
    let manifest_path = package_path.join(PLUGIN_MANIFEST_FILE);
    let manifest_path = if manifest_path.is_file() {
        manifest_path
    } else {
        let legacy_path = package_path.join("manifest.json");
        if legacy_path.is_file() {
            legacy_path
        } else {
            return Err(PluginSystemError::Package(format!(
                "missing {} in {}",
                PLUGIN_MANIFEST_FILE,
                package_path.display()
            )));
        }
    };
    let contents = fs::read_to_string(manifest_path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn normalize_package_path(path: &Path) -> Result<PathBuf, PluginSystemError> {
    let path = if path.is_file() {
        path.parent().ok_or_else(|| {
            PluginSystemError::Package("manifest has no parent directory".to_owned())
        })?
    } else {
        path
    };
    if !path.is_dir() {
        return Err(PluginSystemError::Package(format!(
            "package path is not a directory: {}",
            path.display()
        )));
    }
    Ok(fs::canonicalize(path)?)
}

fn discover_package_paths(root: &Path) -> Result<Vec<PathBuf>, PluginSystemError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    if root.join(PLUGIN_MANIFEST_FILE).is_file() || root.join("manifest.json").is_file() {
        return Ok(vec![root.to_owned()]);
    }

    let mut paths = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .filter(|path| {
            path.join(PLUGIN_MANIFEST_FILE).is_file() || path.join("manifest.json").is_file()
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn copy_package_tree(source: &Path, destination: &Path) -> Result<(), PluginSystemError> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(PluginSystemError::Package(format!(
                "symbolic links are not allowed in Plugin packages: {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            copy_package_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn install_staging_paths(staging_root: &Path, plugin_id: &str) -> (PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let suffix = format!("{}-{nonce}", std::process::id());
    (
        staging_root.join(format!("{plugin_id}.install-{suffix}")),
        staging_root.join(format!("{plugin_id}.backup-{suffix}")),
    )
}

fn removal_staging_path(staging_root: &Path, plugin_id: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    staging_root.join(format!("{plugin_id}.remove-{}-{nonce}", std::process::id()))
}

fn remove_path(path: &Path) -> Result<(), std::io::Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn remove_dir_if_empty(path: &Path) -> Result<(), std::io::Error> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn is_within(path: &Path, root: &Path) -> bool {
    path == root || path.strip_prefix(root).is_ok()
}

fn paths_overlap(first: &Path, second: &Path) -> bool {
    is_within(first, second) || is_within(second, first)
}

fn invalid_record_id(path: &Path, origin: PluginOrigin) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    origin.as_str().hash(&mut hasher);
    format!("invalid-{:x}", hasher.finish())
}

fn manifest_fingerprint(manifest: &PluginManifest) -> Result<String, PluginSystemError> {
    let bytes = serde_json::to_vec(manifest)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn append_diagnostic(diagnostics: &mut Vec<PluginDiagnostic>, diagnostic: PluginDiagnostic) {
    diagnostics.push(diagnostic);
    trim_diagnostics(diagnostics);
}

fn trim_diagnostics(diagnostics: &mut Vec<PluginDiagnostic>) {
    const MAX_PLUGIN_DIAGNOSTICS: usize = 200;
    if diagnostics.len() > MAX_PLUGIN_DIAGNOSTICS {
        let remove_count = diagnostics.len() - MAX_PLUGIN_DIAGNOSTICS;
        diagnostics.drain(0..remove_count);
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_legacy_lx_audio_source(manifest: &PluginManifest) -> bool {
    manifest.provider_entrypoints.iter().any(|provider| {
        provider
            .entrypoint
            .starts_with(IMPORTED_LX_ENTRYPOINT_PREFIX)
    })
}

fn valid_entrypoint(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
}

fn valid_semver(value: &str) -> bool {
    semver::Version::parse(value).is_ok()
}

fn capability_from_str(value: &str) -> Option<SourceCapability> {
    match value {
        "network:any" => Some(SourceCapability::NetworkAny),
        "account:ref" => Some(SourceCapability::AccountRef),
        "playlist:read" => Some(SourceCapability::PlaylistRead),
        "playlist:write" => Some(SourceCapability::PlaylistWrite),
        "metadata:read" => Some(SourceCapability::MetadataRead),
        "cache:read-write" => Some(SourceCapability::CacheReadWrite),
        "bridge:netease-api-enhanced" => Some(SourceCapability::BridgeNeteaseApiEnhanced),
        "bridge:kugou-music-api" => Some(SourceCapability::BridgeKugouMusicApi),
        _ => None,
    }
}

pub fn parse_capabilities(values: &[String]) -> Result<BTreeSet<SourceCapability>, String> {
    values
        .iter()
        .map(|value| {
            capability_from_str(value)
                .ok_or_else(|| format!("unsupported Source Provider capability: {value}"))
        })
        .collect()
}

fn diagnostic_level_as_str(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "info",
        DiagnosticLevel::Warn => "warn",
        DiagnosticLevel::Error => "error",
        DiagnosticLevel::Security => "security",
    }
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_runtime::SourceQuality;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let id = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "fika-music-plugin-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test directory should be created");
        path
    }

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("test database should open");
        crate::database::initialize(&mut connection).expect("plugin schema should initialize");
        connection
    }

    fn manifest(id: &str, entrypoint: &str, capabilities: &[SourceCapability]) -> PluginManifest {
        PluginManifest {
            manifest_version: 1,
            id: id.to_owned(),
            name: "Test Plugin".to_owned(),
            version: "1.0.0".to_owned(),
            description: None,
            author: Some("Fika Tests".to_owned()),
            homepage: None,
            provider_entrypoints: vec![PluginProviderEntrypoint {
                id: format!("{id}-provider"),
                entrypoint: entrypoint.to_owned(),
                capabilities: capabilities.iter().copied().collect(),
                source_catalog: BTreeMap::new(),
            }],
            capabilities: capabilities.iter().copied().collect(),
            compatibility_target: PLUGIN_COMPATIBILITY_TARGET.to_owned(),
            supported_api_version: PLUGIN_RUNTIME_API_VERSION,
            required_host_bridges: BTreeSet::new(),
        }
    }

    fn write_package(root: &Path, manifest: &PluginManifest) -> PathBuf {
        let package = root.join(&manifest.id);
        fs::create_dir_all(&package).expect("package directory should be created");
        fs::write(
            package.join(PLUGIN_MANIFEST_FILE),
            serde_json::to_vec_pretty(manifest).expect("manifest should serialize"),
        )
        .expect("manifest should be written");
        package
    }

    #[test]
    fn bundled_runtime_demo_manifest_should_validate() {
        let manifest = serde_json::from_str::<PluginManifest>(include_str!(
            "../fixtures/plugins/runtime-demo/plugin.json"
        ))
        .expect("bundled demo manifest should deserialize");

        manifest
            .validate()
            .expect("bundled demo manifest should validate");
    }

    #[test]
    fn bundled_netease_manifest_should_validate() {
        let manifest =
            serde_json::from_str::<PluginManifest>(include_str!("../plugins/netease/plugin.json"))
                .expect("bundled NetEase manifest should deserialize");

        manifest
            .validate()
            .expect("bundled NetEase manifest should validate");
        let mut connection = Connection::open_in_memory().expect("test database should open");
        crate::database::initialize(&mut connection).expect("test database should initialize");
        let bridge: Arc<dyn NeteaseProviderBridge> = Arc::new(
            crate::netease::NeteaseServiceBridge::new(
                Arc::new(Mutex::new(connection)),
                Arc::new(source_runtime::DefaultSourceHost::new(
                    std::time::Duration::from_secs(1),
                    1024,
                )),
            )
            .expect("NetEase Service Bridge should initialize"),
        );
        let provider = build_provider(
            &manifest,
            &manifest.provider_entrypoints[0],
            Path::new("."),
            Some(bridge),
            None,
        )
        .expect("bundled NetEase Provider should load through its Plugin entrypoint");

        assert_eq!(provider.id(), crate::netease::NETEASE_PROVIDER_ID);
    }

    #[test]
    fn builtin_netease_entrypoint_should_be_reserved_for_the_bundled_package() {
        let mut impostor = manifest("user.netease", "builtin:netease", &[]);
        impostor.provider_entrypoints[0].id = NETEASE_PROVIDER_ID.to_owned();

        let error = match build_provider(
            &impostor,
            &impostor.provider_entrypoints[0],
            Path::new("."),
            None,
            None,
        ) {
            Ok(_) => panic!("a noncanonical package must not construct the NetEase Provider"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            PluginSystemError::ProviderLoad { message, .. }
                if message.contains("only built-in provider entrypoints")
        ));
    }

    #[test]
    fn bundled_kugou_manifest_should_validate_and_load() {
        let manifest =
            serde_json::from_str::<PluginManifest>(include_str!("../plugins/kugou/plugin.json"))
                .expect("bundled KuGou manifest should deserialize");
        manifest
            .validate()
            .expect("bundled KuGou manifest should validate");
        let mut connection = Connection::open_in_memory().expect("test database should open");
        crate::database::initialize(&mut connection).expect("test database should initialize");
        let bridge: Arc<dyn KugouProviderBridge> = Arc::new(
            crate::kugou::KugouServiceBridge::new(
                Arc::new(Mutex::new(connection)),
                Arc::new(source_runtime::DefaultSourceHost::new(
                    std::time::Duration::from_secs(1),
                    1024,
                )),
            )
            .expect("KuGou Service Bridge should initialize"),
        );
        let provider = build_provider(
            &manifest,
            &manifest.provider_entrypoints[0],
            Path::new("."),
            None,
            Some(bridge),
        )
        .expect("bundled KuGou Provider should load through its Plugin entrypoint");

        assert_eq!(provider.id(), crate::kugou::KUGOU_PROVIDER_ID);
    }

    #[test]
    fn builtin_kugou_entrypoint_should_be_reserved_for_the_bundled_package() {
        let mut impostor = manifest("user.kugou", "builtin:kugou", &[]);
        impostor.provider_entrypoints[0].id = KUGOU_PROVIDER_ID.to_owned();

        let error = match build_provider(
            &impostor,
            &impostor.provider_entrypoints[0],
            Path::new("."),
            None,
            None,
        ) {
            Ok(_) => panic!("a noncanonical package must not construct the KuGou Provider"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            PluginSystemError::ProviderLoad { message, .. }
                if message.contains("only built-in provider entrypoints")
        ));
    }

    #[test]
    fn manifest_validation_rejects_invalid_versions_and_duplicate_provider_ids() {
        let mut plugin = manifest("test.plugin", "builtin:runtime-demo", &[]);
        plugin.version = "1.0".to_owned();
        plugin
            .provider_entrypoints
            .push(plugin.provider_entrypoints[0].clone());

        let errors = plugin.validate().expect_err("manifest should be invalid");

        assert!(errors.iter().any(|error| error.contains("semver")));
        assert!(errors.iter().any(|error| error.contains("unique")));
    }

    #[test]
    fn manifest_validation_rejects_numeric_prerelease_identifiers_with_leading_zeroes() {
        let mut plugin = manifest("test.plugin", "builtin:runtime-demo", &[]);
        plugin.version = "1.0.0-01".to_owned();

        let errors = plugin.validate().expect_err("manifest should be invalid");

        assert!(errors.iter().any(|error| error.contains("semver")));
    }

    #[test]
    fn manifest_validation_rejects_duplicate_source_action_routes() {
        let mut plugin = manifest("test.plugin", "builtin:runtime-demo", &[]);
        let source = source_runtime::lx_music_source(
            source_runtime::LX_SOURCE_WY,
            "First Provider",
            vec![SourceAction::MusicSearch],
            Vec::new(),
        );
        plugin.provider_entrypoints[0]
            .source_catalog
            .insert(source.id.clone(), source.clone());
        let mut second_provider = plugin.provider_entrypoints[0].clone();
        second_provider.id = "test.plugin-second-provider".to_owned();
        second_provider
            .source_catalog
            .insert(source.id.clone(), source);
        plugin.provider_entrypoints.push(second_provider);

        let errors = plugin.validate().expect_err("manifest should be invalid");

        assert!(errors
            .iter()
            .any(|error| error.contains("exposed by both providers")));
    }

    #[test]
    fn refresh_does_not_expose_legacy_lx_audio_sources_as_plugins() {
        let root = temp_dir("legacy-lx-source");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let legacy = manifest(
            "legacy.audio-source",
            &format!(
                "{IMPORTED_LX_ENTRYPOINT_PREFIX}static-templates:{}",
                "0".repeat(64)
            ),
            &[SourceCapability::NetworkAny],
        );
        write_package(&user, &legacy);
        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));

        let records = registry
            .refresh(&connection)
            .expect("registry should refresh");

        assert!(records.is_empty());
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn refresh_discovers_bundled_and_user_plugins_and_flags_invalid_packages() {
        let root = temp_dir("discovery");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("bundled.plugin", "builtin:runtime-demo", &[]),
        );
        let invalid = user.join("invalid");
        fs::create_dir_all(&invalid).expect("invalid package directory should be created");
        fs::write(invalid.join(PLUGIN_MANIFEST_FILE), b"{}")
            .expect("invalid manifest should write");

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, runtime);
        let records = registry
            .refresh(&connection)
            .expect("registry should refresh");

        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|record| record.id == "bundled.plugin"));
        assert!(records
            .iter()
            .any(|record| record.state == PluginState::Invalid));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn lifecycle_persists_review_enable_revoke_and_disable() {
        let root = temp_dir("lifecycle");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let package = write_package(
            &bundled,
            &manifest(
                "runtime.plugin",
                "builtin:runtime-demo",
                &[SourceCapability::NetworkAny],
            ),
        );

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        assert_eq!(
            registry.record("runtime.plugin").unwrap().state,
            PluginState::NeedsReview
        );

        registry
            .set_capabilities(
                &connection,
                "runtime.plugin",
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("capabilities should be reviewed");
        let enabled = registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        assert_eq!(enabled.state, PluginState::Enabled);
        assert!(enabled
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "runtime-log"));

        let request = SourceRequest::MusicUrl {
            source: source_runtime::LX_SOURCE_WY.to_owned(),
            music_info: json!({ "id": "track-1" }),
            quality: SourceQuality::K128,
        };
        registry
            .set_capabilities(&connection, "runtime.plugin", [], true)
            .expect("capability should be revocable while enabled");
        let error = registry
            .dispatch_request(
                &connection,
                "runtime.plugin",
                request,
                source_runtime::SourceCancellationToken::default(),
            )
            .expect_err("revoked network capability should deny the request");
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "security-denial"));

        registry
            .set_enabled(&connection, "runtime.plugin", false)
            .expect("plugin should disable");
        let persisted = load_persisted_states(&connection).expect("state should load");
        assert!(!persisted["runtime.plugin"].enabled);
        assert!(persisted["runtime.plugin"].permissions_reviewed);
        assert!(persisted["runtime.plugin"]
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "security-denial" }));

        let mut changed_manifest = manifest(
            "runtime.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        changed_manifest.version = "1.0.1".to_owned();
        fs::write(
            package.join(PLUGIN_MANIFEST_FILE),
            serde_json::to_vec_pretty(&changed_manifest)
                .expect("changed manifest should serialize"),
        )
        .expect("changed manifest should be written");
        let mut restarted_registry =
            PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        restarted_registry
            .refresh(&connection)
            .expect("restarted registry should refresh");
        let changed = restarted_registry
            .record("runtime.plugin")
            .expect("changed plugin should remain visible");
        assert_eq!(changed.state, PluginState::NeedsReview);
        assert!(changed.granted_capabilities.is_empty());

        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn refresh_should_reinitialize_a_persisted_enabled_provider() {
        let root = temp_dir("refresh-enabled");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("runtime.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, runtime);
        registry
            .refresh(&connection)
            .expect("registry should initially refresh");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");

        registry
            .refresh(&connection)
            .expect("enabled plugin should refresh");
        let refreshed = registry
            .record("runtime.plugin")
            .expect("refreshed plugin should remain registered");
        let outcome = registry
            .dispatch_request(
                &connection,
                "runtime.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "refresh".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("refreshed provider should remain dispatchable");

        assert!(refreshed.enabled);
        assert!(refreshed
            .providers
            .iter()
            .all(|provider| provider.initialized));
        assert!(matches!(outcome.response, SourceResponse::MusicSearch(_)));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn dispatch_should_route_by_source_and_action_across_providers() {
        let root = temp_dir("provider-action-routing");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let mut plugin = manifest("runtime.plugin", "builtin:runtime-demo", &[]);
        plugin.provider_entrypoints[0].source_catalog.insert(
            source_runtime::LX_SOURCE_WY.to_owned(),
            source_runtime::lx_music_source(
                source_runtime::LX_SOURCE_WY,
                "Search Provider",
                vec![SourceAction::MusicSearch],
                Vec::new(),
            ),
        );
        let mut lyric_provider = plugin.provider_entrypoints[0].clone();
        lyric_provider.id = "runtime.plugin-lyric-provider".to_owned();
        lyric_provider.source_catalog.insert(
            source_runtime::LX_SOURCE_WY.to_owned(),
            source_runtime::lx_music_source(
                source_runtime::LX_SOURCE_WY,
                "Lyric Provider",
                vec![SourceAction::Lyric],
                Vec::new(),
            ),
        );
        let lyric_provider_id = lyric_provider.id.clone();
        plugin.provider_entrypoints.push(lyric_provider);
        write_package(&bundled, &plugin);

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        let request = SourceRequest::Lyric {
            source: source_runtime::LX_SOURCE_WY.to_owned(),
            music_info: json!({ "id": "track-1" }),
        };
        let dispatch = registry
            .prepare_dispatch("runtime.plugin", &request)
            .expect("lyric request should resolve to a provider");

        assert_eq!(dispatch.provider_id, lyric_provider_id);
        assert!(matches!(
            dispatch
                .execute(request, source_runtime::SourceCancellationToken::default())
                .expect("lyric provider should handle the request")
                .response,
            SourceResponse::Lyric(_)
        ));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn enable_should_reject_runtime_provider_route_collisions() {
        let root = temp_dir("provider-route-collision");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let mut plugin = manifest("runtime.plugin", "builtin:runtime-demo", &[]);
        let mut second_provider = plugin.provider_entrypoints[0].clone();
        second_provider.id = "runtime.plugin-second-provider".to_owned();
        plugin.provider_entrypoints.push(second_provider);
        write_package(&bundled, &plugin);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("registry should refresh");

        let error = registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect_err("overlapping runtime routes should reject activation");
        let record = registry
            .record("runtime.plugin")
            .expect("plugin should remain registered");

        assert!(error.to_string().contains("ambiguous route"));
        assert_eq!(record.state, PluginState::Error);
        assert!(!record.enabled);
        for provider in &plugin.provider_entrypoints {
            assert!(runtime
                .granted_capabilities_for(&provider.id)
                .expect("provider grants should remain readable")
                .is_empty());
        }
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn refresh_should_restore_the_active_registry_when_persistence_fails() {
        let root = temp_dir("refresh-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("runtime.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should initially refresh");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_refresh_persist
                 BEFORE UPDATE ON plugin_states
                 BEGIN
                     SELECT RAISE(ABORT, 'forced refresh persistence failure');
                 END;",
            )
            .expect("refresh failure trigger should be created");

        registry
            .refresh(&connection)
            .expect_err("database failure should reject refresh");
        let record = registry
            .record("runtime.plugin")
            .expect("previous plugin should remain registered");
        let outcome = registry
            .dispatch_request(
                &connection,
                "runtime.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "rollback".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("previous provider should remain dispatchable");

        assert!(record.enabled);
        assert!(record.providers.iter().all(|provider| provider.initialized));
        assert!(matches!(outcome.response, SourceResponse::MusicSearch(_)));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn restarted_registry_should_reinitialize_a_persisted_enabled_provider() {
        let root = temp_dir("restart-enabled");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("runtime.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should initially refresh");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        drop(registry);

        let mut restarted = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        restarted
            .refresh(&connection)
            .expect("restarted registry should refresh");
        restarted
            .dispatch_request(
                &connection,
                "runtime.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "restart".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("restarted provider should be dispatchable");

        assert!(restarted
            .record("runtime.plugin")
            .expect("restarted plugin should remain registered")
            .providers
            .iter()
            .all(|provider| provider.initialized));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn capability_update_should_restore_runtime_and_memory_when_persistence_fails() {
        let root = temp_dir("capability-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let plugin = manifest(
            "runtime.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        let provider_id = plugin.provider_entrypoints[0].id.clone();
        write_package(&bundled, &plugin);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_capabilities(
                &connection,
                "runtime.plugin",
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("capabilities should be reviewed");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_capability_update
                 BEFORE UPDATE ON plugin_states
                 WHEN NEW.granted_capabilities != OLD.granted_capabilities
                 BEGIN
                     SELECT RAISE(ABORT, 'forced capability update failure');
                 END;",
            )
            .expect("capability failure trigger should be created");

        registry
            .set_capabilities(&connection, "runtime.plugin", [], true)
            .expect_err("database failure should reject capability update");
        let record = registry
            .record("runtime.plugin")
            .expect("plugin should remain registered");
        let persisted = load_persisted_states(&connection).expect("persisted state should load");
        let runtime_grants = runtime
            .granted_capabilities_for(&provider_id)
            .expect("runtime grants should remain readable");

        assert!(record
            .granted_capabilities
            .contains(&SourceCapability::NetworkAny));
        assert!(persisted["runtime.plugin"]
            .granted_capabilities
            .contains(&SourceCapability::NetworkAny));
        assert!(runtime_grants.contains(&SourceCapability::NetworkAny));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn capability_update_should_roll_back_partial_database_writes() {
        let root = temp_dir("capability-diagnostic-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let plugin = manifest(
            "runtime.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        let provider_id = plugin.provider_entrypoints[0].id.clone();
        write_package(&bundled, &plugin);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_capabilities(
                &connection,
                "runtime.plugin",
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("capabilities should be reviewed");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        let previous_diagnostics = connection
            .query_row(
                "SELECT COUNT(*) FROM plugin_diagnostics WHERE plugin_id = ?1",
                ["runtime.plugin"],
                |row| row.get::<_, i64>(0),
            )
            .expect("diagnostic count should load");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_diagnostic_insert
                 BEFORE INSERT ON plugin_diagnostics
                 BEGIN
                     SELECT RAISE(ABORT, 'forced diagnostic insert failure');
                 END;",
            )
            .expect("diagnostic failure trigger should be created");

        registry
            .set_capabilities(&connection, "runtime.plugin", [], true)
            .expect_err("diagnostic persistence failure should reject capability update");
        let persisted = load_persisted_states(&connection).expect("persisted state should load");
        let persisted_diagnostics = connection
            .query_row(
                "SELECT COUNT(*) FROM plugin_diagnostics WHERE plugin_id = ?1",
                ["runtime.plugin"],
                |row| row.get::<_, i64>(0),
            )
            .expect("diagnostic count should reload");
        let runtime_grants = runtime
            .granted_capabilities_for(&provider_id)
            .expect("runtime grants should remain readable");

        assert!(persisted["runtime.plugin"]
            .granted_capabilities
            .contains(&SourceCapability::NetworkAny));
        assert_eq!(persisted_diagnostics, previous_diagnostics);
        assert!(runtime_grants.contains(&SourceCapability::NetworkAny));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn entrypoint_capabilities_should_not_leak_between_providers() {
        let root = temp_dir("provider-capability-scope");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let mut plugin = manifest(
            "runtime.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        plugin.capabilities.clear();
        plugin.provider_entrypoints[0].source_catalog.insert(
            source_runtime::LX_SOURCE_WY.to_owned(),
            source_runtime::lx_music_source(
                source_runtime::LX_SOURCE_WY,
                "Network Provider",
                vec![SourceAction::MusicUrl],
                source_runtime::standard_lx_qualities(),
            ),
        );
        let network_provider_id = plugin.provider_entrypoints[0].id.clone();
        let mut cache_provider = plugin.provider_entrypoints[0].clone();
        cache_provider.id = "runtime.plugin-cache-provider".to_owned();
        cache_provider.capabilities = [SourceCapability::CacheReadWrite].into_iter().collect();
        cache_provider.source_catalog.insert(
            source_runtime::LX_SOURCE_WY.to_owned(),
            source_runtime::lx_music_source(
                source_runtime::LX_SOURCE_WY,
                "Cache Provider",
                vec![SourceAction::MusicSearch],
                Vec::new(),
            ),
        );
        let cache_provider_id = cache_provider.id.clone();
        plugin.provider_entrypoints.push(cache_provider);
        write_package(&bundled, &plugin);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_capabilities(
                &connection,
                "runtime.plugin",
                [
                    SourceCapability::NetworkAny,
                    SourceCapability::CacheReadWrite,
                ],
                true,
            )
            .expect("capabilities should be reviewed");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        let network_grants = runtime
            .granted_capabilities_for(&network_provider_id)
            .expect("network provider grants should load");
        let cache_grants = runtime
            .granted_capabilities_for(&cache_provider_id)
            .expect("cache provider grants should load");

        assert_eq!(
            network_grants,
            BTreeSet::from([SourceCapability::NetworkAny])
        );
        assert_eq!(
            cache_grants,
            BTreeSet::from([SourceCapability::CacheReadWrite])
        );
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn enable_should_restore_disabled_state_when_persistence_fails() {
        let root = temp_dir("enable-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("runtime.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_enable_update
                 BEFORE UPDATE ON plugin_states
                 WHEN NEW.enabled != OLD.enabled
                 BEGIN
                     SELECT RAISE(ABORT, 'forced enable update failure');
                 END;",
            )
            .expect("enable failure trigger should be created");

        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect_err("database failure should reject enable");
        let record = registry
            .record("runtime.plugin")
            .expect("plugin should remain registered");

        assert!(!record.enabled);
        assert_eq!(record.state, PluginState::Disabled);
        assert!(record
            .providers
            .iter()
            .all(|provider| !provider.initialized));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn disable_should_restore_enabled_state_when_persistence_fails() {
        let root = temp_dir("disable-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("runtime.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_disable_update
                 BEFORE UPDATE ON plugin_states
                 WHEN NEW.enabled != OLD.enabled
                 BEGIN
                     SELECT RAISE(ABORT, 'forced disable update failure');
                 END;",
            )
            .expect("disable failure trigger should be created");

        registry
            .set_enabled(&connection, "runtime.plugin", false)
            .expect_err("database failure should reject disable");
        let record = registry
            .record("runtime.plugin")
            .expect("plugin should remain registered");
        let outcome = registry
            .dispatch_request(
                &connection,
                "runtime.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "rollback".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("restored provider should remain dispatchable");

        assert!(record.enabled);
        assert_eq!(record.state, PluginState::Enabled);
        assert!(matches!(outcome.response, SourceResponse::MusicSearch(_)));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn disable_should_restore_runtime_when_cleanup_fails_after_partial_teardown() {
        let root = temp_dir("disable-runtime-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        write_package(
            &bundled,
            &manifest("runtime.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_enabled(&connection, "runtime.plugin", true)
            .expect("plugin should enable");

        let error = registry
            .deactivate_plugin_with(&connection, "runtime.plugin", |runtime, previous| {
                let provider_id = previous
                    .providers
                    .keys()
                    .next()
                    .expect("enabled plugin should have a provider");
                runtime
                    .uninitialize_provider(provider_id)
                    .expect("partial teardown should uninitialize the provider");
                runtime
                    .clear_provider_granted_capabilities(provider_id)
                    .expect("partial teardown should clear provider grants");
                Err(PluginSystemError::Package(
                    "forced runtime cleanup failure".to_owned(),
                ))
            })
            .expect_err("runtime cleanup failure should reject disable");
        let record = registry
            .record("runtime.plugin")
            .expect("plugin should remain registered");
        let persisted = load_persisted_states(&connection).expect("state should load");
        let outcome = registry
            .dispatch_request(
                &connection,
                "runtime.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "rollback".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("restored provider should remain dispatchable");

        assert!(error.to_string().contains("forced runtime cleanup failure"));
        assert!(record.enabled);
        assert!(persisted["runtime.plugin"].enabled);
        assert!(matches!(outcome.response, SourceResponse::MusicSearch(_)));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn failed_provider_initialization_should_clear_its_capability_policy() {
        let root = temp_dir("activation-cleanup");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let mut plugin = manifest(
            "broken.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        let provider_id = plugin.provider_entrypoints[0].id.clone();
        plugin.provider_entrypoints[0].source_catalog.insert(
            "broken-source".to_owned(),
            source_runtime::lx_music_source(
                "broken-source",
                "Broken Source",
                vec![SourceAction::MusicSearch, SourceAction::MusicSearch],
                Vec::new(),
            ),
        );
        write_package(&bundled, &plugin);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .set_capabilities(
                &connection,
                "broken.plugin",
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("capabilities should be reviewed");

        registry
            .set_enabled(&connection, "broken.plugin", true)
            .expect_err("invalid provider catalog should fail initialization");
        let remaining_grants = runtime
            .granted_capabilities_for(&provider_id)
            .expect("provider grants should remain readable");

        assert!(!remaining_grants.contains(&SourceCapability::NetworkAny));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn incompatible_manifest_is_visible_but_cannot_enable() {
        let root = temp_dir("compatibility");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let mut plugin = manifest(
            "future.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        plugin.supported_api_version = SourceRuntimeApiVersion::new(2, 0);
        write_package(&bundled, &plugin);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, runtime);
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        let reviewed = registry
            .set_capabilities(
                &connection,
                "future.plugin",
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("incompatible plugin permissions can be inspected");
        assert_eq!(reviewed.state, PluginState::Incompatible);
        let error = registry
            .set_enabled(&connection, "future.plugin", true)
            .expect_err("incompatible plugin should not enable");

        assert!(matches!(error, PluginSystemError::InvalidState(_, _)));
        assert_eq!(
            registry.record("future.plugin").unwrap().state,
            PluginState::Incompatible
        );
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn duplicate_provider_ids_are_rejected_across_packages() {
        let root = temp_dir("provider-collision");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let first = manifest("first.plugin", "builtin:runtime-demo", &[]);
        let mut second = manifest("second.plugin", "builtin:runtime-demo", &[]);
        second.provider_entrypoints[0].id = first.provider_entrypoints[0].id.clone();
        write_package(&bundled, &first);
        write_package(&user, &second);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, runtime);
        let records = registry
            .refresh(&connection)
            .expect("registry should refresh");

        assert_eq!(
            records
                .iter()
                .filter(|record| record.state == PluginState::Invalid)
                .count(),
            1
        );
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn install_copies_user_package_and_remove_deletes_it() {
        let root = temp_dir("install-remove");
        let bundled = root.join("bundled");
        let user = root.join("user");
        let source_root = root.join("source");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        fs::create_dir_all(&source_root).expect("source directory should be created");
        let plugin = manifest(
            "user.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        );
        let package = write_package(&source_root, &plugin);
        fs::write(package.join("asset.txt"), b"package asset")
            .expect("package asset should be written");

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, runtime);
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        let installed = registry
            .install(&connection, &package)
            .expect("package should install");
        assert_eq!(installed.origin, PluginOrigin::User);
        assert_eq!(installed.state, PluginState::NeedsReview);
        assert!(Path::new(&installed.path).join("asset.txt").is_file());

        registry
            .remove(&connection, &installed.id)
            .expect("user package should be removable");
        assert!(!Path::new(&installed.path).exists());
        assert!(registry.record(&installed.id).is_none());
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn remove_should_restore_package_and_runtime_when_database_delete_fails() {
        let root = temp_dir("remove-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        let source_root = root.join("source");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        fs::create_dir_all(&source_root).expect("source directory should be created");
        let package = write_package(
            &source_root,
            &manifest("user.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .install(&connection, &package)
            .expect("package should install");
        registry
            .set_enabled(&connection, "user.plugin", true)
            .expect("plugin should enable");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_plugin_delete
                 BEFORE DELETE ON plugin_states
                 BEGIN
                     SELECT RAISE(ABORT, 'forced plugin delete failure');
                 END;",
            )
            .expect("delete failure trigger should be created");

        registry
            .remove(&connection, "user.plugin")
            .expect_err("database failure should reject removal");
        let record = registry
            .record("user.plugin")
            .expect("plugin should remain registered");
        let outcome = registry
            .dispatch_request(
                &connection,
                "user.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "rollback".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("restored provider should remain dispatchable");

        assert!(Path::new(&record.path).is_dir());
        assert!(record.enabled);
        assert!(matches!(outcome.response, SourceResponse::MusicSearch(_)));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn remove_should_restore_package_database_and_runtime_when_cleanup_fails() {
        let root = temp_dir("remove-cleanup-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        let source_root = root.join("source");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        fs::create_dir_all(&source_root).expect("source directory should be created");
        let package = write_package(
            &source_root,
            &manifest("user.plugin", "builtin:runtime-demo", &[]),
        );

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        registry
            .install(&connection, &package)
            .expect("package should install");
        registry
            .set_enabled(&connection, "user.plugin", true)
            .expect("plugin should enable");

        let error = registry
            .remove_with_package_cleanup(&connection, "user.plugin", |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "forced quarantine cleanup failure",
                ))
            })
            .expect_err("cleanup failure should reject removal");
        let record = registry
            .record("user.plugin")
            .expect("plugin should be restored");
        let persisted = load_persisted_states(&connection).expect("state should load");
        let outcome = registry
            .dispatch_request(
                &connection,
                "user.plugin",
                SourceRequest::MusicSearch {
                    source: source_runtime::LX_SOURCE_WY.to_owned(),
                    keyword: "rollback".to_owned(),
                    page: 1,
                    page_size: 10,
                },
                source_runtime::SourceCancellationToken::default(),
            )
            .expect("restored provider should remain dispatchable");

        assert!(error
            .to_string()
            .contains("forced quarantine cleanup failure"));
        assert!(Path::new(&record.path).is_dir());
        assert!(record.enabled);
        assert!(persisted["user.plugin"].enabled);
        assert!(matches!(outcome.response, SourceResponse::MusicSearch(_)));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn install_should_reject_a_source_containing_the_installation_workspace() {
        let root = temp_dir("install-containment");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let plugin = manifest("user.plugin", "builtin:runtime-demo", &[]);
        fs::write(
            user.join(PLUGIN_MANIFEST_FILE),
            serde_json::to_vec_pretty(&plugin).expect("manifest should serialize"),
        )
        .expect("manifest should be written");

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        let error = registry
            .install(&connection, &user)
            .expect_err("source containing staging should be rejected");

        assert!(error
            .to_string()
            .contains("installation workspace cannot be inside the source package"));
        assert!(!user.join(".install-staging").exists());
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn install_should_reject_a_source_inside_the_managed_plugin_directory() {
        let root = temp_dir("install-managed-source");
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        let mut plugin = manifest("user.plugin", "builtin:runtime-demo", &[]);
        plugin.id = "different.destination".to_owned();
        let package = user.join("source-copy");
        fs::create_dir_all(&package).expect("source package should be created");
        fs::write(
            package.join(PLUGIN_MANIFEST_FILE),
            serde_json::to_vec_pretty(&plugin).expect("manifest should serialize"),
        )
        .expect("manifest should be written");

        let connection = database();
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("registry should refresh");
        let error = registry
            .install(&connection, &package)
            .expect_err("managed source package should be rejected");

        assert!(error
            .to_string()
            .contains("source package cannot be inside the managed Plugin directory"));
        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn manifest_fingerprint_should_be_a_sha256_digest() {
        let fingerprint = manifest_fingerprint(&manifest(
            "runtime.plugin",
            "builtin:runtime-demo",
            &[SourceCapability::NetworkAny],
        ))
        .expect("manifest should serialize");

        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn install_should_restore_previous_package_when_database_update_fails() {
        let root = temp_dir("install-rollback");
        let bundled = root.join("bundled");
        let user = root.join("user");
        let source_root = root.join("source");
        fs::create_dir_all(&bundled).expect("bundled directory should be created");
        fs::create_dir_all(&user).expect("user directory should be created");
        fs::create_dir_all(&source_root).expect("source directory should be created");
        let existing = manifest("user.plugin", "builtin:runtime-demo", &[]);
        write_package(&user, &existing);
        let mut replacement = existing.clone();
        replacement.version = "2.0.0".to_owned();
        let replacement_package = write_package(&source_root, &replacement);

        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = PluginRegistry::new(&user, &bundled, runtime);
        registry
            .refresh(&connection)
            .expect("registry should refresh before replacement");
        registry
            .set_enabled(&connection, "user.plugin", true)
            .expect("existing Plugin should enable before replacement");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_plugin_manifest_update
                 BEFORE UPDATE ON plugin_states
                 WHEN NEW.manifest_fingerprint != OLD.manifest_fingerprint
                 BEGIN
                     SELECT RAISE(ABORT, 'forced manifest update failure');
                 END;",
            )
            .expect("manifest update failure trigger should be created");

        registry
            .install(&connection, &replacement_package)
            .expect_err("database failure should fail installation");
        let restored =
            read_manifest(&user.join("user.plugin")).expect("previous package should be restored");
        let restored_record = registry
            .record("user.plugin")
            .expect("restored Plugin should remain registered");

        assert_eq!(
            (
                restored.version.as_str(),
                restored_record.version.as_deref(),
                restored_record.enabled,
            ),
            ("1.0.0", Some("1.0.0"), true)
        );
        fs::remove_dir_all(root).expect("test directory should be removed");
    }
}
