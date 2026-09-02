use std::fs;
use std::path::Path;
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
use crate::youtube_music::{
    YoutubeMusicSourceProvider, YOUTUBE_MUSIC_PLUGIN_ID, YOUTUBE_MUSIC_PROVIDER_API_VERSION,
    YOUTUBE_MUSIC_PROVIDER_ENTRYPOINT, YOUTUBE_MUSIC_PROVIDER_ID,
};

const PACKAGE_FILES: &[(&str, &[u8])] = &[
    (
        "kugou/plugin.json",
        include_bytes!("../plugins/kugou/plugin.json"),
    ),
    ("kugou/LICENSE", include_bytes!("../plugins/kugou/LICENSE")),
    (
        "netease/plugin.json",
        include_bytes!("../plugins/netease/plugin.json"),
    ),
    (
        "youtube-music/plugin.json",
        include_bytes!("../plugins/youtube-music/plugin.json"),
    ),
];

pub(crate) fn materialize_packages(root: &Path) -> Result<(), std::io::Error> {
    if root.is_dir() {
        fs::remove_dir_all(root)?;
    } else if root.exists() {
        fs::remove_file(root)?;
    }
    fs::create_dir_all(root)?;
    for (relative_path, contents) in PACKAGE_FILES {
        let target = root.join(relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, contents)?;
    }
    Ok(())
}

/// Returns the manifest contracts for every production Provider entrypoint.
///
/// This catalog has no factories and is intended for package validation tools.
pub fn contract_catalog() -> Result<PluginProviderCatalog, PluginSystemError> {
    PluginProviderCatalog::new()
        .with_contract(netease_contract())?
        .with_contract(kugou_contract())?
        .with_contract(youtube_music_contract())
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
    let youtube_music_registration =
        PluginProviderRegistration::new(youtube_music_contract(), move |context| {
            let provider: Arc<dyn SourceProvider> = Arc::new(YoutubeMusicSourceProvider::new(
                context.provider_id,
                context.declared_capabilities,
            ));
            Ok(provider)
        });

    PluginProviderCatalog::new()
        .with_registration(netease_registration)?
        .with_registration(kugou_registration)?
        .with_registration(youtube_music_registration)
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

fn youtube_music_contract() -> PluginProviderContract {
    PluginProviderContract::new(
        YOUTUBE_MUSIC_PLUGIN_ID,
        YOUTUBE_MUSIC_PROVIDER_ID,
        YOUTUBE_MUSIC_PROVIDER_ENTRYPOINT,
        YOUTUBE_MUSIC_PROVIDER_API_VERSION,
        [SourceCapability::NetworkAny],
        std::iter::empty::<String>(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rusqlite::Connection;

    use super::*;
    use crate::plugin_system::PluginRegistry;
    use crate::source_runtime::SourceRuntime;

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn materialized_packages_should_expose_the_mobile_bundled_plugins() {
        let id = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "fika-music-bundled-plugins-{}-{id}",
            std::process::id()
        ));
        let bundled = root.join("bundled");
        let user = root.join("user");
        fs::create_dir_all(&user).expect("user plugin directory should be created");
        materialize_packages(&bundled).expect("bundled packages should be materialized");
        let mut connection = Connection::open_in_memory().expect("database should open");
        crate::database::initialize(&mut connection).expect("database should initialize");
        let catalog = contract_catalog().expect("bundled contracts should register");
        let mut registry = PluginRegistry::new(&user, &bundled, Arc::new(SourceRuntime::new()))
            .with_provider_catalog(catalog);

        let plugin_ids = registry
            .refresh(&connection)
            .expect("registry should refresh")
            .into_iter()
            .map(|record| record.id)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            plugin_ids,
            BTreeSet::from([
                KUGOU_PLUGIN_ID.to_owned(),
                NETEASE_PLUGIN_ID.to_owned(),
                YOUTUBE_MUSIC_PLUGIN_ID.to_owned(),
            ])
        );
        fs::remove_dir_all(root).expect("test directory should be removed");
    }
}
