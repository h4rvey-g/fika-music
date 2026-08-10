pub use audio_source_system::AudioSourceAvailability;
use audio_source_system::{
    AudioSourceCommandError, AudioSourceRecord, AudioSourceRegistry, AudioSourceSystemError,
};
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::tag::{Accessor, ItemKey, Tag};
use plugin_system::{PluginDiagnostic, PluginRecord, PluginRegistry, PluginSystemError};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::convert::TryFrom;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use walkdir::WalkDir;

mod account_commands;
mod album_art;
pub mod audio_source_system;
pub mod bundled_plugins;
mod chksz_playback;
mod collections;
mod database;
mod download_source_router;
pub mod kugou;
mod library;
mod library_watcher;
pub mod lx_js_importer;
mod lx_js_runtime;
pub mod lyrics;
mod menu_bar_lyrics;
pub mod netease;
pub mod online_download;
mod online_execution;
pub mod online_music;
mod online_settings_commands;
mod playback_commands;
pub mod plugin_system;
mod registry_support;
mod source_request_registry;
pub mod source_runtime;
mod youtube_media_proxy;
pub mod youtube_music;
pub mod youtube_music_playback;
mod yt_dlp_sidecar;

use account_commands::{
    cancel_kugou_qr_login, cancel_netease_qr_login, disconnect_kugou_account,
    disconnect_netease_account, list_kugou_accounts, list_netease_accounts,
    list_netease_mutation_audit, poll_kugou_qr_login, poll_netease_qr_login, start_kugou_qr_login,
    start_netease_qr_login,
};
pub use account_commands::{KugouCommandError, NeteaseCommandError};
pub use album_art::{
    AlbumArtSettings, AlbumArtTaskStatus, AlbumCoverCandidate, AlbumCoverResult, AlbumCoverStatus,
    LibraryTaskState, MetadataLookupItemResult, MetadataLookupTaskStatus,
};
pub use collections::{
    MusicCollectionDetail, MusicCollectionItem, MusicCollectionItemKind, MusicCollectionMutation,
    MusicCollectionSummary, SmartCollectionField, SmartCollectionOperator, SmartCollectionRule,
    SmartCollectionRules,
};
pub use library::{
    LibraryAlbumGroup, LibraryGroupToggleResult, LibraryPlaybackQueue, LibraryQueryPage,
    LibraryQueryRequest, LibraryQueueTrack, LibrarySelectionRange, LibrarySelectionRequest,
    LibrarySortDirection, LibrarySortField, LibraryTextField, LibraryViewItem, LibraryViewItemKind,
    LibraryViewRange,
};
use library_watcher::{BatchDisposition, LibraryChangeBatch, LibraryWatcher};
use online_settings_commands::{
    clear_online_search_history, get_online_music_settings, select_online_download_directory,
    update_online_music_settings,
};
use playback_commands::{
    local_track_media_source, local_track_playback_details, resolve_remote_track_lyrics,
};

const SCAN_PROGRESS_EVENT: &str = "library:scan-progress";
const LIBRARY_CHANGED_EVENT: &str = "library:changed";
const ALBUM_ART_PROGRESS_EVENT: &str = "library:album-art-progress";
const METADATA_LOOKUP_PROGRESS_EVENT: &str = "library:metadata-lookup-progress";
const ONLINE_SEARCH_SECTION_EVENT: &str = "online-music:search-section";
const ONLINE_DOWNLOAD_TASK_EVENT: &str = "online-music:download-task";
const ONLINE_DOWNLOAD_PROGRESS_EVENT: &str = "online-music:download-progress";
const ONLINE_DOWNLOAD_COMPLETED_EVENT: &str = "online-music:download-completed";
const LIBRARY_METADATA_VERSION: i64 = 1;
const LIBRARY_FOLDER_SETTING_KEY: &str = "local_music_folder";
const MAX_ONLINE_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_DOWNLOAD_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(60);
const YT_DLP_DOWNLOAD_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(120);

macro_rules! with_tauri_commands {
    ($consumer:ident) => {
        $consumer! {
            select_music_folder,
            start_library_scan,
            get_scan_status,
            query_local_library,
            local_library_view_range,
            local_library_track_position,
            set_local_library_group_collapsed,
            list_music_collections,
            create_music_collection,
            rename_music_collection,
            delete_music_collection,
            get_music_collection,
            add_local_selection_to_music_collection,
            add_online_tracks_to_music_collection,
            add_music_collection_items_to_music_collection,
            remove_music_collection_items,
            get_album_art_settings,
            set_album_art_network_enabled,
            resolve_local_album_cover,
            get_album_art_task_status,
            start_album_art_backfill,
            resume_album_art_backfill,
            pause_album_art_backfill,
            get_metadata_lookup_task_status,
            start_local_metadata_lookup,
            start_music_collection_metadata_lookup,
            resume_local_metadata_lookup,
            pause_local_metadata_lookup,
            create_local_library_playback_queue,
            local_library_queue_track,
            increment_local_track_play_count,
            local_track_media_source,
            local_track_playback_details,
            resolve_remote_track_lyrics,
            set_menu_bar_lyrics,
            get_online_music_settings,
            update_online_music_settings,
            list_online_music_channels,
            online_music_recommendations,
            online_music_playlists,
            online_music_suggestions,
            start_online_music_search,
            online_music_search_page,
            online_music_artist_tracks,
            online_music_artist_albums,
            online_music_artist_biography,
            online_music_album_tracks,
            online_music_playlist_tracks,
            clear_online_search_history,
            select_online_download_directory,
            create_online_download_task,
            list_online_download_tasks,
            start_online_download_task,
            pause_online_download_task,
            cancel_online_download_task,
            retry_online_download_item,
            refresh_online_download_item_candidates,
            cancel_source_request,
            list_audio_sources,
            select_audio_source_file,
            refresh_audio_sources,
            import_audio_source,
            import_audio_source_url,
            set_audio_source_capabilities,
            set_audio_source_enabled,
            remove_audio_source,
            clear_audio_source_diagnostics,
            dispatch_audio_source_request,
            check_audio_source_availability,
            get_chksz_api_key_status,
            set_chksz_api_key,
            clear_chksz_api_key,
            list_plugins,
            select_plugin_package,
            refresh_plugins,
            install_plugin_package,
            set_plugin_capabilities,
            set_plugin_enabled,
            remove_plugin,
            clear_plugin_diagnostics,
            dispatch_plugin_request,
            start_netease_qr_login,
            poll_netease_qr_login,
            cancel_netease_qr_login,
            list_netease_accounts,
            disconnect_netease_account,
            list_netease_mutation_audit,
            start_kugou_qr_login,
            poll_kugou_qr_login,
            cancel_kugou_qr_login,
            list_kugou_accounts,
            disconnect_kugou_account,
        }
    };
}

macro_rules! declare_command_names {
    ($($command:ident),* $(,)?) => {
        pub const TAURI_COMMAND_NAMES: &[&str] = &[$(stringify!($command)),*];
    };
}

macro_rules! generate_command_handler {
    ($($command:ident),* $(,)?) => {
        tauri::generate_handler![$($command),*]
    };
}

with_tauri_commands!(declare_command_names);

