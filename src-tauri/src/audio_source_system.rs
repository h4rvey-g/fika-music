use crate::lx_js_importer::{self, LxJsImportAdapter, LxJsImportReport};
use crate::lx_js_runtime::ImportedLxJsProvider;
use crate::lx_v8_sidecar::{ImportedLxV8Provider, LxV8Sidecar};
use crate::plugin_system::PluginManifest;
use crate::registry_support::{
    manifest_fingerprint, now_timestamp, operation_nonce, remove_path, sha256_hex, valid_identifier,
};
use crate::source_runtime::{
    self, DiagnosticLevel, SourceAction, SourceCapability, SourceInfo, SourceProvider,
    SourceQuality, SourceRequest, SourceRequestOutcome, SourceRuntime, SourceRuntimeApiVersion,
};
use percent_encoding::percent_decode_str;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::redirect::Policy;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub const AUDIO_SOURCE_MANIFEST_FILE: &str = "audio-source.json";
pub const AUDIO_SOURCE_MANIFEST_VERSION: u32 = 1;
const AUDIO_SOURCE_FILE: &str = "source.js";
const AUDIO_SOURCE_REPORT_FILE: &str = "import-report.json";
const LEGACY_IMPORTED_LX_ENTRYPOINT_PREFIX: &str = "builtin:lx-js:";
const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REMOTE_SOURCE_URL_BYTES: usize = 4_096;
const MAX_REMOTE_SOURCE_REDIRECTS: usize = 5;
const REMOTE_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const REMOTE_SOURCE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REMOTE_SOURCE_USER_AGENT: &str = "FikaMusic/0.1 audio-source-importer";
const MAX_DIAGNOSTICS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioSourceManifest {
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
    pub provider_id: String,
    pub adapter: String,
    pub source_fingerprint: String,
    #[serde(default)]
    pub capabilities: BTreeSet<SourceCapability>,
    pub supported_api_version: SourceRuntimeApiVersion,
    pub source_catalog: BTreeMap<String, SourceInfo>,
}

impl AudioSourceManifest {
    fn validate(&self, runtime_api_version: SourceRuntimeApiVersion) -> Result<(), String> {
        self.validate_with_adapter(runtime_api_version, |adapter| {
            LxJsImportAdapter::parse(adapter).is_some()
        })
    }

    fn validate_bundled(&self, runtime_api_version: SourceRuntimeApiVersion) -> Result<(), String> {
        self.validate_with_adapter(runtime_api_version, |adapter| {
            adapter.starts_with("builtin:")
                && valid_identifier(adapter.trim_start_matches("builtin:"))
        })
    }

