use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, Tag};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use walkdir::WalkDir;

const SCAN_PROGRESS_EVENT: &str = "library:scan-progress";

type AppResult<T> = Result<T, AppError>;
type CommandResult<T> = Result<T, String>;

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
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
    #[error("music folder does not exist or is not a directory: {0}")]
    InvalidMusicFolder(String),
    #[error("a library scan is already running")]
    ScanAlreadyRunning,
    #[error("local track was not found: {0}")]
    TrackNotFound(i64),
    #[error("local track file is missing: {0}")]
    TrackFileMissing(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalTrack {
    id: i64,
    file_path: String,
    file_name: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_seconds: Option<i64>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    file_size_bytes: i64,
    modified_at: Option<i64>,
    indexed_at: i64,
}

#[derive(Debug)]
struct LocalTrackDraft {
    file_path: String,
    file_name: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_seconds: Option<i64>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    file_size_bytes: i64,
    modified_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaSource {
    file_path: String,
    mime_type: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanStatus {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgressEvent {
    status: ScanStatus,
    message: Option<String>,
}

struct AppState {
    db: Mutex<Connection>,
    scan_status: Mutex<ScanStatus>,
}

#[derive(Debug, Default)]
struct DiscoveredAudioFiles {
    files: Vec<PathBuf>,
    errors: Vec<String>,
}

impl AppState {
    fn new(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(db_path)?;
        initialize_database(&connection)?;

        Ok(Self {
            db: Mutex::new(connection),
            scan_status: Mutex::new(ScanStatus::default()),
        })
    }
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

    let initial_status = {
        let mut status = state
            .scan_status
            .lock()
            .map_err(|_| AppError::StatePoisoned("scan_status").to_string())?;

        if status.is_running {
            return Err(AppError::ScanAlreadyRunning.to_string());
        }

        *status = ScanStatus {
            is_running: true,
            folder_path: Some(path_to_string(&folder)),
            started_at: Some(now_timestamp()),
            ..ScanStatus::default()
        };

        status.clone()
    };

    emit_scan_status(
        &app,
        initial_status.clone(),
        Some("Started indexing local tracks.".into()),
    );

    std::thread::spawn(move || run_library_scan(app, folder));

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
fn list_local_tracks(state: State<'_, AppState>) -> CommandResult<Vec<LocalTrack>> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;

    list_tracks(&db).map_err(|error| error.to_string())
}

#[tauri::command]
fn local_track_media_source(
    app: AppHandle,
    state: State<'_, AppState>,
    track_id: i64,
) -> CommandResult<MediaSource> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::StatePoisoned("db").to_string())?;

    let file_path: String = db
        .query_row(
            "SELECT file_path FROM local_tracks WHERE id = ?1",
            params![track_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| AppError::TrackNotFound(track_id).to_string())?;

    if !Path::new(&file_path).is_file() {
        return Err(AppError::TrackFileMissing(file_path).to_string());
    }

    app.asset_protocol_scope()
        .allow_file(&file_path)
        .map_err(|error| error.to_string())?;

    let mime_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned();

    Ok(MediaSource {
        file_path,
        mime_type,
    })
}

fn initialize_database(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS local_tracks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL UNIQUE,
            file_name TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT,
            album TEXT,
            duration_seconds INTEGER,
            track_number INTEGER,
            disc_number INTEGER,
            file_size_bytes INTEGER NOT NULL,
            modified_at INTEGER,
            indexed_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_local_tracks_title ON local_tracks(title);
        CREATE INDEX IF NOT EXISTS idx_local_tracks_artist ON local_tracks(artist);
        CREATE INDEX IF NOT EXISTS idx_local_tracks_album ON local_tracks(album);
        ",
    )?;

    Ok(())
}

fn run_library_scan(app: AppHandle, folder: PathBuf) {
    let result = {
        let state = app.state::<AppState>();
        scan_folder(&app, &state, &folder)
    };

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

fn scan_folder(app: &AppHandle, state: &AppState, folder: &Path) -> AppResult<()> {
    let discovery = collect_supported_audio_files(folder);

    update_scan_status(
        app,
        state,
        Some(format!(
            "Discovered {} supported audio files.",
            discovery.files.len()
        )),
        |status| {
            status.discovered_files = discovery.files.len();
        },
    );

    for error in discovery.errors {
        let message = format!("Failed to inspect library path: {error}");
        update_scan_status(app, state, Some(message.clone()), |status| {
            status.error_count += 1;
            status.last_error = Some(message);
        });
    }

    for path in discovery.files {
        match extract_local_track(&path) {
            Ok(draft) => {
                {
                    let db = state.db.lock().map_err(|_| AppError::StatePoisoned("db"))?;
                    upsert_local_track(&db, &draft)?;
                }

                update_scan_status(app, state, None, |status| {
                    status.scanned_files += 1;
                    status.indexed_tracks += 1;
                });
            }
            Err(error) => {
                let message = format!("Skipped {}: {error}", path.display());
                update_scan_status(app, state, Some(message.clone()), |status| {
                    status.scanned_files += 1;
                    status.skipped_files += 1;
                    status.error_count += 1;
                    status.last_error = Some(message);
                });
            }
        }
    }

    update_scan_status(
        app,
        state,
        Some("Finished indexing local tracks.".into()),
        |status| {
            status.is_running = false;
            status.finished_at = Some(now_timestamp());
        },
    );

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

    let tagged_file = lofty::read_from_path(path).map_err(|source| AppError::Metadata {
        path: file_path.clone(),
        source,
    })?;
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    let fallback_title = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| file_name.clone());

    let title = tag_string(tag, |tag| tag.title()).unwrap_or(fallback_title);
    let duration_seconds = seconds_to_i64(tagged_file.properties().duration().as_secs());

    Ok(LocalTrackDraft {
        file_path,
        file_name,
        title,
        artist: tag_string(tag, |tag| tag.artist()),
        album: tag_string(tag, |tag| tag.album()),
        duration_seconds,
        track_number: tag.and_then(|tag| tag.track()).map(i64::from),
        disc_number: tag.and_then(|tag| tag.disk()).map(i64::from),
        file_size_bytes: u64_to_i64(metadata.len()),
        modified_at: metadata.modified().ok().and_then(system_time_to_timestamp),
    })
}

fn tag_string<'tag>(
    tag: Option<&'tag Tag>,
    getter: impl Fn(&'tag Tag) -> Option<std::borrow::Cow<'tag, str>>,
) -> Option<String> {
    tag.and_then(getter)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(file_path) DO UPDATE SET
            file_name = excluded.file_name,
            title = excluded.title,
            artist = excluded.artist,
            album = excluded.album,
            duration_seconds = excluded.duration_seconds,
            track_number = excluded.track_number,
            disc_number = excluded.disc_number,
            file_size_bytes = excluded.file_size_bytes,
            modified_at = excluded.modified_at,
            indexed_at = excluded.indexed_at
        ",
        params![
            draft.file_path,
            draft.file_name,
            draft.title,
            draft.artist,
            draft.album,
            draft.duration_seconds,
            draft.track_number,
            draft.disc_number,
            draft.file_size_bytes,
            draft.modified_at,
            indexed_at,
        ],
    )?;

    track_by_path(connection, &draft.file_path)
}

fn list_tracks(connection: &Connection) -> AppResult<Vec<LocalTrack>> {
    let mut statement = connection.prepare(
        "
        SELECT
            id,
            file_path,
            file_name,
            title,
            artist,
            album,
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at
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
            duration_seconds,
            track_number,
            disc_number,
            file_size_bytes,
            modified_at,
            indexed_at
        FROM local_tracks
        WHERE file_path = ?1
        ",
            params![file_path],
            local_track_from_row,
        )
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
        duration_seconds: row.get(6)?,
        track_number: row.get(7)?,
        disc_number: row.get(8)?,
        file_size_bytes: row.get(9)?,
        modified_at: row.get(10)?,
        indexed_at: row.get(11)?,
    })
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
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let db_path = app_data_dir.join("fika-library.sqlite3");
            let state = AppState::new(&db_path)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            select_music_folder,
            start_library_scan,
            get_scan_status,
            list_local_tracks,
            local_track_media_source
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR_ID: AtomicU64 = AtomicU64::new(0);

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
            duration_seconds: Some(180),
            track_number: Some(1),
            disc_number: Some(1),
            file_size_bytes: 1024,
            modified_at: Some(1_700_000_000),
        }
    }

    fn initialized_connection() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory database should open");
        initialize_database(&connection).expect("schema should initialize");
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
    fn is_supported_audio_file_should_accept_slice_formats_case_insensitively() {
        let actual = ["track.mp3", "track.FLAC", "track.m4a", "track.AAC"]
            .map(|path| is_supported_audio_file(Path::new(path)));

        assert_eq!(actual, [true, true, true, true]);
    }

    #[test]
    fn is_supported_audio_file_should_reject_unsupported_or_missing_extensions() {
        let actual = ["cover.jpg", "notes.txt", "track"]
            .map(|path| is_supported_audio_file(Path::new(path)));

        assert_eq!(actual, [false, false, false]);
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
        let track = upsert_local_track(
            &connection,
            &draft("/library/alpha.mp3", "Alpha", Some("Artist A")),
        )
        .expect("track should insert");

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
}
