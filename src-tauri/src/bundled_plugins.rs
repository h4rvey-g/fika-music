use std::sync::Arc;

use crate::kugou::{
    KugouProviderBridge, KugouSourceProvider, KUGOU_HOST_BRIDGE_ID, KUGOU_PLUGIN_ID,
    KUGOU_PROVIDER_API_VERSION, KUGOU_PROVIDER_ENTRYPOINT, KUGOU_PROVIDER_ID,
};
use crate::netease::{
    NeteaseProviderBridge, NeteaseSourceProvider, NETEASE_HOST_BRIDGE_ID, NETEASE_PLUGIN_ID,
    NETEASE_PROVIDER_API_VERSION, NETEASE_PROVIDER_ENTRYPOINT, NETEASE_PROVIDER_ID,
};
use crate::plugin_system::{
    PluginProviderCatalog, PluginProviderContract, PluginProviderRegistration, PluginSystemError,
};
use crate::source_runtime::{SourceCapability, SourceProvider};

/// Returns the manifest contracts for every production Provider entrypoint.
///
/// This catalog has no factories and is intended for package validation tools.
pub fn contract_catalog() -> Result<PluginProviderCatalog, PluginSystemError> {
    PluginProviderCatalog::new()
        .with_contract(netease_contract())?
        .with_contract(kugou_contract())
}

/// Wires every production Provider entrypoint to its host-owned dependencies.
pub fn provider_catalog(
    netease_bridge: Arc<dyn NeteaseProviderBridge>,
    kugou_bridge: Arc<dyn KugouProviderBridge>,
) -> Result<PluginProviderCatalog, PluginSystemError> {
    let netease_registration =
        PluginProviderRegistration::new(netease_contract(), move |context| {
            let provider: Arc<dyn SourceProvider> = Arc::new(NeteaseSourceProvider::new(
                context.provider_id,
                context.declared_capabilities,
                Arc::clone(&netease_bridge),
            ));
            Ok(provider)
        });
    let kugou_registration = PluginProviderRegistration::new(kugou_contract(), move |context| {
        let provider: Arc<dyn SourceProvider> = Arc::new(KugouSourceProvider::new(
            context.provider_id,
            context.declared_capabilities,
            Arc::clone(&kugou_bridge),
        ));
        Ok(provider)
    });

    PluginProviderCatalog::new()
        .with_registration(netease_registration)?
        .with_registration(kugou_registration)
}

fn netease_contract() -> PluginProviderContract {
    PluginProviderContract::new(
        NETEASE_PLUGIN_ID,
        NETEASE_PROVIDER_ID,
        NETEASE_PROVIDER_ENTRYPOINT,
        NETEASE_PROVIDER_API_VERSION,
        [
            SourceCapability::AccountRef,
            SourceCapability::PlaylistRead,
            SourceCapability::PlaylistWrite,
            SourceCapability::BridgeNeteaseApiEnhanced,
        ],
        [NETEASE_HOST_BRIDGE_ID],
    )
}

fn kugou_contract() -> PluginProviderContract {
    PluginProviderContract::new(
        KUGOU_PLUGIN_ID,
        KUGOU_PROVIDER_ID,
        KUGOU_PROVIDER_ENTRYPOINT,
        KUGOU_PROVIDER_API_VERSION,
        [
            SourceCapability::AccountRef,
            SourceCapability::PlaylistRead,
            SourceCapability::PlaylistWrite,
            SourceCapability::BridgeKugouMusicApi,
        ],
        [KUGOU_HOST_BRIDGE_ID],
    )
}