    fn validate_with_adapter(
        &self,
        runtime_api_version: SourceRuntimeApiVersion,
        adapter_is_supported: impl FnOnce(&str) -> bool,
    ) -> Result<(), String> {
        let mut errors = Vec::new();
        if self.manifest_version != AUDIO_SOURCE_MANIFEST_VERSION {
            errors.push(format!(
                "manifest version {} is not supported (expected {})",
                self.manifest_version, AUDIO_SOURCE_MANIFEST_VERSION
            ));
        }
        if !valid_identifier(&self.id) {
            errors.push("id must contain only letters, numbers, '.', '_' or '-'".to_owned());
        }
        if !valid_identifier(&self.provider_id) {
            errors.push("providerId must be a valid identifier".to_owned());
        }
        if self.name.trim().is_empty() {
            errors.push("name must not be empty".to_owned());
        }
        if semver::Version::parse(&self.version).is_err() {
            errors.push(format!("version is not valid semver: {}", self.version));
        }
        if !adapter_is_supported(&self.adapter) {
            errors.push(format!(
                "unsupported audio source adapter: {}",
                self.adapter
            ));
        }
        if self.source_fingerprint.len() != 64
            || !self
                .source_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            errors.push("sourceFingerprint must be a hexadecimal SHA-256 digest".to_owned());
        }
        if !self
            .supported_api_version
            .is_compatible_with(runtime_api_version)
        {
            errors.push(format!(
                "audio source supports Source Runtime API {}, but the runtime is {}",
                self.supported_api_version, runtime_api_version
            ));
        }
        if self.source_catalog.is_empty() {
            errors.push("sourceCatalog must contain at least one source".to_owned());
        }
        for (source_id, source) in &self.source_catalog {
            if source_id != &source.id {
                errors.push(format!(
                    "source catalog key {source_id} does not match source id {}",
                    source.id
                ));
            }
            if source.actions.is_empty()
                || source
                    .actions
                    .iter()
                    .any(|action| *action != SourceAction::MusicUrl)
            {
                errors.push(format!(
                    "audio source {source_id} may expose only the musicUrl action"
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[derive(Debug, Clone)]
pub struct BundledAudioSourceBuildContext {
    pub audio_source_id: String,
    pub provider_id: String,
    pub declared_capabilities: BTreeSet<SourceCapability>,
    pub source_catalog: BTreeMap<String, SourceInfo>,
}

type BundledProviderFactory =
    dyn Fn(BundledAudioSourceBuildContext) -> Result<Arc<dyn SourceProvider>, String> + Send + Sync;

#[derive(Clone)]
pub struct BundledAudioSourceRegistration {
    manifest: AudioSourceManifest,
    factory: Arc<BundledProviderFactory>,
}

impl std::fmt::Debug for BundledAudioSourceRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BundledAudioSourceRegistration")
            .field("audio_source_id", &self.manifest.id)
            .field("provider_id", &self.manifest.provider_id)
            .field("adapter", &self.manifest.adapter)
            .finish_non_exhaustive()
    }
}

impl BundledAudioSourceRegistration {
    pub fn new<F>(manifest: AudioSourceManifest, factory: F) -> Self
    where
        F: Fn(BundledAudioSourceBuildContext) -> Result<Arc<dyn SourceProvider>, String>
            + Send
            + Sync
            + 'static,
    {
        Self {
            manifest,
            factory: Arc::new(factory),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export_to = "bindings.ts")]
pub enum AudioSourceState {
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
pub struct AudioSourceDiagnostic {
    pub code: String,
    pub level: DiagnosticLevel,
    pub source_id: Option<String>,
    pub message: String,
    pub timestamp: i64,
}

impl AudioSourceDiagnostic {
    fn info(
        code: impl Into<String>,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(code, DiagnosticLevel::Info, source_id, message)
    }

    fn warning(
        code: impl Into<String>,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(code, DiagnosticLevel::Warn, source_id, message)
    }

    fn error(
        code: impl Into<String>,
        source_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(code, DiagnosticLevel::Error, source_id, message)
    }

    fn security(source_id: Option<String>, message: impl Into<String>) -> Self {
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

    fn from_runtime(diagnostic: &source_runtime::SourceDiagnostic) -> Self {
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
pub struct AudioSourceRecord {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub path: String,
    pub adapter: Option<String>,
    pub state: AudioSourceState,
    pub enabled: bool,
    pub permissions_reviewed: bool,
    pub declared_capabilities: BTreeSet<SourceCapability>,
    pub granted_capabilities: BTreeSet<SourceCapability>,
    pub sources: Vec<SourceInfo>,
    pub diagnostics: Vec<AudioSourceDiagnostic>,
    pub can_remove: bool,
    pub can_enable: bool,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct AudioSourceCommandError {
    pub message: String,
    pub diagnostics: Vec<AudioSourceDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct AudioSourceAvailability {
    pub audio_source_id: String,
    pub source_id: String,
    pub source_name: String,
    pub quality: SourceQuality,
    pub available: bool,
    pub latency_ms: u64,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioSourceSystemError {
    #[error("audio source filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("audio source manifest error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("audio source database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid audio source manifest: {0}")]
    InvalidManifest(String),
    #[error("audio source {0} was not found")]
    NotFound(String),
    #[error("audio source {0} has an invalid lifecycle state: {1}")]
    InvalidState(String, String),
    #[error("audio source {audio_source_id} does not declare capability {capability}")]
    InvalidCapability {
        audio_source_id: String,
        capability: String,
    },
    #[error("audio source {audio_source_id} failed to load: {message}")]
    ProviderLoad {
        audio_source_id: String,
        message: String,
    },
    #[error("audio source {audio_source_id} runtime error: {message}")]
    Runtime {
        audio_source_id: String,
        message: String,
        diagnostics: Vec<AudioSourceDiagnostic>,
    },
    #[error("audio source package is invalid: {0}")]
    Package(String),
}

impl AudioSourceSystemError {
    pub fn diagnostics(&self) -> Vec<AudioSourceDiagnostic> {
        match self {
            Self::Runtime { diagnostics, .. } => diagnostics.clone(),
            Self::InvalidManifest(message) | Self::Package(message) => {
                vec![AudioSourceDiagnostic::error(
                    "manifest",
                    None,
                    message.clone(),
                )]
            }
            Self::ProviderLoad {
                audio_source_id,
                message,
            } => vec![AudioSourceDiagnostic::error(
                "load-error",
                Some(audio_source_id.clone()),
                message.clone(),
            )],
            _ => Vec::new(),
        }
    }
}

impl From<AudioSourceSystemError> for AudioSourceCommandError {
    fn from(error: AudioSourceSystemError) -> Self {
        Self {
            message: error.to_string(),
            diagnostics: error.diagnostics(),
        }
    }
}

#[derive(Debug, Clone)]
struct PersistedAudioSourceState {
    manifest_fingerprint: String,
    enabled: bool,
    permissions_reviewed: bool,
    granted_capabilities: BTreeSet<SourceCapability>,
    diagnostics: Vec<AudioSourceDiagnostic>,
}

#[derive(Clone)]
struct AudioSourceEntryRuntime {
    manifest: Option<AudioSourceManifest>,
    record: AudioSourceRecord,
    provider: Option<Arc<dyn SourceProvider>>,
    origin: AudioSourceOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioSourceOrigin {
    Bundled,
    Imported,
}

#[derive(Clone)]
pub(crate) struct PreparedAudioSourceRequest {
    audio_source_id: String,
    provider_id: String,
    provider: Arc<dyn SourceProvider>,
    runtime: Arc<SourceRuntime>,
}

impl PreparedAudioSourceRequest {
    pub(crate) fn execute(
        &self,
        request: SourceRequest,
        cancellation: source_runtime::SourceCancellationToken,
    ) -> Result<SourceRequestOutcome, AudioSourceSystemError> {
        self.runtime
            .dispatch_request_with_cancellation(self.provider.as_ref(), request, cancellation)
            .map_err(|error| runtime_error(&self.audio_source_id, error))
    }
}

#[derive(Debug)]
pub(crate) struct PreparedAudioSourceImport {
    manifest: AudioSourceManifest,
    source: String,
    report: LxJsImportReport,
    provenance: AudioSourceImportProvenance,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioSourceImportProvenance {
    kind: String,
    source_file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedAudioSourceImportReport {
    #[serde(flatten)]
    analysis: LxJsImportReport,
    provenance: AudioSourceImportProvenance,
}

#[derive(Debug)]
struct DownloadedAudioSource {
    source_file_name: String,
    source: String,
    requested_url: Url,
    final_url: Url,
}

pub struct AudioSourceRegistry {
    audio_sources_dir: PathBuf,
    runtime: Arc<SourceRuntime>,
    v8_sidecar: Option<Arc<LxV8Sidecar>>,
    bundled_sources: BTreeMap<String, BundledAudioSourceRegistration>,
    entries: BTreeMap<String, AudioSourceEntryRuntime>,
}

impl std::fmt::Debug for AudioSourceRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioSourceRegistry")
            .field("audio_sources_dir", &self.audio_sources_dir)
            .field("bundled_source_count", &self.bundled_sources.len())
            .field("source_count", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl AudioSourceRegistry {
    pub fn new(audio_sources_dir: impl Into<PathBuf>, runtime: Arc<SourceRuntime>) -> Self {
        Self {
            audio_sources_dir: audio_sources_dir.into(),
            runtime,
            v8_sidecar: None,
            bundled_sources: BTreeMap::new(),
            entries: BTreeMap::new(),
        }
    }

    pub(crate) fn with_v8_sidecar(mut self, sidecar: Arc<LxV8Sidecar>) -> Self {
        self.v8_sidecar = Some(sidecar);
        self
    }

    pub fn with_bundled_source(
        mut self,
        registration: BundledAudioSourceRegistration,
    ) -> Result<Self, AudioSourceSystemError> {
        registration
            .manifest
            .validate_bundled(self.runtime.api_version())
            .map_err(AudioSourceSystemError::InvalidManifest)?;
        if self
            .bundled_sources
            .values()
            .any(|existing| existing.manifest.provider_id == registration.manifest.provider_id)
        {
            return Err(AudioSourceSystemError::InvalidManifest(format!(
                "bundled Provider id is already registered: {}",
                registration.manifest.provider_id
            )));
        }
        let audio_source_id = registration.manifest.id.clone();
        if self
            .bundled_sources
            .insert(audio_source_id.clone(), registration)
            .is_some()
        {
            return Err(AudioSourceSystemError::InvalidManifest(format!(
                "bundled audio source id is already registered: {audio_source_id}"
            )));
        }
        Ok(self)
    }

    pub fn records(&self) -> Vec<AudioSourceRecord> {
        self.entries
            .values()
            .map(|entry| entry.record.clone())
            .collect()
    }

    pub fn record(&self, audio_source_id: &str) -> Option<AudioSourceRecord> {
        self.entries
            .get(audio_source_id)
            .map(|entry| entry.record.clone())
    }

    pub fn refresh(
        &mut self,
        connection: &Connection,
    ) -> Result<Vec<AudioSourceRecord>, AudioSourceSystemError> {
        const SAVEPOINT: &str = "fika_audio_source_refresh";

        connection.execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))?;
        let previous = std::mem::take(&mut self.entries);
        if let Err(error) = clear_runtime_entries(&self.runtime, &previous) {
            let _ = connection.execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {SAVEPOINT}; RELEASE SAVEPOINT {SAVEPOINT}"
            ));
            self.entries = previous;
            return match restore_runtime_entries(&self.runtime, &self.entries) {
                Ok(()) => Err(error),
                Err(restore_error) => {
                    Err(restoration_error("refresh cleanup", error, restore_error))
                }
            };
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
                        error.into(),
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
    ) -> Result<Vec<AudioSourceRecord>, AudioSourceSystemError> {
        let persisted = load_persisted_states(connection)?;
        let mut seen_ids = BTreeSet::new();
        let mut seen_provider_ids = BTreeSet::new();
        for registration in self.bundled_sources.values() {
            let manifest = registration.manifest.clone();
            seen_ids.insert(manifest.id.clone());
            seen_provider_ids.insert(manifest.provider_id.clone());
            let path = PathBuf::from(format!("builtin:{}", manifest.id));
            let record = record_for_manifest(
                &manifest,
                &path,
                persisted.get(&manifest.id),
                self.runtime.api_version(),
                false,
            )?;
            self.entries.insert(
                manifest.id.clone(),
                AudioSourceEntryRuntime {
                    manifest: Some(manifest),
                    record,
                    provider: None,
                    origin: AudioSourceOrigin::Bundled,
                },
            );
        }
        for path in discover_audio_source_paths(&self.audio_sources_dir)? {
            let mut manifest = match read_manifest(&path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    self.insert_invalid_record(path, error.to_string());
                    continue;
                }
            };
            if let Err(message) = manifest.validate(self.runtime.api_version()) {
                self.insert_invalid_record(path, message);
                continue;
            }
            if !seen_ids.insert(manifest.id.clone()) {
                self.insert_invalid_record(
                    path,
                    format!("duplicate audio source id discovered: {}", manifest.id),
                );
                continue;
            }
            if !seen_provider_ids.insert(manifest.provider_id.clone()) {
                self.insert_invalid_record(
                    path,
                    format!(
                        "duplicate audio source Provider id discovered: {}",
                        manifest.provider_id
                    ),
                );
                continue;
            }
            if let Err(error) = upgrade_legacy_execution_manifest(&path, &mut manifest) {
                self.insert_invalid_record(path, error.to_string());
                continue;
            }
            let record = record_for_manifest(
                &manifest,
                &path,
                persisted.get(&manifest.id),
                self.runtime.api_version(),
                true,
            )?;
            self.entries.insert(
                manifest.id.clone(),
                AudioSourceEntryRuntime {
                    manifest: Some(manifest),
                    record,
                    provider: None,
                    origin: AudioSourceOrigin::Imported,
                },
            );
        }

        let requested_enabled = self
            .entries
            .iter()
            .filter_map(|(id, entry)| entry.record.enabled.then_some(id.clone()))
            .collect::<Vec<_>>();
        for audio_source_id in requested_enabled {
            if let Err(error) = self.activate(connection, &audio_source_id) {
                if matches!(
                    error,
                    AudioSourceSystemError::Database(_)
                        | AudioSourceSystemError::Io(_)
                        | AudioSourceSystemError::Json(_)
                ) {
                    return Err(error);
                }
            }
        }
        for entry in self.entries.values() {
            if entry.manifest.is_some() {
                persist_record(connection, &entry.record, entry.manifest.as_ref())?;
            }
        }
        Ok(self.records())
    }

    fn restore_failed_refresh(
        &mut self,
        connection: &Connection,
        savepoint: &str,
        previous: BTreeMap<String, AudioSourceEntryRuntime>,
        refresh_error: AudioSourceSystemError,
    ) -> AudioSourceSystemError {
        let cleanup_error = clear_runtime_entries(&self.runtime, &self.entries).err();
        let rollback_error = connection
            .execute_batch(&format!(
                "ROLLBACK TO SAVEPOINT {savepoint}; RELEASE SAVEPOINT {savepoint}"
            ))
            .err()
            .map(AudioSourceSystemError::Database);
        self.entries = previous;
        let restore_error = restore_runtime_entries(&self.runtime, &self.entries).err();

        if cleanup_error.is_none() && rollback_error.is_none() && restore_error.is_none() {
            return refresh_error;
        }

        let mut failures = vec![format!("refresh failed: {refresh_error}")];
        if let Some(error) = cleanup_error {
            failures.push(format!("new runtime cleanup failed: {error}"));
        }
        if let Some(error) = rollback_error {
            failures.push(format!("database rollback failed: {error}"));
        }
        if let Some(error) = restore_error {
            failures.push(format!("previous runtime restoration failed: {error}"));
        }
        AudioSourceSystemError::Package(failures.join("; "))
    }

    pub fn import_file(
        &mut self,
        connection: &Connection,
        source_path: &Path,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        let (manifest, source, report) = prepare_import(source_path, self.v8_sidecar.is_some())?;
        let source_file_name = source_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| AUDIO_SOURCE_FILE.to_owned());
        self.install_prepared(
            connection,
            PreparedAudioSourceImport {
                manifest,
                source,
                report,
                provenance: AudioSourceImportProvenance {
                    kind: "local-file".to_owned(),
                    source_file_name,
                    requested_url: None,
                    final_url: None,
                },
            },
        )
    }

    pub(crate) fn install_prepared(
        &mut self,
        connection: &Connection,
        prepared: PreparedAudioSourceImport,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        let PreparedAudioSourceImport {
            manifest,
            source,
            report,
            provenance,
        } = prepared;
        manifest
            .validate(self.runtime.api_version())
            .map_err(AudioSourceSystemError::InvalidManifest)?;
        if self.bundled_sources.contains_key(&manifest.id)
            || self
                .bundled_sources
                .values()
                .any(|registration| registration.manifest.provider_id == manifest.provider_id)
        {
            return Err(AudioSourceSystemError::InvalidManifest(
                "audio source id or Provider id is reserved by a bundled Audio Source".to_owned(),
            ));
        }

        fs::create_dir_all(&self.audio_sources_dir)?;
        let root = fs::canonicalize(&self.audio_sources_dir)?;
        let destination = root.join(&manifest.id);
        let staging_root = root.join(".install-staging");
        fs::create_dir_all(&staging_root)?;
        let nonce = operation_nonce();
        let staged = staging_root.join(format!("{}.install-{nonce}", manifest.id));
        let backup = staging_root.join(format!("{}.backup-{nonce}", manifest.id));
        fs::create_dir_all(&staged)?;
        fs::write(
            staged.join(AUDIO_SOURCE_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        fs::write(staged.join(AUDIO_SOURCE_FILE), source.as_bytes())?;
        fs::write(
            staged.join(AUDIO_SOURCE_REPORT_FILE),
            serde_json::to_vec_pretty(&ManagedAudioSourceImportReport {
                analysis: report,
                provenance,
            })?,
        )?;

        let had_previous = destination.exists();
        if had_previous {
            fs::rename(&destination, &backup)?;
        }
        if let Err(error) = fs::rename(&staged, &destination) {
            if had_previous {
                let _ = fs::rename(&backup, &destination);
            }
            return Err(error.into());
        }

        let transaction = connection.unchecked_transaction()?;
        let installed = match self.refresh(&transaction).and_then(|_| {
            let installed = self
                .record(&manifest.id)
                .ok_or_else(|| AudioSourceSystemError::NotFound(manifest.id.clone()))?;
            // Importing is the trust decision: grant the source's declared capabilities,
            // while leaving activation as a separate user action.
            self.set_capabilities(
                &transaction,
                &manifest.id,
                installed.declared_capabilities,
                true,
            )
        }) {
            Ok(installed) => installed,
            Err(error) => {
                drop(transaction);
                let _ = remove_path(&destination);
                if had_previous {
                    let _ = fs::rename(&backup, &destination);
                }
                let _ = self.refresh(connection);
                return Err(error);
            }
        };
        if let Err(error) = transaction.commit() {
            let _ = remove_path(&destination);
            if had_previous {
                let _ = fs::rename(&backup, &destination);
            }
            let _ = self.refresh(connection);
            return Err(error.into());
        }
        if had_previous {
            remove_path(&backup)?;
        }
        let _ = fs::remove_dir(&staging_root);
        Ok(installed)
    }

    pub fn set_capabilities(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
        capabilities: impl IntoIterator<Item = SourceCapability>,
        reviewed: bool,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        let requested = capabilities.into_iter().collect::<BTreeSet<_>>();
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Err(AudioSourceSystemError::NotFound(audio_source_id.to_owned()));
        };
        if let Some(capability) = requested
            .difference(&entry.record.declared_capabilities)
            .next()
        {
            return Err(AudioSourceSystemError::InvalidCapability {
                audio_source_id: audio_source_id.to_owned(),
                capability: capability.as_str().to_owned(),
            });
        }
        if entry.record.enabled && !reviewed {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "an enabled audio source must remain permission-reviewed".to_owned(),
            ));
        }

        let mut candidate = entry.record.clone();
        if candidate.enabled {
            for capability in candidate
                .granted_capabilities
                .difference(&requested)
                .copied()
            {
                append_diagnostic(
                    &mut candidate.diagnostics,
                    AudioSourceDiagnostic::security(
                        Some(audio_source_id.to_owned()),
                        format!("capability revoked: {}", capability.as_str()),
                    ),
                );
            }
        }
        candidate.granted_capabilities = requested.clone();
        candidate.permissions_reviewed = reviewed;
        if !candidate.enabled {
            candidate.state = if !candidate.declared_capabilities.is_empty() && !reviewed {
                AudioSourceState::NeedsReview
            } else {
                AudioSourceState::Disabled
            };
        }
        update_action_flags(&mut candidate);

        let provider = entry.provider.clone();
        let previous_grants = provider.as_ref().map(|provider| {
            entry
                .record
                .granted_capabilities
                .intersection(&provider.required_capabilities())
                .copied()
                .collect::<BTreeSet<_>>()
        });
        if let Some(provider) = provider.as_ref() {
            let grants = requested
                .intersection(&provider.required_capabilities())
                .copied()
                .collect::<BTreeSet<_>>();
            self.runtime
                .replace_provider_granted_capabilities(provider.id().to_owned(), grants)
                .map_err(|error| runtime_error(audio_source_id, error))?;
        }
        if let Err(error) = persist_record(connection, &candidate, entry.manifest.as_ref()) {
            if let (Some(provider), Some(previous_grants)) = (provider.as_ref(), previous_grants) {
                if let Err(restore_error) = self
                    .runtime
                    .replace_provider_granted_capabilities(
                        provider.id().to_owned(),
                        previous_grants,
                    )
                    .map_err(|restore_error| runtime_error(audio_source_id, restore_error))
                {
                    return Err(restoration_error(
                        "permission persistence",
                        error,
                        restore_error,
                    ));
                }
            }
            return Err(error);
        }
        self.entries
            .get_mut(audio_source_id)
            .expect("audio source checked above")
            .record = candidate.clone();
        Ok(candidate)
    }

    pub fn set_enabled(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
        enabled: bool,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        if enabled {
            self.activate(connection, audio_source_id)
        } else {
            self.deactivate(connection, audio_source_id)
        }
    }

    pub fn remove(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
    ) -> Result<Vec<AudioSourceRecord>, AudioSourceSystemError> {
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Err(AudioSourceSystemError::NotFound(audio_source_id.to_owned()));
        };
        if !entry.record.can_remove {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "audio source cannot be removed".to_owned(),
            ));
        }
        let path = PathBuf::from(&entry.record.path);
        let root = fs::canonicalize(&self.audio_sources_dir)?;
        let path = fs::canonicalize(path)?;
        if path == root || !path.starts_with(&root) || !path.is_dir() {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "audio source path is outside the managed directory".to_owned(),
            ));
        }

        self.deactivate(connection, audio_source_id)?;
        let staging_root = root.join(".remove-staging");
        fs::create_dir_all(&staging_root)?;
        let staged = staging_root.join(format!("{audio_source_id}.remove-{}", operation_nonce()));
        fs::rename(&path, &staged)?;
        let transaction = connection.unchecked_transaction()?;
        if let Err(error) = delete_persisted_source(&transaction, audio_source_id) {
            drop(transaction);
            let _ = fs::rename(&staged, &path);
            let _ = self.activate(connection, audio_source_id);
            return Err(error);
        }
        if let Err(error) = transaction.commit() {
            let _ = fs::rename(&staged, &path);
            let _ = self.activate(connection, audio_source_id);
            return Err(error.into());
        }
        self.entries.remove(audio_source_id);
        remove_path(&staged)?;
        let _ = fs::remove_dir(&staging_root);
        Ok(self.records())
    }

    pub fn clear_diagnostics(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Err(AudioSourceSystemError::NotFound(audio_source_id.to_owned()));
        };
        let mut candidate = entry.record.clone();
        candidate.diagnostics.clear();
        persist_record(connection, &candidate, entry.manifest.as_ref())?;
        self.entries
            .get_mut(audio_source_id)
            .expect("audio source checked above")
            .record = candidate.clone();
        Ok(candidate)
    }

    pub(crate) fn prepare_dispatch(
        &self,
        audio_source_id: &str,
        request: &SourceRequest,
    ) -> Result<PreparedAudioSourceRequest, AudioSourceSystemError> {
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Err(AudioSourceSystemError::NotFound(audio_source_id.to_owned()));
        };
        if !entry.record.enabled {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "audio source must be enabled before dispatch".to_owned(),
            ));
        }
        if request.action() != SourceAction::MusicUrl {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "audio sources accept only musicUrl requests".to_owned(),
            ));
        }
        if !entry.record.sources.iter().any(|source| {
            source.id == request.source() && source.actions.contains(&SourceAction::MusicUrl)
        }) {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                format!("source {} is not configured", request.source()),
            ));
        }
        let provider = entry.provider.clone().ok_or_else(|| {
            AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "audio source Provider is not initialized".to_owned(),
            )
        })?;
        Ok(PreparedAudioSourceRequest {
            audio_source_id: audio_source_id.to_owned(),
            provider_id: provider.id().to_owned(),
            provider,
            runtime: Arc::clone(&self.runtime),
        })
    }

    pub(crate) fn complete_dispatch_best_effort(
        &mut self,
        connection: &Connection,
        dispatch: &PreparedAudioSourceRequest,
        result: &Result<SourceRequestOutcome, AudioSourceSystemError>,
    ) {
        let diagnostics = match result {
            Ok(outcome) => outcome
                .diagnostics
                .iter()
                .map(AudioSourceDiagnostic::from_runtime)
                .collect::<Vec<_>>(),
            Err(error) => error.diagnostics(),
        };
        if diagnostics.is_empty() {
            return;
        }
        let Some(entry) = self.entries.get(&dispatch.audio_source_id) else {
            return;
        };
        let is_current = entry
            .provider
            .as_ref()
            .is_some_and(|provider| Arc::ptr_eq(provider, &dispatch.provider));
        if !is_current {
            return;
        }
        let mut candidate = entry.record.clone();
        for diagnostic in diagnostics {
            append_diagnostic(&mut candidate.diagnostics, diagnostic);
        }
        if persist_record(connection, &candidate, entry.manifest.as_ref()).is_ok() {
            self.entries
                .get_mut(&dispatch.audio_source_id)
                .expect("audio source checked above")
                .record = candidate;
        } else if let Some(entry) = self.entries.get_mut(&dispatch.audio_source_id) {
            append_diagnostic(
                &mut entry.record.diagnostics,
                AudioSourceDiagnostic::warning(
                    "diagnostic-persistence",
                    Some(dispatch.provider_id.clone()),
                    "request completed, but diagnostics could not be persisted",
                ),
            );
        }
    }

    fn activate(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Err(AudioSourceSystemError::NotFound(audio_source_id.to_owned()));
        };
        if entry.record.enabled && entry.provider.is_some() {
            return Ok(entry.record.clone());
        }
        let manifest = entry.manifest.clone().ok_or_else(|| {
            AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "manifest is invalid".to_owned(),
            )
        })?;
        if entry.record.state == AudioSourceState::Incompatible {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "audio source is incompatible with the current Source Runtime".to_owned(),
            ));
        }
        if !entry.record.declared_capabilities.is_empty() && !entry.record.permissions_reviewed {
            return Err(AudioSourceSystemError::InvalidState(
                audio_source_id.to_owned(),
                "capabilities must be reviewed before enabling the audio source".to_owned(),
            ));
        }

        let origin = entry.origin;
        let package_path = PathBuf::from(&entry.record.path);
        let provider_result = match origin {
            AudioSourceOrigin::Bundled => self.build_bundled_provider(&manifest),
            AudioSourceOrigin::Imported => build_imported_provider(
                &manifest,
                &package_path,
                self.v8_sidecar.as_ref().map(Arc::clone),
            ),
        };
        let provider = match provider_result {
            Ok(provider) => provider,
            Err(error) => {
                self.activation_failed(connection, audio_source_id, &error)?;
                return Err(error);
            }
        };
        let provider_id = provider.id().to_owned();
        let grants = entry
            .record
            .granted_capabilities
            .intersection(&provider.required_capabilities())
            .copied()
            .collect::<BTreeSet<_>>();
        if let Err(error) = self
            .runtime
            .replace_provider_granted_capabilities(provider_id.clone(), grants)
        {
            let error = runtime_error(audio_source_id, error);
            self.activation_failed(connection, audio_source_id, &error)?;
            return Err(error);
        }
        let report = match self.runtime.initialize_provider(provider.as_ref()) {
            Ok(report) => report,
            Err(error) => {
                let _ = self
                    .runtime
                    .clear_provider_granted_capabilities(&provider_id);
                let error = runtime_error(audio_source_id, error);
                self.activation_failed(connection, audio_source_id, &error)?;
                return Err(error);
            }
        };
        if report.sources != manifest.source_catalog {
            let _ = self.runtime.uninitialize_provider(&provider_id);
            let _ = self
                .runtime
                .clear_provider_granted_capabilities(&provider_id);
            let error = AudioSourceSystemError::ProviderLoad {
                audio_source_id: audio_source_id.to_owned(),
                message: "Provider source catalog does not match the bundled registration"
                    .to_owned(),
            };
            self.activation_failed(connection, audio_source_id, &error)?;
            return Err(error);
        }

        let mut candidate = entry.record.clone();
        candidate.enabled = true;
        candidate.state = AudioSourceState::Enabled;
        candidate.sources = report.sources.values().cloned().collect();
        for diagnostic in &report.diagnostics {
            append_diagnostic(
                &mut candidate.diagnostics,
                AudioSourceDiagnostic::from_runtime(diagnostic),
            );
        }
        append_diagnostic(
            &mut candidate.diagnostics,
            AudioSourceDiagnostic::info(
                "lifecycle",
                Some(audio_source_id.to_owned()),
                "audio source initialized",
            ),
        );
        update_action_flags(&mut candidate);
        if let Err(error) = persist_record(connection, &candidate, Some(&manifest)) {
            let _ = self.runtime.uninitialize_provider(&provider_id);
            let _ = self
                .runtime
                .clear_provider_granted_capabilities(&provider_id);
            return Err(error);
        }
        let entry = self
            .entries
            .get_mut(audio_source_id)
            .expect("audio source checked above");
        entry.provider = Some(provider);
        entry.record = candidate.clone();
        Ok(candidate)
    }

    fn build_bundled_provider(
        &self,
        manifest: &AudioSourceManifest,
    ) -> Result<Arc<dyn SourceProvider>, AudioSourceSystemError> {
        let registration = self.bundled_sources.get(&manifest.id).ok_or_else(|| {
            AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: "bundled Audio Source registration is unavailable".to_owned(),
            }
        })?;
        if registration.manifest != *manifest {
            return Err(AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: "bundled Audio Source manifest does not match its registration".to_owned(),
            });
        }
        let context = BundledAudioSourceBuildContext {
            audio_source_id: manifest.id.clone(),
            provider_id: manifest.provider_id.clone(),
            declared_capabilities: manifest.capabilities.clone(),
            source_catalog: manifest.source_catalog.clone(),
        };
        let provider = catch_unwind(AssertUnwindSafe(|| (registration.factory)(context)))
            .map_err(|_| AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: "bundled Provider factory panicked".to_owned(),
            })?
            .map_err(|message| AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message,
            })?;
        let provider_id =
            catch_unwind(AssertUnwindSafe(|| provider.id().to_owned())).map_err(|_| {
                AudioSourceSystemError::ProviderLoad {
                    audio_source_id: manifest.id.clone(),
                    message: "reading the bundled Provider ID panicked".to_owned(),
                }
            })?;
        if provider_id != manifest.provider_id {
            return Err(AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: format!(
                    "factory returned Provider {provider_id}, expected {}",
                    manifest.provider_id
                ),
            });
        }
        let api_version =
            catch_unwind(AssertUnwindSafe(|| provider.api_version())).map_err(|_| {
                AudioSourceSystemError::ProviderLoad {
                    audio_source_id: manifest.id.clone(),
                    message: "reading the bundled Provider API version panicked".to_owned(),
                }
            })?;
        if api_version != manifest.supported_api_version {
            return Err(AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: format!(
                    "factory returned Provider API {api_version}, expected {}",
                    manifest.supported_api_version
                ),
            });
        }
        let capabilities = catch_unwind(AssertUnwindSafe(|| provider.required_capabilities()))
            .map_err(|_| AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: "reading the bundled Provider capabilities panicked".to_owned(),
            })?;
        if capabilities != manifest.capabilities {
            return Err(AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: "factory Provider capabilities do not match the registration".to_owned(),
            });
        }
        Ok(provider)
    }

    fn activation_failed(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
        error: &AudioSourceSystemError,
    ) -> Result<(), AudioSourceSystemError> {
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Ok(());
        };
        let mut candidate = entry.record.clone();
        candidate.enabled = false;
        candidate.state = AudioSourceState::Error;
        for diagnostic in error.diagnostics() {
            append_diagnostic(&mut candidate.diagnostics, diagnostic);
        }
        update_action_flags(&mut candidate);
        persist_record(connection, &candidate, entry.manifest.as_ref())?;
        let entry = self
            .entries
            .get_mut(audio_source_id)
            .expect("audio source checked above");
        entry.provider = None;
        entry.record = candidate;
        Ok(())
    }

    fn deactivate(
        &mut self,
        connection: &Connection,
        audio_source_id: &str,
    ) -> Result<AudioSourceRecord, AudioSourceSystemError> {
        let Some(entry) = self.entries.get(audio_source_id) else {
            return Err(AudioSourceSystemError::NotFound(audio_source_id.to_owned()));
        };
        let mut candidate = entry.record.clone();
        let provider = entry.provider.clone();
        let previous_grants = provider.as_ref().map(|provider| {
            entry
                .record
                .granted_capabilities
                .intersection(&provider.required_capabilities())
                .copied()
                .collect::<BTreeSet<_>>()
        });
        if let Some(provider) = provider.as_ref() {
            let provider_id = provider.id();
            self.runtime
                .uninitialize_provider(provider_id)
                .map_err(|error| runtime_error(audio_source_id, error))?;
            if let Err(error) = self
                .runtime
                .clear_provider_granted_capabilities(provider_id)
            {
                let error = runtime_error(audio_source_id, error);
                let restore_error = restore_active_provider(
                    &self.runtime,
                    provider,
                    previous_grants.clone().unwrap_or_default(),
                    audio_source_id,
                );
                return match restore_error {
                    Ok(()) => Err(error),
                    Err(restore_error) => Err(restoration_error(
                        "runtime deactivation",
                        error,
                        restore_error,
                    )),
                };
            }
        }
        candidate.enabled = false;
        candidate.state =
            if !candidate.declared_capabilities.is_empty() && !candidate.permissions_reviewed {
                AudioSourceState::NeedsReview
            } else {
                AudioSourceState::Disabled
            };
        append_diagnostic(
            &mut candidate.diagnostics,
            AudioSourceDiagnostic::info(
                "lifecycle",
                Some(audio_source_id.to_owned()),
                "audio source disabled",
            ),
        );
        update_action_flags(&mut candidate);
        if let Err(error) = persist_record(connection, &candidate, entry.manifest.as_ref()) {
            if let (Some(provider), Some(previous_grants)) = (provider.as_ref(), previous_grants) {
                if let Err(restore_error) = restore_active_provider(
                    &self.runtime,
                    provider,
                    previous_grants,
                    audio_source_id,
                ) {
                    return Err(restoration_error(
                        "disable persistence",
                        error,
                        restore_error,
                    ));
                }
            }
            return Err(error);
        }
        let entry = self
            .entries
            .get_mut(audio_source_id)
            .expect("audio source checked above");
        entry.provider = None;
        entry.record = candidate.clone();
        Ok(candidate)
    }

    fn insert_invalid_record(&mut self, path: PathBuf, message: String) {
        let id = format!(
            "invalid-{}",
            &sha256_hex(path.to_string_lossy().as_bytes())[..16]
        );
        self.entries.insert(
            id.clone(),
            AudioSourceEntryRuntime {
                manifest: None,
                record: AudioSourceRecord {
                    id,
                    name: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Invalid Audio Source")
                        .to_owned(),
                    version: None,
                    description: None,
                    author: None,
                    homepage: None,
                    path: path.to_string_lossy().into_owned(),
                    adapter: None,
                    state: AudioSourceState::Invalid,
                    enabled: false,
                    permissions_reviewed: false,
                    declared_capabilities: BTreeSet::new(),
                    granted_capabilities: BTreeSet::new(),
                    sources: Vec::new(),
                    diagnostics: vec![AudioSourceDiagnostic::error("manifest", None, message)],
                    can_remove: true,
                    can_enable: false,
                },
                provider: None,
                origin: AudioSourceOrigin::Imported,
            },
        );
    }
}

