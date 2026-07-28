use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;

use crate::registry_support::valid_identifier;
use crate::source_runtime::{
    SourceCapability, SourceInfo, SourceProvider, SourceRuntimeApiVersion,
};

use super::{
    provider_declared_capabilities, valid_entrypoint, PluginManifest, PluginProviderEntrypoint,
    PluginSystemError,
};

/// Values supplied by the Plugin System when a registered Provider is built.
///
/// A factory receives only normalized host-owned values. Package code is not
/// loaded or executed by the registration mechanism.
#[derive(Debug, Clone)]
pub struct PluginProviderBuildContext {
    pub plugin_id: String,
    pub provider_id: String,
    pub package_path: PathBuf,
    pub declared_capabilities: BTreeSet<SourceCapability>,
    pub source_catalog: BTreeMap<String, SourceInfo>,
}

/// The stable host contract for one symbolic Provider entrypoint.
///
/// Production entrypoints are reserved for one Plugin ID and one Provider ID.
/// The manifest must declare the same Source Runtime version and include every
/// capability and host bridge required by this contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginProviderContract {
    plugin_id: Option<String>,
    provider_id: Option<String>,
    entrypoint: String,
    supported_api_version: Option<SourceRuntimeApiVersion>,
    required_capabilities: BTreeSet<SourceCapability>,
    required_host_bridges: BTreeSet<String>,
}

impl PluginProviderContract {
    pub fn new<I, S>(
        plugin_id: impl Into<String>,
        provider_id: impl Into<String>,
        entrypoint: impl Into<String>,
        supported_api_version: SourceRuntimeApiVersion,
        required_capabilities: impl IntoIterator<Item = SourceCapability>,
        required_host_bridges: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            plugin_id: Some(plugin_id.into()),
            provider_id: Some(provider_id.into()),
            entrypoint: entrypoint.into(),
            supported_api_version: Some(supported_api_version),
            required_capabilities: required_capabilities.into_iter().collect(),
            required_host_bridges: required_host_bridges.into_iter().map(Into::into).collect(),
        }
    }

    #[cfg(test)]
    pub(super) fn unscoped_test_entrypoint(entrypoint: impl Into<String>) -> Self {
        Self {
            plugin_id: None,
            provider_id: None,
            entrypoint: entrypoint.into(),
            supported_api_version: None,
            required_capabilities: BTreeSet::new(),
            required_host_bridges: BTreeSet::new(),
        }
    }

    pub fn entrypoint(&self) -> &str {
        &self.entrypoint
    }

    fn validate_definition(&self) -> Result<(), PluginSystemError> {
        if self
            .plugin_id
            .as_deref()
            .is_some_and(|id| !valid_identifier(id))
        {
            return Err(PluginSystemError::InvalidRegistration(format!(
                "{} has an invalid Plugin ID",
                self.entrypoint
            )));
        }
        if self
            .provider_id
            .as_deref()
            .is_some_and(|id| !valid_identifier(id))
        {
            return Err(PluginSystemError::InvalidRegistration(format!(
                "{} has an invalid Provider ID",
                self.entrypoint
            )));
        }
        if !valid_entrypoint(&self.entrypoint) {
            return Err(PluginSystemError::InvalidRegistration(format!(
                "invalid Provider entrypoint: {}",
                self.entrypoint
            )));
        }
        if let Some(bridge) = self
            .required_host_bridges
            .iter()
            .find(|bridge| !valid_identifier(bridge))
        {
            return Err(PluginSystemError::InvalidRegistration(format!(
                "{} has an invalid host bridge ID: {bridge}",
                self.entrypoint
            )));
        }
        Ok(())
    }

    fn append_manifest_errors(
        &self,
        manifest: &PluginManifest,
        provider: &PluginProviderEntrypoint,
        errors: &mut Vec<String>,
    ) {
        if self
            .plugin_id
            .as_deref()
            .is_some_and(|plugin_id| plugin_id != manifest.id)
        {
            errors.push(format!(
                "entrypoint {} is reserved for Plugin {}",
                self.entrypoint,
                self.plugin_id.as_deref().unwrap_or_default()
            ));
        }
        if self
            .provider_id
            .as_deref()
            .is_some_and(|provider_id| provider_id != provider.id)
        {
            errors.push(format!(
                "entrypoint {} is reserved for Provider {}",
                self.entrypoint,
                self.provider_id.as_deref().unwrap_or_default()
            ));
        }
        if self
            .supported_api_version
            .is_some_and(|version| version != manifest.supported_api_version)
        {
            errors.push(format!(
                "entrypoint {} requires Source Runtime API {}, but the manifest declares {}",
                self.entrypoint,
                self.supported_api_version
                    .map(|version| version.to_string())
                    .unwrap_or_default(),
                manifest.supported_api_version
            ));
        }

        let declared_capabilities = provider_declared_capabilities(manifest, provider);
        for capability in self
            .required_capabilities
            .difference(&declared_capabilities)
        {
            errors.push(format!(
                "entrypoint {} requires capability {}",
                self.entrypoint,
                capability.as_str()
            ));
        }
        for bridge in self
            .required_host_bridges
            .difference(&manifest.required_host_bridges)
        {
            errors.push(format!(
                "entrypoint {} requires host bridge {bridge}",
                self.entrypoint
            ));
        }
    }
}