type AppResult<T> = Result<T, AppError>;
type CommandResult<T> = Result<T, String>;

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("database migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("file system error: {0}")]
    Io(#[from] std::io::Error),
    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("metadata parse failed for {path}: {source}")]
    Metadata {
        path: String,
        source: lofty::error::LoftyError,
    },
    #[error("internal state lock was poisoned: {0}")]
    StatePoisoned(&'static str),
    #[error("plugin system error: {0}")]
    Plugin(#[from] PluginSystemError),
    #[error("audio source system error: {0}")]
    AudioSource(#[from] AudioSourceSystemError),
    #[error("NetEase service error: {0}")]
    Netease(#[from] netease::NeteaseBridgeError),
    #[error("KuGou service error: {0}")]
    Kugou(#[from] kugou::KugouBridgeError),
    #[error("music folder does not exist or is not a directory: {0}")]
    InvalidMusicFolder(String),
    #[error("a library scan is already running")]
    ScanAlreadyRunning,
    #[error("local track was not found: {0}")]
    TrackNotFound(i64),
    #[error("local track file is missing: {0}")]
    TrackFileMissing(String),
    #[error("library error: {0}")]
    Library(#[from] library::LibraryError),
    #[error("collection error: {0}")]
    Collection(#[from] collections::CollectionError),
    #[error("library watcher error: {0}")]
    LibraryWatcher(#[from] library_watcher::LibraryWatcherError),
    #[error("album-art error: {0}")]
    AlbumArt(#[from] album_art::AlbumArtError),
    #[error("online music error: {0}")]
    OnlineMusic(#[from] online_music::OnlineMusicError),
    #[error("online download error: {0}")]
    OnlineDownload(#[from] online_download::OnlineDownloadError),
    #[error("online execution error: {0}")]
    OnlineExecution(#[from] online_execution::OnlineExecutionError),
    #[error("yt-dlp sidecar error: {0}")]
    YtDlp(#[from] yt_dlp_sidecar::YtDlpSidecarError),
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LocalTrack {
    id: i64,
    file_path: String,
    file_name: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    genre: Option<String>,
    year: Option<i64>,
    codec: Option<String>,
    bitrate_kbps: Option<i64>,
    sample_rate_hz: Option<i64>,
    duration_seconds: Option<i64>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    file_size_bytes: i64,
    modified_at: Option<i64>,
    indexed_at: i64,
    play_count: i64,
}

#[derive(Debug)]
struct LocalTrackDraft {
    file_path: String,
    file_name: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    genre: Option<String>,
    year: Option<i64>,
    codec: Option<String>,
    bitrate_kbps: Option<i64>,
    sample_rate_hz: Option<i64>,
    duration_seconds: Option<i64>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    file_size_bytes: i64,
    modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MediaSource {
    file_path: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct RemoteCommandError {
    message: String,
    diagnostics: Vec<source_runtime::SourceDiagnostic>,
}

type RemoteCommandResult<T> = Result<T, RemoteCommandError>;

fn remote_error(message: impl Into<String>) -> RemoteCommandError {
    RemoteCommandError {
        message: message.into(),
        diagnostics: Vec::new(),
    }
}

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct ScanStatus {
    is_running: bool,
    folder_path: Option<String>,
    discovered_files: usize,
    scanned_files: usize,
    indexed_tracks: usize,
    skipped_files: usize,
    error_count: usize,
    last_error: Option<String>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct ScanProgressEvent {
    status: ScanStatus,
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LibraryChangedEvent {
    added_or_updated: usize,
    removed: usize,
}

struct AppState {
    db: Arc<Mutex<Connection>>,
    library: Arc<Mutex<library::LibraryService>>,
    album_art: Arc<album_art::AlbumArtService>,
    scan_status: Mutex<ScanStatus>,
    library_sync: Mutex<()>,
    library_watcher: Mutex<Option<LibraryWatcher>>,
    source_requests: source_request_registry::SourceRequestRegistry,
    online_download_requests: Mutex<BTreeMap<String, source_runtime::SourceCancellationToken>>,
    download_source_router: Mutex<download_source_router::DownloadSourceRouter>,
    online_music_cache: Arc<online_music::OnlineMusicCache>,
    online_executor: Arc<online_execution::OnlineExecutor>,
    yt_dlp_sidecar: Arc<yt_dlp_sidecar::YtDlpSidecar>,
    audio_source_registry: Mutex<AudioSourceRegistry>,
    plugin_registry: Mutex<PluginRegistry>,
    chksz_playback: Arc<chksz_playback::ChkszPlaybackService>,
    netease_bridge: Arc<netease::NeteaseServiceBridge>,
    kugou_bridge: Arc<kugou::KugouServiceBridge>,
}

#[derive(Debug, Default)]
struct DiscoveredAudioFiles {
    files: Vec<PathBuf>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalTrackSignature {
    file_size_bytes: i64,
    modified_at: Option<i64>,
    metadata_version: i64,
}

#[derive(Debug, Default)]
struct LibraryReconcileResult {
    added_or_updated: usize,
    removed: usize,
    errors: Vec<String>,
}

#[derive(Debug)]
struct LibraryReconcileScope {
    root: PathBuf,
    complete: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct LibraryReconcileProgress {
    discovered_files: usize,
    scanned_files: usize,
    indexed_tracks: usize,
    skipped_files: usize,
}

impl AppState {
    #[cfg(test)]
    fn new(db_path: &Path) -> AppResult<Self> {
        let app_data_dir = db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::new_with_plugin_dirs(
            db_path,
            app_data_dir.join("plugins"),
            app_data_dir.join("bundled-plugins"),
        )
    }

    fn new_with_plugin_dirs(
        db_path: &Path,
        user_plugins_dir: PathBuf,
        bundled_plugins_dir: PathBuf,
    ) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
            restrict_path_to_current_user(parent, 0o700)?;
        }

        let mut connection = Connection::open(db_path)?;
        restrict_path_to_current_user(db_path, 0o600)?;
        database::initialize(&mut connection)?;
        let db = Arc::new(Mutex::new(connection));
        let configured_music_folder = {
            let connection = db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
            online_download::recover_interrupted_tasks(&connection, now_timestamp())?;
            load_library_folder(&connection)?
        };
        let library = {
            let connection = db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
            library::LibraryService::load(&connection)?
        };
        let library = Arc::new(Mutex::new(library));
        let album_art = Arc::new(album_art::AlbumArtService::new(
            Arc::clone(&db),
            Arc::clone(&library),
        )?);
        let online_executor = Arc::new(online_execution::OnlineExecutor::new(4)?);
        let source_host = Arc::new(source_runtime::DefaultSourceHost::new(
            Duration::from_secs(8),
            4 * 1024 * 1024,
        ));
        let runtime_host: Arc<dyn source_runtime::SourceHost> = source_host.clone();
        let source_runtime = Arc::new(source_runtime::SourceRuntime::with_host(
            Arc::clone(&runtime_host),
            [],
        ));
        let chksz_playback = Arc::new(chksz_playback::ChkszPlaybackService::new(
            Arc::clone(&db),
            runtime_host,
        ));
        let netease_bridge = Arc::new(netease::NeteaseServiceBridge::new(
            Arc::clone(&db),
            Arc::clone(&source_host),
        )?);
        let provider_bridge: Arc<dyn netease::NeteaseProviderBridge> = netease_bridge.clone();
        let kugou_bridge = Arc::new(kugou::KugouServiceBridge::new(
            Arc::clone(&db),
            source_host,
        )?);
        let kugou_provider_bridge: Arc<dyn kugou::KugouProviderBridge> = kugou_bridge.clone();
        let audio_sources_dir = user_plugins_dir
            .parent()
            .map(|path| path.join("audio-sources"))
            .unwrap_or_else(|| PathBuf::from("audio-sources"));
        let yt_dlp_sidecar = Arc::new(yt_dlp_sidecar::YtDlpSidecar::new(
            user_plugins_dir
                .parent()
                .map(|path| path.join("runtime").join("yt-dlp"))
                .unwrap_or_else(|| PathBuf::from("runtime").join("yt-dlp")),
        )?);
        {
            let connection = db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
            audio_source_system::migrate_legacy_lx_plugins(
                &connection,
                &user_plugins_dir,
                &audio_sources_dir,
            )?;
        }
        let mut audio_source_registry =
            AudioSourceRegistry::new(audio_sources_dir, Arc::clone(&source_runtime))
                .with_bundled_source(youtube_music_playback::bundled_audio_source_registration(
                    Arc::clone(&yt_dlp_sidecar),
                ))?
                .with_bundled_source(chksz_playback::bundled_audio_source_registration(
                    Arc::clone(&chksz_playback),
                ))?;
        let provider_catalog =
            bundled_plugins::provider_catalog(provider_bridge, kugou_provider_bridge)?;
        #[cfg(test)]
        let provider_catalog = plugin_system::with_test_provider_registration(provider_catalog)?;
        let mut plugin_registry = PluginRegistry::new(
            user_plugins_dir,
            bundled_plugins_dir,
            Arc::clone(&source_runtime),
        )
        .with_provider_catalog(provider_catalog);
        {
            let connection = db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
            audio_source_registry.refresh(&connection)?;
            plugin_registry.refresh(&connection)?;
        }

        Ok(Self {
            db,
            library,
            album_art,
            scan_status: Mutex::new(ScanStatus {
                folder_path: configured_music_folder.as_deref().map(path_to_string),
                ..ScanStatus::default()
            }),
            library_sync: Mutex::new(()),
            library_watcher: Mutex::new(None),
            source_requests: source_request_registry::SourceRequestRegistry::default(),
            online_download_requests: Mutex::new(BTreeMap::new()),
            download_source_router: Mutex::new(
                download_source_router::DownloadSourceRouter::default(),
            ),
            online_music_cache: Arc::new(online_music::OnlineMusicCache::default()),
            online_executor,
            yt_dlp_sidecar,
            audio_source_registry: Mutex::new(audio_source_registry),
            plugin_registry: Mutex::new(plugin_registry),
            chksz_playback,
            netease_bridge,
            kugou_bridge,
        })
    }
}

#[cfg(unix)]
fn restrict_path_to_current_user(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn restrict_path_to_current_user(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

fn register_source_request(
    state: &AppState,
    request_id: Option<&str>,
) -> RemoteCommandResult<source_runtime::SourceCancellationToken> {
    state
        .source_requests
        .register(request_id)
        .map_err(|error| remote_error(error.to_string()))
}

fn unregister_source_request(state: &AppState, request_id: Option<&str>) {
    state.source_requests.unregister(request_id);
}

#[cfg(test)]
async fn run_remote_request<T, F>(
    state: &AppState,
    request_id: Option<&str>,
    task: F,
    task_failure_message: &'static str,
) -> RemoteCommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(source_runtime::SourceCancellationToken) -> RemoteCommandResult<T> + Send + 'static,
{
    let cancellation = register_source_request(state, request_id)?;
    let result = tauri::async_runtime::spawn_blocking(move || task(cancellation)).await;
    unregister_source_request(state, request_id);

    match result {
        Ok(result) => result,
        Err(error) => Err(remote_error(format!("{task_failure_message}: {error}"))),
    }
}

#[tauri::command]
fn cancel_source_request(state: State<'_, AppState>, request_id: String) -> CommandResult<bool> {
    state
        .source_requests
        .cancel(&request_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_menu_bar_lyrics(
    app: AppHandle,
    enabled: bool,
    line: String,
    title: String,
    subtitle: String,
    max_width: u16,
) -> CommandResult<()> {
    menu_bar_lyrics::update(&app, enabled, &line, &title, &subtitle, max_width)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_online_download_task(
    state: State<'_, AppState>,
    kind: String,
    title: String,
    tracks: Vec<online_music::OnlineTrack>,
    selected_audio_source_id: Option<String>,
    local_music_folder: Option<String>,
) -> CommandResult<online_download::OnlineDownloadTask> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    let mut settings = online_music::load_settings(&db).map_err(|error| error.to_string())?;
    let destination = match settings.download_directory.clone() {
        Some(destination) => destination,
        None => {
            let fallback = local_music_folder
                .as_deref()
                .map(str::trim)
                .filter(|path| Path::new(path).is_dir())
                .ok_or_else(|| {
                    "Choose a download directory in Online Music settings before downloading."
                        .to_owned()
                })?
                .to_owned();
            settings.download_directory = Some(fallback.clone());
            online_music::save_settings(&db, &settings, now_timestamp())
                .map_err(|error| error.to_string())?;
            fallback
        }
    };
    online_download::create_task(
        &db,
        &kind,
        &title,
        Path::new(&destination),
        &tracks,
        selected_audio_source_id.as_deref(),
        now_timestamp(),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_online_download_tasks(
    state: State<'_, AppState>,
) -> CommandResult<Vec<online_download::OnlineDownloadTask>> {
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_download::list_tasks(&db).map_err(|error| error.to_string())
}

#[tauri::command]
fn start_online_download_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> CommandResult<online_download::OnlineDownloadTask> {
    let task_id = task_id.trim().to_owned();
    if task_id.is_empty() {
        return Err("download task id must not be empty".to_owned());
    }
    let (task, cancellation) = prepare_online_download_task_start(state.inner(), &task_id)?;
    emit_online_download_task(&app, &task);
    std::thread::spawn(move || run_online_download_task(app, task_id, cancellation));
    Ok(task)
}

fn prepare_online_download_task_start(
    state: &AppState,
    task_id: &str,
) -> CommandResult<(
    online_download::OnlineDownloadTask,
    source_runtime::SourceCancellationToken,
)> {
    let cancellation = source_runtime::SourceCancellationToken::default();
    let mut active = state
        .online_download_requests
        .lock()
        .map_err(|_| "download request lock was poisoned".to_owned())?;
    if active.contains_key(task_id) {
        return Err("download task is already running".to_owned());
    }
    active.insert(task_id.to_owned(), cancellation.clone());

    let result = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())
        .and_then(|db| {
            online_download::prepare_task_start(&db, task_id, now_timestamp())
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(task) => Ok((task, cancellation)),
        Err(error) => {
            active.remove(task_id);
            Err(error)
        }
    }
}

#[tauri::command]
fn pause_online_download_task(
    state: State<'_, AppState>,
    task_id: String,
) -> CommandResult<online_download::OnlineDownloadTask> {
    cancel_online_download_worker(state.inner(), task_id.trim());
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_download::remove_temporary_files(&db, task_id.trim())
        .map_err(|error| error.to_string())?;
    online_download::mark_pending_items(
        &db,
        task_id.trim(),
        online_download::OnlineDownloadItemState::Paused,
        "Paused by user",
    )
    .map_err(|error| error.to_string())?;
    online_download::set_task_state(
        &db,
        task_id.trim(),
        online_download::OnlineDownloadState::Paused,
        now_timestamp(),
    )
    .map_err(|error| error.to_string())?;
    online_download::task(&db, task_id.trim())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "download task was not found".to_owned())
}

#[tauri::command]
fn cancel_online_download_task(
    state: State<'_, AppState>,
    task_id: String,
) -> CommandResult<online_download::OnlineDownloadTask> {
    cancel_online_download_worker(state.inner(), task_id.trim());
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_download::remove_temporary_files(&db, task_id.trim())
        .map_err(|error| error.to_string())?;
    online_download::mark_pending_items(
        &db,
        task_id.trim(),
        online_download::OnlineDownloadItemState::Cancelled,
        "Cancelled by user",
    )
    .map_err(|error| error.to_string())?;
    online_download::set_task_state(
        &db,
        task_id.trim(),
        online_download::OnlineDownloadState::Cancelled,
        now_timestamp(),
    )
    .map_err(|error| error.to_string())?;
    online_download::task(&db, task_id.trim())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "download task was not found".to_owned())
}

#[tauri::command]
fn retry_online_download_item(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    item_id: String,
) -> CommandResult<online_download::OnlineDownloadTask> {
    ensure_online_download_worker_stopped(state.inner(), task_id.trim())?;
    {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_download::retry_item(&db, task_id.trim(), item_id.trim())
            .map_err(|error| error.to_string())?;
        online_download::set_task_state(
            &db,
            task_id.trim(),
            online_download::OnlineDownloadState::Paused,
            now_timestamp(),
        )
        .map_err(|error| error.to_string())?;
        online_download::refresh_task_counts(&db, task_id.trim(), now_timestamp())
            .map_err(|error| error.to_string())?;
    }
    start_online_download_task(app, state, task_id)
}

#[tauri::command]
async fn refresh_online_download_item_candidates(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    item_id: String,
) -> CommandResult<online_download::OnlineDownloadTask> {
    let task_id = task_id.trim().to_owned();
    let item_id = item_id.trim().to_owned();
    ensure_online_download_worker_stopped(state.inner(), &task_id)?;
    let snapshot = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        let task = online_download::task(&db, &task_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "download task was not found".to_owned())?;
        let item = task
            .items
            .into_iter()
            .find(|item| item.item_id == item_id)
            .ok_or_else(|| "download item was not found".to_owned())?;
        if item.state != online_download::OnlineDownloadItemState::Failed {
            return Err("only failed download items can refresh candidates".to_owned());
        }
        item.track
    };
    let cancellation = source_runtime::SourceCancellationToken::default();
    let keyword = snapshot.title.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.online_music_cache.invalidate();
        online_music_section(
            &state,
            &keyword,
            online_music::OnlineSearchSection::Songs,
            1,
            100,
            cancellation,
        )
    })
    .await
    .map_err(|error| format!("candidate refresh task failed: {error}"))??;
    let online_music::OnlineSearchData::Songs(tracks) = result.data else {
        return Err("candidate refresh returned an unexpected result".to_owned());
    };
    let refreshed = tracks
        .into_iter()
        .find(|track| online_music::track_matches_snapshot(&snapshot, track))
        .ok_or_else(|| "No current channel returned the same song identity.".to_owned())?;
    let db = state
        .db
        .lock()
        .map_err(|_| "database lock was poisoned".to_owned())?;
    online_download::replace_failed_item_track(&db, &task_id, &item_id, &refreshed)
        .map_err(|error| error.to_string())?;
    online_download::task(&db, &task_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "download task was not found".to_owned())
}

fn ensure_online_download_worker_stopped(state: &AppState, task_id: &str) -> CommandResult<()> {
    let active = state
        .online_download_requests
        .lock()
        .map_err(|_| "download request lock was poisoned".to_owned())?;
    if active.contains_key(task_id) {
        return Err("Pause the download task before retrying an item.".to_owned());
    }
    Ok(())
}

fn cancel_online_download_worker(state: &AppState, task_id: &str) {
    if let Ok(active) = state.online_download_requests.lock() {
        if let Some(cancellation) = active.get(task_id) {
            cancellation.cancel();
        }
    }
}

fn emit_online_download_task(app: &AppHandle, task: &online_download::OnlineDownloadTask) {
    let _ = app.emit(ONLINE_DOWNLOAD_TASK_EVENT, task);
}

fn emit_online_download_progress(
    app: &AppHandle,
    task_id: &str,
    item_id: &str,
    state: online_download::OnlineDownloadItemState,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
) {
    let _ = app.emit(
        ONLINE_DOWNLOAD_PROGRESS_EVENT,
        online_download::OnlineDownloadProgressEvent {
            task_id: task_id.to_owned(),
            item_id: item_id.to_owned(),
            state,
            bytes_downloaded,
            total_bytes,
        },
    );
}

fn persist_online_download_progress(
    app: &AppHandle,
    task_id: &str,
    item_id: &str,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
) {
    let state = app.state::<AppState>();
    let persisted = state.db.lock().is_ok_and(|db| {
        online_download::update_item_progress(&db, item_id, bytes_downloaded)
            .is_ok_and(|updated| updated)
    });
    if persisted {
        emit_online_download_progress(
            app,
            task_id,
            item_id,
            online_download::OnlineDownloadItemState::Downloading,
            bytes_downloaded,
            total_bytes,
        );
    }
}

fn run_online_download_task(
    app: AppHandle,
    task_id: String,
    cancellation: source_runtime::SourceCancellationToken,
) {
    let concurrency = {
        let state = app.state::<AppState>();
        state
            .db
            .lock()
            .ok()
            .and_then(|db| online_music::load_settings(&db).ok())
            .map_or(2, |settings| settings.download_concurrency)
    };
    let task_id = Arc::new(task_id);
    let handles = (0..concurrency)
        .map(|_| {
            let app = app.clone();
            let task_id = Arc::clone(&task_id);
            let cancellation = cancellation.clone();
            std::thread::spawn(move || online_download_worker(app, &task_id, cancellation))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let _ = handle.join();
    }
    let state = app.state::<AppState>();
    if let Ok(mut active) = state.online_download_requests.lock() {
        active.remove(task_id.as_str());
    }
    let task = state.db.lock().ok().and_then(|db| {
        if !cancellation.is_cancelled() {
            let _ = online_download::refresh_task_counts(&db, &task_id, now_timestamp());
        }
        online_download::task(&db, &task_id).ok().flatten()
    });
    if let Some(task) = task {
        emit_online_download_task(&app, &task);
        let should_notify = state
            .db
            .lock()
            .ok()
            .and_then(|db| online_music::load_settings(&db).ok())
            .is_some_and(|settings| settings.batch_notifications);
        if should_notify
            && matches!(
                task.state,
                online_download::OnlineDownloadState::Completed
                    | online_download::OnlineDownloadState::CompletedWithErrors
            )
        {
            let _ = app.emit(ONLINE_DOWNLOAD_COMPLETED_EVENT, &task);
        }
    }
}

fn online_download_worker(
    app: AppHandle,
    task_id: &str,
    cancellation: source_runtime::SourceCancellationToken,
) {
    while !cancellation.is_cancelled() {
        let item = {
            let state = app.state::<AppState>();
            let Ok(db) = state.db.lock() else { return };
            match online_download::claim_next_item(&db, task_id) {
                Ok(item) => item,
                Err(_) => return,
            }
        };
        let Some(item) = item else { return };
        emit_online_download_progress(
            &app,
            task_id,
            &item.item_id,
            online_download::OnlineDownloadItemState::Resolving,
            0,
            None,
        );
        let outcome = download_online_item(&app, task_id, &item, &cancellation);
        let state = app.state::<AppState>();
        if let Ok(db) = state.db.lock() {
            match outcome {
                Ok((path, bytes, warning)) => {
                    let _ = online_download::set_item_state(
                        &db,
                        &item.item_id,
                        online_download::OnlineDownloadItemState::Completed,
                        Some(&path),
                        warning.as_deref(),
                        bytes,
                        Some(bytes),
                    );
                }
                Err(OnlineItemDownloadError::Skipped(path, message)) => {
                    let _ = online_download::set_item_state(
                        &db,
                        &item.item_id,
                        online_download::OnlineDownloadItemState::Skipped,
                        Some(&path),
                        Some(&message),
                        0,
                        None,
                    );
                }
                Err(OnlineItemDownloadError::Cancelled) => {
                    let _ = online_download::set_item_state(
                        &db,
                        &item.item_id,
                        online_download::OnlineDownloadItemState::Paused,
                        None,
                        Some("Paused by user"),
                        0,
                        None,
                    );
                }
                Err(error) => {
                    let _ = online_download::set_item_state(
                        &db,
                        &item.item_id,
                        online_download::OnlineDownloadItemState::Failed,
                        None,
                        Some(&error.to_string()),
                        0,
                        None,
                    );
                }
            }
            let _ = online_download::refresh_task_counts(&db, task_id, now_timestamp());
            if let Ok(Some(task)) = online_download::task(&db, task_id) {
                emit_online_download_task(&app, &task);
            }
        };
    }
}

#[derive(Debug, thiserror::Error)]
enum OnlineItemDownloadError {
    #[error("download was cancelled")]
    Cancelled,
    #[error("{1}")]
    Skipped(PathBuf, String),
    #[error("{0}")]
    Message(String),
    #[error("download file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
struct ResolvedOnlineDownload {
    url: String,
    channel_name: String,
    attempt: download_source_router::DownloadAttemptKey,
    youtube_video_id: Option<String>,
}

#[derive(Debug)]
struct DownloadCandidateOutcome {
    attempt: download_source_router::DownloadAttemptKey,
    resolved: Option<ResolvedOnlineDownload>,
    latency: Duration,
}

#[derive(Debug, Clone, Copy)]
struct DownloadResolutionPolicy<'a> {
    qualities: &'a [source_runtime::SourceQuality],
    selection_mode: online_music::AudioSourceSelectionMode,
    layer_timeout: Duration,
    deadline: Instant,
}

fn report_download_route_success(
    app: &AppHandle,
    attempt: download_source_router::DownloadAttemptKey,
    latency: Duration,
) {
    let state = app.state::<AppState>();
    if let Ok(mut router) = state.download_source_router.lock() {
        router.report_success(attempt, latency, Instant::now());
    };
}

fn report_download_route_failure(
    app: &AppHandle,
    attempt: download_source_router::DownloadAttemptKey,
) {
    let state = app.state::<AppState>();
    if let Ok(mut router) = state.download_source_router.lock() {
        router.report_failure(attempt, Instant::now());
    };
}

fn download_online_item(
    app: &AppHandle,
    task_id: &str,
    item: &online_download::OnlineDownloadItem,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<(PathBuf, u64, Option<String>), OnlineItemDownloadError> {
    let (task, settings, channels, audio_sources, client) = {
        let state = app.state::<AppState>();
        let db = state.db.lock().map_err(|_| {
            OnlineItemDownloadError::Message("database lock was poisoned".to_owned())
        })?;
        let task = online_download::task(&db, task_id)
            .map_err(|error| OnlineItemDownloadError::Message(error.to_string()))?
            .ok_or_else(|| {
                OnlineItemDownloadError::Message("download task was not found".to_owned())
            })?;
        let settings = online_music::load_settings(&db)
            .map_err(|error| OnlineItemDownloadError::Message(error.to_string()))?;
        drop(db);
        let channels =
            online_music_channels(state.inner()).map_err(OnlineItemDownloadError::Message)?;
        let audio_sources = state
            .audio_source_registry
            .lock()
            .map_err(|_| {
                OnlineItemDownloadError::Message("audio source lock was poisoned".to_owned())
            })?
            .records();
        let client = state.online_executor.http_client();
        (task, settings, channels, audio_sources, client)
    };
    let allowed_channels = channels
        .into_iter()
        .map(|channel| channel.id)
        .collect::<BTreeSet<_>>();
    let candidates = item
        .track
        .candidates
        .iter()
        .filter(|candidate| allowed_channels.contains(&candidate.channel_id))
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(OnlineItemDownloadError::Message(
            "No enabled search channel remains available for this track.".to_owned(),
        ));
    }
    let qualities = quality_fallback(settings.preferred_quality);
    let sources = {
        let state = app.state::<AppState>();
        let mut router = state.download_source_router.lock().map_err(|_| {
            OnlineItemDownloadError::Message("download source router lock was poisoned".to_owned())
        })?;
        router.order_sources(
            audio_sources,
            download_source_router::DownloadSourceOrder {
                candidates: &candidates,
                qualities: &qualities,
                mode: settings.audio_source_selection_mode,
                configured_priority: &settings.audio_source_priority,
                selected_audio_source_id: task.selected_audio_source_id.as_deref(),
                now: Instant::now(),
            },
        )
    };
    let resolution_timeout = if sources
        .iter()
        .any(|source| source.id == youtube_music_playback::YOUTUBE_MUSIC_AUDIO_SOURCE_ID)
    {
        YT_DLP_DOWNLOAD_RESOLUTION_TIMEOUT
    } else {
        DEFAULT_DOWNLOAD_RESOLUTION_TIMEOUT
    };
    let deadline = Instant::now() + resolution_timeout;
    let resolved = resolve_online_download_url(
        app,
        &sources,
        &candidates,
        DownloadResolutionPolicy {
            qualities: &qualities,
            selection_mode: settings.audio_source_selection_mode,
            layer_timeout: Duration::from_secs(settings.layer_timeout_seconds),
            deadline,
        },
        cancellation,
    )?;
    if cancellation.is_cancelled() {
        return Err(OnlineItemDownloadError::Cancelled);
    }
    let filename = online_download::render_filename(
        &settings.filename_template,
        &item.track,
        &resolved.channel_name,
    )
    .map_err(|error| OnlineItemDownloadError::Message(error.to_string()))?;
    let destination = PathBuf::from(&task.destination);
    let temporary = match if resolved.attempt.audio_source_id
        == youtube_music_playback::YOUTUBE_MUSIC_AUDIO_SOURCE_ID
    {
        download_online_item_with_ytdlp(app, task_id, item, &destination, &resolved, cancellation)
    } else {
        download_online_item_with_http(
            app,
            task_id,
            item,
            &destination,
            &resolved,
            &client,
            cancellation,
        )
    } {
        Ok(temporary) => temporary,
        Err(error) => {
            report_download_route_failure(app, resolved.attempt.clone());
            return Err(error);
        }
    };

    let extension = match online_download::downloaded_audio_extension(&temporary) {
        Ok(extension) => extension,
        Err(error) => {
            report_download_route_failure(app, resolved.attempt.clone());
            return Err(OnlineItemDownloadError::Message(error.to_string()));
        }
    };
    let target = destination.join(format!("{filename}.{extension}"));
    validate_download_conflict(&target)?;

    let cover = load_download_cover(&client, &item.track, cancellation);
    let warning = online_download::write_metadata(&temporary, &item.track, cover.as_deref())
        .err()
        .map(|error| format!("Audio saved, but metadata tagging failed: {error}"));
    if cancellation.is_cancelled() {
        return Err(OnlineItemDownloadError::Cancelled);
    }
    validate_download_conflict(&target)?;
    temporary
        .persist_noclobber(&target)
        .map_err(|error| OnlineItemDownloadError::Io(error.error))?;
    let final_bytes = fs::metadata(&target)?.len();
    Ok((target, final_bytes, warning))
}

fn download_online_item_with_ytdlp(
    app: &AppHandle,
    task_id: &str,
    item: &online_download::OnlineDownloadItem,
    destination: &Path,
    resolved: &ResolvedOnlineDownload,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<tempfile::TempPath, OnlineItemDownloadError> {
    let video_id = resolved.youtube_video_id.as_deref().ok_or_else(|| {
        OnlineItemDownloadError::Message(
            "YouTube download requires a canonical video ID.".to_owned(),
        )
    })?;
    let temporary = tempfile::Builder::new()
        .prefix(".fika-download-")
        .suffix(".m4a")
        .tempfile_in(destination)?
        .into_temp_path();
    begin_online_item_download(app, task_id, item, &temporary, None)?;
    let sidecar = {
        let state = app.state::<AppState>();
        Arc::clone(&state.yt_dlp_sidecar)
    };
    let mut last_progress_update = Instant::now();
    let bytes = sidecar
        .download_audio(video_id, &temporary, cancellation, |downloaded, total| {
            if last_progress_update.elapsed() >= Duration::from_millis(250) || total.is_some() {
                persist_online_download_progress(app, task_id, &item.item_id, downloaded, total);
                last_progress_update = Instant::now();
            }
        })
        .map_err(|error| match error {
            yt_dlp_sidecar::YtDlpSidecarError::Cancelled => OnlineItemDownloadError::Cancelled,
            error => OnlineItemDownloadError::Message(error.to_string()),
        })?;
    if cancellation.is_cancelled() {
        return Err(OnlineItemDownloadError::Cancelled);
    }
    persist_online_download_progress(app, task_id, &item.item_id, bytes, Some(bytes));
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary)?
        .sync_all()?;
    Ok(temporary)
}

fn download_online_item_with_http(
    app: &AppHandle,
    task_id: &str,
    item: &online_download::OnlineDownloadItem,
    destination: &Path,
    resolved: &ResolvedOnlineDownload,
    client: &reqwest::blocking::Client,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<tempfile::TempPath, OnlineItemDownloadError> {
    let mut request = client
        .get(&resolved.url)
        .timeout(Duration::from_secs(15 * 60));
    if let Some(headers) = youtube_media_proxy::registered_headers(&resolved.url) {
        request = request.headers(headers);
    }
    let mut response = request.send().map_err(|_| {
        OnlineItemDownloadError::Message("media download connection failed".to_owned())
    })?;
    if !response.status().is_success() {
        return Err(OnlineItemDownloadError::Message(format!(
            "media download returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let extension_hint = media_extension(content_type, &resolved.url);
    if extension_hint.is_none() && !is_generic_media_type(content_type) {
        return Err(OnlineItemDownloadError::Message(
            "media type is not a supported audio format".to_owned(),
        ));
    }
    let total_bytes = response.content_length();
    if total_bytes.is_some_and(|length| length > MAX_ONLINE_DOWNLOAD_BYTES) {
        return Err(OnlineItemDownloadError::Message(
            "media download is larger than 2 GiB".to_owned(),
        ));
    }
    let temporary_suffix = extension_hint
        .map(|extension| format!(".{extension}"))
        .unwrap_or_else(|| ".audio".to_owned());
    let mut temporary = tempfile::Builder::new()
        .prefix(".fika-download-")
        .suffix(&temporary_suffix)
        .tempfile_in(destination)?;
    begin_online_item_download(app, task_id, item, temporary.path(), total_bytes)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded = 0_u64;
    let mut last_progress_update = Instant::now();
    loop {
        if cancellation.is_cancelled() {
            return Err(OnlineItemDownloadError::Cancelled);
        }
        let count = response.read(&mut buffer).map_err(|_| {
            OnlineItemDownloadError::Message("media download stream failed".to_owned())
        })?;
        if count == 0 {
            break;
        }
        downloaded = downloaded.saturating_add(count as u64);
        if downloaded > MAX_ONLINE_DOWNLOAD_BYTES {
            return Err(OnlineItemDownloadError::Message(
                "media download exceeded 2 GiB".to_owned(),
            ));
        }
        temporary.write_all(&buffer[..count])?;
        if last_progress_update.elapsed() >= Duration::from_millis(250) {
            persist_online_download_progress(app, task_id, &item.item_id, downloaded, total_bytes);
            last_progress_update = Instant::now();
        }
    }
    if downloaded == 0 {
        return Err(OnlineItemDownloadError::Message(
            "media download returned an empty file".to_owned(),
        ));
    }
    persist_online_download_progress(app, task_id, &item.item_id, downloaded, total_bytes);
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    Ok(temporary.into_temp_path())
}

fn begin_online_item_download(
    app: &AppHandle,
    task_id: &str,
    item: &online_download::OnlineDownloadItem,
    temporary_path: &Path,
    total_bytes: Option<u64>,
) -> Result<(), OnlineItemDownloadError> {
    let item_started = {
        let state = app.state::<AppState>();
        let db = state.db.lock().map_err(|_| {
            OnlineItemDownloadError::Message("database lock was poisoned".to_owned())
        })?;
        online_download::set_item_downloading(&db, &item.item_id, temporary_path, total_bytes)
            .map_err(|error| OnlineItemDownloadError::Message(error.to_string()))?
    };
    if !item_started {
        return Err(OnlineItemDownloadError::Cancelled);
    }
    emit_online_download_progress(
        app,
        task_id,
        &item.item_id,
        online_download::OnlineDownloadItemState::Downloading,
        0,
        total_bytes,
    );
    Ok(())
}

fn resolve_online_download_url(
    app: &AppHandle,
    sources: &[AudioSourceRecord],
    candidates: &[online_music::OnlineTrackCandidate],
    policy: DownloadResolutionPolicy<'_>,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<ResolvedOnlineDownload, OnlineItemDownloadError> {
    if sources.is_empty() {
        return Err(OnlineItemDownloadError::Message(
            "No enabled Audio Source can resolve this track.".to_owned(),
        ));
    }
    let mut remaining_sources = sources;
    if policy.selection_mode == online_music::AudioSourceSelectionMode::Automatic
        && sources.len() > 1
    {
        let hedge_delay = {
            let state = app.state::<AppState>();
            let router = state.download_source_router.lock().map_err(|_| {
                OnlineItemDownloadError::Message(
                    "download source router lock was poisoned".to_owned(),
                )
            })?;
            router.hedge_delay(&sources[0], candidates, policy.qualities)
        };
        if let Some(resolved) = race_download_source_layers(
            app,
            (&sources[0], &sources[1]),
            candidates,
            policy,
            hedge_delay,
            cancellation,
        )? {
            return Ok(resolved);
        }
        remaining_sources = &sources[2..];
    }

    for source in remaining_sources {
        if let Some(resolved) =
            resolve_download_source_layer(app, source, candidates, policy, cancellation)?
        {
            return Ok(resolved);
        }
    }
    Err(OnlineItemDownloadError::Message(
        "Download is unavailable from the configured Audio Sources.".to_owned(),
    ))
}

fn race_download_source_layers(
    app: &AppHandle,
    sources: (&AudioSourceRecord, &AudioSourceRecord),
    candidates: &[online_music::OnlineTrackCandidate],
    policy: DownloadResolutionPolicy<'_>,
    hedge_delay: Duration,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<Option<ResolvedOnlineDownload>, OnlineItemDownloadError> {
    let (primary, secondary) = sources;
    let race_cancellation = source_runtime::SourceCancellationToken::default();
    // Only a failed primary releases the delay; primary success cancels the backup.
    let primary_unavailable = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sender, receiver) = std::sync::mpsc::channel();

    std::thread::scope(|scope| {
        let primary_sender = sender.clone();
        let primary_token = race_cancellation.clone();
        let primary_unavailable_worker = Arc::clone(&primary_unavailable);
        scope.spawn(move || {
            let result =
                resolve_download_source_layer(app, primary, candidates, policy, &primary_token);
            if !matches!(&result, Ok(Some(_))) {
                primary_unavailable_worker.store(true, std::sync::atomic::Ordering::Release);
            }
            let _ = primary_sender.send(result);
        });

        let secondary_sender = sender.clone();
        let secondary_token = race_cancellation.clone();
        let primary_unavailable_worker = Arc::clone(&primary_unavailable);
        scope.spawn(move || {
            let should_start = wait_for_download_hedge(
                &primary_unavailable_worker,
                &secondary_token,
                hedge_delay,
                policy.deadline,
            );
            let result = if secondary_token.is_cancelled() {
                Err(OnlineItemDownloadError::Cancelled)
            } else if !should_start {
                Ok(None)
            } else {
                resolve_download_source_layer(app, secondary, candidates, policy, &secondary_token)
            };
            let _ = secondary_sender.send(result);
        });
        drop(sender);

        let mut completed = 0;
        let mut first_error = None;
        while completed < 2 {
            if cancellation.is_cancelled() {
                race_cancellation.cancel();
                return Err(OnlineItemDownloadError::Cancelled);
            }
            if Instant::now() >= policy.deadline {
                race_cancellation.cancel();
                return Ok(None);
            }
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(Some(resolved))) => {
                    race_cancellation.cancel();
                    return Ok(Some(resolved));
                }
                Ok(Ok(None)) => completed += 1,
                Ok(Err(OnlineItemDownloadError::Cancelled)) => completed += 1,
                Ok(Err(error)) => {
                    completed += 1;
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        first_error.map_or(Ok(None), Err)
    })
}

fn wait_for_download_hedge(
    primary_unavailable: &std::sync::atomic::AtomicBool,
    cancellation: &source_runtime::SourceCancellationToken,
    hedge_delay: Duration,
    deadline: Instant,
) -> bool {
    let wait_started_at = Instant::now();
    while wait_started_at.elapsed() < hedge_delay
        && Instant::now() < deadline
        && !primary_unavailable.load(std::sync::atomic::Ordering::Acquire)
        && !cancellation.is_cancelled()
    {
        let remaining = hedge_delay.saturating_sub(wait_started_at.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(20)));
    }
    !cancellation.is_cancelled() && Instant::now() < deadline
}

fn resolve_download_source_layer(
    app: &AppHandle,
    source: &AudioSourceRecord,
    candidates: &[online_music::OnlineTrackCandidate],
    policy: DownloadResolutionPolicy<'_>,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<Option<ResolvedOnlineDownload>, OnlineItemDownloadError> {
    let layer_deadline = download_source_layer_deadline(&source.id, Instant::now(), policy);
    for quality in policy.qualities.iter().copied() {
        if cancellation.is_cancelled() {
            return Err(OnlineItemDownloadError::Cancelled);
        }
        if Instant::now() >= layer_deadline || Instant::now() >= policy.deadline {
            break;
        }
        let supported = candidates
            .iter()
            .filter(|candidate| {
                source.sources.iter().any(|info| {
                    info.id == candidate.source_id
                        && info
                            .actions
                            .contains(&source_runtime::SourceAction::MusicUrl)
                        && (info.qualities.is_empty() || info.qualities.contains(&quality))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let supported = {
            let state = app.state::<AppState>();
            let router = state.download_source_router.lock().map_err(|_| {
                OnlineItemDownloadError::Message(
                    "download source router lock was poisoned".to_owned(),
                )
            })?;
            router.available_candidates(
                &source.id,
                supported,
                quality,
                policy.selection_mode,
                Instant::now(),
            )
        };
        if supported.is_empty() {
            continue;
        }
        if let Some(resolved) = race_download_candidates(
            app,
            source,
            &supported,
            quality,
            layer_deadline.min(policy.deadline),
            cancellation,
        )? {
            return Ok(Some(resolved));
        }
    }
    Ok(None)
}

fn download_source_layer_deadline(
    audio_source_id: &str,
    now: Instant,
    policy: DownloadResolutionPolicy<'_>,
) -> Instant {
    if audio_source_id == youtube_music_playback::YOUTUBE_MUSIC_AUDIO_SOURCE_ID {
        policy.deadline
    } else {
        policy.deadline.min(now + policy.layer_timeout)
    }
}

fn quality_fallback(
    preferred: source_runtime::SourceQuality,
) -> Vec<source_runtime::SourceQuality> {
    use source_runtime::SourceQuality::{Flac, Flac24Bit, K128, K320};
    match preferred {
        Flac24Bit => vec![Flac24Bit, Flac, K320, K128],
        Flac => vec![Flac, K320, K128],
        K320 => vec![K320, K128],
        K128 => vec![K128],
    }
}

fn race_download_candidates(
    app: &AppHandle,
    source: &AudioSourceRecord,
    candidates: &[online_music::OnlineTrackCandidate],
    quality: source_runtime::SourceQuality,
    deadline: Instant,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Result<Option<ResolvedOnlineDownload>, OnlineItemDownloadError> {
    let (prepared, preparation_failures, executor) = {
        let state = app.state::<AppState>();
        let registry = state.audio_source_registry.lock().map_err(|_| {
            OnlineItemDownloadError::Message("audio source lock was poisoned".to_owned())
        })?;
        let mut prepared = Vec::new();
        let mut preparation_failures = Vec::new();
        for candidate in candidates {
            let request = download_music_url_request(candidate, quality);
            let youtube_video_id = (source.id
                == youtube_music_playback::YOUTUBE_MUSIC_AUDIO_SOURCE_ID)
                .then(|| youtube_video_id_from_candidate(candidate))
                .flatten();
            let attempt = download_source_router::DownloadAttemptKey::new(
                &source.id,
                &candidate.channel_id,
                quality,
            );
            match registry.prepare_dispatch(&source.id, &request) {
                Ok(dispatch) => prepared.push((
                    dispatch,
                    request,
                    candidate.channel_name.clone(),
                    attempt,
                    youtube_video_id,
                )),
                Err(_) => preparation_failures.push(attempt),
            }
        }
        (
            prepared,
            preparation_failures,
            Arc::clone(&state.online_executor),
        )
    };
    for attempt in preparation_failures {
        report_download_route_failure(app, attempt);
    }
    if prepared.is_empty() {
        return Ok(None);
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let candidate_count = prepared.len();
    let mut pending_attempts = prepared
        .iter()
        .map(|(_, _, _, attempt, _)| attempt.clone())
        .collect::<BTreeSet<_>>();
    let race_cancellation = source_runtime::SourceCancellationToken::default();
    let client = executor.http_client();
    for (dispatch, request, channel_name, attempt, youtube_video_id) in prepared {
        let sender = sender.clone();
        let token = race_cancellation.clone();
        let client = client.clone();
        let failed_attempt = attempt.clone();
        executor.spawn(
            move || {
                let started_at = Instant::now();
                if token.is_cancelled() {
                    return DownloadCandidateOutcome {
                        attempt,
                        resolved: None,
                        latency: Duration::ZERO,
                    };
                }
                let resolved = dispatch
                    .execute(request, token.clone())
                    .ok()
                    .and_then(|outcome| match outcome.response {
                        source_runtime::SourceResponse::MusicUrl(url) => Some(url),
                        _ => None,
                    })
                    .filter(|url| probe_download_url(&client, url, deadline, &token))
                    .map(|url| ResolvedOnlineDownload {
                        url,
                        channel_name,
                        attempt: attempt.clone(),
                        youtube_video_id,
                    });
                DownloadCandidateOutcome {
                    attempt,
                    resolved,
                    latency: started_at.elapsed(),
                }
            },
            move |result| {
                let outcome = result.unwrap_or(DownloadCandidateOutcome {
                    attempt: failed_attempt,
                    resolved: None,
                    latency: Duration::ZERO,
                });
                let _ = sender.send(outcome);
            },
        );
    }
    drop(sender);
    let mut completed = 0;
    while completed < candidate_count && Instant::now() < deadline {
        if cancellation.is_cancelled() {
            race_cancellation.cancel();
            return Err(OnlineItemDownloadError::Cancelled);
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(outcome) => {
                if cancellation.is_cancelled() {
                    race_cancellation.cancel();
                    return Err(OnlineItemDownloadError::Cancelled);
                }
                pending_attempts.remove(&outcome.attempt);
                if let Some(result) = outcome.resolved {
                    report_download_route_success(app, outcome.attempt, outcome.latency);
                    race_cancellation.cancel();
                    return Ok(Some(result));
                }
                report_download_route_failure(app, outcome.attempt);
                completed += 1;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    race_cancellation.cancel();
    if cancellation.is_cancelled() {
        return Err(OnlineItemDownloadError::Cancelled);
    }
    for attempt in pending_attempts {
        report_download_route_failure(app, attempt);
    }
    Ok(None)
}

fn download_music_url_request(
    candidate: &online_music::OnlineTrackCandidate,
    quality: source_runtime::SourceQuality,
) -> source_runtime::SourceRequest {
    let mut music_info = JsonMap::new();
    for (key, value) in &candidate.platform_ids {
        let value = match value {
            source_runtime::JsonScalar::String(value) => JsonValue::String(value.clone()),
            source_runtime::JsonScalar::Number(value) => JsonValue::from(*value),
        };
        music_info.insert(key.clone(), value);
    }
    music_info.insert("id".to_owned(), JsonValue::String(candidate.id.clone()));
    music_info.insert(
        "title".to_owned(),
        JsonValue::String(candidate.title.clone()),
    );
    music_info.insert(
        "name".to_owned(),
        JsonValue::String(candidate.title.clone()),
    );
    music_info.insert(
        "artist".to_owned(),
        JsonValue::String(candidate.artist.clone()),
    );
    music_info.insert(
        "singer".to_owned(),
        JsonValue::String(candidate.artist.clone()),
    );
    if let Some(album) = &candidate.album {
        music_info.insert("album".to_owned(), JsonValue::String(album.clone()));
        music_info.insert("albumName".to_owned(), JsonValue::String(album.clone()));
    }
    if let Some(duration) = candidate.duration_seconds {
        music_info.insert("duration".to_owned(), JsonValue::from(duration));
    }
    source_runtime::SourceRequest::MusicUrl {
        source: candidate.source_id.clone(),
        music_info: JsonValue::Object(music_info),
        quality,
    }
}

fn youtube_video_id_from_candidate(
    candidate: &online_music::OnlineTrackCandidate,
) -> Option<String> {
    let platform_video_id = candidate
        .platform_ids
        .get("videoId")
        .and_then(|value| match value {
            source_runtime::JsonScalar::String(value) => Some(value.as_str()),
            source_runtime::JsonScalar::Number(_) => None,
        })
        .filter(|video_id| yt_dlp_sidecar::is_canonical_video_id(video_id));
    platform_video_id
        .or_else(|| {
            yt_dlp_sidecar::is_canonical_video_id(&candidate.id).then_some(candidate.id.as_str())
        })
        .map(str::to_owned)
}

fn probe_download_url(
    client: &reqwest::blocking::Client,
    url: &str,
    deadline: Instant,
    cancellation: &source_runtime::SourceCancellationToken,
) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() || cancellation.is_cancelled() {
        return false;
    }
    let mut request = client
        .get(url)
        .timeout(remaining)
        .header(reqwest::header::RANGE, "bytes=0-1023");
    if let Some(headers) = youtube_media_proxy::registered_headers(url) {
        request = request.headers(headers);
    }
    let Ok(mut response) = request.send() else {
        return false;
    };
    if !response.status().is_success() || cancellation.is_cancelled() {
        return false;
    }
    let mut byte = [0_u8; 1];
    response.read(&mut byte).is_ok_and(|count| count == 1)
}

fn media_extension(content_type: Option<&str>, url: &str) -> Option<&'static str> {
    let media_type = normalized_media_type(content_type);
    match media_type.as_str() {
        "audio/mpeg" | "audio/mp3" => return Some("mp3"),
        "audio/flac" | "audio/x-flac" => return Some("flac"),
        "audio/mp4" | "audio/x-m4a" => return Some("m4a"),
        "audio/aac" => return Some("aac"),
        "audio/ogg" | "audio/opus" => return Some("ogg"),
        "" | "application/octet-stream" => {}
        _ => return None,
    }
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    for extension in ["mp3", "flac", "m4a", "aac", "ogg", "opus"] {
        if path.ends_with(&format!(".{extension}")) {
            return Some(if extension == "opus" {
                "ogg"
            } else {
                extension
            });
        }
    }
    None
}

fn is_generic_media_type(content_type: Option<&str>) -> bool {
    matches!(
        normalized_media_type(content_type).as_str(),
        "" | "application/octet-stream"
    )
}

fn normalized_media_type(content_type: Option<&str>) -> String {
    content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn validate_download_conflict(target: &Path) -> Result<(), OnlineItemDownloadError> {
    if !target.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(target)?;
    if metadata.len() == 0 {
        return Err(OnlineItemDownloadError::Message(format!(
            "A zero-byte file already exists at {}.",
            target.display()
        )));
    }
    if lofty::read_from_path(target).is_ok() {
        return Err(OnlineItemDownloadError::Skipped(
            target.to_owned(),
            "A readable audio file already exists; it was not overwritten.".to_owned(),
        ));
    }
    Err(OnlineItemDownloadError::Message(format!(
        "An unreadable file already exists at {}; it was not overwritten.",
        target.display()
    )))
}

fn load_download_cover(
    client: &reqwest::blocking::Client,
    track: &online_music::OnlineTrack,
    cancellation: &source_runtime::SourceCancellationToken,
) -> Option<Vec<u8>> {
    let urls = track
        .cover_url
        .iter()
        .chain(
            track
                .candidates
                .iter()
                .filter_map(|candidate| candidate.cover_url.as_ref()),
        )
        .collect::<Vec<_>>();
    for url in urls {
        if cancellation.is_cancelled() {
            return None;
        }
        let Ok(response) = client.get(url).timeout(Duration::from_secs(10)).send() else {
            continue;
        };
        let Ok(response) = response.error_for_status() else {
            continue;
        };
        if response
            .content_length()
            .is_some_and(|length| length > 10 * 1024 * 1024)
        {
            continue;
        }
        let Ok(bytes) = response.bytes() else {
            continue;
        };
        if !bytes.is_empty()
            && bytes.len() <= 10 * 1024 * 1024
            && image::load_from_memory(&bytes).is_ok()
        {
            return Some(bytes.to_vec());
        }
    }
    placeholder_cover().ok()
}

fn placeholder_cover() -> Result<Vec<u8>, image::ImageError> {
    use image::ImageEncoder;
    let image = image::RgbImage::from_fn(300, 300, |x, y| {
        let accent = ((x / 30 + y / 30) % 2) as u8 * 10;
        image::Rgb([42 + accent, 48 + accent, 52 + accent])
    });
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(bytes)
}

#[tauri::command]
fn list_online_music_channels(
    state: State<'_, AppState>,
    include_excluded: Option<bool>,
) -> CommandResult<Vec<online_music::OnlineChannel>> {
    let channels = all_online_music_channels(state.inner())?;
    Ok(if include_excluded.unwrap_or(false) {
        channels
    } else {
        channels
            .into_iter()
            .filter(|channel| !channel.excluded)
            .collect()
    })
}

fn online_music_channels(state: &AppState) -> CommandResult<Vec<online_music::OnlineChannel>> {
    Ok(all_online_music_channels(state)?
        .into_iter()
        .filter(|channel| !channel.excluded)
        .collect())
}

fn all_online_music_channels(state: &AppState) -> CommandResult<Vec<online_music::OnlineChannel>> {
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    let records = state
        .plugin_registry
        .lock()
        .map_err(|_| "plugin registry lock was poisoned".to_owned())?
        .records();
    Ok(online_music::channels_from_plugins(&records, &settings))
}

#[tauri::command]
async fn online_music_recommendations(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: source_runtime::MusicRecommendationKind,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineRecommendationsResult> {
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_music_recommendations_inner(&state, kind, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online music recommendation task failed: {error}"))?
}

fn online_music_recommendations_inner(
    state: &AppState,
    kind: source_runtime::MusicRecommendationKind,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineRecommendationsResult> {
    let channels = online_music_channels(state)?;
    let eligible = channels
        .into_iter()
        .filter(|channel| {
            channel
                .actions
                .contains(&source_runtime::SourceAction::MusicRecommendations)
                && recommendation_channel_supports(channel, kind)
        })
        .collect::<Vec<_>>();
    let supported_channels = eligible.len() as u32;
    let netease_account_ref = if eligible
        .iter()
        .any(|channel| channel.plugin_id == netease::NETEASE_PLUGIN_ID)
    {
        state
            .netease_bridge
            .accounts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|account| account.status == netease::NeteaseAccountStatus::Active)
            .map(|account| account.account_ref)
    } else {
        None
    };
    let kugou_account_ref = if eligible
        .iter()
        .any(|channel| channel.plugin_id == kugou::KUGOU_PLUGIN_ID)
    {
        state
            .kugou_bridge
            .accounts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|account| account.status == kugou::KugouAccountStatus::Active)
            .map(|account| account.account_ref)
    } else {
        None
    };

    let mut failures = Vec::new();
    let ready = eligible
        .into_iter()
        .filter(|channel| {
            let connected = match channel.plugin_id.as_str() {
                netease::NETEASE_PLUGIN_ID => netease_account_ref.is_some(),
                kugou::KUGOU_PLUGIN_ID => kugou_account_ref.is_some(),
                _ => false,
            };
            if !connected {
                failures.push(online_music::OnlineChannelFailure {
                    channel_id: channel.id.clone(),
                    channel_name: channel.source_name.clone(),
                    message: format!(
                        "Connect an active {} account to load recommendations.",
                        channel.source_name
                    ),
                });
            }
            connected
        })
        .collect::<Vec<_>>();
    let outcomes = dispatch_channels(state, &ready, cancellation, |channel| {
        let account_ref = match channel.plugin_id.as_str() {
            netease::NETEASE_PLUGIN_ID => netease_account_ref.as_deref(),
            kugou::KUGOU_PLUGIN_ID => kugou_account_ref.as_deref(),
            _ => None,
        }
        .unwrap_or_default()
        .to_owned();
        source_runtime::SourceRequest::MusicRecommendations {
            source: channel.source_id.clone(),
            account_ref,
            kind,
            limit: recommendation_request_limit(kind),
        }
    });
    let completed_channels = outcomes.len() as u32;
    let mut candidates = Vec::new();
    for (channel, outcome) in outcomes {
        match outcome.and_then(|outcome| match outcome.response {
            source_runtime::SourceResponse::MusicRecommendations(response) => Ok(response.list),
            _ => Err("provider returned an unexpected response".to_owned()),
        }) {
            Ok(tracks) => {
                candidates.extend(tracks.into_iter().enumerate().map(|(index, track)| {
                    online_music::OnlineTrackCandidate::from_source(
                        &channel,
                        track,
                        index as u32 + 1,
                    )
                }))
            }
            Err(message) => failures.push(online_music::OnlineChannelFailure {
                channel_id: channel.id,
                channel_name: channel.source_name,
                message,
            }),
        }
    }
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    Ok(online_music::OnlineRecommendationsResult {
        kind,
        items: online_music::merge_tracks(candidates, &settings.channel_priority),
        failures,
        supported_channels,
        completed_channels,
    })
}

fn recommendation_channel_supports(
    channel: &online_music::OnlineChannel,
    kind: source_runtime::MusicRecommendationKind,
) -> bool {
    match kind {
        source_runtime::MusicRecommendationKind::Daily => matches!(
            channel.plugin_id.as_str(),
            netease::NETEASE_PLUGIN_ID | kugou::KUGOU_PLUGIN_ID
        ),
        source_runtime::MusicRecommendationKind::Roaming
        | source_runtime::MusicRecommendationKind::Radar => {
            channel.plugin_id == netease::NETEASE_PLUGIN_ID
        }
    }
}

fn recommendation_request_limit(kind: source_runtime::MusicRecommendationKind) -> u64 {
    match kind {
        source_runtime::MusicRecommendationKind::Roaming => 3,
        source_runtime::MusicRecommendationKind::Daily
        | source_runtime::MusicRecommendationKind::Radar => 50,
    }
}

#[tauri::command]
async fn online_music_playlists(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlinePlaylistsResult> {
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_music_playlists_inner(&state, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online music playlist task failed: {error}"))?
}

fn online_music_playlists_inner(
    state: &AppState,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlinePlaylistsResult> {
    let eligible = online_music_channels(state)?
        .into_iter()
        .filter(|channel| {
            channel
                .actions
                .contains(&source_runtime::SourceAction::PlaylistList)
                && channel
                    .actions
                    .contains(&source_runtime::SourceAction::PlaylistRead)
                && matches!(
                    channel.plugin_id.as_str(),
                    netease::NETEASE_PLUGIN_ID | kugou::KUGOU_PLUGIN_ID
                )
        })
        .collect::<Vec<_>>();
    let supported_channels = eligible.len() as u32;
    let netease_account_ref = if eligible
        .iter()
        .any(|channel| channel.plugin_id == netease::NETEASE_PLUGIN_ID)
    {
        state
            .netease_bridge
            .accounts()
            .map(|accounts| {
                accounts
                    .into_iter()
                    .find(|account| account.status == netease::NeteaseAccountStatus::Active)
                    .map(|account| account.account_ref)
            })
            .map_err(|error| error.to_string())
    } else {
        Ok(None)
    };
    let kugou_account_ref = if eligible
        .iter()
        .any(|channel| channel.plugin_id == kugou::KUGOU_PLUGIN_ID)
    {
        state
            .kugou_bridge
            .accounts()
            .map(|accounts| {
                accounts
                    .into_iter()
                    .find(|account| account.status == kugou::KugouAccountStatus::Active)
                    .map(|account| account.account_ref)
            })
            .map_err(|error| error.to_string())
    } else {
        Ok(None)
    };

    let account_ref = |channel: &online_music::OnlineChannel| -> Result<Option<&str>, &str> {
        match channel.plugin_id.as_str() {
            netease::NETEASE_PLUGIN_ID => netease_account_ref
                .as_ref()
                .map(Option::as_deref)
                .map_err(String::as_str),
            kugou::KUGOU_PLUGIN_ID => kugou_account_ref
                .as_ref()
                .map(Option::as_deref)
                .map_err(String::as_str),
            _ => Ok(None),
        }
    };
    let mut failures = Vec::new();
    let mut ready = Vec::new();
    for channel in eligible {
        match account_ref(&channel) {
            Ok(Some(_)) => ready.push(channel),
            Ok(None) => failures.push(online_music::OnlineChannelFailure {
                channel_id: channel.id,
                channel_name: channel.source_name.clone(),
                message: format!(
                    "Connect an active {} account to load playlists.",
                    channel.source_name
                ),
            }),
            Err(message) => failures.push(online_music::OnlineChannelFailure {
                channel_id: channel.id,
                channel_name: channel.source_name.clone(),
                message: format!("Could not load {} accounts: {message}", channel.source_name),
            }),
        }
    }
    let outcomes = dispatch_channels(state, &ready, cancellation, |channel| {
        source_runtime::SourceRequest::PlaylistList {
            source: channel.source_id.clone(),
            account_ref: account_ref(channel)
                .ok()
                .flatten()
                .unwrap_or_default()
                .to_owned(),
        }
    });
    let completed_channels = outcomes.len() as u32;
    let mut items = Vec::new();
    for (channel, outcome) in outcomes {
        match outcome.and_then(|outcome| match outcome.response {
            source_runtime::SourceResponse::PlaylistList(playlists) => Ok(playlists),
            _ => Err("provider returned an unexpected response".to_owned()),
        }) {
            Ok(playlists) => {
                let channel_account_ref = account_ref(&channel).ok().flatten().unwrap_or_default();
                items.extend(playlists.into_iter().enumerate().map(|(index, playlist)| {
                    online_music::OnlinePlaylist::from_account(
                        &channel,
                        channel_account_ref,
                        playlist,
                        index as u32 + 1,
                    )
                }));
            }
            Err(message) => failures.push(online_music::OnlineChannelFailure {
                channel_id: channel.id,
                channel_name: channel.source_name,
                message,
            }),
        }
    }
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    online_music::sort_playlists(&mut items, &settings.channel_priority);
    Ok(online_music::OnlinePlaylistsResult {
        items,
        failures,
        supported_channels,
        completed_channels,
    })
}

#[tauri::command]
async fn online_music_suggestions(
    app: AppHandle,
    state: State<'_, AppState>,
    keyword: String,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineSuggestionsResult> {
    let keyword = keyword.trim().to_owned();
    if keyword.is_empty() {
        let suggestions = {
            let db = state
                .db
                .lock()
                .map_err(|_| "database lock was poisoned".to_owned())?;
            online_music::search_history(&db)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|entry| entry.query)
                .collect()
        };
        return Ok(online_music::OnlineSuggestionsResult {
            suggestions,
            failures: Vec::new(),
        });
    }
    if keyword.chars().count() < 2 {
        return Ok(online_music::OnlineSuggestionsResult {
            suggestions: Vec::new(),
            failures: Vec::new(),
        });
    }
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_music_suggestions_inner(&state, &keyword, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online music suggestion task failed: {error}"))?
}

fn online_music_suggestions_inner(
    state: &AppState,
    keyword: &str,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineSuggestionsResult> {
    let channels = online_music_channels(state)?;
    let history = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::search_history(&db).map_err(|error| error.to_string())?
    };
    let eligible = channels
        .into_iter()
        .filter(|channel| {
            channel
                .actions
                .contains(&source_runtime::SourceAction::SearchSuggestions)
        })
        .collect::<Vec<_>>();
    let outcomes = dispatch_channels(state, &eligible, cancellation, |channel| {
        source_runtime::SourceRequest::SearchSuggestions {
            source: channel.source_id.clone(),
            keyword: keyword.to_owned(),
            limit: 8,
        }
    });
    let mut online = Vec::new();
    let mut failures = Vec::new();
    for (channel, outcome) in outcomes {
        match outcome.and_then(|outcome| match outcome.response {
            source_runtime::SourceResponse::SearchSuggestions(response) => Ok(response.list),
            _ => Err("provider returned an unexpected response".to_owned()),
        }) {
            Ok(suggestions) => online.push((channel, suggestions)),
            Err(message) => failures.push(online_music::OnlineChannelFailure {
                channel_id: channel.id,
                channel_name: channel.source_name,
                message,
            }),
        }
    }
    Ok(online_music::OnlineSuggestionsResult {
        suggestions: online_music::merge_suggestions(keyword, &history, &online),
        failures,
    })
}

#[tauri::command]
fn start_online_music_search(
    app: AppHandle,
    state: State<'_, AppState>,
    keyword: String,
) -> CommandResult<String> {
    let keyword = keyword.trim().to_owned();
    if keyword.is_empty() {
        return Err("Search query must not be empty".to_owned());
    }
    let search_id = uuid::Uuid::new_v4().to_string();
    let cancellation =
        register_source_request(state.inner(), Some(&search_id)).map_err(|error| error.message)?;
    {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::record_search(&db, &keyword, now_timestamp())
            .map_err(|error| error.to_string())?;
    }
    let task_search_id = search_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        run_online_search(&app, &state, &task_search_id, &keyword, cancellation);
        unregister_source_request(&state, Some(&task_search_id));
    });
    Ok(search_id)
}

fn run_online_search(
    app: &AppHandle,
    state: &AppState,
    search_id: &str,
    keyword: &str,
    cancellation: source_runtime::SourceCancellationToken,
) {
    let sections = [
        online_music::OnlineSearchSection::Songs,
        online_music::OnlineSearchSection::Artists,
        online_music::OnlineSearchSection::Albums,
        online_music::OnlineSearchSection::Playlists,
    ];
    let worker_results = state.online_executor.map(sections.to_vec(), |section| {
        let cancellation = cancellation.clone();
        if cancellation.is_cancelled() {
            return;
        }
        let result = online_music_section(state, keyword, section, 1, 20, cancellation)
            .unwrap_or_else(|message| online_music::OnlineSearchSectionResult {
                section,
                data: empty_online_search_data(section),
                failures: vec![online_music::OnlineChannelFailure {
                    channel_id: "host".to_owned(),
                    channel_name: "Fika Music".to_owned(),
                    message,
                }],
                supported_channels: 0,
                completed_channels: 0,
                has_more: false,
            });
        let mut summary = result;
        summary.has_more |= summary.data.len() > 5;
        summary.data.truncate(5);
        let _ = app.emit(
            ONLINE_SEARCH_SECTION_EVENT,
            online_music::OnlineSearchSectionEvent {
                search_id: search_id.to_owned(),
                result: summary,
            },
        );
    });
    for (section, worker_result) in sections.into_iter().zip(worker_results) {
        let Err(message) = worker_result else {
            continue;
        };
        let _ = app.emit(
            ONLINE_SEARCH_SECTION_EVENT,
            online_music::OnlineSearchSectionEvent {
                search_id: search_id.to_owned(),
                result: online_music::OnlineSearchSectionResult {
                    section,
                    data: empty_online_search_data(section),
                    failures: vec![online_music::OnlineChannelFailure {
                        channel_id: "host".to_owned(),
                        channel_name: "Fika Music".to_owned(),
                        message,
                    }],
                    supported_channels: 0,
                    completed_channels: 0,
                    has_more: false,
                },
            },
        );
    }
}

#[tauri::command]
async fn online_music_search_page(
    app: AppHandle,
    state: State<'_, AppState>,
    keyword: String,
    section: online_music::OnlineSearchSection,
    page: u64,
    page_size: u64,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineSearchSectionResult> {
    let keyword = keyword.trim().to_owned();
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_music_section(&state, &keyword, section, page, page_size, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online music search task failed: {error}"))?
}

fn online_music_section(
    state: &AppState,
    keyword: &str,
    section: online_music::OnlineSearchSection,
    page: u64,
    page_size: u64,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineSearchSectionResult> {
    if keyword.trim().is_empty() || page == 0 || !(1..=100).contains(&page_size) {
        return Err("Invalid online music search request".to_owned());
    }
    let channels = online_music_channels(state)?;
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    let eligible = channels
        .into_iter()
        .filter(|channel| channel.actions.contains(&section.action()))
        .collect::<Vec<_>>();
    let supported_channels = eligible.len() as u32;
    let outcomes = dispatch_channels(state, &eligible, cancellation, |channel| {
        online_search_request(section, &channel.source_id, keyword, page, page_size)
    });
    let mut failures = Vec::new();
    let mut has_more = false;
    let mut tracks = Vec::new();
    let mut artists = Vec::new();
    let mut albums = Vec::new();
    let mut playlists = Vec::new();
    let completed_channels = outcomes.len() as u32;
    for (channel, outcome) in outcomes {
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(message) => {
                failures.push(online_music::OnlineChannelFailure {
                    channel_id: channel.id,
                    channel_name: channel.source_name,
                    message,
                });
                continue;
            }
        };
        match outcome.response {
            source_runtime::SourceResponse::MusicSearch(response) => {
                has_more |= !response.is_end;
                tracks.extend(response.list.into_iter().enumerate().map(|(index, track)| {
                    online_music::OnlineTrackCandidate::from_source(
                        &channel,
                        track,
                        rank_for_page(page, page_size, index),
                    )
                }));
            }
            source_runtime::SourceResponse::ArtistSearch(response) => {
                has_more |= !response.is_end;
                artists.extend(
                    response
                        .list
                        .into_iter()
                        .enumerate()
                        .map(|(index, artist)| {
                            online_music::OnlineArtistCandidate::from_source(
                                &channel,
                                artist,
                                rank_for_page(page, page_size, index),
                            )
                        }),
                );
            }
            source_runtime::SourceResponse::AlbumSearch(response) => {
                has_more |= !response.is_end;
                albums.extend(response.list.into_iter().enumerate().map(|(index, album)| {
                    online_music::OnlineAlbumCandidate::from_source(
                        &channel,
                        album,
                        rank_for_page(page, page_size, index),
                    )
                }));
            }
            source_runtime::SourceResponse::PlaylistSearch(response) => {
                has_more |= !response.is_end;
                playlists.extend(
                    response
                        .list
                        .into_iter()
                        .enumerate()
                        .map(|(index, playlist)| {
                            online_music::OnlinePlaylist::from_source(
                                &channel,
                                playlist,
                                rank_for_page(page, page_size, index),
                            )
                        }),
                );
            }
            _ => failures.push(online_music::OnlineChannelFailure {
                channel_id: channel.id,
                channel_name: channel.source_name,
                message: "provider returned an unexpected response".to_owned(),
            }),
        }
    }

    let data = match section {
        online_music::OnlineSearchSection::Songs => online_music::OnlineSearchData::Songs(
            online_music::merge_tracks(tracks, &settings.channel_priority),
        ),
        online_music::OnlineSearchSection::Artists => {
            let groups = online_music::group_artist_candidates(artists)
                .into_values()
                .flat_map(|group| disambiguate_artist_group(state, group, &settings, page_size))
                .collect();
            online_music::OnlineSearchData::Artists(online_music::merge_artists(
                groups,
                &settings.channel_priority,
            ))
        }
        online_music::OnlineSearchSection::Albums => {
            let groups = online_music::group_album_candidates(albums)
                .into_values()
                .flat_map(|group| disambiguate_album_group(state, group, &settings, page_size))
                .collect();
            online_music::OnlineSearchData::Albums(online_music::merge_albums(
                groups,
                &settings.channel_priority,
            ))
        }
        online_music::OnlineSearchSection::Playlists => {
            online_music::sort_playlists(&mut playlists, &settings.channel_priority);
            online_music::OnlineSearchData::Playlists(playlists)
        }
    };
    Ok(online_music::OnlineSearchSectionResult {
        section,
        data,
        failures,
        supported_channels,
        completed_channels,
        has_more,
    })
}

fn rank_for_page(page: u64, page_size: u64, index: usize) -> u32 {
    let rank = page
        .saturating_sub(1)
        .saturating_mul(page_size)
        .saturating_add(index as u64)
        .saturating_add(1);
    u32::try_from(rank).unwrap_or(u32::MAX)
}

fn online_search_request(
    section: online_music::OnlineSearchSection,
    source: &str,
    keyword: &str,
    page: u64,
    page_size: u64,
) -> source_runtime::SourceRequest {
    match section {
        online_music::OnlineSearchSection::Songs => source_runtime::SourceRequest::MusicSearch {
            source: source.to_owned(),
            keyword: keyword.to_owned(),
            page,
            page_size,
        },
        online_music::OnlineSearchSection::Artists => source_runtime::SourceRequest::ArtistSearch {
            source: source.to_owned(),
            keyword: keyword.to_owned(),
            page,
            page_size,
        },
        online_music::OnlineSearchSection::Albums => source_runtime::SourceRequest::AlbumSearch {
            source: source.to_owned(),
            keyword: keyword.to_owned(),
            page,
            page_size,
        },
        online_music::OnlineSearchSection::Playlists => {
            source_runtime::SourceRequest::PlaylistSearch {
                source: source.to_owned(),
                keyword: keyword.to_owned(),
                page,
                page_size,
            }
        }
    }
}

fn empty_online_search_data(
    section: online_music::OnlineSearchSection,
) -> online_music::OnlineSearchData {
    match section {
        online_music::OnlineSearchSection::Songs => {
            online_music::OnlineSearchData::Songs(Vec::new())
        }
        online_music::OnlineSearchSection::Artists => {
            online_music::OnlineSearchData::Artists(Vec::new())
        }
        online_music::OnlineSearchSection::Albums => {
            online_music::OnlineSearchData::Albums(Vec::new())
        }
        online_music::OnlineSearchSection::Playlists => {
            online_music::OnlineSearchData::Playlists(Vec::new())
        }
    }
}

fn dispatch_channels<F>(
    state: &AppState,
    channels: &[online_music::OnlineChannel],
    cancellation: source_runtime::SourceCancellationToken,
    build_request: F,
) -> Vec<(
    online_music::OnlineChannel,
    Result<source_runtime::SourceRequestOutcome, String>,
)>
where
    F: Fn(&online_music::OnlineChannel) -> source_runtime::SourceRequest + Sync,
{
    let prepared = {
        let registry = match state.plugin_registry.lock() {
            Ok(registry) => registry,
            Err(_) => {
                return channels
                    .iter()
                    .cloned()
                    .map(|channel| (channel, Err("plugin registry lock was poisoned".to_owned())))
                    .collect()
            }
        };
        channels
            .iter()
            .cloned()
            .map(|channel| {
                let request = build_request(&channel);
                let dispatch = registry
                    .prepare_action(&channel.plugin_id, &channel.source_id, request.action())
                    .map_err(|error| error.to_string());
                (channel, request, dispatch)
            })
            .collect::<Vec<_>>()
    };
    let worker_channels = prepared
        .iter()
        .map(|(channel, _, _)| channel.clone())
        .collect::<Vec<_>>();
    let cache = Arc::clone(&state.online_music_cache);
    let results = state
        .online_executor
        .map(prepared, move |(channel, request, dispatch)| {
            let cancellation = cancellation.clone();
            dispatch.and_then(|dispatch| {
                execute_cached_plugin_request(&cache, &channel, request, cancellation, dispatch)
                    .map_err(|error| error.to_string())
            })
        });
    worker_channels
        .into_iter()
        .zip(results)
        .map(|(channel, result)| match result {
            Ok(result) => (channel, result),
            Err(message) => (channel, Err(message)),
        })
        .collect()
}

fn candidate_channel(
    channel_id: &str,
    plugin_id: &str,
    source_id: &str,
    channel_name: &str,
    action: source_runtime::SourceAction,
) -> online_music::OnlineChannel {
    online_music::OnlineChannel {
        id: channel_id.to_owned(),
        plugin_id: plugin_id.to_owned(),
        plugin_name: plugin_id.to_owned(),
        provider_id: String::new(),
        source_id: source_id.to_owned(),
        source_name: channel_name.to_owned(),
        excluded: false,
        actions: vec![action],
    }
}

type CandidateCluster<Candidate, Sample> = (Vec<Candidate>, Option<Sample>);
type AlbumCandidateSample = (Option<u32>, Vec<online_music::OnlineTrack>);
type AlbumCandidateCluster =
    CandidateCluster<online_music::OnlineAlbumCandidate, AlbumCandidateSample>;

fn disambiguate_artist_group(
    state: &AppState,
    group: Vec<online_music::OnlineArtistCandidate>,
    settings: &online_music::OnlineMusicSettings,
    _page_size: u64,
) -> Vec<Vec<online_music::OnlineArtistCandidate>> {
    if group.len() <= 1 {
        return vec![group];
    }
    let original_candidates = group.clone();
    let samples = state
        .online_executor
        .map(group, |candidate| {
            let tracks = artist_candidate_track_page(state, &candidate, 10)
                .ok()
                .map(|response| {
                    online_music::merge_tracks(
                        response
                            .list
                            .into_iter()
                            .enumerate()
                            .map(|(index, track)| {
                                online_music::OnlineTrackCandidate::from_source(
                                    &candidate_channel(
                                        &candidate.channel_id,
                                        &candidate.plugin_id,
                                        &candidate.source_id,
                                        &candidate.channel_name,
                                        source_runtime::SourceAction::ArtistTopTracks,
                                    ),
                                    track,
                                    index as u32 + 1,
                                )
                            })
                            .collect(),
                        &settings.channel_priority,
                    )
                });
            (candidate, tracks)
        })
        .into_iter()
        .zip(original_candidates)
        .map(|(result, candidate)| result.unwrap_or((candidate, None)))
        .collect::<Vec<_>>();
    let mut clusters: Vec<
        CandidateCluster<online_music::OnlineArtistCandidate, Vec<online_music::OnlineTrack>>,
    > = Vec::new();
    for (candidate, sample) in samples {
        let matching = sample.as_ref().and_then(|sample| {
            clusters.iter().position(|(_, existing)| {
                existing
                    .as_ref()
                    .is_some_and(|existing| online_music::artist_samples_match(existing, sample))
            })
        });
        if let Some(index) = matching {
            clusters[index].0.push(candidate);
        } else {
            clusters.push((vec![candidate], sample));
        }
    }
    clusters.into_iter().map(|(group, _)| group).collect()
}

fn disambiguate_album_group(
    state: &AppState,
    group: Vec<online_music::OnlineAlbumCandidate>,
    settings: &online_music::OnlineMusicSettings,
    _page_size: u64,
) -> Vec<Vec<online_music::OnlineAlbumCandidate>> {
    if group.len() <= 1 {
        return vec![group];
    }
    let original_candidates = group.clone();
    let samples = state
        .online_executor
        .map(group, |candidate| {
            let tracks = album_candidate_track_page(state, &candidate, 1, 30)
                .ok()
                .map(|response| {
                    online_music::merge_tracks(
                        response
                            .list
                            .into_iter()
                            .enumerate()
                            .map(|(index, track)| {
                                online_music::OnlineTrackCandidate::from_source(
                                    &candidate_channel(
                                        &candidate.channel_id,
                                        &candidate.plugin_id,
                                        &candidate.source_id,
                                        &candidate.channel_name,
                                        source_runtime::SourceAction::AlbumRead,
                                    ),
                                    track,
                                    index as u32 + 1,
                                )
                            })
                            .collect(),
                        &settings.channel_priority,
                    )
                });
            (candidate, tracks)
        })
        .into_iter()
        .zip(original_candidates)
        .map(|(result, candidate)| result.unwrap_or((candidate, None)))
        .collect::<Vec<_>>();
    let mut clusters: Vec<AlbumCandidateCluster> = Vec::new();
    for (candidate, sample) in samples {
        let sample = sample.map(|tracks| (candidate.release_year, tracks));
        let matching = sample.as_ref().and_then(|(year, tracks)| {
            clusters.iter().position(|(_, existing)| {
                existing.as_ref().is_some_and(|(existing_year, existing)| {
                    online_music::album_samples_match(*existing_year, existing, *year, tracks)
                })
            })
        });
        if let Some(index) = matching {
            clusters[index].0.push(candidate);
        } else {
            clusters.push((vec![candidate], sample));
        }
    }
    clusters.into_iter().map(|(group, _)| group).collect()
}

fn dispatch_candidate_request(
    state: &AppState,
    channel: online_music::OnlineChannel,
    request: source_runtime::SourceRequest,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<source_runtime::SourceRequestOutcome> {
    let mut outcomes = dispatch_channels(state, &[channel], cancellation, |_| request.clone());
    outcomes
        .pop()
        .ok_or_else(|| "No provider handled the detail request".to_owned())?
        .1
}

fn dispatch_candidate_request_typed(
    state: &AppState,
    channel: &online_music::OnlineChannel,
    request: source_runtime::SourceRequest,
    cancellation: source_runtime::SourceCancellationToken,
) -> Result<source_runtime::SourceRequestOutcome, PluginSystemError> {
    let dispatch = {
        let registry = state.plugin_registry.lock().map_err(|_| {
            PluginSystemError::Package("plugin registry lock was poisoned".to_owned())
        })?;
        registry.prepare_action(&channel.plugin_id, &channel.source_id, request.action())?
    };
    execute_cached_plugin_request(
        &state.online_music_cache,
        channel,
        request,
        cancellation,
        dispatch,
    )
}

fn should_cache_online_request(request: &source_runtime::SourceRequest) -> bool {
    !matches!(
        request,
        source_runtime::SourceRequest::MusicRecommendations {
            kind: source_runtime::MusicRecommendationKind::Roaming,
            ..
        } | source_runtime::SourceRequest::PlaylistList { .. }
    )
}

fn execute_cached_plugin_request(
    cache: &online_music::OnlineMusicCache,
    channel: &online_music::OnlineChannel,
    request: source_runtime::SourceRequest,
    cancellation: source_runtime::SourceCancellationToken,
    dispatch: plugin_system::PreparedPluginRequest,
) -> Result<source_runtime::SourceRequestOutcome, PluginSystemError> {
    if cancellation.is_cancelled() {
        return Err(PluginSystemError::Package("request cancelled".to_owned()));
    }
    if !should_cache_online_request(&request) {
        return dispatch.execute(request, cancellation);
    }
    let cache_key = serde_json::to_string(&(
        channel.plugin_id.as_str(),
        channel.source_id.as_str(),
        &request,
    ))?;
    if let Some(cached) = cache.get(&cache_key) {
        return Ok(cached);
    }
    let outcome = dispatch.execute(request, cancellation)?;
    cache.insert(cache_key, outcome.clone());
    Ok(outcome)
}

fn artist_candidate_track_page(
    state: &AppState,
    candidate: &online_music::OnlineArtistCandidate,
    limit: u64,
) -> CommandResult<source_runtime::SourceSearchResponse> {
    let outcome = dispatch_candidate_request(
        state,
        candidate_channel(
            &candidate.channel_id,
            &candidate.plugin_id,
            &candidate.source_id,
            &candidate.channel_name,
            source_runtime::SourceAction::ArtistTopTracks,
        ),
        source_runtime::SourceRequest::ArtistTopTracks {
            source: candidate.source_id.clone(),
            artist: source_runtime::SourceEntityRef {
                id: candidate.id.clone(),
                platform_ids: candidate.platform_ids.clone(),
                raw_info: candidate.raw_info.clone(),
            },
            limit,
        },
        source_runtime::SourceCancellationToken::default(),
    )?;
    match outcome.response {
        source_runtime::SourceResponse::ArtistTopTracks(response) => Ok(response),
        _ => Err("provider returned an unexpected artist response".to_owned()),
    }
}

fn artist_candidate_album_page(
    state: &AppState,
    candidate: &online_music::OnlineArtistCandidate,
    page: u64,
    page_size: u64,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<source_runtime::SourceAlbumSearchResponse> {
    let outcome = dispatch_candidate_request(
        state,
        candidate_channel(
            &candidate.channel_id,
            &candidate.plugin_id,
            &candidate.source_id,
            &candidate.channel_name,
            source_runtime::SourceAction::ArtistAlbums,
        ),
        source_runtime::SourceRequest::ArtistAlbums {
            source: candidate.source_id.clone(),
            artist: source_runtime::SourceEntityRef {
                id: candidate.id.clone(),
                platform_ids: candidate.platform_ids.clone(),
                raw_info: candidate.raw_info.clone(),
            },
            page,
            page_size,
        },
        cancellation,
    )?;
    match outcome.response {
        source_runtime::SourceResponse::ArtistAlbums(response) => Ok(response),
        _ => Err("provider returned an unexpected artist albums response".to_owned()),
    }
}

fn artist_candidate_biography(
    state: &AppState,
    candidate: &online_music::OnlineArtistCandidate,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<source_runtime::SourceArtistBiography> {
    let outcome = dispatch_candidate_request(
        state,
        candidate_channel(
            &candidate.channel_id,
            &candidate.plugin_id,
            &candidate.source_id,
            &candidate.channel_name,
            source_runtime::SourceAction::ArtistBiography,
        ),
        source_runtime::SourceRequest::ArtistBiography {
            source: candidate.source_id.clone(),
            artist: source_runtime::SourceEntityRef {
                id: candidate.id.clone(),
                platform_ids: candidate.platform_ids.clone(),
                raw_info: candidate.raw_info.clone(),
            },
        },
        cancellation,
    )?;
    match outcome.response {
        source_runtime::SourceResponse::ArtistBiography(response) => Ok(response),
        _ => Err("provider returned an unexpected artist biography response".to_owned()),
    }
}

fn album_candidate_track_page(
    state: &AppState,
    candidate: &online_music::OnlineAlbumCandidate,
    page: u64,
    page_size: u64,
) -> CommandResult<source_runtime::SourceSearchResponse> {
    let outcome = dispatch_candidate_request(
        state,
        candidate_channel(
            &candidate.channel_id,
            &candidate.plugin_id,
            &candidate.source_id,
            &candidate.channel_name,
            source_runtime::SourceAction::AlbumRead,
        ),
        source_runtime::SourceRequest::AlbumRead {
            source: candidate.source_id.clone(),
            album: source_runtime::SourceEntityRef {
                id: candidate.id.clone(),
                platform_ids: candidate.platform_ids.clone(),
                raw_info: candidate.raw_info.clone(),
            },
            page,
            page_size,
        },
        source_runtime::SourceCancellationToken::default(),
    )?;
    match outcome.response {
        source_runtime::SourceResponse::AlbumRead(response) => Ok(response),
        _ => Err("provider returned an unexpected album response".to_owned()),
    }
}

#[tauri::command]
async fn online_music_artist_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    artist: online_music::OnlineArtist,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineTrackPage> {
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_artist_tracks_inner(&state, artist, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online artist detail task failed: {error}"))?
}

fn online_artist_tracks_inner(
    state: &AppState,
    artist: online_music::OnlineArtist,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineTrackPage> {
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    let mut candidates = Vec::new();
    for artist_candidate in artist.candidates {
        if cancellation.is_cancelled() {
            return Err("request cancelled".to_owned());
        }
        if let Ok(response) = artist_candidate_track_page(state, &artist_candidate, 50) {
            candidates.extend(response.list.into_iter().enumerate().map(|(index, track)| {
                online_music::OnlineTrackCandidate::from_source(
                    &candidate_channel(
                        &artist_candidate.channel_id,
                        &artist_candidate.plugin_id,
                        &artist_candidate.source_id,
                        &artist_candidate.channel_name,
                        source_runtime::SourceAction::ArtistTopTracks,
                    ),
                    track,
                    index as u32 + 1,
                )
            }));
        }
    }
    let mut items = online_music::merge_tracks(candidates, &settings.channel_priority);
    items.truncate(50);
    Ok(online_music::OnlineTrackPage {
        total: Some(items.len() as u64),
        items,
        has_more: false,
    })
}

#[tauri::command]
async fn online_music_artist_albums(
    app: AppHandle,
    state: State<'_, AppState>,
    artist: online_music::OnlineArtist,
    page: u64,
    page_size: u64,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineAlbumPage> {
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_artist_albums_inner(&state, artist, page, page_size, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online artist albums task failed: {error}"))?
}

fn online_artist_albums_inner(
    state: &AppState,
    artist: online_music::OnlineArtist,
    page: u64,
    page_size: u64,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineAlbumPage> {
    if page == 0 || !(1..=200).contains(&page_size) {
        return Err("Invalid artist albums page".to_owned());
    }
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    let mut candidates = Vec::new();
    let mut has_more = false;
    let mut total = None;
    let mut completed_candidates = 0_u64;
    for artist_candidate in artist.candidates {
        if cancellation.is_cancelled() {
            return Err("request cancelled".to_owned());
        }
        let Ok(response) = artist_candidate_album_page(
            state,
            &artist_candidate,
            page,
            page_size,
            cancellation.clone(),
        ) else {
            continue;
        };
        completed_candidates += 1;
        has_more |= !response.is_end;
        total = total.max(response.total);
        let channel = candidate_channel(
            &artist_candidate.channel_id,
            &artist_candidate.plugin_id,
            &artist_candidate.source_id,
            &artist_candidate.channel_name,
            source_runtime::SourceAction::ArtistAlbums,
        );
        candidates.extend(response.list.into_iter().enumerate().map(|(index, album)| {
            online_music::OnlineAlbumCandidate::from_source(
                &channel,
                album,
                rank_for_page(page, page_size, index),
            )
        }));
    }
    if completed_candidates == 0 {
        return Err("Artist albums are unavailable from the configured sources".to_owned());
    }
    let groups = online_music::group_album_candidates(candidates)
        .into_values()
        .collect();
    Ok(online_music::OnlineAlbumPage {
        items: online_music::merge_albums(groups, &settings.channel_priority),
        has_more,
        total,
    })
}

#[tauri::command]
async fn online_music_artist_biography(
    app: AppHandle,
    state: State<'_, AppState>,
    artist: online_music::OnlineArtist,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineArtistBiography> {
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_artist_biography_inner(&state, artist, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online artist biography task failed: {error}"))?
}

fn online_artist_biography_inner(
    state: &AppState,
    artist: online_music::OnlineArtist,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineArtistBiography> {
    let mut empty_biography = None;
    for artist_candidate in artist.candidates {
        if cancellation.is_cancelled() {
            return Err("request cancelled".to_owned());
        }
        let Ok(biography) =
            artist_candidate_biography(state, &artist_candidate, cancellation.clone())
        else {
            continue;
        };
        let biography = online_music::OnlineArtistBiography::from_source(
            artist_candidate.channel_name,
            biography,
        );
        if biography.summary.is_some() || !biography.sections.is_empty() {
            return Ok(biography);
        }
        empty_biography = Some(biography);
    }
    empty_biography
        .ok_or_else(|| "Artist biography is unavailable from the configured sources".to_owned())
}

#[tauri::command]
async fn online_music_album_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    album: online_music::OnlineAlbum,
    page: u64,
    page_size: u64,
    request_id: Option<String>,
) -> CommandResult<online_music::OnlineTrackPage> {
    let cancellation = register_source_request(state.inner(), request_id.as_deref())
        .map_err(|error| error.message)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        online_album_tracks_inner(&state, album, page, page_size, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    result.map_err(|error| format!("online album detail task failed: {error}"))?
}

fn online_album_tracks_inner(
    state: &AppState,
    album: online_music::OnlineAlbum,
    page: u64,
    page_size: u64,
    cancellation: source_runtime::SourceCancellationToken,
) -> CommandResult<online_music::OnlineTrackPage> {
    if page == 0 || !(1..=200).contains(&page_size) {
        return Err("Invalid album page".to_owned());
    }
    let settings = {
        let db = state
            .db
            .lock()
            .map_err(|_| "database lock was poisoned".to_owned())?;
        online_music::load_settings(&db).map_err(|error| error.to_string())?
    };
    let mut candidates = Vec::new();
    let mut has_more = false;
    let mut total = None;
    for album_candidate in album.candidates {
        if cancellation.is_cancelled() {
            return Err("request cancelled".to_owned());
        }
        if let Ok(response) = album_candidate_track_page(state, &album_candidate, page, page_size) {
            has_more |= !response.is_end;
            total = total.max(response.total);
            candidates.extend(response.list.into_iter().enumerate().map(|(index, track)| {
                online_music::OnlineTrackCandidate::from_source(
                    &candidate_channel(
                        &album_candidate.channel_id,
                        &album_candidate.plugin_id,
                        &album_candidate.source_id,
                        &album_candidate.channel_name,
                        source_runtime::SourceAction::AlbumRead,
                    ),
                    track,
                    rank_for_page(page, page_size, index),
                )
            }));
        }
    }
    let mut items = online_music::merge_tracks(candidates, &settings.channel_priority);
    items.sort_by_key(|track| {
        (
            track.disc_number.unwrap_or(u32::MAX),
            track.track_number.unwrap_or(u32::MAX),
            track.key.clone(),
        )
    });
    Ok(online_music::OnlineTrackPage {
        items,
        has_more,
        total,
    })
}

#[tauri::command]
async fn online_music_playlist_tracks(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist: online_music::OnlinePlaylist,
    page: u64,
    page_size: u64,
    request_id: Option<String>,
) -> Result<online_music::OnlineTrackPage, online_music::OnlinePlaylistDetailError> {
    let cancellation =
        register_source_request(state.inner(), request_id.as_deref()).map_err(|error| {
            online_music::OnlinePlaylistDetailError {
                code: "request-failure".to_owned(),
                message: error.message,
                plugin_id: playlist.plugin_id.clone(),
                channel_name: playlist.channel_name.clone(),
            }
        })?;
    let error_plugin_id = playlist.plugin_id.clone();
    let error_channel_name = playlist.channel_name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<_, PluginSystemError> {
        let state = app.state::<AppState>();
        if page == 0 || !(1..=200).contains(&page_size) {
            return Err(online_music_plugin_error(
                &playlist.plugin_id,
                "invalid-request",
                "Invalid playlist page",
            ));
        }
        let channel = candidate_channel(
            &playlist.channel_id,
            &playlist.plugin_id,
            &playlist.source_id,
            &playlist.channel_name,
            if playlist.account_ref.is_some() {
                source_runtime::SourceAction::PlaylistRead
            } else {
                source_runtime::SourceAction::PlaylistReadPublic
            },
        );
        let (tracks, has_more, total) = if let Some(account_ref) = playlist.account_ref {
            let outcome = dispatch_candidate_request_typed(
                &state,
                &channel,
                source_runtime::SourceRequest::PlaylistRead {
                    source: playlist.source_id.clone(),
                    account_ref: account_ref.clone(),
                    playlist_id: playlist.id,
                },
                cancellation,
            )?;
            let source_runtime::SourceResponse::PlaylistRead(response) = outcome.response else {
                return Err(online_music_plugin_error(
                    &channel.plugin_id,
                    "unexpected-response",
                    "provider returned an unexpected playlist response",
                ));
            };
            account_playlist_track_page(response.tracks, &account_ref, page, page_size)
        } else {
            let outcome = dispatch_candidate_request_typed(
                &state,
                &channel,
                source_runtime::SourceRequest::PlaylistReadPublic {
                    source: playlist.source_id.clone(),
                    playlist: source_runtime::SourceEntityRef {
                        id: playlist.id,
                        platform_ids: playlist.platform_ids,
                        raw_info: playlist.raw_info,
                    },
                    page,
                    page_size,
                },
                cancellation,
            )?;
            let source_runtime::SourceResponse::PlaylistReadPublic(response) = outcome.response
            else {
                return Err(online_music_plugin_error(
                    &channel.plugin_id,
                    "unexpected-response",
                    "provider returned an unexpected playlist response",
                ));
            };
            (response.list, !response.is_end, response.total)
        };
        let items = online_music::merge_tracks(
            tracks
                .into_iter()
                .enumerate()
                .map(|(index, track)| {
                    online_music::OnlineTrackCandidate::from_source(
                        &channel,
                        track,
                        rank_for_page(page, page_size, index),
                    )
                })
                .collect(),
            &[playlist.channel_id],
        );
        Ok(online_music::OnlineTrackPage {
            items,
            has_more,
            total,
        })
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());
    match result {
        Ok(Ok(page)) => Ok(page),
        Ok(Err(error)) => Err(online_music::OnlinePlaylistDetailError {
            code: plugin_error_code(&error)
                .unwrap_or("provider-failure")
                .to_owned(),
            message: error.to_string(),
            plugin_id: error_plugin_id,
            channel_name: error_channel_name,
        }),
        Err(error) => Err(online_music::OnlinePlaylistDetailError {
            code: "task-failure".to_owned(),
            message: format!("online playlist detail task failed: {error}"),
            plugin_id: error_plugin_id,
            channel_name: error_channel_name,
        }),
    }
}

fn account_playlist_track_page(
    tracks: Vec<source_runtime::SourceSearchResult>,
    account_ref: &str,
    page: u64,
    page_size: u64,
) -> (Vec<source_runtime::SourceSearchResult>, bool, Option<u64>) {
    let total = tracks.len() as u64;
    let start =
        usize::try_from(page.saturating_sub(1).saturating_mul(page_size)).unwrap_or(usize::MAX);
    let limit = usize::try_from(page_size).unwrap_or(usize::MAX);
    let items = tracks
        .into_iter()
        .skip(start)
        .take(limit)
        .map(|mut track| {
            track.platform_ids.insert(
                "accountRef".to_owned(),
                source_runtime::JsonScalar::String(account_ref.to_owned()),
            );
            track
        })
        .collect();
    (items, page.saturating_mul(page_size) < total, Some(total))
}

fn plugin_error_code(error: &PluginSystemError) -> Option<&str> {
    match error {
        PluginSystemError::Runtime { code, .. } => code.as_deref(),
        _ => None,
    }
}

fn online_music_plugin_error(plugin_id: &str, code: &str, message: &str) -> PluginSystemError {
    PluginSystemError::Runtime {
        plugin_id: plugin_id.to_owned(),
        code: Some(code.to_owned()),
        message: message.to_owned(),
        diagnostics: Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct PluginCommandError {
    message: String,
    diagnostics: Vec<PluginDiagnostic>,
}

impl From<PluginSystemError> for PluginCommandError {
    fn from(error: PluginSystemError) -> Self {
        Self {
            message: error.to_string(),
            diagnostics: error.diagnostics(),
        }
    }
}

fn plugin_lock_error(name: &'static str) -> PluginCommandError {
    PluginCommandError {
        message: format!("plugin {name} lock was poisoned"),
        diagnostics: Vec::new(),
    }
}

fn plugin_command_error(message: impl Into<String>) -> PluginCommandError {
    PluginCommandError {
        message: message.into(),
        diagnostics: Vec::new(),
    }
}

fn audio_source_lock_error(name: &'static str) -> AudioSourceCommandError {
    AudioSourceCommandError {
        message: format!("audio source {name} lock was poisoned"),
        diagnostics: Vec::new(),
    }
}

fn audio_source_command_error(message: impl Into<String>) -> AudioSourceCommandError {
    AudioSourceCommandError {
        message: message.into(),
        diagnostics: Vec::new(),
    }
}

fn chksz_audio_source_command_error(
    error: chksz_playback::ChkszPlaybackError,
) -> AudioSourceCommandError {
    audio_source_command_error(error.to_string())
}

fn parse_audio_source_capabilities(
    capabilities: &[String],
) -> Result<Vec<source_runtime::SourceCapability>, AudioSourceCommandError> {
    capabilities
        .iter()
        .map(|capability| {
            serde_json::from_value(serde_json::Value::String(capability.clone())).map_err(|_| {
                audio_source_command_error(format!(
                    "unsupported audio source capability: {capability}"
                ))
            })
        })
        .collect()
}

#[tauri::command]
fn list_audio_sources(
    state: State<'_, AppState>,
) -> Result<Vec<AudioSourceRecord>, AudioSourceCommandError> {
    let registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    Ok(registry.records())
}

#[tauri::command]
fn get_chksz_api_key_status(state: State<'_, AppState>) -> Result<bool, AudioSourceCommandError> {
    state
        .chksz_playback
        .api_key_configured()
        .map_err(chksz_audio_source_command_error)
}

#[tauri::command]
fn set_chksz_api_key(
    state: State<'_, AppState>,
    api_key: String,
) -> Result<(), AudioSourceCommandError> {
    state
        .chksz_playback
        .set_api_key(&api_key)
        .map_err(chksz_audio_source_command_error)
}

#[tauri::command]
fn clear_chksz_api_key(state: State<'_, AppState>) -> Result<(), AudioSourceCommandError> {
    state
        .chksz_playback
        .clear_api_key()
        .map_err(chksz_audio_source_command_error)
}

#[tauri::command]
async fn select_audio_source_file() -> Result<Option<String>, AudioSourceCommandError> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("Choose an audio source")
        .add_filter("JavaScript source", &["js", "mjs", "cjs"])
        .pick_file()
        .await;
    Ok(file.map(|handle| handle.path().to_string_lossy().into_owned()))
}

#[tauri::command]
fn refresh_audio_sources(
    state: State<'_, AppState>,
) -> Result<Vec<AudioSourceRecord>, AudioSourceCommandError> {
    let db = state
        .db
        .lock()
        .map_err(|_| audio_source_lock_error("database"))?;
    let mut registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    registry.refresh(&db).map_err(Into::into)
}

#[tauri::command]
fn import_audio_source(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<AudioSourceRecord, AudioSourceCommandError> {
    let source_path = source_path.trim();
    if source_path.is_empty() {
        return Err(audio_source_command_error(
            "audio source path must not be empty",
        ));
    }
    let db = state
        .db
        .lock()
        .map_err(|_| audio_source_lock_error("database"))?;
    let mut registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    registry
        .import_file(&db, Path::new(source_path))
        .map_err(Into::into)
}

#[tauri::command]
async fn import_audio_source_url(
    app: AppHandle,
    source_url: String,
) -> Result<AudioSourceRecord, AudioSourceCommandError> {
    let source_url = source_url.trim().to_owned();
    if source_url.is_empty() {
        return Err(audio_source_command_error(
            "audio source URL must not be empty",
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let prepared = audio_source_system::prepare_remote_audio_source_import(&source_url)
            .map_err(AudioSourceCommandError::from)?;
        let state = app.state::<AppState>();
        let db = state
            .db
            .lock()
            .map_err(|_| audio_source_lock_error("database"))?;
        let mut registry = state
            .audio_source_registry
            .lock()
            .map_err(|_| audio_source_lock_error("registry"))?;
        registry.install_prepared(&db, prepared).map_err(Into::into)
    })
    .await
    .map_err(|error| {
        audio_source_command_error(format!("audio source URL import task failed: {error}"))
    })?
}

#[tauri::command]
fn set_audio_source_capabilities(
    state: State<'_, AppState>,
    audio_source_id: String,
    capabilities: Vec<String>,
    reviewed: bool,
) -> Result<AudioSourceRecord, AudioSourceCommandError> {
    let capabilities = parse_audio_source_capabilities(&capabilities)?;
    let db = state
        .db
        .lock()
        .map_err(|_| audio_source_lock_error("database"))?;
    let mut registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    registry
        .set_capabilities(&db, audio_source_id.trim(), capabilities, reviewed)
        .map_err(Into::into)
}

#[tauri::command]
fn set_audio_source_enabled(
    state: State<'_, AppState>,
    audio_source_id: String,
    enabled: bool,
) -> Result<AudioSourceRecord, AudioSourceCommandError> {
    let db = state
        .db
        .lock()
        .map_err(|_| audio_source_lock_error("database"))?;
    let mut registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    registry
        .set_enabled(&db, audio_source_id.trim(), enabled)
        .map_err(Into::into)
}

#[tauri::command]
fn remove_audio_source(
    state: State<'_, AppState>,
    audio_source_id: String,
) -> Result<Vec<AudioSourceRecord>, AudioSourceCommandError> {
    let db = state
        .db
        .lock()
        .map_err(|_| audio_source_lock_error("database"))?;
    let mut registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    registry
        .remove(&db, audio_source_id.trim())
        .map_err(Into::into)
}

#[tauri::command]
fn clear_audio_source_diagnostics(
    state: State<'_, AppState>,
    audio_source_id: String,
) -> Result<AudioSourceRecord, AudioSourceCommandError> {
    let db = state
        .db
        .lock()
        .map_err(|_| audio_source_lock_error("database"))?;
    let mut registry = state
        .audio_source_registry
        .lock()
        .map_err(|_| audio_source_lock_error("registry"))?;
    registry
        .clear_diagnostics(&db, audio_source_id.trim())
        .map_err(Into::into)
}

#[tauri::command]
async fn dispatch_audio_source_request(
    app: AppHandle,
    state: State<'_, AppState>,
    audio_source_id: String,
    request: source_runtime::SourceRequest,
    request_id: Option<String>,
) -> Result<source_runtime::SourceRequestOutcome, AudioSourceCommandError> {
    let audio_source_id = audio_source_id.trim().to_owned();
    if audio_source_id.is_empty() {
        return Err(audio_source_command_error(
            "audio source id must not be empty",
        ));
    }
    let cancellation =
        register_source_request(state.inner(), request_id.as_deref()).map_err(|error| {
            AudioSourceCommandError {
                message: error.message,
                diagnostics: Vec::new(),
            }
        })?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        dispatch_audio_source_request_inner(&state, &audio_source_id, request, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());

    match result {
        Ok(result) => result,
        Err(error) => Err(audio_source_command_error(format!(
            "audio source request task failed: {error}"
        ))),
    }
}

fn dispatch_audio_source_request_inner(
    state: &AppState,
    audio_source_id: &str,
    request: source_runtime::SourceRequest,
    cancellation: source_runtime::SourceCancellationToken,
) -> Result<source_runtime::SourceRequestOutcome, AudioSourceCommandError> {
    let dispatch = {
        let registry = state
            .audio_source_registry
            .lock()
            .map_err(|_| audio_source_lock_error("registry"))?;
        registry.prepare_dispatch(audio_source_id, &request)?
    };
    let result = dispatch.execute(request, cancellation);
    if let Ok(db) = state.db.lock() {
        if let Ok(mut registry) = state.audio_source_registry.lock() {
            registry.complete_dispatch_best_effort(&db, &dispatch, &result);
        }
    }
    result.map_err(Into::into)
}

#[tauri::command]
async fn check_audio_source_availability(
    app: AppHandle,
    audio_source_id: String,
    source_id: Option<String>,
) -> Result<Vec<AudioSourceAvailability>, AudioSourceCommandError> {
    let audio_source_id = audio_source_id.trim().to_owned();
    if audio_source_id.is_empty() {
        return Err(audio_source_command_error(
            "audio source id must not be empty",
        ));
    }
    let source_id = source_id.map(|value| value.trim().to_owned());
    if source_id.as_deref().is_some_and(str::is_empty) {
        return Err(audio_source_command_error("source id must not be empty"));
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        check_audio_source_availability_inner(&state, &audio_source_id, source_id.as_deref())
    })
    .await;

    match result {
        Ok(result) => result,
        Err(error) => Err(audio_source_command_error(format!(
            "audio source availability task failed: {error}"
        ))),
    }
}

fn check_audio_source_availability_inner(
    state: &AppState,
    audio_source_id: &str,
    requested_source_id: Option<&str>,
) -> Result<Vec<AudioSourceAvailability>, AudioSourceCommandError> {
    let probes = {
        let registry = state
            .audio_source_registry
            .lock()
            .map_err(|_| audio_source_lock_error("registry"))?;
        let Some(record) = registry.record(audio_source_id) else {
            return Err(audio_source_command_error(format!(
                "audio source {audio_source_id} was not found"
            )));
        };
        if !record.enabled {
            return Err(audio_source_command_error(
                "audio source must be enabled before availability checks",
            ));
        }

        let probes = record
            .sources
            .into_iter()
            .filter(|source| {
                source
                    .actions
                    .contains(&source_runtime::SourceAction::MusicUrl)
                    && requested_source_id.is_none_or(|id| source.id == id)
            })
            .map(|source| {
                (
                    source.id,
                    source.name,
                    source.qualities.first().copied().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        if probes.is_empty() {
            return Err(audio_source_command_error(match requested_source_id {
                Some(source_id) => {
                    format!("audio source does not declare musicUrl for source {source_id}")
                }
                None => "audio source does not declare any musicUrl sources".to_owned(),
            }));
        }
        probes
    };

    let probe_metadata = probes.clone();
    let results = state
        .online_executor
        .map(probes, |(source_id, source_name, quality)| {
            let started_at = Instant::now();
            let request = audio_source_availability_request(&source_id, quality);
            let (available, message) = match dispatch_audio_source_request_inner(
                state,
                audio_source_id,
                request,
                source_runtime::SourceCancellationToken::default(),
            ) {
                Ok(outcome) => match outcome.response {
                    source_runtime::SourceResponse::MusicUrl(url) if !url.trim().is_empty() => {
                        (true, None)
                    }
                    source_runtime::SourceResponse::MusicUrl(_) => (
                        false,
                        Some("provider returned an empty playback URL".to_owned()),
                    ),
                    _ => (
                        false,
                        Some("provider returned an unexpected response".to_owned()),
                    ),
                },
                Err(error) => (false, Some(error.message)),
            };

            AudioSourceAvailability {
                audio_source_id: audio_source_id.to_owned(),
                source_id,
                source_name,
                quality,
                available,
                latency_ms: started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                message,
            }
        });

    Ok(results
        .into_iter()
        .zip(probe_metadata)
        .map(|(result, (source_id, source_name, quality))| {
            result.unwrap_or_else(|message| AudioSourceAvailability {
                audio_source_id: audio_source_id.to_owned(),
                source_id,
                source_name,
                quality,
                available: false,
                latency_ms: 0,
                message: Some(message),
            })
        })
        .collect())
}

fn audio_source_availability_request(
    source_id: &str,
    quality: source_runtime::SourceQuality,
) -> source_runtime::SourceRequest {
    let (track_id, title, artist) = match source_id {
        source_runtime::LX_SOURCE_KG => (
            "4D766DEC7A90A011D730ED939D158131",
            "Under My Skin",
            "Andrew Cui",
        ),
        source_runtime::LX_SOURCE_WY => ("347230", "Test Track", "Test Artist"),
        youtube_music::YOUTUBE_MUSIC_SOURCE_ID => ("ZrOKjDZOtkA", "Test Track", "Test Artist"),
        _ => ("347230", "Test Track", "Test Artist"),
    };
    let mut music_info = JsonMap::new();
    music_info.insert("id".to_owned(), JsonValue::String(track_id.to_owned()));
    music_info.insert("title".to_owned(), JsonValue::String(title.to_owned()));
    music_info.insert("name".to_owned(), JsonValue::String(title.to_owned()));
    music_info.insert("artist".to_owned(), JsonValue::String(artist.to_owned()));
    music_info.insert("singer".to_owned(), JsonValue::String(artist.to_owned()));
    if source_id == source_runtime::LX_SOURCE_WY {
        music_info.insert("songId".to_owned(), JsonValue::String(track_id.to_owned()));
    }
    if source_id == source_runtime::LX_SOURCE_KG {
        music_info.insert("hash".to_owned(), JsonValue::String(track_id.to_owned()));
    }
    if source_id == youtube_music::YOUTUBE_MUSIC_SOURCE_ID {
        music_info.insert("videoId".to_owned(), JsonValue::String(track_id.to_owned()));
    }
    source_runtime::SourceRequest::MusicUrl {
        source: source_id.to_owned(),
        music_info: JsonValue::Object(music_info),
        quality,
    }
}

#[tauri::command]
fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginRecord>, PluginCommandError> {
    let registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    Ok(registry.records())
}

#[tauri::command]
async fn select_plugin_package() -> Result<Option<String>, PluginCommandError> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Choose a Plugin package")
        .pick_folder()
        .await;
    Ok(folder.map(|handle| handle.path().to_string_lossy().into_owned()))
}

#[tauri::command]
fn refresh_plugins(state: State<'_, AppState>) -> Result<Vec<PluginRecord>, PluginCommandError> {
    let db = state.db.lock().map_err(|_| plugin_lock_error("database"))?;
    let mut registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    registry.refresh(&db).map_err(Into::into)
}

#[tauri::command]
fn install_plugin_package(
    state: State<'_, AppState>,
    package_path: String,
) -> Result<PluginRecord, PluginCommandError> {
    let package_path = package_path.trim();
    if package_path.is_empty() {
        return Err(PluginCommandError {
            message: "Plugin package path must not be empty".to_owned(),
            diagnostics: Vec::new(),
        });
    }
    let db = state.db.lock().map_err(|_| plugin_lock_error("database"))?;
    let mut registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    registry
        .install(&db, Path::new(package_path))
        .map_err(Into::into)
}

#[tauri::command]
fn set_plugin_capabilities(
    state: State<'_, AppState>,
    plugin_id: String,
    capabilities: Vec<String>,
    reviewed: bool,
) -> Result<PluginRecord, PluginCommandError> {
    let capabilities =
        plugin_system::parse_capabilities(&capabilities).map_err(|message| PluginCommandError {
            message,
            diagnostics: Vec::new(),
        })?;
    let db = state.db.lock().map_err(|_| plugin_lock_error("database"))?;
    let mut registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    registry
        .set_capabilities(&db, &plugin_id, capabilities, reviewed)
        .map_err(Into::into)
}

#[tauri::command]
fn set_plugin_enabled(
    state: State<'_, AppState>,
    plugin_id: String,
    enabled: bool,
) -> Result<PluginRecord, PluginCommandError> {
    let db = state.db.lock().map_err(|_| plugin_lock_error("database"))?;
    let mut registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    registry
        .set_enabled(&db, &plugin_id, enabled)
        .map_err(Into::into)
}

#[tauri::command]
fn remove_plugin(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<PluginRecord>, PluginCommandError> {
    let db = state.db.lock().map_err(|_| plugin_lock_error("database"))?;
    let mut registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    registry.remove(&db, &plugin_id).map_err(Into::into)
}

#[tauri::command]
fn clear_plugin_diagnostics(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<PluginRecord, PluginCommandError> {
    let db = state.db.lock().map_err(|_| plugin_lock_error("database"))?;
    let mut registry = state
        .plugin_registry
        .lock()
        .map_err(|_| plugin_lock_error("registry"))?;
    registry
        .clear_diagnostics(&db, &plugin_id)
        .map_err(Into::into)
}

#[tauri::command]
async fn dispatch_plugin_request(
    app: AppHandle,
    state: State<'_, AppState>,
    plugin_id: String,
    request: source_runtime::SourceRequest,
    request_id: Option<String>,
) -> Result<source_runtime::SourceRequestOutcome, PluginCommandError> {
    let plugin_id = plugin_id.trim().to_owned();
    if plugin_id.is_empty() {
        return Err(plugin_command_error("Plugin id must not be empty"));
    }
    let cancellation =
        register_source_request(state.inner(), request_id.as_deref()).map_err(|error| {
            PluginCommandError {
                message: error.message,
                diagnostics: Vec::new(),
            }
        })?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        dispatch_plugin_request_inner(&state, &plugin_id, request, cancellation)
    })
    .await;
    unregister_source_request(state.inner(), request_id.as_deref());

    match result {
        Ok(result) => result,
        Err(error) => Err(plugin_command_error(format!(
            "Plugin request task failed: {error}"
        ))),
    }
}

fn dispatch_plugin_request_inner(
    state: &AppState,
    plugin_id: &str,
    request: source_runtime::SourceRequest,
    cancellation: source_runtime::SourceCancellationToken,
) -> Result<source_runtime::SourceRequestOutcome, PluginCommandError> {
    dispatch_plugin_request_with(
        state,
        plugin_id,
        request,
        cancellation,
        |dispatch, request, cancellation| dispatch.execute(request, cancellation),
    )
}

fn dispatch_plugin_request_with<F>(
    state: &AppState,
    plugin_id: &str,
    request: source_runtime::SourceRequest,
    cancellation: source_runtime::SourceCancellationToken,
    execute: F,
) -> Result<source_runtime::SourceRequestOutcome, PluginCommandError>
where
    F: FnOnce(
        &plugin_system::PreparedPluginRequest,
        source_runtime::SourceRequest,
        source_runtime::SourceCancellationToken,
    ) -> Result<source_runtime::SourceRequestOutcome, PluginSystemError>,
{
    let dispatch = {
        let registry = state
            .plugin_registry
            .lock()
            .map_err(|_| plugin_lock_error("registry"))?;
        registry.prepare_dispatch(plugin_id, &request)?
    };

    let result = execute(&dispatch, request, cancellation);

    if let Ok(db) = state.db.lock() {
        if let Ok(mut registry) = state.plugin_registry.lock() {
            registry.complete_dispatch_best_effort(&db, &dispatch, &result);
        }
    }

    result.map_err(Into::into)
}

#[tauri::command]
async fn select_music_folder() -> CommandResult<Option<String>> {
    let folder = rfd::AsyncFileDialog::new()
        .set_title("Choose a music folder")
        .pick_folder()
        .await;

    Ok(folder.map(|handle| handle.path().to_string_lossy().into_owned()))
}

#[tauri::command]
fn start_library_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_path: String,
) -> CommandResult<ScanStatus> {
    let folder = PathBuf::from(&folder_path);
    if !folder.is_dir() {
        return Err(AppError::InvalidMusicFolder(folder_path).to_string());
    }

    configure_library_folder(&state, &folder).map_err(|error| error.to_string())?;
    begin_library_scan(&app, &state, folder, true, "Started indexing local tracks.")
        .map_err(|error| error.to_string())
}

fn configure_library_folder(state: &AppState, folder: &Path) -> AppResult<()> {
    if let Some(watcher) = state
        .library_watcher
        .lock()
        .map_err(|_| AppError::StatePoisoned("library_watcher"))?
        .as_ref()
    {
        watcher.set_folder(folder.to_path_buf())?;
    }

    let db = state.db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
    save_library_folder(&db, folder)
}

fn begin_library_scan(
    app: &AppHandle,
    state: &AppState,
    folder: PathBuf,
    force_reindex: bool,
    message: &str,
) -> AppResult<ScanStatus> {
    if !folder.is_dir() {
        return Err(AppError::InvalidMusicFolder(path_to_string(&folder)));
    }

    let initial_status = {
        let mut status = state
            .scan_status
            .lock()
            .map_err(|_| AppError::StatePoisoned("scan_status"))?;

        if status.is_running {
            return Err(AppError::ScanAlreadyRunning);
        }

        *status = ScanStatus {
            is_running: true,
            folder_path: Some(path_to_string(&folder)),
            started_at: Some(now_timestamp()),
            ..ScanStatus::default()
        };

        status.clone()
    };

    emit_scan_status(app, initial_status.clone(), Some(message.to_owned()));

    let scan_app = app.clone();
    std::thread::spawn(move || run_library_scan(scan_app, folder, force_reindex));

    Ok(initial_status)
}

#[tauri::command]
fn get_scan_status(state: State<'_, AppState>) -> CommandResult<ScanStatus> {
    state
        .scan_status
        .lock()
        .map(|status| status.clone())
        .map_err(|_| AppError::StatePoisoned("scan_status").to_string())
}

#[tauri::command]
async fn query_local_library(
    state: State<'_, AppState>,
    request: LibraryQueryRequest,
) -> CommandResult<LibraryQueryPage> {
    let library = Arc::clone(&state.library);
    tauri::async_runtime::spawn_blocking(move || {
        let mut library = library
            .lock()
            .map_err(|_| AppError::StatePoisoned("library").to_string())?;
        Ok(library.query(request))
    })
    .await
    .map_err(|error| format!("library query task failed: {error}"))?
}

#[tauri::command]
fn local_library_view_range(
    state: State<'_, AppState>,
    snapshot_id: String,
    offset: usize,
    limit: usize,
) -> CommandResult<LibraryViewRange> {
    let library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    library
        .view_in_range(snapshot_id.trim(), offset, limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn local_library_track_position(
    state: State<'_, AppState>,
    snapshot_id: String,
    track_id: i64,
) -> CommandResult<Option<usize>> {
    let library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    library
        .track_position(snapshot_id.trim(), track_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_local_library_group_collapsed(
    state: State<'_, AppState>,
    snapshot_id: String,
    group_id: String,
    collapsed: bool,
) -> CommandResult<LibraryGroupToggleResult> {
    let mut library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    library
        .set_group_collapsed(snapshot_id.trim(), group_id.trim(), collapsed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_music_collections(
    state: State<'_, AppState>,
) -> CommandResult<Vec<MusicCollectionSummary>> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::list_collections(&db).map_err(|error| error.to_string())
}

#[tauri::command]
fn create_music_collection(
    state: State<'_, AppState>,
    name: String,
    smart_rules: Option<SmartCollectionRules>,
) -> CommandResult<MusicCollectionSummary> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::create_collection(&mut db, &name, smart_rules).map_err(|error| error.to_string())
}

#[tauri::command]
fn rename_music_collection(
    state: State<'_, AppState>,
    collection_id: String,
    name: String,
) -> CommandResult<MusicCollectionSummary> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::rename_collection(&mut db, collection_id.trim(), &name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_music_collection(state: State<'_, AppState>, collection_id: String) -> CommandResult<()> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::delete_collection(&db, collection_id.trim()).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_music_collection(
    state: State<'_, AppState>,
    collection_id: String,
) -> CommandResult<MusicCollectionDetail> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::collection_detail(&db, collection_id.trim()).map_err(|error| error.to_string())
}

#[tauri::command]
fn add_local_selection_to_music_collection(
    state: State<'_, AppState>,
    collection_id: String,
    snapshot_id: String,
    selection: LibrarySelectionRequest,
) -> CommandResult<MusicCollectionMutation> {
    let track_ids = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?
        .selected_tracks(snapshot_id.trim(), &selection)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|track| track.id)
        .collect::<Vec<_>>();
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::add_local_tracks(&mut db, collection_id.trim(), &track_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_online_tracks_to_music_collection(
    state: State<'_, AppState>,
    collection_id: String,
    tracks: Vec<online_music::OnlineTrack>,
) -> CommandResult<MusicCollectionMutation> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::add_online_tracks(&mut db, collection_id.trim(), &tracks)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_music_collection_items_to_music_collection(
    state: State<'_, AppState>,
    collection_id: String,
    source_collection_id: String,
    item_ids: Vec<String>,
) -> CommandResult<MusicCollectionMutation> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::copy_items(
        &mut db,
        collection_id.trim(),
        source_collection_id.trim(),
        &item_ids,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_music_collection_items(
    state: State<'_, AppState>,
    collection_id: String,
    item_ids: Vec<String>,
) -> CommandResult<MusicCollectionMutation> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;
    collections::remove_items(&mut db, collection_id.trim(), &item_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_album_art_settings(state: State<'_, AppState>) -> CommandResult<AlbumArtSettings> {
    state
        .album_art
        .settings()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_album_art_network_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<AlbumArtSettings> {
    state
        .album_art
        .set_network_enabled(enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn resolve_local_album_cover(
    state: State<'_, AppState>,
    group_id: String,
    release_group_id: Option<String>,
) -> CommandResult<AlbumCoverResult> {
    let target = {
        let library = state
            .library
            .lock()
            .map_err(|_| AppError::StatePoisoned("library").to_string())?;
        library
            .album_target(group_id.trim())
            .map_err(|error| error.to_string())?
    };
    let service = Arc::clone(&state.album_art);
    tauri::async_runtime::spawn_blocking(move || {
        service
            .resolve_album(&target, release_group_id.as_deref())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("album cover task failed: {error}"))?
}

#[tauri::command]
fn get_album_art_task_status(state: State<'_, AppState>) -> CommandResult<AlbumArtTaskStatus> {
    state
        .album_art
        .album_task_status()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_album_art_backfill(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AlbumArtTaskStatus> {
    let targets = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?
        .album_targets();
    state
        .album_art
        .start_album_backfill(targets, move |status| {
            let _ = app.emit(ALBUM_ART_PROGRESS_EVENT, status);
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resume_album_art_backfill(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AlbumArtTaskStatus> {
    state
        .album_art
        .resume_album_backfill(move |status| {
            let _ = app.emit(ALBUM_ART_PROGRESS_EVENT, status);
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn pause_album_art_backfill(state: State<'_, AppState>) -> CommandResult<AlbumArtTaskStatus> {
    state
        .album_art
        .pause_album_backfill()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_metadata_lookup_task_status(
    state: State<'_, AppState>,
) -> CommandResult<MetadataLookupTaskStatus> {
    state
        .album_art
        .metadata_task_status()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_local_metadata_lookup(
    app: AppHandle,
    state: State<'_, AppState>,
    snapshot_id: String,
    selection: LibrarySelectionRequest,
) -> CommandResult<MetadataLookupTaskStatus> {
    let tracks = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?
        .selected_tracks(snapshot_id.trim(), &selection)
        .map_err(|error| error.to_string())?;
    if tracks.is_empty() {
        return Err("the metadata lookup selection is empty".to_owned());
    }
    state
        .album_art
        .start_metadata_lookup(tracks, move |status| {
            let _ = app.emit(METADATA_LOOKUP_PROGRESS_EVENT, status);
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_music_collection_metadata_lookup(
    app: AppHandle,
    state: State<'_, AppState>,
    collection_id: String,
    item_ids: Vec<String>,
) -> CommandResult<MetadataLookupTaskStatus> {
    let tracks = {
        let db = state
            .db
            .lock()
            .map_err(|_| AppError::StatePoisoned("db").to_string())?;
        collections::local_tracks_for_items(&db, collection_id.trim(), &item_ids)
            .map_err(|error| error.to_string())?
    };
    if tracks.is_empty() {
        return Err("the metadata lookup selection has no local tracks".to_owned());
    }
    state
        .album_art
        .start_metadata_lookup(tracks, move |status| {
            let _ = app.emit(METADATA_LOOKUP_PROGRESS_EVENT, status);
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resume_local_metadata_lookup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<MetadataLookupTaskStatus> {
    state
        .album_art
        .resume_metadata_lookup(move |status| {
            let _ = app.emit(METADATA_LOOKUP_PROGRESS_EVENT, status);
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn pause_local_metadata_lookup(
    state: State<'_, AppState>,
) -> CommandResult<MetadataLookupTaskStatus> {
    state
        .album_art
        .pause_metadata_lookup()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_local_library_playback_queue(
    state: State<'_, AppState>,
    snapshot_id: String,
    start_index: usize,
    selection: Option<LibrarySelectionRequest>,
) -> CommandResult<LibraryPlaybackQueue> {
    let mut library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    library
        .create_playback_queue(snapshot_id.trim(), start_index, selection.as_ref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn local_library_queue_track(
    state: State<'_, AppState>,
    queue_id: String,
    index: usize,
) -> CommandResult<LibraryQueueTrack> {
    let library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    library
        .queue_track(queue_id.trim(), index)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn increment_local_track_play_count(
    state: State<'_, AppState>,
    track_id: i64,
) -> CommandResult<i64> {
    let play_count = {
        let db = state
            .db
            .lock()
            .map_err(|_| AppError::StatePoisoned("db").to_string())?;
        db.execute(
            "UPDATE local_tracks SET play_count = play_count + 1 WHERE id = ?1",
            params![track_id],
        )
        .map_err(|error| error.to_string())?;
        db.query_row(
            "SELECT play_count FROM local_tracks WHERE id = ?1",
            params![track_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| AppError::TrackNotFound(track_id).to_string())?
    };

    let mut library = state
        .library
        .lock()
        .map_err(|_| AppError::StatePoisoned("library").to_string())?;
    library.update_play_count(track_id, play_count);
    Ok(play_count)
}

fn run_library_scan(app: AppHandle, folder: PathBuf, force_reindex: bool) {
    let result = (|| {
        let state = app.state::<AppState>();
        let _sync = state
            .library_sync
            .lock()
            .map_err(|_| AppError::StatePoisoned("library_sync"))?;
        scan_folder(&app, &state, &folder, force_reindex)
    })();

    if let Err(error) = result {
        let state = app.state::<AppState>();
        update_scan_status(
            &app,
            &state,
            Some(format!("Indexing failed: {error}")),
            |status| {
                status.is_running = false;
                status.finished_at = Some(now_timestamp());
                status.error_count += 1;
                status.last_error = Some(error.to_string());
            },
        );
    }
}

fn scan_folder(
    app: &AppHandle,
    state: &AppState,
    folder: &Path,
    force_reindex: bool,
) -> AppResult<()> {
    let mut last_progress_emit = Instant::now();
    let mut report_progress = |progress: LibraryReconcileProgress| {
        let should_emit = progress.scanned_files == 0
            || progress.scanned_files == progress.discovered_files
            || progress.scanned_files.is_multiple_of(32)
            || last_progress_emit.elapsed() >= Duration::from_millis(100);
        if !should_emit {
            return;
        }
        last_progress_emit = Instant::now();
        let message = (progress.scanned_files == 0).then(|| {
            format!(
                "Discovered {} supported audio files.",
                progress.discovered_files
            )
        });
        update_scan_status(app, state, message, |status| {
            status.discovered_files = progress.discovered_files;
            status.scanned_files = progress.scanned_files;
            status.indexed_tracks = progress.indexed_tracks;
            status.skipped_files = progress.skipped_files;
        });
    };

    let result = reconcile_library_paths(
        state,
        &[folder.to_path_buf()],
        force_reindex,
        true,
        Some(&mut report_progress),
    )?;

    if result.added_or_updated > 0 || result.removed > 0 {
        emit_library_changed(app, result.added_or_updated, result.removed);
    }

    let last_error = result.errors.last().cloned();
    let message = format!(
        "Finished indexing local tracks: {} added or updated, {} removed.",
        result.added_or_updated, result.removed
    );

    update_scan_status(app, state, Some(message), |status| {
        status.is_running = false;
        status.finished_at = Some(now_timestamp());
        status.error_count = result.errors.len();
        status.last_error = last_error;
    });

    Ok(())
}

fn reconcile_library_paths(
    state: &AppState,
    paths: &[PathBuf],
    force_reindex: bool,
    prune_outside_scopes: bool,
    mut progress: Option<&mut dyn FnMut(LibraryReconcileProgress)>,
) -> AppResult<LibraryReconcileResult> {
    let roots = minimal_reconcile_paths(paths);
    let mut scopes = Vec::with_capacity(roots.len());
    let mut candidates = HashSet::new();
    let mut current_paths = HashSet::new();
    let mut errors = Vec::new();

    for root in roots {
        if root.is_dir() {
            let discovery = collect_supported_audio_files(&root);
            let complete = discovery.errors.is_empty();
            for error in discovery.errors {
                errors.push(format!("Failed to inspect {}: {error}", root.display()));
            }
            for path in discovery.files {
                current_paths.insert(path_to_string(&path));
                candidates.insert(path);
            }
            scopes.push(LibraryReconcileScope { root, complete });
        } else if root.is_file() && is_supported_audio_file(&root) {
            let file_path = path_to_string(&root);
            current_paths.insert(file_path);
            candidates.insert(root.clone());
            scopes.push(LibraryReconcileScope {
                root,
                complete: true,
            });
        } else {
            scopes.push(LibraryReconcileScope {
                root,
                complete: true,
            });
        }
    }

    let existing = {
        let db = state.db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
        if prune_outside_scopes {
            local_track_signatures(&db)?
        } else {
            local_track_signatures_for_scopes(&db, &scopes)?
        }
    };
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_unstable();
    let mut drafts = Vec::new();
    let mut reconcile_progress = LibraryReconcileProgress {
        discovered_files: candidates.len(),
        ..LibraryReconcileProgress::default()
    };
    if let Some(report) = progress.as_deref_mut() {
        report(reconcile_progress);
    }

    for path in candidates {
        let file_path = path_to_string(&path);
        let signature = match local_file_signature(&path) {
            Ok(signature) => signature,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current_paths.remove(&file_path);
                reconcile_progress.scanned_files += 1;
                reconcile_progress.skipped_files += 1;
                if let Some(report) = progress.as_deref_mut() {
                    report(reconcile_progress);
                }
                continue;
            }
            Err(error) => {
                errors.push(format!("Failed to inspect {}: {error}", path.display()));
                reconcile_progress.scanned_files += 1;
                reconcile_progress.skipped_files += 1;
                if let Some(report) = progress.as_deref_mut() {
                    report(reconcile_progress);
                }
                continue;
            }
        };

        if !force_reindex && existing.get(&file_path) == Some(&signature) {
            reconcile_progress.scanned_files += 1;
            reconcile_progress.indexed_tracks += 1;
            if let Some(report) = progress.as_deref_mut() {
                report(reconcile_progress);
            }
            continue;
        }

        match extract_local_track(&path) {
            Ok(draft) => {
                drafts.push(draft);
                reconcile_progress.indexed_tracks += 1;
            }
            Err(error) => {
                errors.push(format!("Skipped {}: {error}", path.display()));
                reconcile_progress.skipped_files += 1;
            }
        }
        reconcile_progress.scanned_files += 1;
        if let Some(report) = progress.as_deref_mut() {
            report(reconcile_progress);
        }
    }

    let all_scopes_complete = scopes.iter().all(|scope| scope.complete);
    let stale_paths = existing
        .keys()
        .filter(|file_path| {
            let path = Path::new(file_path);
            let should_reconcile = (prune_outside_scopes && all_scopes_complete)
                || scopes
                    .iter()
                    .any(|scope| scope.complete && path.starts_with(&scope.root));
            should_reconcile && !current_paths.contains(file_path.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();

    let added_or_updated = drafts.len();
    let mut removed = 0;
    if added_or_updated > 0 || !stale_paths.is_empty() {
        let mut db = state.db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
        let transaction = db.transaction()?;
        let mut upserted_tracks = Vec::with_capacity(drafts.len());
        for draft in &drafts {
            upserted_tracks.push(upsert_local_track(&transaction, draft)?);
        }
        let removed_paths = stale_paths.into_iter().collect::<HashSet<_>>();
        {
            let mut delete =
                transaction.prepare("DELETE FROM local_tracks WHERE file_path = ?1")?;
            for file_path in &removed_paths {
                removed += delete.execute([file_path])?;
            }
        }
        let needs_reindex = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM local_tracks WHERE metadata_version < ?1 LIMIT 1
            )",
            [LIBRARY_METADATA_VERSION],
            |row| row.get(0),
        )?;
        transaction.commit()?;

        let mut library = state
            .library
            .lock()
            .map_err(|_| AppError::StatePoisoned("library"))?;
        library.apply_changes(upserted_tracks, &removed_paths, needs_reindex);
    }

    Ok(LibraryReconcileResult {
        added_or_updated,
        removed,
        errors,
    })
}

fn minimal_reconcile_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = paths.to_vec();
    paths.sort_unstable_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });
    paths.dedup();

    let mut roots = Vec::<PathBuf>::new();
    for path in paths {
        if roots.iter().any(|root| path.starts_with(root)) {
            continue;
        }
        roots.push(path);
    }
    roots
}

fn local_file_signature(path: &Path) -> std::io::Result<LocalTrackSignature> {
    let metadata = fs::metadata(path)?;
    Ok(LocalTrackSignature {
        file_size_bytes: u64_to_i64(metadata.len()),
        modified_at: metadata.modified().ok().and_then(system_time_to_timestamp),
        metadata_version: LIBRARY_METADATA_VERSION,
    })
}

fn local_track_signatures(
    connection: &Connection,
) -> rusqlite::Result<HashMap<String, LocalTrackSignature>> {
    let mut statement = connection.prepare(
        "SELECT file_path, file_size_bytes, modified_at, metadata_version FROM local_tracks",
    )?;
    let signatures = statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                LocalTrackSignature {
                    file_size_bytes: row.get(1)?,
                    modified_at: row.get(2)?,
                    metadata_version: row.get(3)?,
                },
            ))
        })?
        .collect();
    signatures
}

fn local_track_signatures_for_scopes(
    connection: &Connection,
    scopes: &[LibraryReconcileScope],
) -> rusqlite::Result<HashMap<String, LocalTrackSignature>> {
    let mut exact_statement = connection.prepare(
        "SELECT file_path, file_size_bytes, modified_at, metadata_version
         FROM local_tracks
         WHERE file_path = ?1",
    )?;
    let mut subtree_statement = connection.prepare(
        "SELECT file_path, file_size_bytes, modified_at, metadata_version
         FROM local_tracks
         WHERE file_path = ?1 OR substr(file_path, 1, length(?2)) = ?2",
    )?;
    let mut signatures = HashMap::new();
    for scope in scopes {
        let root = path_to_string(&scope.root);
        if scope.root.is_file() || is_supported_audio_file(&scope.root) {
            let row = exact_statement
                .query_row([&root], |row| {
                    Ok((
                        row.get(0)?,
                        LocalTrackSignature {
                            file_size_bytes: row.get(1)?,
                            modified_at: row.get(2)?,
                            metadata_version: row.get(3)?,
                        },
                    ))
                })
                .optional()?;
            if let Some((file_path, signature)) = row {
                signatures.insert(file_path, signature);
            }
            continue;
        }
        let prefix = if root.ends_with(std::path::MAIN_SEPARATOR) {
            root.clone()
        } else {
            format!("{root}{}", std::path::MAIN_SEPARATOR)
        };
        let rows = subtree_statement.query_map(params![root, prefix], |row| {
            Ok((
                row.get(0)?,
                LocalTrackSignature {
                    file_size_bytes: row.get(1)?,
                    modified_at: row.get(2)?,
                    metadata_version: row.get(3)?,
                },
            ))
        })?;
        for row in rows {
            let (file_path, signature) = row?;
            signatures.insert(file_path, signature);
        }
    }
    Ok(signatures)
}

fn load_library_folder(connection: &Connection) -> AppResult<Option<PathBuf>> {
    connection
        .query_row(
            "SELECT setting_value FROM app_settings WHERE setting_key = ?1",
            [LIBRARY_FOLDER_SETTING_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map(|folder| folder.map(PathBuf::from))
        .map_err(AppError::from)
}

fn save_library_folder(connection: &Connection, folder: &Path) -> AppResult<()> {
    connection.execute(
        "INSERT INTO app_settings (setting_key, setting_value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(setting_key) DO UPDATE SET
             setting_value = excluded.setting_value,
             updated_at = excluded.updated_at",
        params![
            LIBRARY_FOLDER_SETTING_KEY,
            path_to_string(folder),
            now_timestamp()
        ],
    )?;
    Ok(())
}

fn collect_supported_audio_files(folder: &Path) -> DiscoveredAudioFiles {
    let mut discovery = DiscoveredAudioFiles::default();

    for entry in WalkDir::new(folder).follow_links(false) {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() {
                    let path = entry.into_path();
                    if is_supported_audio_file(&path) {
                        discovery.files.push(path);
                    }
                }
            }
            Err(error) => discovery.errors.push(error.to_string()),
        }
    }

    discovery
}

fn is_supported_audio_file(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(".fika-metadata-") || name.starts_with(".fika-download-")
        })
    {
        return false;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3" | "flac" | "m4a" | "aac"
            )
        })
        .unwrap_or(false)
}

fn extract_local_track(path: &Path) -> AppResult<LocalTrackDraft> {
    let metadata = fs::metadata(path)?;
    let file_path = path_to_string(path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| file_path.clone());

    let (tagged_file, codec) = if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("m4a"))
    {
        let mut file = fs::File::open(path)?;
        let mp4_file = Mp4File::read_from(&mut file, ParseOptions::new()).map_err(|source| {
            AppError::Metadata {
                path: file_path.clone(),
                source,
            }
        })?;
        let codec = mp4_codec_label(*mp4_file.properties().codec()).to_owned();
        (mp4_file.into(), Some(codec))
    } else {
        let tagged_file = lofty::read_from_path(path).map_err(|source| AppError::Metadata {
            path: file_path.clone(),
            source,
        })?;
        let codec = codec_label(tagged_file.file_type()).map(str::to_owned);
        (tagged_file, codec)
    };
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let fallback_title = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| file_name.clone());

    let title = tag_string(tag, |tag| tag.title()).unwrap_or(fallback_title);
    let properties = tagged_file.properties();
    let duration_seconds = seconds_to_i64(properties.duration().as_secs());

    Ok(LocalTrackDraft {
        file_path,
        file_name,
        title,
        artist: tag_string(tag, |tag| tag.artist()),
        album: tag_string(tag, |tag| tag.album()),
        album_artist: tag_item_string(tag, ItemKey::AlbumArtist),
        genre: tag_string(tag, |tag| tag.genre()),
        year: tag
            .and_then(|tag| tag.date())
            .map(|date| i64::from(date.year)),
        codec,
        bitrate_kbps: properties.audio_bitrate().map(i64::from),
        sample_rate_hz: properties.sample_rate().map(i64::from),
        duration_seconds,
        track_number: tag.and_then(|tag| tag.track()).map(i64::from),
        disc_number: tag.and_then(|tag| tag.disk()).map(i64::from),
        file_size_bytes: u64_to_i64(metadata.len()),
        modified_at: metadata.modified().ok().and_then(system_time_to_timestamp),
    })
}

fn codec_label(file_type: FileType) -> Option<&'static str> {
    match file_type {
        FileType::Aac => Some("AAC"),
        FileType::Flac => Some("FLAC"),
        FileType::Mpeg => Some("MP3"),
        FileType::Mp4 => Some("MP4 Audio"),
        _ => None,
    }
}

fn mp4_codec_label(codec: Mp4Codec) -> &'static str {
    match codec {
        Mp4Codec::AAC => "AAC",
        Mp4Codec::ALAC => "ALAC",
        Mp4Codec::MP3 => "MP3",
        Mp4Codec::FLAC => "FLAC",
        _ => "MP4 Audio",
    }
}

fn tag_string<'tag>(
    tag: Option<&'tag Tag>,
    getter: impl Fn(&'tag Tag) -> Option<std::borrow::Cow<'tag, str>>,
) -> Option<String> {
    tag.and_then(getter)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn tag_item_string(tag: Option<&Tag>, key: ItemKey) -> Option<String> {
    tag.and_then(|tag| tag.get_string(key))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn upsert_local_track(connection: &Connection, draft: &LocalTrackDraft) -> AppResult<LocalTrack> {
    let indexed_at = now_timestamp();

    connection.execute(
        "
        INSERT INTO local_tracks (
            file_path,
            file_name,
            title,
            artist,
            album,
            album_artist,
            genre,
            year,
            codec,
            bitrate_kbps,
            sample_rate_hz,
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at,
            metadata_version
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
            ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
        )
        ON CONFLICT(file_path) DO UPDATE SET
            file_name = excluded.file_name,
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            album_artist = excluded.album_artist,
            genre = excluded.genre,
            year = excluded.year,
            codec = excluded.codec,
            bitrate_kbps = excluded.bitrate_kbps,
            sample_rate_hz = excluded.sample_rate_hz,
            duration_seconds = excluded.duration_seconds,
            track_number = excluded.track_number,
            disc_number = excluded.disc_number,
            file_size_bytes = excluded.file_size_bytes,
            modified_at = excluded.modified_at,
            indexed_at = excluded.indexed_at,
            metadata_version = excluded.metadata_version
        ",
        params![
            draft.file_path,
            draft.file_name,
            draft.title,
            draft.artist,
            draft.album,
            draft.album_artist,
            draft.genre,
            draft.year,
            draft.codec,
            draft.bitrate_kbps,
            draft.sample_rate_hz,
            draft.duration_seconds,
            draft.track_number,
            draft.disc_number,
            draft.file_size_bytes,
            draft.modified_at,
            indexed_at,
            LIBRARY_METADATA_VERSION,
        ],
    )?;

    track_by_path(connection, &draft.file_path)
}

fn list_tracks(connection: &Connection) -> rusqlite::Result<Vec<LocalTrack>> {
    let mut statement = connection.prepare(
        "
        SELECT
            id,
            file_path,
            file_name,
            title,
            artist,
            album,
            album_artist,
            genre,
            year,
            codec,
            bitrate_kbps,
            sample_rate_hz,
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at,
            play_count
        FROM local_tracks
        ORDER BY artist IS NULL, artist COLLATE NOCASE, album IS NULL, album COLLATE NOCASE, track_number IS NULL, track_number, title COLLATE NOCASE
        ",
    )?;

    let tracks = statement
        .query_map([], local_track_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
}

fn track_by_path(connection: &Connection, file_path: &str) -> AppResult<LocalTrack> {
    connection
        .query_row(
            "
        SELECT
            id,
            file_path,
            file_name,
            title,
            artist,
            album,
            album_artist,
            genre,
            year,
            codec,
            bitrate_kbps,
            sample_rate_hz,
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at,
            play_count
        FROM local_tracks
        WHERE file_path = ?1
        ",
            params![file_path],
            local_track_from_row,
        )
        .map_err(AppError::from)
}

fn track_by_id(connection: &Connection, track_id: i64) -> AppResult<Option<LocalTrack>> {
    connection
        .query_row(
            "
        SELECT
            id,
            file_path,
            file_name,
            title,
            artist,
            album,
            album_artist,
            genre,
            year,
            codec,
            bitrate_kbps,
            sample_rate_hz,
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at,
            play_count
        FROM local_tracks
        WHERE id = ?1
        ",
            params![track_id],
            local_track_from_row,
        )
        .optional()
        .map_err(AppError::from)
}

fn local_track_from_row(row: &Row<'_>) -> rusqlite::Result<LocalTrack> {
    Ok(LocalTrack {
        id: row.get(0)?,
        file_path: row.get(1)?,
        file_name: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        album: row.get(5)?,
        album_artist: row.get(6)?,
        genre: row.get(7)?,
        year: row.get(8)?,
        codec: row.get(9)?,
        bitrate_kbps: row.get(10)?,
        sample_rate_hz: row.get(11)?,
        duration_seconds: row.get(12)?,
        track_number: row.get(13)?,
        disc_number: row.get(14)?,
        file_size_bytes: row.get(15)?,
        modified_at: row.get(16)?,
        indexed_at: row.get(17)?,
        play_count: row.get(18)?,
    })
}

fn initialize_library_watcher(app: &AppHandle) -> AppResult<()> {
    let watcher_app = app.clone();
    let watcher =
        LibraryWatcher::new(move |batch| handle_library_change_batch(&watcher_app, batch))?;
    let configured_folder = {
        let state = app.state::<AppState>();
        let folder = state
            .scan_status
            .lock()
            .map_err(|_| AppError::StatePoisoned("scan_status"))?
            .folder_path
            .as_deref()
            .map(PathBuf::from);
        folder
    };

    if let Some(folder) = configured_folder.as_ref().filter(|folder| folder.is_dir()) {
        watcher.set_folder(folder.clone())?;
    }

    {
        let state = app.state::<AppState>();
        *state
            .library_watcher
            .lock()
            .map_err(|_| AppError::StatePoisoned("library_watcher"))? = Some(watcher);
    }

    if let Some(folder) = configured_folder {
        if folder.is_dir() {
            let state = app.state::<AppState>();
            begin_library_scan(
                app,
                &state,
                folder,
                false,
                "Checking the local music folder for changes.",
            )?;
        } else {
            let state = app.state::<AppState>();
            let message = format!(
                "The configured music folder is unavailable: {}",
                folder.display()
            );
            if let Ok(mut status) = state.scan_status.lock() {
                status.error_count = 1;
                status.last_error = Some(message);
            };
        }
    }

    Ok(())
}

fn handle_library_change_batch(app: &AppHandle, batch: &LibraryChangeBatch) -> BatchDisposition {
    let state = app.state::<AppState>();
    let active_folder_matches = state
        .scan_status
        .lock()
        .map(|status| {
            !status.is_running
                && status.folder_path.as_deref() == Some(path_to_string(&batch.folder).as_str())
        })
        .unwrap_or(false);
    if !active_folder_matches {
        let scan_is_running = state
            .scan_status
            .lock()
            .map(|status| status.is_running)
            .unwrap_or(false);
        return if scan_is_running {
            BatchDisposition::Retry
        } else {
            BatchDisposition::Complete
        };
    }

    let _sync = match state.library_sync.try_lock() {
        Ok(sync) => sync,
        Err(std::sync::TryLockError::WouldBlock) => return BatchDisposition::Retry,
        Err(std::sync::TryLockError::Poisoned(_)) => {
            update_scan_status(
                app,
                &state,
                Some("Automatic indexing failed: library sync lock was poisoned.".to_owned()),
                |status| {
                    status.error_count = 1;
                    status.last_error = Some("library sync lock was poisoned".to_owned());
                },
            );
            return BatchDisposition::Complete;
        }
    };

    let roots = if batch.force_full_rescan || batch.paths.is_empty() {
        vec![batch.folder.clone()]
    } else {
        batch.paths.clone()
    };
    match reconcile_library_paths(&state, &roots, false, batch.force_full_rescan, None) {
        Ok(mut result) => {
            result.errors.extend(batch.errors.iter().cloned());
            if result.added_or_updated > 0 || result.removed > 0 {
                emit_library_changed(app, result.added_or_updated, result.removed);
            }
            let error_count = result.errors.len();
            let last_error = result.errors.last().cloned();
            let message = (error_count > 0)
                .then(|| format!("Automatic indexing skipped {error_count} file system changes."));
            update_scan_status(app, &state, message, |status| {
                status.error_count = error_count;
                status.last_error = last_error;
                status.finished_at = Some(now_timestamp());
            });
        }
        Err(error) => {
            let message = format!("Automatic indexing failed: {error}");
            update_scan_status(app, &state, Some(message.clone()), |status| {
                status.error_count = 1;
                status.last_error = Some(message);
                status.finished_at = Some(now_timestamp());
            });
        }
    }

    BatchDisposition::Complete
}

fn update_scan_status(
    app: &AppHandle,
    state: &AppState,
    message: Option<String>,
    update: impl FnOnce(&mut ScanStatus),
) {
    let Ok(mut status) = state.scan_status.lock() else {
        return;
    };

    update(&mut status);
    let event = ScanProgressEvent {
        status: status.clone(),
        message,
    };
    drop(status);

    let _ = app.emit(SCAN_PROGRESS_EVENT, event);
}

fn emit_scan_status(app: &AppHandle, status: ScanStatus, message: Option<String>) {
    let event = ScanProgressEvent { status, message };
    let _ = app.emit(SCAN_PROGRESS_EVENT, event);
}

fn emit_library_changed(app: &AppHandle, added_or_updated: usize, removed: usize) {
    let _ = app.emit(
        LIBRARY_CHANGED_EVENT,
        LibraryChangedEvent {
            added_or_updated,
            removed,
        },
    );
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn seconds_to_i64(seconds: u64) -> Option<i64> {
    if seconds == 0 {
        return None;
    }

    i64::try_from(seconds).ok()
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn system_time_to_timestamp(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

fn now_timestamp() -> i64 {
    system_time_to_timestamp(SystemTime::now()).unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol(
            youtube_media_proxy::YOUTUBE_MEDIA_PROTOCOL,
            |_context, request, responder| {
                let _task = tauri::async_runtime::spawn_blocking(move || {
                    responder.respond(youtube_media_proxy::protocol_response(request));
                });
            },
        )
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let db_path = app_data_dir.join("fika-library.sqlite3");
            let resource_plugins_dir = app.path().resource_dir()?.join("plugins");
            let source_plugins_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins");
            let bundled_plugins_dir = if resource_plugins_dir.is_dir() {
                resource_plugins_dir
            } else {
                source_plugins_dir
            };
            let state = AppState::new_with_plugin_dirs(
                &db_path,
                app_data_dir.join("plugins"),
                bundled_plugins_dir,
            )?;
            app.manage(state);
            initialize_library_watcher(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(with_tauri_commands!(generate_command_handler))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn availability_request_should_include_netease_track_aliases() {
        let request = audio_source_availability_request(
            source_runtime::LX_SOURCE_WY,
            source_runtime::SourceQuality::K128,
        );
        let source_runtime::SourceRequest::MusicUrl { music_info, .. } = request else {
            panic!("availability request should resolve music URLs");
        };

        assert_eq!(
            music_info,
            serde_json::json!({
                "id": "347230",
                "songId": "347230",
                "title": "Test Track",
                "name": "Test Track",
                "artist": "Test Artist",
                "singer": "Test Artist"
            })
        );
    }

    #[test]
    fn availability_request_should_include_kugou_hash() {
        let request = audio_source_availability_request(
            source_runtime::LX_SOURCE_KG,
            source_runtime::SourceQuality::K320,
        );
        let source_runtime::SourceRequest::MusicUrl { music_info, .. } = request else {
            panic!("availability request should resolve music URLs");
        };

        assert_eq!(
            music_info.get("hash"),
            Some(&serde_json::json!("4D766DEC7A90A011D730ED939D158131"))
        );
    }

    #[test]
    fn failed_primary_bypasses_the_download_hedge_delay() {
        let primary_unavailable = AtomicBool::new(true);
        let cancellation = source_runtime::SourceCancellationToken::default();
        let started_at = Instant::now();

        assert!(wait_for_download_hedge(
            &primary_unavailable,
            &cancellation,
            Duration::from_secs(10),
            Instant::now() + Duration::from_secs(11),
        ));
        assert!(started_at.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn cancelled_download_race_does_not_start_the_hedged_source() {
        let primary_unavailable = AtomicBool::new(false);
        let cancellation = source_runtime::SourceCancellationToken::default();
        cancellation.cancel();

        assert!(!wait_for_download_hedge(
            &primary_unavailable,
            &cancellation,
            Duration::from_secs(1),
            Instant::now() + Duration::from_secs(2),
        ));
    }

    #[test]
    fn failed_download_start_should_release_the_active_registration() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let state = AppState::new(&directory.path().join("library.sqlite3"))
            .expect("test app state should initialize");

        for _ in 0..2 {
            let error = match prepare_online_download_task_start(&state, "missing-task") {
                Ok(_) => panic!("missing download task should not start"),
                Err(error) => error,
            };
            assert!(error.contains("download task missing-task was not found"));
            assert!(!state
                .online_download_requests
                .lock()
                .expect("download request registry should not be poisoned")
                .contains_key("missing-task"));
        }
    }

    #[test]
    fn yt_dlp_download_source_uses_the_remaining_resolution_budget() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(20);
        let qualities = [source_runtime::SourceQuality::K128];
        let policy = DownloadResolutionPolicy {
            qualities: &qualities,
            selection_mode: online_music::AudioSourceSelectionMode::Manual,
            layer_timeout: Duration::from_secs(8),
            deadline,
        };

        assert_eq!(
            download_source_layer_deadline(
                youtube_music_playback::YOUTUBE_MUSIC_AUDIO_SOURCE_ID,
                now,
                policy,
            ),
            deadline
        );
        assert_eq!(
            download_source_layer_deadline("ordinary-source", now, policy),
            now + Duration::from_secs(8)
        );
    }

    #[test]
    fn media_extension_should_not_guess_format_for_generic_response() {
        assert_eq!(
            media_extension(
                Some("application/octet-stream"),
                "https://media.example/download",
            ),
            None
        );
    }

    #[test]
    fn downloaded_audio_extension_should_use_content_instead_of_temporary_suffix() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let path = directory.path().join("response.flac");
        let mut mp3_frame = vec![0xff, 0xfb, 0x90, 0x64];
        mp3_frame.resize(417, 0);
        mp3_frame.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
        mp3_frame.resize(834, 0);
        fs::write(&path, mp3_frame).expect("test MP3 frame should write");

        assert_eq!(
            online_download::downloaded_audio_extension(&path)
                .expect("MP3 content should be detected"),
            "mp3"
        );
    }

    #[test]
    fn downloaded_audio_extension_should_reject_non_audio_response() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let path = directory.path().join("response.flac");
        fs::write(&path, br#"{"code":403,"message":"forbidden"}"#)
            .expect("test response should write");

        let error = online_download::downloaded_audio_extension(&path)
            .expect_err("non-audio response should be rejected");

        assert!(error
            .to_string()
            .contains("downloaded audio metadata is unreadable"));
    }

    fn draft(file_path: &str, title: &str, artist: Option<&str>) -> LocalTrackDraft {
        LocalTrackDraft {
            file_path: file_path.to_owned(),
            file_name: Path::new(file_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(file_path)
                .to_owned(),
            title: title.to_owned(),
            artist: artist.map(str::to_owned),
            album: Some("Test Album".to_owned()),
            album_artist: artist.map(str::to_owned),
            genre: Some("Test Genre".to_owned()),
            year: Some(2024),
            codec: Some("MP3".to_owned()),
            bitrate_kbps: Some(320),
            sample_rate_hz: Some(44_100),
            duration_seconds: Some(180),
            track_number: Some(1),
            disc_number: Some(1),
            file_size_bytes: 1024,
            modified_at: Some(1_700_000_000),
        }
    }

    fn initialized_connection() -> Connection {
        let mut connection = Connection::open_in_memory().expect("in-memory database should open");
        database::initialize(&mut connection).expect("schema should initialize");
        connection
    }

    fn temp_dir(name: &str) -> PathBuf {
        let id = NEXT_TEST_DIR_ID.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fika-music-{name}-{}-{id}", std::process::id()));
        fs::create_dir_all(&dir).expect("test temp directory should be created");
        dir
    }

    #[test]
    fn recommendation_channel_support_should_match_each_home_entry_provider() {
        let netease = candidate_channel(
            "netease",
            netease::NETEASE_PLUGIN_ID,
            source_runtime::LX_SOURCE_WY,
            "NetEase",
            source_runtime::SourceAction::MusicRecommendations,
        );
        let kugou = candidate_channel(
            "kugou",
            kugou::KUGOU_PLUGIN_ID,
            source_runtime::LX_SOURCE_KG,
            "KuGou",
            source_runtime::SourceAction::MusicRecommendations,
        );
        let actual = [
            recommendation_channel_supports(
                &netease,
                source_runtime::MusicRecommendationKind::Daily,
            ),
            recommendation_channel_supports(&kugou, source_runtime::MusicRecommendationKind::Daily),
            recommendation_channel_supports(
                &netease,
                source_runtime::MusicRecommendationKind::Roaming,
            ),
            recommendation_channel_supports(
                &kugou,
                source_runtime::MusicRecommendationKind::Roaming,
            ),
            recommendation_channel_supports(
                &netease,
                source_runtime::MusicRecommendationKind::Radar,
            ),
            recommendation_channel_supports(&kugou, source_runtime::MusicRecommendationKind::Radar),
        ];

        assert_eq!(actual, [true, true, true, false, true, false]);
    }

    #[test]
    fn private_roaming_recommendations_should_not_use_response_cache() {
        let request = source_runtime::SourceRequest::MusicRecommendations {
            source: source_runtime::LX_SOURCE_WY.to_owned(),
            account_ref: "account".to_owned(),
            kind: source_runtime::MusicRecommendationKind::Roaming,
            limit: 3,
        };

        assert!(!should_cache_online_request(&request));
    }

    #[test]
    fn account_playlist_lists_should_not_use_response_cache() {
        let request = source_runtime::SourceRequest::PlaylistList {
            source: source_runtime::LX_SOURCE_WY.to_owned(),
            account_ref: "account".to_owned(),
        };

        assert!(!should_cache_online_request(&request));
    }

    #[test]
    fn account_playlist_track_page_should_paginate_and_attach_account_context() {
        let tracks = (1..=3)
            .map(|index| source_runtime::SourceSearchResult {
                id: index.to_string(),
                source: source_runtime::LX_SOURCE_KG.to_owned(),
                title: format!("Track {index}"),
                artist: "Artist".to_owned(),
                album: None,
                duration_seconds: None,
                cover_url: None,
                track_number: None,
                disc_number: None,
                platform_ids: BTreeMap::new(),
                raw_info: JsonValue::Object(Default::default()),
            })
            .collect();

        let (page, has_more, total) = account_playlist_track_page(tracks, "kugou-account:1", 2, 2);

        assert_eq!(
            (
                page.iter()
                    .map(|track| track.id.as_str())
                    .collect::<Vec<_>>(),
                page[0].platform_ids.get("accountRef"),
                has_more,
                total,
            ),
            (
                vec!["3"],
                Some(&source_runtime::JsonScalar::String(
                    "kugou-account:1".to_owned()
                )),
                false,
                Some(3),
            )
        );
    }

    #[test]
    fn stable_recommendations_should_use_response_cache() {
        let requests = [
            source_runtime::MusicRecommendationKind::Daily,
            source_runtime::MusicRecommendationKind::Radar,
        ]
        .map(|kind| source_runtime::SourceRequest::MusicRecommendations {
            source: source_runtime::LX_SOURCE_WY.to_owned(),
            account_ref: "account".to_owned(),
            kind,
            limit: 50,
        });

        assert!(requests.iter().all(should_cache_online_request));
    }

    #[test]
    fn recommendation_request_limit_should_match_feed_semantics() {
        let limits = [
            source_runtime::MusicRecommendationKind::Daily,
            source_runtime::MusicRecommendationKind::Roaming,
            source_runtime::MusicRecommendationKind::Radar,
        ]
        .map(recommendation_request_limit);

        assert_eq!(limits, [50, 3, 50]);
    }

    #[test]
    fn is_supported_audio_file_should_accept_slice_formats_case_insensitively() {
        let actual = ["track.mp3", "track.FLAC", "track.m4a", "track.AAC"]
            .map(|path| is_supported_audio_file(Path::new(path)));

        assert_eq!(actual, [true, true, true, true]);
    }

    #[test]
    fn is_supported_audio_file_should_reject_unsupported_or_missing_extensions() {
        let actual = ["cover.jpg", "notes.txt", "track", ".fika-metadata-abc.flac"]
            .map(|path| is_supported_audio_file(Path::new(path)));

        assert_eq!(actual, [false, false, false, false]);
    }

    #[test]
    fn collect_supported_audio_files_should_return_supported_files() {
        let root = temp_dir("supported-files");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("nested test directory should be created");
        fs::write(root.join("song.MP3"), []).expect("test audio file should be written");
        fs::write(nested.join("tune.flac"), []).expect("test audio file should be written");
        fs::write(root.join("cover.jpg"), []).expect("test non-audio file should be written");

        let discovery = collect_supported_audio_files(&root);
        let mut file_names = discovery
            .files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        file_names.sort();

        fs::remove_dir_all(root).expect("test temp directory should be removed");

        assert_eq!(
            (file_names, discovery.errors),
            (vec!["song.MP3".to_owned(), "tune.flac".to_owned()], vec![])
        );
    }

    #[test]
    fn collect_supported_audio_files_should_report_root_walk_errors() {
        let root = temp_dir("missing-root");
        fs::remove_dir_all(&root).expect("test temp directory should be removed before scan");

        let discovery = collect_supported_audio_files(&root);

        assert_eq!((discovery.files.len(), discovery.errors.len()), (0, 1));
    }

    #[test]
    fn upsert_local_track_should_insert_new_track() {
        let connection = initialized_connection();
        upsert_local_track(
            &connection,
            &draft("/library/alpha.mp3", "Alpha", Some("Artist A")),
        )
        .expect("track should insert");
        let tracks = list_tracks(&connection).expect("inserted track should load");
        let track = &tracks[0];

        assert_eq!(
            (
                track.file_path.as_str(),
                track.title.as_str(),
                track.artist.as_deref()
            ),
            ("/library/alpha.mp3", "Alpha", Some("Artist A"))
        );
    }

    #[test]
    fn upsert_local_track_should_update_existing_track_by_path() {
        let connection = initialized_connection();
        upsert_local_track(
            &connection,
            &draft("/library/alpha.mp3", "Alpha", Some("Artist A")),
        )
        .expect("track should insert");
        upsert_local_track(
            &connection,
            &draft("/library/alpha.mp3", "Alpha Revised", Some("Artist B")),
        )
        .expect("track should update");

        let tracks = list_tracks(&connection).expect("tracks should list");

        assert_eq!(
            (
                tracks.len(),
                tracks[0].title.as_str(),
                tracks[0].artist.as_deref()
            ),
            (1, "Alpha Revised", Some("Artist B"))
        );
    }

    #[test]
    fn library_folder_setting_should_persist() {
        let connection = initialized_connection();

        save_library_folder(&connection, Path::new("/music/library"))
            .expect("library folder should save");
        let folder = load_library_folder(&connection).expect("library folder should load");

        assert_eq!(folder.as_deref(), Some(Path::new("/music/library")));
    }

    #[test]
    fn minimal_reconcile_paths_should_drop_descendants() {
        let roots = minimal_reconcile_paths(&[
            PathBuf::from("/music/artist/album/song.mp3"),
            PathBuf::from("/music/artist"),
            PathBuf::from("/music/artist/album"),
        ]);

        assert_eq!(roots, vec![PathBuf::from("/music/artist")]);
    }

    #[test]
    fn reconcile_library_paths_should_remove_deleted_tracks() {
        let root = temp_dir("reconcile-deleted");
        let state =
            AppState::new(&root.join("library.sqlite3")).expect("test app state should initialize");
        let missing_track = root.join("deleted.mp3");
        {
            let db = state.db.lock().expect("database should not be poisoned");
            upsert_local_track(
                &db,
                &draft(&path_to_string(&missing_track), "Deleted", Some("Artist")),
            )
            .expect("track should insert");
        }

        let result =
            reconcile_library_paths(&state, std::slice::from_ref(&root), false, false, None)
                .expect("folder should reconcile");

        assert_eq!((result.removed, result.added_or_updated), (1, 0));
        fs::remove_dir_all(root).expect("test temp directory should be removed");
    }

    #[test]
    fn reconcile_library_paths_should_skip_unchanged_audio_metadata() {
        let root = temp_dir("reconcile-unchanged");
        let state =
            AppState::new(&root.join("library.sqlite3")).expect("test app state should initialize");
        let track_path = root.join("unchanged.mp3");
        fs::write(&track_path, b"not real audio").expect("test audio file should be written");
        let signature = local_file_signature(&track_path).expect("file signature should load");
        let mut unchanged = draft(&path_to_string(&track_path), "Unchanged", Some("Artist"));
        unchanged.file_size_bytes = signature.file_size_bytes;
        unchanged.modified_at = signature.modified_at;
        {
            let db = state.db.lock().expect("database should not be poisoned");
            upsert_local_track(&db, &unchanged).expect("track should insert");
        }

        let result =
            reconcile_library_paths(&state, std::slice::from_ref(&root), false, false, None)
                .expect("folder should reconcile");

        assert_eq!((result.added_or_updated, result.errors.len()), (0, 0));
        fs::remove_dir_all(root).expect("test temp directory should be removed");
    }

    #[test]
    fn remote_request_helper_should_allow_in_flight_cancellation_and_cleanup() {
        let root = temp_dir("remote-request");
        let state = Arc::new(
            AppState::new(&root.join("library.sqlite3")).expect("test app state should initialize"),
        );
        let started = Arc::new(AtomicBool::new(false));
        let task_state = Arc::clone(&state);
        let task_started = Arc::clone(&started);
        let handle = thread::spawn(move || {
            tauri::async_runtime::block_on(run_remote_request::<(), _>(
                task_state.as_ref(),
                Some("request-1"),
                move |cancellation| {
                    task_started.store(true, Ordering::Release);
                    while !cancellation.is_cancelled() {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(remote_error("request cancelled in test"))
                },
                "remote task failed",
            ))
        });

        for _ in 0..1_000 {
            if started.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(started.load(Ordering::Acquire));

        state
            .source_requests
            .cancel("request-1")
            .expect("in-flight request should cancel");

        let result = handle.join().expect("remote request task should not panic");
        assert!(result.is_err());
        assert!(state.source_requests.is_empty());
        fs::remove_dir_all(root).expect("test temp directory should be removed");
    }

    #[test]
    fn plugin_request_helper_should_dispatch_through_the_enabled_registry_provider() {
        let root = temp_dir("plugin-request");
        let bundled_package = root.join("bundled-plugins/runtime-demo");
        fs::create_dir_all(&bundled_package).expect("bundled Plugin directory should be created");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/plugins/runtime-demo/plugin.json"),
            bundled_package.join("plugin.json"),
        )
        .expect("bundled Plugin manifest should be copied");
        let state =
            AppState::new(&root.join("library.sqlite3")).expect("test app state should initialize");
        {
            let db = state.db.lock().expect("database should not be poisoned");
            let mut registry = state
                .plugin_registry
                .lock()
                .expect("registry should not be poisoned");
            registry
                .set_capabilities(
                    &db,
                    "fika.runtime-demo",
                    [source_runtime::SourceCapability::NetworkAny],
                    true,
                )
                .expect("Plugin capabilities should be reviewed");
            registry
                .set_enabled(&db, "fika.runtime-demo", true)
                .expect("Plugin should enable");
        }

        let outcome = dispatch_plugin_request_inner(
            &state,
            "fika.runtime-demo",
            source_runtime::SourceRequest::MusicSearch {
                source: source_runtime::LX_SOURCE_WY.to_owned(),
                keyword: "integration".to_owned(),
                page: 1,
                page_size: 10,
            },
            source_runtime::SourceCancellationToken::default(),
        )
        .expect("Plugin request should dispatch");
        let source_runtime::SourceResponse::MusicSearch(response) = outcome.response else {
            panic!("Plugin should return music search results");
        };

        assert_eq!(response.list[0].title, "Demo result for integration");
        fs::remove_dir_all(root).expect("test temp directory should be removed");
    }

    #[test]
    fn plugin_request_execution_should_not_hold_app_state_locks() {
        let root = temp_dir("plugin-request-locks");
        let bundled_package = root.join("bundled-plugins/runtime-demo");
        fs::create_dir_all(&bundled_package).expect("bundled Plugin directory should be created");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/plugins/runtime-demo/plugin.json"),
            bundled_package.join("plugin.json"),
        )
        .expect("bundled Plugin manifest should be copied");
        let state =
            AppState::new(&root.join("library.sqlite3")).expect("test app state should initialize");
        {
            let db = state.db.lock().expect("database should not be poisoned");
            let mut registry = state
                .plugin_registry
                .lock()
                .expect("registry should not be poisoned");
            registry
                .set_capabilities(
                    &db,
                    "fika.runtime-demo",
                    [source_runtime::SourceCapability::NetworkAny],
                    true,
                )
                .expect("Plugin capabilities should be reviewed");
            registry
                .set_enabled(&db, "fika.runtime-demo", true)
                .expect("Plugin should enable");
        }

        let outcome = dispatch_plugin_request_with(
            &state,
            "fika.runtime-demo",
            source_runtime::SourceRequest::MusicSearch {
                source: source_runtime::LX_SOURCE_WY.to_owned(),
                keyword: "unlocked".to_owned(),
                page: 1,
                page_size: 10,
            },
            source_runtime::SourceCancellationToken::default(),
            |dispatch, request, cancellation| {
                assert!(state.db.try_lock().is_ok());
                assert!(state.plugin_registry.try_lock().is_ok());
                dispatch.execute(request, cancellation)
            },
        )
        .expect("Plugin request should dispatch without AppState locks");

        assert!(matches!(
            outcome.response,
            source_runtime::SourceResponse::MusicSearch(_)
        ));
        fs::remove_dir_all(root).expect("test temp directory should be removed");
    }

    #[test]
    fn plugin_request_should_return_provider_result_when_diagnostic_persistence_fails() {
        let root = temp_dir("plugin-request-diagnostic-failure");
        let bundled_package = root.join("bundled-plugins/runtime-demo");
        fs::create_dir_all(&bundled_package).expect("bundled Plugin directory should be created");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/plugins/runtime-demo/plugin.json"),
            bundled_package.join("plugin.json"),
        )
        .expect("bundled Plugin manifest should be copied");
        let state =
            AppState::new(&root.join("library.sqlite3")).expect("test app state should initialize");
        {
            let db = state.db.lock().expect("database should not be poisoned");
            let mut registry = state
                .plugin_registry
                .lock()
                .expect("registry should not be poisoned");
            registry
                .set_capabilities(
                    &db,
                    "fika.runtime-demo",
                    [source_runtime::SourceCapability::NetworkAny],
                    true,
                )
                .expect("Plugin capabilities should be reviewed");
            registry
                .set_enabled(&db, "fika.runtime-demo", true)
                .expect("Plugin should enable");
            db.execute_batch(
                "CREATE TRIGGER fail_dispatch_diagnostic_insert
                 BEFORE INSERT ON plugin_diagnostics
                 BEGIN
                     SELECT RAISE(ABORT, 'forced diagnostic persistence failure');
                 END;",
            )
            .expect("diagnostic failure trigger should be created");
        }

        let outcome = dispatch_plugin_request_inner(
            &state,
            "fika.runtime-demo",
            source_runtime::SourceRequest::MusicSearch {
                source: source_runtime::LX_SOURCE_WY.to_owned(),
                keyword: "diagnostics".to_owned(),
                page: 1,
                page_size: 10,
            },
            source_runtime::SourceCancellationToken::default(),
        )
        .expect("diagnostic persistence must not replace the provider result");

        assert!(matches!(
            outcome.response,
            source_runtime::SourceResponse::MusicSearch(_)
        ));
        let diagnostics = state
            .plugin_registry
            .lock()
            .expect("registry should not be poisoned")
            .record("fika.runtime-demo")
            .expect("Plugin should remain registered")
            .diagnostics;
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "diagnostic-persistence"));
        fs::remove_dir_all(root).expect("test temp directory should be removed");
    }
}