pub(crate) fn prepare_remote_audio_source_import(
    source_url: &str,
    v8_sidecar_available: bool,
) -> Result<PreparedAudioSourceImport, AudioSourceSystemError> {
    let downloaded = download_audio_source(source_url)?;
    let source_path = PathBuf::from(&downloaded.source_file_name);
    let (manifest, source, report) =
        prepare_import_contents(&source_path, downloaded.source, v8_sidecar_available)?;
    Ok(PreparedAudioSourceImport {
        manifest,
        source,
        report,
        provenance: AudioSourceImportProvenance {
            kind: "remote-url".to_owned(),
            source_file_name: downloaded.source_file_name,
            requested_url: Some(display_remote_source_url(&downloaded.requested_url)),
            final_url: Some(display_remote_source_url(&downloaded.final_url)),
        },
    })
}

pub fn migrate_legacy_lx_plugins(
    connection: &Connection,
    plugins_dir: &Path,
    audio_sources_dir: &Path,
) -> Result<usize, AudioSourceSystemError> {
    if !plugins_dir.is_dir() {
        return Ok(0);
    }
    fs::create_dir_all(audio_sources_dir)?;
    let audio_sources_root = fs::canonicalize(audio_sources_dir)?;
    let mut migrated = 0;
    let mut paths = fs::read_dir(plugins_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    paths.sort();

    for legacy_path in paths {
        let manifest_path = legacy_path.join(crate::plugin_system::PLUGIN_MANIFEST_FILE);
        if !manifest_path.is_file() {
            continue;
        }
        let legacy_manifest = match fs::read(&manifest_path)
            .map_err(AudioSourceSystemError::from)
            .and_then(|bytes| serde_json::from_slice::<PluginManifest>(&bytes).map_err(Into::into))
        {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        let Some(audio_manifest) = audio_manifest_from_legacy(&legacy_manifest) else {
            continue;
        };
        if audio_manifest
            .validate(source_runtime::SOURCE_RUNTIME_API_VERSION)
            .is_err()
        {
            continue;
        }
        let source_path = legacy_path.join(AUDIO_SOURCE_FILE);
        if !source_path.is_file() {
            continue;
        }
        let destination = audio_sources_root.join(&audio_manifest.id);
        if destination.exists() {
            let existing = read_manifest(&destination)?;
            if existing.id != audio_manifest.id
                || existing.source_fingerprint != audio_manifest.source_fingerprint
            {
                return Err(AudioSourceSystemError::Package(format!(
                    "cannot migrate legacy LX source {} because the audio source destination already contains a different package",
                    legacy_manifest.id
                )));
            }
        } else {
            let staging_root = audio_sources_root.join(".migration-staging");
            fs::create_dir_all(&staging_root)?;
            let staged = staging_root.join(format!(
                "{}.migrate-{}",
                audio_manifest.id,
                operation_nonce()
            ));
            fs::create_dir_all(&staged)?;
            fs::write(
                staged.join(AUDIO_SOURCE_MANIFEST_FILE),
                serde_json::to_vec_pretty(&audio_manifest)?,
            )?;
            fs::copy(&source_path, staged.join(AUDIO_SOURCE_FILE))?;
            let legacy_report = legacy_path.join(AUDIO_SOURCE_REPORT_FILE);
            if legacy_report.is_file() {
                fs::copy(legacy_report, staged.join(AUDIO_SOURCE_REPORT_FILE))?;
            }
            fs::rename(&staged, &destination)?;
            let _ = fs::remove_dir(&staging_root);
        }

        let had_audio_state = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM audio_source_states WHERE audio_source_id = ?1)",
            params![audio_manifest.id],
            |row| row.get::<_, i64>(0),
        )? != 0;
        let transaction = connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO audio_source_states (
                audio_source_id, package_path, manifest_fingerprint, enabled,
                permissions_reviewed, granted_capabilities, installed_at, updated_at
             )
             SELECT plugin_id, ?2, ?3, enabled, permissions_reviewed,
                    granted_capabilities, installed_at, updated_at
             FROM plugin_states WHERE plugin_id = ?1
             ON CONFLICT(audio_source_id) DO NOTHING",
            params![
                legacy_manifest.id,
                destination.to_string_lossy(),
                manifest_fingerprint(&audio_manifest)?
            ],
        )?;
        if !had_audio_state {
            transaction.execute(
                "INSERT INTO audio_source_diagnostics (
                    audio_source_id, code, level, source_id, message, timestamp
                 )
                 SELECT plugin_id, code, level, source_id, message, timestamp
                 FROM plugin_diagnostics WHERE plugin_id = ?1",
                params![legacy_manifest.id],
            )?;
        }
        transaction.execute(
            "DELETE FROM plugin_diagnostics WHERE plugin_id = ?1",
            params![legacy_manifest.id],
        )?;
        transaction.execute(
            "DELETE FROM plugin_states WHERE plugin_id = ?1",
            params![legacy_manifest.id],
        )?;
        transaction.commit()?;
        remove_path(&legacy_path)?;
        migrated += 1;
    }
    Ok(migrated)
}