type ProviderFactory =
    dyn Fn(PluginProviderBuildContext) -> Result<Arc<dyn SourceProvider>, String> + Send + Sync;

/// Connects a [`PluginProviderContract`] to its in-process Provider factory.
#[derive(Clone)]
pub struct PluginProviderRegistration {
    contract: PluginProviderContract,
    factory: Arc<ProviderFactory>,
}

impl fmt::Debug for PluginProviderRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginProviderRegistration")
            .field("contract", &self.contract)
            .finish_non_exhaustive()
    }
}

impl PluginProviderRegistration {
    pub fn new<F>(contract: PluginProviderContract, factory: F) -> Self
    where
        F: Fn(PluginProviderBuildContext) -> Result<Arc<dyn SourceProvider>, String>
            + Send
            + Sync
            + 'static,
    {
        Self {
            contract,
            factory: Arc::new(factory),
        }
    }
}

/// Registry of symbolic Provider contracts and their host-owned factories.
///
/// Manifest validation and Provider construction both cross this interface, so
/// an entrypoint cannot be documented as supported without being registered.
#[derive(Clone, Default)]
pub struct PluginProviderCatalog {
    contracts: BTreeMap<String, PluginProviderContract>,
    factories: BTreeMap<String, Arc<ProviderFactory>>,
    available_host_bridges: BTreeSet<String>,
}

impl fmt::Debug for PluginProviderCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginProviderCatalog")
            .field("entrypoints", &self.contracts.keys().collect::<Vec<_>>())
            .field(
                "factory_entrypoints",
                &self.factories.keys().collect::<Vec<_>>(),
            )
            .field("available_host_bridges", &self.available_host_bridges)
            .finish()
    }
}