fn audio_manifest_from_legacy(manifest: &PluginManifest) -> Option<AudioSourceManifest> {
    if manifest.provider_entrypoints.len() != 1 {
        return None;
    }
    let provider = &manifest.provider_entrypoints[0];
    let remainder = provider
        .entrypoint
        .strip_prefix(LEGACY_IMPORTED_LX_ENTRYPOINT_PREFIX)?;
    let (adapter, source_fingerprint) = remainder.split_once(':')?;
    LxJsImportAdapter::parse(adapter)?;
    Some(AudioSourceManifest {
        manifest_version: AUDIO_SOURCE_MANIFEST_VERSION,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        author: manifest.author.clone(),
        homepage: manifest.homepage.clone(),
        provider_id: provider.id.clone(),
        adapter: adapter.to_owned(),
        source_fingerprint: source_fingerprint.to_owned(),
        capabilities: manifest.capabilities.clone(),
        supported_api_version: manifest.supported_api_version,
        source_catalog: provider.source_catalog.clone(),
    })
}

fn prepare_import(
    source_path: &Path,
    v8_sidecar_available: bool,
) -> Result<(AudioSourceManifest, String, LxJsImportReport), AudioSourceSystemError> {
    if !source_path.is_file() {
        return Err(AudioSourceSystemError::Package(format!(
            "LX JavaScript source is not a file: {}",
            source_path.display()
        )));
    }
    validate_source_extension(source_path)?;
    let source_size = fs::metadata(source_path)?.len();
    if source_size > MAX_SOURCE_BYTES as u64 {
        return Err(source_too_large_error());
    }
    let source = fs::read_to_string(source_path)?;
    prepare_import_contents(source_path, source, v8_sidecar_available)
}

fn prepare_import_contents(
    source_path: &Path,
    source: String,
    v8_sidecar_available: bool,
) -> Result<(AudioSourceManifest, String, LxJsImportReport), AudioSourceSystemError> {
    validate_source_extension(source_path)?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(source_too_large_error());
    }
    let report = lx_js_importer::analyze_lx_js_source(source_path, &source)
        .map_err(|error| AudioSourceSystemError::Package(error.to_string()))?;
    let (adapter, source_catalog) = if let Some(adapter) =
        lx_js_importer::supported_import_adapter(&report)
    {
        (adapter, imported_javascript_source_catalog(&report))
    } else if v8_sidecar_available && report.obfuscation.likely_obfuscated {
        (LxJsImportAdapter::V8Sidecar, opaque_v8_source_catalog())
    } else {
        return Err(AudioSourceSystemError::Package(
            "file does not expose a supported LX musicUrl request contract; opaque V8 sources require the isolated V8 sidecar"
                .to_owned(),
        ));
    };
    if source_catalog.is_empty() {
        return Err(AudioSourceSystemError::Package(
            "LX source did not expose a supported music source catalog".to_owned(),
        ));
    }

    let source_fingerprint = sha256_hex(source.as_bytes());
    let has_stable_identity = report.metadata.name.is_some()
        || report.metadata.author.is_some()
        || report.metadata.homepage.is_some()
        || report.metadata.update_url.is_some();
    let audio_source_id = if has_stable_identity {
        report.manifest.provider_id.clone()
    } else {
        format!(
            "{}-{}",
            report.manifest.provider_id,
            &source_fingerprint[..16]
        )
    };
    let provider_id = format!("{audio_source_id}-provider");
    let adapter_note = match adapter {
        LxJsImportAdapter::V8Sidecar => {
            "Imported from opaque LX Music JavaScript and executed in Fika's isolated V8 Audio Source sidecar."
        }
        _ => {
            "Imported from LX Music JavaScript and executed in Fika's constrained QuickJS Audio Source runtime."
        }
    };
    let description = report
        .metadata
        .description
        .as_deref()
        .map(|description| format!("{description} {adapter_note}"))
        .unwrap_or_else(|| adapter_note.to_owned());
    let name = truncate_text(&report.manifest.display_name, 120);
    let name = if name.is_empty() {
        "Imported LX Source".to_owned()
    } else {
        name
    };
    let manifest = AudioSourceManifest {
        manifest_version: AUDIO_SOURCE_MANIFEST_VERSION,
        id: audio_source_id,
        name,
        version: normalize_version(report.metadata.version.as_deref()),
        description: Some(truncate_text(&description, 600)),
        author: report
            .metadata
            .author
            .as_deref()
            .map(|author| truncate_text(author, 120)),
        homepage: report
            .metadata
            .homepage
            .as_deref()
            .map(|homepage| truncate_text(homepage, 2_048)),
        provider_id,
        adapter: adapter.as_str().to_owned(),
        source_fingerprint,
        capabilities: BTreeSet::from([SourceCapability::NetworkAny]),
        supported_api_version: source_runtime::SOURCE_RUNTIME_API_VERSION,
        source_catalog,
    };
    Ok((manifest, source, report))
}

fn build_imported_provider(
    manifest: &AudioSourceManifest,
    package_path: &Path,
    v8_sidecar: Option<Arc<LxV8Sidecar>>,
) -> Result<Arc<dyn SourceProvider>, AudioSourceSystemError> {
    let (source, report) = read_verified_source(manifest, package_path)?;
    match LxJsImportAdapter::parse(&manifest.adapter) {
        Some(LxJsImportAdapter::V8Sidecar) => {
            let sidecar = v8_sidecar.ok_or_else(|| AudioSourceSystemError::ProviderLoad {
                audio_source_id: manifest.id.clone(),
                message: "isolated LX V8 sidecar is unavailable".to_owned(),
            })?;
            Ok(Arc::new(ImportedLxV8Provider::new(
                sidecar,
                manifest.provider_id.clone(),
                report.manifest.display_name,
                source,
                report.metadata,
                manifest.source_catalog.clone(),
            )))
        }
        Some(_) => {
            let catalog = imported_javascript_source_catalog(&report);
            if catalog != manifest.source_catalog {
                return Err(AudioSourceSystemError::ProviderLoad {
                    audio_source_id: manifest.id.clone(),
                    message: "source.js does not match the declared source catalog".to_owned(),
                });
            }
            Ok(Arc::new(ImportedLxJsProvider::new(
                manifest.provider_id.clone(),
                report.manifest.display_name,
                source,
                report.metadata,
                catalog,
            )))
        }
        None => Err(AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: format!("unsupported adapter: {}", manifest.adapter),
        }),
    }
}

fn read_verified_source(
    manifest: &AudioSourceManifest,
    package_path: &Path,
) -> Result<(String, LxJsImportReport), AudioSourceSystemError> {
    let source_path = package_path.join(AUDIO_SOURCE_FILE);
    let source_metadata = fs::symlink_metadata(&source_path).map_err(|error| {
        AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: format!("could not inspect source.js: {error}"),
        }
    })?;
    if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
        return Err(AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: "source.js must be a regular managed file".to_owned(),
        });
    }
    if source_metadata.len() > MAX_SOURCE_BYTES as u64 {
        return Err(AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: "source.js exceeds the configured size limit".to_owned(),
        });
    }
    let source =
        fs::read_to_string(&source_path).map_err(|error| AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: format!("could not read source.js: {error}"),
        })?;
    if sha256_hex(source.as_bytes()) != manifest.source_fingerprint {
        return Err(AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: "source.js failed its integrity check".to_owned(),
        });
    }
    let report = lx_js_importer::analyze_lx_js_source(&source_path, &source).map_err(|error| {
        AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: error.to_string(),
        }
    })?;
    LxJsImportAdapter::parse(&manifest.adapter).ok_or_else(|| {
        AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: format!("unsupported adapter: {}", manifest.adapter),
        }
    })?;
    Ok((source, report))
}

fn download_audio_source(
    source_url: &str,
) -> Result<DownloadedAudioSource, AudioSourceSystemError> {
    let requested_url = parse_remote_source_url(source_url)?;
    let request_url = normalize_remote_source_url(requested_url.clone());
    let initial_request_is_https = request_url.scheme() == "https";
    let client = Client::builder()
        .timeout(REMOTE_SOURCE_TIMEOUT)
        .connect_timeout(REMOTE_SOURCE_CONNECT_TIMEOUT)
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() > MAX_REMOTE_SOURCE_REDIRECTS {
                return attempt.error("audio source redirect limit exceeded");
            }
            if validate_remote_source_url(attempt.url()).is_err() {
                return attempt.error("audio source redirected to an unsupported URL");
            }
            if initial_request_is_https && attempt.url().scheme() != "https" {
                return attempt.error("audio source redirect attempted to downgrade HTTPS");
            }
            attempt.follow()
        }))
        .build()
        .map_err(|_| {
            AudioSourceSystemError::Package(
                "could not initialize the audio source download client".to_owned(),
            )
        })?;
    let diagnostic_url = display_remote_source_url(&request_url);
    let mut response = client
        .get(request_url)
        .header(USER_AGENT, REMOTE_SOURCE_USER_AGENT)
        .header(
            ACCEPT,
            "application/javascript, text/javascript, text/plain;q=0.9, */*;q=0.1",
        )
        .send()
        .map_err(|error| {
            let reason = if error.is_timeout() {
                "request timed out"
            } else if error.is_connect() {
                "could not connect"
            } else {
                "network request failed"
            };
            AudioSourceSystemError::Package(format!(
                "audio source download {reason}: {diagnostic_url}"
            ))
        })?;
    let final_url = response.url().clone();
    validate_remote_source_url(&final_url).map_err(AudioSourceSystemError::Package)?;
    let final_diagnostic_url = display_remote_source_url(&final_url);
    if !response.status().is_success() {
        return Err(AudioSourceSystemError::Package(format!(
            "audio source download returned HTTP {}: {final_diagnostic_url}",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SOURCE_BYTES as u64)
    {
        return Err(source_too_large_error());
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase);
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            AudioSourceSystemError::Package(format!(
                "could not read the audio source response: {final_diagnostic_url}"
            ))
        })?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(source_too_large_error());
    }
    if remote_source_looks_like_html(content_type.as_deref(), &bytes) {
        return Err(AudioSourceSystemError::Package(
            "audio source URL returned HTML instead of JavaScript; use a direct file URL"
                .to_owned(),
        ));
    }
    let source = String::from_utf8(bytes).map_err(|_| {
        AudioSourceSystemError::Package(
            "downloaded audio source JavaScript must be UTF-8".to_owned(),
        )
    })?;
    Ok(DownloadedAudioSource {
        source_file_name: remote_source_file_name(&final_url),
        source,
        requested_url,
        final_url,
    })
}

fn parse_remote_source_url(source_url: &str) -> Result<Url, AudioSourceSystemError> {
    let source_url = source_url.trim();
    if source_url.is_empty() {
        return Err(AudioSourceSystemError::Package(
            "audio source URL must not be empty".to_owned(),
        ));
    }
    if source_url.len() > MAX_REMOTE_SOURCE_URL_BYTES {
        return Err(AudioSourceSystemError::Package(
            "audio source URL is too long".to_owned(),
        ));
    }
    let url = Url::parse(source_url)
        .map_err(|_| AudioSourceSystemError::Package("audio source URL is invalid".to_owned()))?;
    validate_remote_source_url(&url).map_err(AudioSourceSystemError::Package)?;
    Ok(url)
}

fn validate_remote_source_url(url: &Url) -> Result<(), String> {
    if url.as_str().len() > MAX_REMOTE_SOURCE_URL_BYTES {
        return Err("audio source URL is too long".to_owned());
    }
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("audio source URL must use HTTP or HTTPS".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("audio source URL must not contain credentials".to_owned());
    }
    Ok(())
}

fn normalize_remote_source_url(mut url: Url) -> Url {
    let is_github_blob = url.host_str() == Some("github.com")
        && url
            .path_segments()
            .is_some_and(|mut segments| segments.nth(2) == Some("blob"));
    if is_github_blob {
        let query_pairs = url
            .query_pairs()
            .filter(|(key, _)| key != "raw")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        url.set_query(None);
        let mut query = url.query_pairs_mut();
        for (key, value) in query_pairs {
            query.append_pair(&key, &value);
        }
        query.append_pair("raw", "1");
    }
    url.set_fragment(None);
    url
}

fn display_remote_source_url(url: &Url) -> String {
    let mut display_url = url.clone();
    display_url.set_query(None);
    display_url.set_fragment(None);
    display_url.to_string()
}

fn remote_source_file_name(url: &Url) -> String {
    let candidate = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| percent_decode_str(segment).decode_utf8_lossy())
        .unwrap_or_else(|| "remote-source".into());
    let cleaned = candidate
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '/' | '\\'))
        .take(160)
        .collect::<String>();
    let cleaned = if cleaned.trim().is_empty() {
        "remote-source".to_owned()
    } else {
        cleaned
    };
    let extension = Path::new(&cleaned)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "js" | "mjs" | "cjs") {
        cleaned
    } else {
        format!("{cleaned}.js")
    }
}