impl PluginProviderCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_contract(
        mut self,
        contract: PluginProviderContract,
    ) -> Result<Self, PluginSystemError> {
        self.register_contract(contract)?;
        Ok(self)
    }

    pub fn with_registration(
        mut self,
        registration: PluginProviderRegistration,
    ) -> Result<Self, PluginSystemError> {
        self.register_provider(registration)?;
        Ok(self)
    }

    pub fn register_contract(
        &mut self,
        contract: PluginProviderContract,
    ) -> Result<(), PluginSystemError> {
        contract.validate_definition()?;
        if let Some(existing) = self.contracts.get(contract.entrypoint()) {
            if existing == &contract {
                return Ok(());
            }
            return Err(PluginSystemError::InvalidRegistration(format!(
                "Provider entrypoint {} has conflicting contracts",
                contract.entrypoint()
            )));
        }
        self.contracts.insert(contract.entrypoint.clone(), contract);
        Ok(())
    }

    pub fn register_provider(
        &mut self,
        registration: PluginProviderRegistration,
    ) -> Result<(), PluginSystemError> {
        let entrypoint = registration.contract.entrypoint.clone();
        if self.factories.contains_key(&entrypoint) {
            return Err(PluginSystemError::InvalidRegistration(format!(
                "Provider entrypoint {entrypoint} has more than one factory"
            )));
        }
        self.register_contract(registration.contract.clone())?;
        self.available_host_bridges
            .extend(registration.contract.required_host_bridges.iter().cloned());
        self.factories.insert(entrypoint, registration.factory);
        Ok(())
    }

    /// Validates both the versioned manifest schema and host entrypoint rules.
    pub fn validate_manifest(&self, manifest: &PluginManifest) -> Result<(), Vec<String>> {
        let mut errors = manifest.validate().err().unwrap_or_default();
        for provider in &manifest.provider_entrypoints {
            let Some(contract) = self.contracts.get(&provider.entrypoint) else {
                errors.push(format!(
                    "provider {} uses unregistered entrypoint {}",
                    provider.id, provider.entrypoint
                ));
                continue;
            };
            contract.append_manifest_errors(manifest, provider, &mut errors);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub(super) fn available_host_bridges(&self) -> &BTreeSet<String> {
        &self.available_host_bridges
    }

    pub(super) fn build_provider(
        &self,
        manifest: &PluginManifest,
        entrypoint: &PluginProviderEntrypoint,
        package_path: PathBuf,
    ) -> Result<Arc<dyn SourceProvider>, PluginSystemError> {
        let Some(contract) = self.contracts.get(&entrypoint.entrypoint) else {
            return Err(provider_load_error(
                manifest,
                entrypoint,
                "the Provider entrypoint is not registered",
            ));
        };
        let mut errors = Vec::new();
        contract.append_manifest_errors(manifest, entrypoint, &mut errors);
        if !errors.is_empty() {
            return Err(PluginSystemError::InvalidManifest(errors.join("; ")));
        }
        let Some(factory) = self.factories.get(&entrypoint.entrypoint) else {
            return Err(provider_load_error(
                manifest,
                entrypoint,
                "the Provider entrypoint has no factory in this host",
            ));
        };
        let declared_capabilities = provider_declared_capabilities(manifest, entrypoint);
        let context = PluginProviderBuildContext {
            plugin_id: manifest.id.clone(),
            provider_id: entrypoint.id.clone(),
            package_path,
            declared_capabilities: declared_capabilities.clone(),
            source_catalog: entrypoint.source_catalog.clone(),
        };
        let provider = catch_unwind(AssertUnwindSafe(|| factory(context)))
            .map_err(|_| {
                provider_load_error(manifest, entrypoint, "the Provider factory panicked")
            })?
            .map_err(|message| provider_load_error(manifest, entrypoint, message))?;

        let provider_id =
            catch_unwind(AssertUnwindSafe(|| provider.id().to_owned())).map_err(|_| {
                provider_load_error(manifest, entrypoint, "reading the Provider ID panicked")
            })?;
        if provider_id != entrypoint.id {
            return Err(provider_load_error(
                manifest,
                entrypoint,
                format!(
                    "the factory returned Provider {provider_id}, expected {}",
                    entrypoint.id
                ),
            ));
        }
        let provider_api_version = catch_unwind(AssertUnwindSafe(|| provider.api_version()))
            .map_err(|_| {
                provider_load_error(
                    manifest,
                    entrypoint,
                    "reading the Provider API version panicked",
                )
            })?;
        if contract
            .supported_api_version
            .is_some_and(|expected| provider_api_version != expected)
        {
            return Err(provider_load_error(
                manifest,
                entrypoint,
                format!(
                    "the factory returned Provider API {provider_api_version}, expected {}",
                    contract
                        .supported_api_version
                        .map(|version| version.to_string())
                        .unwrap_or_default()
                ),
            ));
        }
        let provider_capabilities =
            catch_unwind(AssertUnwindSafe(|| provider.required_capabilities())).map_err(|_| {
                provider_load_error(
                    manifest,
                    entrypoint,
                    "reading the Provider capabilities panicked",
                )
            })?;
        if provider_capabilities != declared_capabilities {
            return Err(provider_load_error(
                manifest,
                entrypoint,
                "the factory Provider capabilities do not match the manifest declaration",
            ));
        }
        Ok(provider)
    }
}

fn provider_load_error(
    manifest: &PluginManifest,
    entrypoint: &PluginProviderEntrypoint,
    message: impl Into<String>,
) -> PluginSystemError {
    PluginSystemError::ProviderLoad {
        plugin_id: manifest.id.clone(),
        entrypoint: entrypoint.entrypoint.clone(),
        message: message.into(),
    }
}