fn remote_source_looks_like_html(content_type: Option<&str>, bytes: &[u8]) -> bool {
    if content_type.is_some_and(|content_type| {
        content_type.contains("text/html") || content_type.contains("application/xhtml+xml")
    }) {
        return true;
    }
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(256)])
        .trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n'])
        .to_ascii_lowercase();
    prefix.starts_with("<!doctype html") || prefix.starts_with("<html")
}

fn imported_javascript_source_catalog(report: &LxJsImportReport) -> BTreeMap<String, SourceInfo> {
    report
        .manifest
        .to_source_catalog()
        .into_iter()
        .filter(|(source_id, source)| {
            matches!(
                source_id.as_str(),
                source_runtime::LX_SOURCE_WY
                    | source_runtime::LX_SOURCE_TX
                    | source_runtime::LX_SOURCE_KW
                    | source_runtime::LX_SOURCE_KG
                    | source_runtime::LX_SOURCE_MG
                    | source_runtime::LX_SOURCE_LOCAL
            ) && source.actions.contains(&SourceAction::MusicUrl)
        })
        .map(|(source_id, source)| {
            let qualities = if source.qualities.is_empty() {
                source_runtime::standard_lx_qualities()
            } else {
                source.qualities
            };
            let name = if source.name.trim().is_empty() {
                imported_source_name(&source_id).to_owned()
            } else {
                source.name
            };
            let source = source_runtime::lx_music_source(
                source_id.clone(),
                name,
                vec![SourceAction::MusicUrl],
                qualities,
            );
            (source_id, source)
        })
        .collect()
}

fn opaque_v8_source_catalog() -> BTreeMap<String, SourceInfo> {
    [
        source_runtime::LX_SOURCE_WY,
        source_runtime::LX_SOURCE_TX,
        source_runtime::LX_SOURCE_KW,
        source_runtime::LX_SOURCE_KG,
        source_runtime::LX_SOURCE_MG,
        source_runtime::LX_SOURCE_LOCAL,
    ]
    .into_iter()
    .map(|source_id| {
        (
            source_id.to_owned(),
            source_runtime::lx_music_source(
                source_id,
                imported_source_name(source_id),
                vec![SourceAction::MusicUrl],
                source_runtime::standard_lx_qualities(),
            ),
        )
    })
    .collect()
}

fn imported_source_name(source_id: &str) -> &str {
    match source_id {
        source_runtime::LX_SOURCE_WY => "NetEase",
        source_runtime::LX_SOURCE_TX => "QQ Music",
        source_runtime::LX_SOURCE_KW => "Kuwo",
        source_runtime::LX_SOURCE_KG => "Kugou",
        source_runtime::LX_SOURCE_MG => "Migu",
        source_runtime::LX_SOURCE_LOCAL => "Local Music",
        _ => source_id,
    }
}

fn record_for_manifest(
    manifest: &AudioSourceManifest,
    path: &Path,
    persisted: Option<&PersistedAudioSourceState>,
    runtime_api_version: SourceRuntimeApiVersion,
    can_remove: bool,
) -> Result<AudioSourceRecord, AudioSourceSystemError> {
    let fingerprint = manifest_fingerprint(manifest)?;
    let manifest_unchanged =
        persisted.is_some_and(|state| state.manifest_fingerprint == fingerprint);
    let enabled = persisted.is_some_and(|state| state.enabled && manifest_unchanged);
    let permissions_reviewed =
        persisted.is_some_and(|state| state.permissions_reviewed && manifest_unchanged);
    let granted_capabilities = persisted
        .filter(|_| manifest_unchanged)
        .map(|state| {
            state
                .granted_capabilities
                .intersection(&manifest.capabilities)
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let compatible = manifest
        .supported_api_version
        .is_compatible_with(runtime_api_version);
    let state = if !compatible {
        AudioSourceState::Incompatible
    } else if !manifest.capabilities.is_empty() && !permissions_reviewed {
        AudioSourceState::NeedsReview
    } else if enabled {
        AudioSourceState::Enabled
    } else {
        AudioSourceState::Disabled
    };
    let mut diagnostics = persisted
        .map(|state| state.diagnostics.clone())
        .unwrap_or_default();
    if persisted.is_some() && !manifest_unchanged {
        append_diagnostic(
            &mut diagnostics,
            AudioSourceDiagnostic::warning(
                "permissions-review",
                Some(manifest.id.clone()),
                "audio source manifest changed; capabilities must be reviewed again",
            ),
        );
    }
    let mut record = AudioSourceRecord {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: Some(manifest.version.clone()),
        description: manifest.description.clone(),
        author: manifest.author.clone(),
        homepage: manifest.homepage.clone(),
        path: path.to_string_lossy().into_owned(),
        adapter: Some(manifest.adapter.clone()),
        state,
        enabled,
        permissions_reviewed,
        declared_capabilities: manifest.capabilities.clone(),
        granted_capabilities,
        sources: manifest.source_catalog.values().cloned().collect(),
        diagnostics,
        can_remove,
        can_enable: false,
    };
    update_action_flags(&mut record);
    Ok(record)
}

fn read_manifest(path: &Path) -> Result<AudioSourceManifest, AudioSourceSystemError> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(AudioSourceSystemError::InvalidManifest(format!(
            "audio source package must not be a symbolic link: {}",
            path.display()
        )));
    }
    let manifest_path = path.join(AUDIO_SOURCE_MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Err(AudioSourceSystemError::InvalidManifest(format!(
            "{} does not contain {AUDIO_SOURCE_MANIFEST_FILE}",
            path.display()
        )));
    }
    Ok(serde_json::from_slice(&fs::read(manifest_path)?)?)
}

fn upgrade_legacy_execution_manifest(
    package_path: &Path,
    manifest: &mut AudioSourceManifest,
) -> Result<(), AudioSourceSystemError> {
    if matches!(
        LxJsImportAdapter::parse(&manifest.adapter),
        Some(LxJsImportAdapter::QuickJs | LxJsImportAdapter::V8Sidecar)
    ) {
        return Ok(());
    }
    let (_, report) = read_verified_source(manifest, package_path)?;
    let source_catalog = imported_javascript_source_catalog(&report);
    if source_catalog.is_empty() {
        return Err(AudioSourceSystemError::ProviderLoad {
            audio_source_id: manifest.id.clone(),
            message: "source.js did not expose a supported music source catalog".to_owned(),
        });
    }
    let legacy_note_prefix = "Imported from LX Music JavaScript with the ";
    let legacy_note_suffix =
        " Rust URL-template adapter. The JavaScript file is stored for provenance and is not executed.";
    if let Some(description) = manifest.description.as_mut() {
        if let Some(note_start) = description.find(legacy_note_prefix) {
            let note_end = description[note_start..]
                .find(legacy_note_suffix)
                .map(|offset| note_start + offset + legacy_note_suffix.len());
            if let Some(note_end) = note_end {
                description.replace_range(
                    note_start..note_end,
                    "Imported from LX Music JavaScript and executed in Fika's constrained QuickJS Audio Source runtime.",
                );
            }
        }
    }
    manifest.adapter = LxJsImportAdapter::QuickJs.as_str().to_owned();
    manifest.source_catalog = source_catalog;
    fs::write(
        package_path.join(AUDIO_SOURCE_MANIFEST_FILE),
        serde_json::to_vec_pretty(manifest)?,
    )?;
    Ok(())
}

fn discover_audio_source_paths(root: &Path) -> Result<Vec<PathBuf>, AudioSourceSystemError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn load_persisted_states(
    connection: &Connection,
) -> Result<BTreeMap<String, PersistedAudioSourceState>, AudioSourceSystemError> {
    let mut diagnostics = BTreeMap::<String, Vec<AudioSourceDiagnostic>>::new();
    let mut diagnostic_statement = connection.prepare(
        "SELECT audio_source_id, code, level, source_id, message, timestamp
         FROM audio_source_diagnostics ORDER BY id ASC",
    )?;
    let diagnostic_rows = diagnostic_statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            AudioSourceDiagnostic {
                code: row.get(1)?,
                level: parse_diagnostic_level(&row.get::<_, String>(2)?),
                source_id: row.get(3)?,
                message: row.get(4)?,
                timestamp: row.get(5)?,
            },
        ))
    })?;
    for row in diagnostic_rows {
        let (audio_source_id, diagnostic) = row?;
        diagnostics
            .entry(audio_source_id)
            .or_default()
            .push(diagnostic);
    }

    let mut statement = connection.prepare(
        "SELECT audio_source_id, manifest_fingerprint, enabled,
                permissions_reviewed, granted_capabilities
         FROM audio_source_states",
    )?;
    let rows = statement.query_map([], |row| {
        let audio_source_id = row.get::<_, String>(0)?;
        let capabilities_json = row.get::<_, String>(4)?;
        let granted_capabilities =
            serde_json::from_str::<BTreeSet<SourceCapability>>(&capabilities_json)
                .unwrap_or_default();
        Ok((
            audio_source_id.clone(),
            PersistedAudioSourceState {
                manifest_fingerprint: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                permissions_reviewed: row.get::<_, i64>(3)? != 0,
                granted_capabilities,
                diagnostics: diagnostics.remove(&audio_source_id).unwrap_or_default(),
            },
        ))
    })?;
    let mut states = BTreeMap::new();
    for row in rows {
        let (audio_source_id, state) = row?;
        states.insert(audio_source_id, state);
    }
    Ok(states)
}

fn persist_record(
    connection: &Connection,
    record: &AudioSourceRecord,
    manifest: Option<&AudioSourceManifest>,
) -> Result<(), AudioSourceSystemError> {
    let Some(manifest) = manifest else {
        return Ok(());
    };
    let now = now_timestamp();
    let fingerprint = manifest_fingerprint(manifest)?;
    let capabilities = serde_json::to_string(&record.granted_capabilities)?;
    connection.execute_batch("SAVEPOINT fika_audio_source_persist")?;
    let persisted = (|| -> Result<(), AudioSourceSystemError> {
        connection.execute(
            "INSERT INTO audio_source_states (
                audio_source_id, package_path, manifest_fingerprint, enabled,
                permissions_reviewed, granted_capabilities, installed_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(audio_source_id) DO UPDATE SET
                package_path = excluded.package_path,
                manifest_fingerprint = excluded.manifest_fingerprint,
                enabled = excluded.enabled,
                permissions_reviewed = excluded.permissions_reviewed,
                granted_capabilities = excluded.granted_capabilities,
                updated_at = excluded.updated_at",
            params![
                record.id,
                record.path,
                fingerprint,
                i64::from(record.enabled),
                i64::from(record.permissions_reviewed),
                capabilities,
                now
            ],
        )?;
        connection.execute(
            "DELETE FROM audio_source_diagnostics WHERE audio_source_id = ?1",
            params![record.id],
        )?;
        for diagnostic in record.diagnostics.iter().rev().take(MAX_DIAGNOSTICS).rev() {
            connection.execute(
                "INSERT INTO audio_source_diagnostics (
                    audio_source_id, code, level, source_id, message, timestamp
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    record.id,
                    diagnostic.code,
                    diagnostic_level_text(diagnostic.level),
                    diagnostic.source_id,
                    diagnostic.message,
                    diagnostic.timestamp
                ],
            )?;
        }
        Ok(())
    })();
    match persisted {
        Ok(()) => {
            connection.execute_batch("RELEASE SAVEPOINT fika_audio_source_persist")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch(
                "ROLLBACK TO SAVEPOINT fika_audio_source_persist; \
                 RELEASE SAVEPOINT fika_audio_source_persist",
            );
            Err(error)
        }
    }
}

fn delete_persisted_source(
    connection: &Connection,
    audio_source_id: &str,
) -> Result<(), AudioSourceSystemError> {
    connection.execute(
        "DELETE FROM audio_source_diagnostics WHERE audio_source_id = ?1",
        params![audio_source_id],
    )?;
    connection.execute(
        "DELETE FROM audio_source_states WHERE audio_source_id = ?1",
        params![audio_source_id],
    )?;
    Ok(())
}

fn clear_runtime_entries(
    runtime: &SourceRuntime,
    entries: &BTreeMap<String, AudioSourceEntryRuntime>,
) -> Result<(), AudioSourceSystemError> {
    for (audio_source_id, entry) in entries {
        let Some(provider) = entry.provider.as_ref() else {
            continue;
        };
        runtime
            .uninitialize_provider(provider.id())
            .map_err(|error| runtime_error(audio_source_id, error))?;
        runtime
            .clear_provider_granted_capabilities(provider.id())
            .map_err(|error| runtime_error(audio_source_id, error))?;
    }
    Ok(())
}

fn restore_runtime_entries(
    runtime: &SourceRuntime,
    entries: &BTreeMap<String, AudioSourceEntryRuntime>,
) -> Result<(), AudioSourceSystemError> {
    let mut restored_provider_ids = Vec::new();
    for (audio_source_id, entry) in entries {
        if !entry.record.enabled {
            continue;
        }
        let Some(provider) = entry.provider.as_ref() else {
            continue;
        };
        let grants = entry
            .record
            .granted_capabilities
            .intersection(&provider.required_capabilities())
            .copied()
            .collect::<BTreeSet<_>>();
        if let Err(error) = runtime
            .replace_provider_granted_capabilities(provider.id().to_owned(), grants)
            .and_then(|_| runtime.initialize_provider(provider.as_ref()).map(|_| ()))
        {
            clear_runtime_provider_state(runtime, restored_provider_ids);
            return Err(runtime_error(audio_source_id, error));
        }
        restored_provider_ids.push(provider.id().to_owned());
    }
    Ok(())
}

fn restore_active_provider(
    runtime: &SourceRuntime,
    provider: &Arc<dyn SourceProvider>,
    grants: BTreeSet<SourceCapability>,
    audio_source_id: &str,
) -> Result<(), AudioSourceSystemError> {
    runtime
        .replace_provider_granted_capabilities(provider.id().to_owned(), grants)
        .and_then(|_| runtime.initialize_provider(provider.as_ref()).map(|_| ()))
        .map_err(|error| runtime_error(audio_source_id, error))
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

fn restoration_error(
    operation: &str,
    error: AudioSourceSystemError,
    restore_error: AudioSourceSystemError,
) -> AudioSourceSystemError {
    AudioSourceSystemError::Package(format!(
        "{operation} failed: {error}; restoring the previous runtime state also failed: {restore_error}"
    ))
}

fn runtime_error(
    audio_source_id: &str,
    error: source_runtime::SourceRuntimeError,
) -> AudioSourceSystemError {
    let diagnostics = error
        .diagnostics()
        .iter()
        .map(AudioSourceDiagnostic::from_runtime)
        .collect();
    AudioSourceSystemError::Runtime {
        audio_source_id: audio_source_id.to_owned(),
        message: error.to_string(),
        diagnostics,
    }
}

fn append_diagnostic(
    diagnostics: &mut Vec<AudioSourceDiagnostic>,
    diagnostic: AudioSourceDiagnostic,
) {
    diagnostics.push(diagnostic);
    if diagnostics.len() > MAX_DIAGNOSTICS {
        diagnostics.drain(0..diagnostics.len() - MAX_DIAGNOSTICS);
    }
}

fn update_action_flags(record: &mut AudioSourceRecord) {
    record.can_enable = !matches!(
        record.state,
        AudioSourceState::Invalid | AudioSourceState::Incompatible
    ) && (record.declared_capabilities.is_empty()
        || record.permissions_reviewed);
}

fn diagnostic_level_text(level: DiagnosticLevel) -> &'static str {
    match level {
        DiagnosticLevel::Info => "info",
        DiagnosticLevel::Warn => "warn",
        DiagnosticLevel::Error => "error",
        DiagnosticLevel::Security => "security",
    }
}

fn parse_diagnostic_level(level: &str) -> DiagnosticLevel {
    match level {
        "warn" => DiagnosticLevel::Warn,
        "error" => DiagnosticLevel::Error,
        "security" => DiagnosticLevel::Security,
        _ => DiagnosticLevel::Info,
    }
}

fn validate_source_extension(path: &Path) -> Result<(), AudioSourceSystemError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "js" | "mjs" | "cjs") {
        Ok(())
    } else {
        Err(AudioSourceSystemError::Package(
            "audio source import accepts only .js, .mjs, or .cjs files".to_owned(),
        ))
    }
}

fn normalize_version(version: Option<&str>) -> String {
    version
        .and_then(|version| semver::Version::parse(version.trim()).ok())
        .map(|version| version.to_string())
        .unwrap_or_else(|| "0.1.0".to_owned())
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn source_too_large_error() -> AudioSourceSystemError {
    AudioSourceSystemError::Package(format!(
        "audio source exceeds the {} MiB import limit",
        MAX_SOURCE_BYTES / (1024 * 1024)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_system::{PluginProviderEntrypoint, PLUGIN_COMPATIBILITY_TARGET};
    use crate::source_runtime::{SourceQuality, LX_SOURCE_KG};

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("database should open");
        crate::database::initialize(&mut connection).expect("database should migrate");
        connection
    }

    fn reference_source() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/lx-js-sources/quantouya-aggregate-v4.1.js")
    }

    fn arithmetic_obfuscated_source() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/lx-js-sources/arithmetic-obfuscated-v1.0.0.js")
    }

    #[test]
    fn imported_source_uses_a_dedicated_package_and_lifecycle() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let audio_sources_dir = root.path().join("audio-sources");
        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry = AudioSourceRegistry::new(&audio_sources_dir, runtime);
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");

        let imported = registry
            .import_file(&connection, &reference_source())
            .expect("reference source should import");

        assert_eq!(imported.state, AudioSourceState::Disabled);
        assert!(imported.permissions_reviewed);
        assert_eq!(
            imported.granted_capabilities,
            BTreeSet::from([SourceCapability::NetworkAny])
        );
        assert!(imported.can_enable);
        assert!(!imported.enabled);
        assert_eq!(imported.adapter.as_deref(), Some("quickjs"));
        assert!(imported
            .description
            .as_deref()
            .is_some_and(|description| description.contains("constrained QuickJS")));
        assert_eq!(
            imported.declared_capabilities,
            BTreeSet::from([SourceCapability::NetworkAny])
        );
        assert!(imported
            .sources
            .iter()
            .all(|source| source.actions == [SourceAction::MusicUrl]));
        let package_path = Path::new(&imported.path);
        assert!(package_path.join(AUDIO_SOURCE_MANIFEST_FILE).is_file());
        assert!(package_path.join(AUDIO_SOURCE_FILE).is_file());
        assert!(!package_path
            .join(crate::plugin_system::PLUGIN_MANIFEST_FILE)
            .exists());

        let enabled = registry
            .set_enabled(&connection, &imported.id, true)
            .expect("trusted imported source should enable directly");
        assert_eq!(enabled.state, AudioSourceState::Enabled);
        assert!(enabled.enabled);
    }

    #[test]
    fn remove_should_delete_an_enabled_imported_source() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let audio_sources_dir = root.path().join("audio-sources");
        let connection = database();
        let mut registry =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");
        let imported = registry
            .import_file(&connection, &reference_source())
            .expect("reference source should import");
        registry
            .set_enabled(&connection, &imported.id, true)
            .expect("trusted source should enable before removal");

        let records = registry
            .remove(&connection, &imported.id)
            .expect("imported source should be removable");

        assert!(records.is_empty());
        assert!(!Path::new(&imported.path).exists());
        let mut restarted =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        assert!(restarted
            .refresh(&connection)
            .expect("registry should refresh after removal")
            .is_empty());
    }

    #[test]
    fn arithmetic_obfuscated_source_import_should_accept_music_url_contract() {
        let (manifest, _, _) = prepare_import(&arithmetic_obfuscated_source(), false)
            .expect("arithmetic-obfuscated source should prepare for import");
        let source = manifest
            .source_catalog
            .get(LX_SOURCE_KG)
            .expect("Kugou source should be imported");

        assert_eq!(manifest.adapter, LxJsImportAdapter::QuickJs.as_str());
        assert_eq!(source.actions, [SourceAction::MusicUrl]);
        assert_eq!(source.qualities, [SourceQuality::K128]);
    }

    #[test]
    fn opaque_source_should_select_v8_sidecar_when_available() {
        let source = format!(
            "/** @name Opaque Source */\nconst bytecode = '{}';",
            "a".repeat(1_500)
        );

        let (manifest, _, _) = prepare_import_contents(Path::new("opaque.js"), source, true)
            .expect("opaque source should prepare for isolated V8 validation");

        assert_eq!(manifest.adapter, LxJsImportAdapter::V8Sidecar.as_str());
        assert_eq!(manifest.source_catalog.len(), 6);
    }

    #[test]
    #[ignore = "requires FIKA_LX_V8_PATH, FIKA_LX_V8_LIVE_SOURCE, and a live third-party endpoint"]
    fn live_opaque_source_should_import_review_and_enable() {
        let source_path = std::env::var_os("FIKA_LX_V8_LIVE_SOURCE")
            .map(PathBuf::from)
            .expect("FIKA_LX_V8_LIVE_SOURCE should point to an LX source");
        let executable = std::env::var_os("FIKA_LX_V8_PATH")
            .map(PathBuf::from)
            .expect("FIKA_LX_V8_PATH should point to Deno");
        let root = tempfile::tempdir().expect("test directory should exist");
        let connection = database();
        let sidecar = Arc::new(crate::lx_v8_sidecar::LxV8Sidecar::with_executable(
            root.path().join("runtime"),
            executable,
        ));
        let mut registry = AudioSourceRegistry::new(
            root.path().join("audio-sources"),
            Arc::new(SourceRuntime::new()),
        )
        .with_v8_sidecar(sidecar);
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");

        let imported = registry
            .import_file(&connection, &source_path)
            .expect("opaque source should import");
        registry
            .set_capabilities(
                &connection,
                &imported.id,
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("network capability should be reviewed");
        let enabled = registry
            .set_enabled(&connection, &imported.id, true)
            .expect("reviewed opaque source should enable");

        assert_eq!(imported.adapter.as_deref(), Some("v8-sidecar"));
        assert_eq!(enabled.state, AudioSourceState::Enabled);
    }

    #[test]
    fn bundled_rust_source_uses_audio_source_lifecycle_and_cannot_be_removed() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let executable = root.path().join("fake-yt-dlp");
        fs::write(&executable, []).expect("fake sidecar should exist");
        let connection = database();
        let mut registry = AudioSourceRegistry::new(
            root.path().join("audio-sources"),
            Arc::new(SourceRuntime::new()),
        )
        .with_bundled_source(
            crate::youtube_music_playback::bundled_audio_source_registration(Arc::new(
                crate::yt_dlp_sidecar::YtDlpSidecar::with_executable(
                    root.path().join("yt-dlp"),
                    executable,
                ),
            )),
        )
        .expect("bundled source should register");

        let records = registry
            .refresh(&connection)
            .expect("registry should discover the bundled source");
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(
            record.id,
            crate::youtube_music_playback::YOUTUBE_MUSIC_AUDIO_SOURCE_ID
        );
        assert_eq!(record.state, AudioSourceState::NeedsReview);
        assert!(!record.can_remove);
        assert_eq!(
            record.adapter.as_deref(),
            Some("builtin:youtube-music-playback")
        );
        assert_eq!(
            record.sources[0].id,
            crate::youtube_music::YOUTUBE_MUSIC_SOURCE_ID
        );

        registry
            .set_capabilities(
                &connection,
                &record.id,
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("network capability should be reviewable");
        let enabled = registry
            .set_enabled(&connection, &record.id, true)
            .expect("bundled source should enable");
        assert_eq!(enabled.state, AudioSourceState::Enabled);
        assert!(enabled.enabled);
        assert!(matches!(
            registry.remove(&connection, &record.id),
            Err(AudioSourceSystemError::InvalidState(_, _))
        ));
    }

    #[test]
    fn imported_source_fails_closed_when_source_changes_after_review() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let connection = database();
        let mut registry = AudioSourceRegistry::new(
            root.path().join("audio-sources"),
            Arc::new(SourceRuntime::new()),
        );
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");
        let imported = registry
            .import_file(&connection, &reference_source())
            .expect("reference source should import");
        registry
            .set_capabilities(
                &connection,
                &imported.id,
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("network access should be reviewed");
        fs::write(
            Path::new(&imported.path).join(AUDIO_SOURCE_FILE),
            "// source changed after review",
        )
        .expect("managed source should be changed");

        let error = registry
            .set_enabled(&connection, &imported.id, true)
            .expect_err("changed source must not activate");

        assert!(matches!(
            error,
            AudioSourceSystemError::ProviderLoad { message, .. }
                if message.contains("integrity check")
        ));
        assert!(
            !registry
                .record(&imported.id)
                .expect("source should remain listed")
                .enabled
        );
    }

    #[test]
    fn refresh_disables_a_persisted_source_when_its_integrity_check_fails() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let audio_sources_dir = root.path().join("audio-sources");
        let connection = database();
        let mut registry =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");
        let imported = registry
            .import_file(&connection, &reference_source())
            .expect("reference source should import");
        registry
            .set_capabilities(
                &connection,
                &imported.id,
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("network access should be reviewed");
        registry
            .set_enabled(&connection, &imported.id, true)
            .expect("source should enable before restart");
        fs::write(
            Path::new(&imported.path).join(AUDIO_SOURCE_FILE),
            "// source changed before restart",
        )
        .expect("managed source should be changed");
        drop(registry);

        let mut restarted =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        let records = restarted
            .refresh(&connection)
            .expect("integrity failure should not abort registry refresh");

        assert_eq!(records[0].state, AudioSourceState::Error);
        assert!(!records[0].enabled);
        assert!(records[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("integrity check")));
    }

    #[test]
    fn capability_update_restores_runtime_grants_when_persistence_fails() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let connection = database();
        let runtime = Arc::new(SourceRuntime::new());
        let mut registry =
            AudioSourceRegistry::new(root.path().join("audio-sources"), Arc::clone(&runtime));
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");
        let imported = registry
            .import_file(&connection, &reference_source())
            .expect("reference source should import");
        registry
            .set_capabilities(
                &connection,
                &imported.id,
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("network access should be reviewed");
        registry
            .set_enabled(&connection, &imported.id, true)
            .expect("source should enable");
        let provider_id = registry.entries[&imported.id]
            .provider
            .as_ref()
            .expect("enabled source should retain its Provider")
            .id()
            .to_owned();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_audio_source_update
                 BEFORE UPDATE ON audio_source_states
                 BEGIN SELECT RAISE(FAIL, 'audio source update failed'); END;",
            )
            .expect("failure trigger should install");

        let error = registry
            .set_capabilities(&connection, &imported.id, [], true)
            .expect_err("persistence failure should reject capability update");

        assert!(matches!(error, AudioSourceSystemError::Database(_)));
        assert_eq!(
            runtime
                .granted_capabilities_for(&provider_id)
                .expect("runtime grants should remain readable"),
            BTreeSet::from([SourceCapability::NetworkAny])
        );
        assert_eq!(
            registry
                .record(&imported.id)
                .expect("source should remain registered")
                .granted_capabilities,
            BTreeSet::from([SourceCapability::NetworkAny])
        );
    }

    #[test]
    fn legacy_adapter_upgrade_should_require_permission_review_before_script_execution() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let audio_sources_dir = root.path().join("audio-sources");
        let connection = database();
        let mut registry =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        registry
            .refresh(&connection)
            .expect("empty registry should refresh");
        let imported = registry
            .import_file(&connection, &reference_source())
            .expect("reference source should import");
        registry
            .set_capabilities(
                &connection,
                &imported.id,
                [SourceCapability::NetworkAny],
                true,
            )
            .expect("network access should be reviewed");
        registry
            .set_enabled(&connection, &imported.id, true)
            .expect("source should enable before simulating a legacy package");

        let manifest_path = Path::new(&imported.path).join(AUDIO_SOURCE_MANIFEST_FILE);
        let mut legacy_manifest =
            read_manifest(Path::new(&imported.path)).expect("managed manifest should be readable");
        legacy_manifest.adapter = LxJsImportAdapter::StaticTemplates.as_str().to_owned();
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&legacy_manifest).expect("legacy manifest should serialize"),
        )
        .expect("legacy manifest should write");
        connection
            .execute(
                "UPDATE audio_source_states
                 SET manifest_fingerprint = ?2, enabled = 1, permissions_reviewed = 1
                 WHERE audio_source_id = ?1",
                params![
                    imported.id,
                    manifest_fingerprint(&legacy_manifest)
                        .expect("legacy manifest should fingerprint")
                ],
            )
            .expect("legacy persisted state should update");
        drop(registry);

        let mut restarted =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        let records = restarted
            .refresh(&connection)
            .expect("legacy execution adapter should upgrade");
        let upgraded_manifest = read_manifest(Path::new(&records[0].path))
            .expect("upgraded manifest should be readable");

        assert_eq!(upgraded_manifest.adapter, "quickjs");
        assert!(upgraded_manifest
            .description
            .as_deref()
            .is_some_and(|description| description.contains("constrained QuickJS")));
        assert_eq!(records[0].state, AudioSourceState::NeedsReview);
        assert!(!records[0].enabled);
        assert!(!records[0].permissions_reviewed);
        assert!(records[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("reviewed again")));
    }

    #[test]
    fn legacy_lx_plugin_migration_preserves_review_enablement_and_diagnostics() {
        let root = tempfile::tempdir().expect("test directory should exist");
        let plugins_dir = root.path().join("plugins");
        let audio_sources_dir = root.path().join("audio-sources");
        fs::create_dir_all(&plugins_dir).expect("Plugin directory should exist");
        let source_path = reference_source();
        let (audio_manifest, source, _) =
            prepare_import(&source_path, false).expect("reference source should prepare");
        let legacy_manifest = PluginManifest {
            manifest_version: crate::plugin_system::PLUGIN_MANIFEST_VERSION,
            id: audio_manifest.id.clone(),
            name: audio_manifest.name.clone(),
            version: audio_manifest.version.clone(),
            description: audio_manifest.description.clone(),
            author: audio_manifest.author.clone(),
            homepage: audio_manifest.homepage.clone(),
            provider_entrypoints: vec![PluginProviderEntrypoint {
                id: audio_manifest.provider_id.clone(),
                entrypoint: format!(
                    "{LEGACY_IMPORTED_LX_ENTRYPOINT_PREFIX}{}:{}",
                    audio_manifest.adapter, audio_manifest.source_fingerprint
                ),
                capabilities: BTreeSet::new(),
                source_catalog: audio_manifest.source_catalog.clone(),
            }],
            capabilities: audio_manifest.capabilities.clone(),
            compatibility_target: PLUGIN_COMPATIBILITY_TARGET.to_owned(),
            supported_api_version: audio_manifest.supported_api_version,
            required_host_bridges: BTreeSet::new(),
        };
        let legacy_path = plugins_dir.join(&legacy_manifest.id);
        fs::create_dir_all(&legacy_path).expect("legacy package should exist");
        fs::write(
            legacy_path.join(crate::plugin_system::PLUGIN_MANIFEST_FILE),
            serde_json::to_vec_pretty(&legacy_manifest).expect("manifest should serialize"),
        )
        .expect("legacy manifest should write");
        fs::write(legacy_path.join(AUDIO_SOURCE_FILE), source).expect("legacy source should write");

        let connection = database();
        connection
            .execute(
                "INSERT INTO plugin_states (
                    plugin_id, package_path, origin, manifest_fingerprint, enabled,
                    permissions_reviewed, granted_capabilities, installed_at, updated_at
                 ) VALUES (?1, ?2, 'user', 'legacy', 1, 1, ?3, 1, 2)",
                params![
                    legacy_manifest.id,
                    legacy_path.to_string_lossy(),
                    serde_json::to_string(&BTreeSet::from([SourceCapability::NetworkAny]))
                        .expect("capabilities should serialize")
                ],
            )
            .expect("legacy state should write");
        connection
            .execute(
                "INSERT INTO plugin_diagnostics (
                    plugin_id, code, level, source_id, message, timestamp
                 ) VALUES (?1, 'legacy', 'warn', NULL, 'migrated diagnostic', 3)",
                params![legacy_manifest.id],
            )
            .expect("legacy diagnostic should write");

        assert_eq!(
            migrate_legacy_lx_plugins(&connection, &plugins_dir, &audio_sources_dir)
                .expect("legacy source should migrate"),
            1
        );
        assert!(!legacy_path.exists());
        assert!(audio_sources_dir
            .join(&legacy_manifest.id)
            .join(AUDIO_SOURCE_MANIFEST_FILE)
            .is_file());
        let remaining_plugin_state = connection
            .query_row(
                "SELECT COUNT(*) FROM plugin_states WHERE plugin_id = ?1",
                params![legacy_manifest.id],
                |row| row.get::<_, i64>(0),
            )
            .expect("Plugin state should query");
        assert_eq!(remaining_plugin_state, 0);

        let mut registry =
            AudioSourceRegistry::new(&audio_sources_dir, Arc::new(SourceRuntime::new()));
        let records = registry
            .refresh(&connection)
            .expect("migrated source should refresh");
        assert_eq!(records.len(), 1);
        assert!(records[0].enabled);
        assert!(records[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "migrated diagnostic"));
    }

    #[test]
    fn remote_source_urls_require_http_and_normalize_github_blob_links() {
        let error = parse_remote_source_url("file:///tmp/source.js")
            .expect_err("file URLs should be rejected");
        let github = parse_remote_source_url(
            "https://github.com/example/sources/blob/main/source.js?plain=1&raw=0#L10",
        )
        .expect("GitHub URL should parse");
        let normalized = normalize_remote_source_url(github);

        assert!(matches!(
            error,
            AudioSourceSystemError::Package(message) if message.contains("HTTP or HTTPS")
        ));
        assert_eq!(normalized.fragment(), None);
        let query_pairs = normalized.query_pairs().collect::<Vec<_>>();
        assert!(query_pairs
            .iter()
            .any(|(key, value)| key == "plain" && value == "1"));
        assert_eq!(
            query_pairs
                .iter()
                .filter(|(key, value)| key == "raw" && value == "1")
                .count(),
            1
        );
    }

    #[test]
    fn managed_package_identifiers_must_start_with_an_alphanumeric_character() {
        assert!(valid_identifier("source.example"));
        assert!(!valid_identifier(".."));
        assert!(!valid_identifier("-source"));
    }
}
