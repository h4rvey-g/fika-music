use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, ItemKey, Tag};
use moka::sync::Cache;
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use reqwest::StatusCode;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::Builder as TempFileBuilder;

use super::library::{LibraryAlbumTarget, LibraryService};
use super::{now_timestamp, LocalTrack, LIBRARY_METADATA_VERSION};

const MUSICBRAINZ_BASE_URL: &str = "https://musicbrainz.org/ws/2";
const COVER_ART_ARCHIVE_BASE_URL: &str = "https://coverartarchive.org";
const MUSICBRAINZ_USER_AGENT: &str = "fika-music/0.1.0";
const NETWORK_SETTING_KEY: &str = "album_art_network_enabled";
const MUSICBRAINZ_INTERVAL: Duration = Duration::from_millis(1_100);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(15);
const NEGATIVE_CACHE_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
const MAX_SOURCE_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_EMBEDDED_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_OUTPUT_IMAGE_BYTES: usize = 512 * 1024;
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const OUTPUT_IMAGE_EDGE: u32 = 500;
const OUTPUT_JPEG_QUALITY: u8 = 85;
const MAX_MUSICBRAINZ_CANDIDATES: usize = 8;
const MAX_CACHED_COVERS: u64 = 128;
const MAX_TASK_RESULTS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum AlbumCoverStatus {
    Embedded,
    Downloaded,
    Placeholder,
    AuthorizationRequired,
    NeedsReview,
    Pending,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct AlbumCoverCandidate {
    pub release_group_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i64>,
    pub score: u32,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct AlbumCoverResult {
    pub group_id: String,
    pub status: AlbumCoverStatus,
    pub data_url: Option<String>,
    pub candidates: Vec<AlbumCoverCandidate>,
    pub message: Option<String>,
    pub written_tracks: usize,
    pub failed_tracks: usize,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct AlbumArtSettings {
    pub network_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum LibraryTaskState {
    Idle,
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct AlbumArtTaskStatus {
    pub state: LibraryTaskState,
    pub total: usize,
    pub processed: usize,
    pub embedded: usize,
    pub downloaded: usize,
    pub not_found: usize,
    pub needs_review: usize,
    pub failed: usize,
    pub current_album: Option<String>,
}

impl Default for AlbumArtTaskStatus {
    fn default() -> Self {
        Self {
            state: LibraryTaskState::Idle,
            total: 0,
            processed: 0,
            embedded: 0,
            downloaded: 0,
            not_found: 0,
            needs_review: 0,
            failed: 0,
            current_album: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MetadataLookupItemResult {
    pub track_id: i64,
    pub title: String,
    pub updated: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MetadataLookupTaskStatus {
    pub state: LibraryTaskState,
    pub total: usize,
    pub processed: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub current_track: Option<String>,
    pub results: Vec<MetadataLookupItemResult>,
}

impl Default for MetadataLookupTaskStatus {
    fn default() -> Self {
        Self {
            state: LibraryTaskState::Idle,
            total: 0,
            processed: 0,
            updated: 0,
            unchanged: 0,
            failed: 0,
            current_track: None,
            results: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AlbumArtError {
    #[error("album-art database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("album-art file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("album-art metadata operation failed: {0}")]
    Metadata(#[from] lofty::error::LoftyError),
    #[error("album-art image operation failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("album-art network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("album-art response was invalid: {0}")]
    InvalidResponse(String),
    #[error("album-art state lock was poisoned: {0}")]
    StatePoisoned(&'static str),
    #[error("online metadata access has not been authorized")]
    AuthorizationRequired,
    #[error("a library background task is already running")]
    TaskAlreadyRunning,
}

#[derive(Debug, Clone)]
struct CachedCover {
    source: AlbumCoverStatus,
    data_url: String,
    message: Option<String>,
    written_tracks: usize,
    failed_tracks: usize,
}

#[derive(Debug)]
struct AlbumLookupCache {
    status: String,
    candidates: Vec<AlbumCoverCandidate>,
    message: Option<String>,
    checked_at: i64,
    written_tracks: usize,
    failed_tracks: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct TrackWriteCounts {
    written: usize,
    failed: usize,
}

pub struct AlbumArtService {
    db: Arc<Mutex<Connection>>,
    library: Arc<Mutex<LibraryService>>,
    musicbrainz_client: Client,
    cover_client: Client,
    next_musicbrainz_request: Mutex<Instant>,
    file_write_lock: Mutex<()>,
    covers: Cache<String, CachedCover>,
    in_flight: Mutex<HashSet<String>>,
    album_task: Mutex<AlbumArtTaskStatus>,
    album_pending: Mutex<VecDeque<LibraryAlbumTarget>>,
    album_task_cancelled: AtomicBool,
    metadata_task: Mutex<MetadataLookupTaskStatus>,
    metadata_pending: Mutex<VecDeque<LocalTrack>>,
    metadata_task_cancelled: AtomicBool,
}

impl AlbumArtService {
    pub fn new(
        db: Arc<Mutex<Connection>>,
        library: Arc<Mutex<LibraryService>>,
    ) -> Result<Self, AlbumArtError> {
        let musicbrainz_client = Client::builder()
            .timeout(NETWORK_TIMEOUT)
            .user_agent(MUSICBRAINZ_USER_AGENT)
            .redirect(Policy::none())
            .build()?;
        let cover_client = Client::builder()
            .timeout(NETWORK_TIMEOUT)
            .user_agent(MUSICBRAINZ_USER_AGENT)
            .redirect(Policy::custom(|attempt| {
                let allowed = attempt
                    .url()
                    .host_str()
                    .is_some_and(is_allowed_artwork_host)
                    && attempt.url().scheme() == "https";
                if allowed && attempt.previous().len() < 5 {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()?;
        Ok(Self {
            db,
            library,
            musicbrainz_client,
            cover_client,
            next_musicbrainz_request: Mutex::new(Instant::now()),
            file_write_lock: Mutex::new(()),
            covers: Cache::builder().max_capacity(MAX_CACHED_COVERS).build(),
            in_flight: Mutex::new(HashSet::new()),
            album_task: Mutex::new(AlbumArtTaskStatus::default()),
            album_pending: Mutex::new(VecDeque::new()),
            album_task_cancelled: AtomicBool::new(false),
            metadata_task: Mutex::new(MetadataLookupTaskStatus::default()),
            metadata_pending: Mutex::new(VecDeque::new()),
            metadata_task_cancelled: AtomicBool::new(false),
        })
    }

    pub fn settings(&self) -> Result<AlbumArtSettings, AlbumArtError> {
        let db = self
            .db
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("db"))?;
        let enabled = db
            .query_row(
                "SELECT setting_value FROM app_settings WHERE setting_key = ?1",
                [NETWORK_SETTING_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some_and(|value| value == "true");
        Ok(AlbumArtSettings {
            network_enabled: enabled,
        })
    }

    pub fn set_network_enabled(&self, enabled: bool) -> Result<AlbumArtSettings, AlbumArtError> {
        let db = self
            .db
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("db"))?;
        db.execute(
            "INSERT INTO app_settings (setting_key, setting_value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(setting_key) DO UPDATE SET
                setting_value = excluded.setting_value,
                updated_at = excluded.updated_at",
            params![
                NETWORK_SETTING_KEY,
                if enabled { "true" } else { "false" },
                now_timestamp()
            ],
        )?;
        Ok(AlbumArtSettings {
            network_enabled: enabled,
        })
    }

    pub fn resolve_album(
        &self,
        target: &LibraryAlbumTarget,
        selected_release_group_id: Option<&str>,
    ) -> Result<AlbumCoverResult, AlbumArtError> {
        if let Some(cached) = self.covers.get(&target.group_id) {
            return Ok(AlbumCoverResult {
                group_id: target.group_id.clone(),
                status: cached.source,
                data_url: Some(cached.data_url),
                candidates: Vec::new(),
                message: cached.message,
                written_tracks: cached.written_tracks,
                failed_tracks: cached.failed_tracks,
            });
        }
        let _in_flight = match InFlightGuard::acquire(&self.in_flight, &target.group_id)? {
            Some(guard) => guard,
            None => return Ok(pending_result(&target.group_id)),
        };
        let (embedded_cover, has_embedded_picture) = scan_album_embedded_cover(&target.tracks);
        if let Some(jpeg) = embedded_cover {
            let persisted = self
                .lookup_cache(&target.group_id)?
                .filter(|lookup| matches!(lookup.status.as_str(), "embedded" | "partial"));
            let data_url = jpeg_data_url(&jpeg);
            self.covers.insert(
                target.group_id.clone(),
                CachedCover {
                    source: AlbumCoverStatus::Embedded,
                    data_url: data_url.clone(),
                    message: persisted.as_ref().and_then(|lookup| lookup.message.clone()),
                    written_tracks: persisted.as_ref().map_or(0, |lookup| lookup.written_tracks),
                    failed_tracks: persisted.as_ref().map_or(0, |lookup| lookup.failed_tracks),
                },
            );
            return Ok(AlbumCoverResult {
                group_id: target.group_id.clone(),
                status: AlbumCoverStatus::Embedded,
                data_url: Some(data_url),
                candidates: Vec::new(),
                message: persisted.as_ref().and_then(|lookup| lookup.message.clone()),
                written_tracks: persisted.as_ref().map_or(0, |lookup| lookup.written_tracks),
                failed_tracks: persisted.as_ref().map_or(0, |lookup| lookup.failed_tracks),
            });
        }
        if has_embedded_picture {
            return Ok(placeholder_result(
                &target.group_id,
                Some("Existing embedded artwork could not be decoded; the audio files were left unchanged."),
            ));
        }
        if !self.settings()?.network_enabled {
            return Ok(AlbumCoverResult {
                group_id: target.group_id.clone(),
                status: AlbumCoverStatus::AuthorizationRequired,
                data_url: None,
                candidates: Vec::new(),
                message: Some("Online cover completion requires permission.".to_owned()),
                written_tracks: 0,
                failed_tracks: 0,
            });
        }

        if let Some(release_group_id) = selected_release_group_id {
            return self.download_and_embed(target, release_group_id);
        }
        if let Some(cached) = self.lookup_cache(&target.group_id)? {
            if now_timestamp() - cached.checked_at <= NEGATIVE_CACHE_TTL_SECONDS {
                match cached.status.as_str() {
                    "not_found" => {
                        return Ok(placeholder_result(
                            &target.group_id,
                            cached.message.as_deref(),
                        ));
                    }
                    "ambiguous" => {
                        return Ok(review_result(
                            &target.group_id,
                            cached.candidates,
                            cached.message,
                        ));
                    }
                    _ => {}
                }
            }
        }

        let candidates = self.search_album_candidates(target)?;
        match candidates.as_slice() {
            [] => {
                self.store_lookup(
                    &target.group_id,
                    "not_found",
                    None,
                    &[],
                    Some("No reliable MusicBrainz album match was found."),
                    TrackWriteCounts::default(),
                )?;
                Ok(placeholder_result(
                    &target.group_id,
                    Some("No reliable MusicBrainz album match was found."),
                ))
            }
            [candidate] => {
                if self.album_tracklist_matches(target, &candidate.release_group_id)? {
                    self.download_and_embed(target, &candidate.release_group_id)
                } else {
                    let message = "The MusicBrainz album match has a different track listing; review it before writing artwork.";
                    self.store_lookup(
                        &target.group_id,
                        "ambiguous",
                        None,
                        &candidates,
                        Some(message),
                        TrackWriteCounts::default(),
                    )?;
                    Ok(review_result(
                        &target.group_id,
                        candidates,
                        Some(message.to_owned()),
                    ))
                }
            }
            _ => {
                self.store_lookup(
                    &target.group_id,
                    "ambiguous",
                    None,
                    &candidates,
                    Some("Multiple MusicBrainz albums matched; choose one to continue."),
                    TrackWriteCounts::default(),
                )?;
                Ok(review_result(
                    &target.group_id,
                    candidates,
                    Some("Multiple MusicBrainz albums matched; choose one to continue.".to_owned()),
                ))
            }
        }
    }

    fn download_and_embed(
        &self,
        target: &LibraryAlbumTarget,
        release_group_id: &str,
    ) -> Result<AlbumCoverResult, AlbumArtError> {
        let Some(jpeg) = self.download_release_group_cover(release_group_id)? else {
            self.store_lookup(
                &target.group_id,
                "not_found",
                Some(release_group_id),
                &[],
                Some("The matched MusicBrainz album has no Cover Art Archive Front image."),
                TrackWriteCounts::default(),
            )?;
            return Ok(placeholder_result(
                &target.group_id,
                Some("The matched MusicBrainz album has no Cover Art Archive Front image."),
            ));
        };

        let mut written_tracks = 0;
        let mut failed_tracks = 0;
        let mut messages = Vec::new();
        for track in &target.tracks {
            let write_result = self
                .file_write_lock
                .lock()
                .map_err(|_| AlbumArtError::StatePoisoned("file_write"))
                .and_then(|_guard| embed_cover_atomic(Path::new(&track.file_path), &jpeg));
            match write_result {
                Ok(WriteOutcome::Changed | WriteOutcome::Unchanged) => written_tracks += 1,
                Err(error) => {
                    failed_tracks += 1;
                    messages.push(format!("{}: {error}", track.file_name));
                }
            }
        }
        if written_tracks == 0 {
            let message = messages
                .first()
                .cloned()
                .unwrap_or_else(|| "No album files could be updated.".to_owned());
            self.store_lookup(
                &target.group_id,
                "failed",
                Some(release_group_id),
                &[],
                Some(&message),
                TrackWriteCounts {
                    written: 0,
                    failed: failed_tracks,
                },
            )?;
            return Ok(AlbumCoverResult {
                group_id: target.group_id.clone(),
                status: AlbumCoverStatus::Failed,
                data_url: None,
                candidates: Vec::new(),
                message: Some(message),
                written_tracks,
                failed_tracks,
            });
        }

        self.store_lookup(
            &target.group_id,
            if failed_tracks == 0 {
                "embedded"
            } else {
                "partial"
            },
            Some(release_group_id),
            &[],
            messages.first().map(String::as_str),
            TrackWriteCounts {
                written: written_tracks,
                failed: failed_tracks,
            },
        )?;
        let data_url = jpeg_data_url(&jpeg);
        self.covers.insert(
            target.group_id.clone(),
            CachedCover {
                source: AlbumCoverStatus::Downloaded,
                data_url: data_url.clone(),
                message: messages.first().cloned(),
                written_tracks,
                failed_tracks,
            },
        );
        Ok(AlbumCoverResult {
            group_id: target.group_id.clone(),
            status: AlbumCoverStatus::Downloaded,
            data_url: Some(data_url),
            candidates: Vec::new(),
            message: if failed_tracks == 0 {
                None
            } else {
                Some(format!(
                    "Embedded the cover in {written_tracks} tracks; {failed_tracks} failed."
                ))
            },
            written_tracks,
            failed_tracks,
        })
    }

    fn search_album_candidates(
        &self,
        target: &LibraryAlbumTarget,
    ) -> Result<Vec<AlbumCoverCandidate>, AlbumArtError> {
        self.wait_for_musicbrainz()?;
        let query = format!(
            "releasegroup:\"{}\" AND artist:\"{}\"",
            escape_lucene(&target.title),
            escape_lucene(&target.album_artist)
        );
        let limit = MAX_MUSICBRAINZ_CANDIDATES.to_string();
        let response = self
            .musicbrainz_client
            .get(format!("{MUSICBRAINZ_BASE_URL}/release-group/"))
            .query(&[
                ("query", query.as_str()),
                ("fmt", "json"),
                ("limit", limit.as_str()),
            ])
            .send()?
            .error_for_status()?;
        let body: MbReleaseGroupSearch = response.json()?;
        let title_key = normalize_match_text(&target.title);
        let artist_key = normalize_match_text(&target.album_artist);
        let mut candidates = body
            .release_groups
            .into_iter()
            .filter(|group| json_score(&group.score) >= 90)
            .filter(|group| normalize_match_text(&group.title) == title_key)
            .filter(|group| credit_matches(&group.artist_credit, &artist_key))
            .map(|group| AlbumCoverCandidate {
                release_group_id: group.id,
                title: group.title,
                artist: credit_name(&group.artist_credit),
                year: year_from_date(group.first_release_date.as_deref()),
                score: json_score(&group.score),
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            let left_year_match = target.year.is_some() && target.year == left.year;
            let right_year_match = target.year.is_some() && target.year == right.year;
            right_year_match
                .cmp(&left_year_match)
                .then_with(|| right.score.cmp(&left.score))
                .then_with(|| left.release_group_id.cmp(&right.release_group_id))
        });
        if candidates.len() > 1 && target.year.is_some() {
            let year_matches = candidates
                .iter()
                .filter(|candidate| candidate.year == target.year)
                .cloned()
                .collect::<Vec<_>>();
            if year_matches.len() == 1 {
                return Ok(year_matches);
            }
        }
        Ok(candidates)
    }

    fn download_release_group_cover(
        &self,
        release_group_id: &str,
    ) -> Result<Option<Vec<u8>>, AlbumArtError> {
        if !is_uuid_like(release_group_id) {
            return Err(AlbumArtError::InvalidResponse(
                "MusicBrainz returned an invalid release-group id".to_owned(),
            ));
        }
        let response = self
            .cover_client
            .get(format!(
                "{COVER_ART_ARCHIVE_BASE_URL}/release-group/{release_group_id}/front-500"
            ))
            .send()?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status()?;
        let bytes = read_bounded_response(response, MAX_SOURCE_IMAGE_BYTES)?;
        Ok(Some(normalize_jpeg(&bytes, true)?))
    }

    fn album_tracklist_matches(
        &self,
        target: &LibraryAlbumTarget,
        release_group_id: &str,
    ) -> Result<bool, AlbumArtError> {
        if !is_uuid_like(release_group_id) {
            return Err(AlbumArtError::InvalidResponse(
                "MusicBrainz returned an invalid release-group id".to_owned(),
            ));
        }
        self.wait_for_musicbrainz()?;
        let response = self
            .musicbrainz_client
            .get(format!("{MUSICBRAINZ_BASE_URL}/release/"))
            .query(&[
                ("release-group", release_group_id),
                ("inc", "media"),
                ("fmt", "json"),
                ("limit", "100"),
            ])
            .send()?
            .error_for_status()?;
        let body: MbReleaseBrowse = response.json()?;
        Ok(body
            .releases
            .iter()
            .any(|release| release_tracklist_matches(target, release)))
    }

    fn wait_for_musicbrainz(&self) -> Result<(), AlbumArtError> {
        let mut next = self
            .next_musicbrainz_request
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("musicbrainz_rate_limit"))?;
        let now = Instant::now();
        if *next > now {
            thread::sleep(*next - now);
        }
        *next = Instant::now() + MUSICBRAINZ_INTERVAL;
        Ok(())
    }

    pub fn album_task_status(&self) -> Result<AlbumArtTaskStatus, AlbumArtError> {
        self.album_task
            .lock()
            .map(|status| status.clone())
            .map_err(|_| AlbumArtError::StatePoisoned("album_art_task"))
    }

    pub fn start_album_backfill<F>(
        self: &Arc<Self>,
        targets: Vec<LibraryAlbumTarget>,
        progress: F,
    ) -> Result<AlbumArtTaskStatus, AlbumArtError>
    where
        F: Fn(AlbumArtTaskStatus) + Send + 'static,
    {
        if !self.settings()?.network_enabled {
            return Err(AlbumArtError::AuthorizationRequired);
        }
        let initial = {
            let mut status = self
                .album_task
                .lock()
                .map_err(|_| AlbumArtError::StatePoisoned("album_art_task"))?;
            if matches!(
                status.state,
                LibraryTaskState::Running | LibraryTaskState::Paused
            ) {
                return Err(AlbumArtError::TaskAlreadyRunning);
            }
            *status = AlbumArtTaskStatus {
                state: LibraryTaskState::Running,
                total: targets.len(),
                ..AlbumArtTaskStatus::default()
            };
            status.clone()
        };
        *self
            .album_pending
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("album_art_pending"))? =
            VecDeque::from(targets);
        self.album_task_cancelled
            .store(false, AtomicOrdering::Release);
        self.spawn_album_worker(initial, progress);
        self.album_task_status()
    }

    pub fn resume_album_backfill<F>(
        self: &Arc<Self>,
        progress: F,
    ) -> Result<AlbumArtTaskStatus, AlbumArtError>
    where
        F: Fn(AlbumArtTaskStatus) + Send + 'static,
    {
        if !self.settings()?.network_enabled {
            return Err(AlbumArtError::AuthorizationRequired);
        }
        let has_pending = !self
            .album_pending
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("album_art_pending"))?
            .is_empty();
        if !has_pending {
            return Err(AlbumArtError::InvalidResponse(
                "there is no paused album cover task to resume".to_owned(),
            ));
        }
        let initial = {
            let mut status = self
                .album_task
                .lock()
                .map_err(|_| AlbumArtError::StatePoisoned("album_art_task"))?;
            if status.state == LibraryTaskState::Running {
                return Err(AlbumArtError::TaskAlreadyRunning);
            }
            status.state = LibraryTaskState::Running;
            status.current_album = None;
            status.clone()
        };
        self.album_task_cancelled
            .store(false, AtomicOrdering::Release);
        self.spawn_album_worker(initial, progress);
        self.album_task_status()
    }

    fn spawn_album_worker<F>(self: &Arc<Self>, initial: AlbumArtTaskStatus, progress: F)
    where
        F: Fn(AlbumArtTaskStatus) + Send + 'static,
    {
        let service = Arc::clone(self);
        thread::spawn(move || {
            progress(initial);
            loop {
                if service.album_task_cancelled.load(AtomicOrdering::Acquire) {
                    if let Ok(mut status) = service.album_task.lock() {
                        status.state = LibraryTaskState::Paused;
                        status.current_album = None;
                        progress(status.clone());
                    }
                    return;
                }
                let target = match service.album_pending.lock() {
                    Ok(mut pending) => pending.pop_front(),
                    Err(_) => {
                        if let Ok(mut status) = service.album_task.lock() {
                            status.failed += 1;
                            status.state = LibraryTaskState::Completed;
                            status.current_album = None;
                            progress(status.clone());
                        }
                        return;
                    }
                };
                let Some(target) = target else {
                    break;
                };
                if let Ok(mut status) = service.album_task.lock() {
                    status.current_album = Some(target.title.clone());
                }
                let result = service.resolve_album(&target, None);
                if result
                    .as_ref()
                    .is_ok_and(|result| result.status == AlbumCoverStatus::Pending)
                {
                    if let Ok(mut pending) = service.album_pending.lock() {
                        pending.push_back(target);
                    }
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }
                if let Ok(mut status) = service.album_task.lock() {
                    status.processed += 1;
                    let result_status = result.as_ref().ok().map(|result| result.status);
                    match result {
                        Ok(result) => match result.status {
                            AlbumCoverStatus::Embedded => status.embedded += 1,
                            AlbumCoverStatus::Downloaded => status.downloaded += 1,
                            AlbumCoverStatus::Placeholder => status.not_found += 1,
                            AlbumCoverStatus::NeedsReview => status.needs_review += 1,
                            AlbumCoverStatus::AuthorizationRequired
                            | AlbumCoverStatus::Pending
                            | AlbumCoverStatus::Failed => status.failed += 1,
                        },
                        Err(_) => status.failed += 1,
                    }
                    let noteworthy = result_status != Some(AlbumCoverStatus::Embedded);
                    if noteworthy || status.processed % 25 == 0 || status.processed == status.total
                    {
                        progress(status.clone());
                    }
                }
            }
            if let Ok(mut status) = service.album_task.lock() {
                status.state = LibraryTaskState::Completed;
                status.current_album = None;
                progress(status.clone());
            }
        });
    }

    pub fn pause_album_backfill(&self) -> Result<AlbumArtTaskStatus, AlbumArtError> {
        self.album_task_cancelled
            .store(true, AtomicOrdering::Release);
        self.album_task_status()
    }

    pub fn metadata_task_status(&self) -> Result<MetadataLookupTaskStatus, AlbumArtError> {
        self.metadata_task
            .lock()
            .map(|status| status.clone())
            .map_err(|_| AlbumArtError::StatePoisoned("metadata_lookup_task"))
    }

    pub fn start_metadata_lookup<F>(
        self: &Arc<Self>,
        tracks: Vec<LocalTrack>,
        progress: F,
    ) -> Result<MetadataLookupTaskStatus, AlbumArtError>
    where
        F: Fn(MetadataLookupTaskStatus) + Send + 'static,
    {
        if !self.settings()?.network_enabled {
            return Err(AlbumArtError::AuthorizationRequired);
        }
        let initial = {
            let mut status = self
                .metadata_task
                .lock()
                .map_err(|_| AlbumArtError::StatePoisoned("metadata_lookup_task"))?;
            if matches!(
                status.state,
                LibraryTaskState::Running | LibraryTaskState::Paused
            ) {
                return Err(AlbumArtError::TaskAlreadyRunning);
            }
            *status = MetadataLookupTaskStatus {
                state: LibraryTaskState::Running,
                total: tracks.len(),
                ..MetadataLookupTaskStatus::default()
            };
            status.clone()
        };
        *self
            .metadata_pending
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("metadata_lookup_pending"))? =
            VecDeque::from(tracks);
        self.metadata_task_cancelled
            .store(false, AtomicOrdering::Release);
        self.spawn_metadata_worker(initial, progress);
        self.metadata_task_status()
    }

    pub fn resume_metadata_lookup<F>(
        self: &Arc<Self>,
        progress: F,
    ) -> Result<MetadataLookupTaskStatus, AlbumArtError>
    where
        F: Fn(MetadataLookupTaskStatus) + Send + 'static,
    {
        if !self.settings()?.network_enabled {
            return Err(AlbumArtError::AuthorizationRequired);
        }
        let has_pending = !self
            .metadata_pending
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("metadata_lookup_pending"))?
            .is_empty();
        if !has_pending {
            return Err(AlbumArtError::InvalidResponse(
                "there is no paused metadata lookup to resume".to_owned(),
            ));
        }
        let initial = {
            let mut status = self
                .metadata_task
                .lock()
                .map_err(|_| AlbumArtError::StatePoisoned("metadata_lookup_task"))?;
            if status.state == LibraryTaskState::Running {
                return Err(AlbumArtError::TaskAlreadyRunning);
            }
            status.state = LibraryTaskState::Running;
            status.current_track = None;
            status.clone()
        };
        self.metadata_task_cancelled
            .store(false, AtomicOrdering::Release);
        self.spawn_metadata_worker(initial, progress);
        self.metadata_task_status()
    }

    fn spawn_metadata_worker<F>(self: &Arc<Self>, initial: MetadataLookupTaskStatus, progress: F)
    where
        F: Fn(MetadataLookupTaskStatus) + Send + 'static,
    {
        let service = Arc::clone(self);
        thread::spawn(move || {
            progress(initial);
            loop {
                if service
                    .metadata_task_cancelled
                    .load(AtomicOrdering::Acquire)
                {
                    let reload_error = service
                        .reload_library()
                        .err()
                        .map(|error| error.to_string());
                    if let Ok(mut status) = service.metadata_task.lock() {
                        if let Some(error) = reload_error {
                            status.failed += 1;
                            status.results.push(MetadataLookupItemResult {
                                track_id: 0,
                                title: "Library".to_owned(),
                                updated: false,
                                message: error,
                            });
                            if status.results.len() > MAX_TASK_RESULTS {
                                status.results.remove(0);
                            }
                        }
                        status.state = LibraryTaskState::Paused;
                        status.current_track = None;
                        progress(status.clone());
                    }
                    return;
                }
                let track = match service.metadata_pending.lock() {
                    Ok(mut pending) => pending.pop_front(),
                    Err(_) => {
                        if let Ok(mut status) = service.metadata_task.lock() {
                            status.failed += 1;
                            status.state = LibraryTaskState::Completed;
                            status.current_track = None;
                            status.results.push(MetadataLookupItemResult {
                                track_id: 0,
                                title: "Library".to_owned(),
                                updated: false,
                                message: "metadata pending queue lock was poisoned".to_owned(),
                            });
                            progress(status.clone());
                        }
                        return;
                    }
                };
                let Some(track) = track else {
                    break;
                };
                if let Ok(mut status) = service.metadata_task.lock() {
                    status.current_track = Some(track.title.clone());
                }
                let (result, failed) = match service.lookup_and_write_track_metadata(&track) {
                    Ok(result) => (result, false),
                    Err(error) => (
                        MetadataLookupItemResult {
                            track_id: track.id,
                            title: track.title.clone(),
                            updated: false,
                            message: error.to_string(),
                        },
                        true,
                    ),
                };
                if let Ok(mut status) = service.metadata_task.lock() {
                    status.processed += 1;
                    if result.updated {
                        status.updated += 1;
                    } else if failed {
                        status.failed += 1;
                    } else {
                        status.unchanged += 1;
                    }
                    status.results.push(result);
                    if status.results.len() > MAX_TASK_RESULTS {
                        status.results.remove(0);
                    }
                    progress(status.clone());
                }
            }
            let reload_error = service
                .reload_library()
                .err()
                .map(|error| error.to_string());
            if let Ok(mut status) = service.metadata_task.lock() {
                if let Some(error) = reload_error {
                    status.failed += 1;
                    status.results.push(MetadataLookupItemResult {
                        track_id: 0,
                        title: "Library".to_owned(),
                        updated: false,
                        message: error,
                    });
                    if status.results.len() > MAX_TASK_RESULTS {
                        status.results.remove(0);
                    }
                }
                status.state = LibraryTaskState::Completed;
                status.current_track = None;
                progress(status.clone());
            }
        });
    }

    pub fn pause_metadata_lookup(&self) -> Result<MetadataLookupTaskStatus, AlbumArtError> {
        self.metadata_task_cancelled
            .store(true, AtomicOrdering::Release);
        self.metadata_task_status()
    }

    fn lookup_and_write_track_metadata(
        &self,
        track: &LocalTrack,
    ) -> Result<MetadataLookupItemResult, AlbumArtError> {
        let candidates = self.search_recording_candidates(track)?;
        let Some(recording) = unique_highest_recording(&candidates) else {
            return Ok(MetadataLookupItemResult {
                track_id: track.id,
                title: track.title.clone(),
                updated: false,
                message: "No reliable unique MusicBrainz recording match was found.".to_owned(),
            });
        };
        let metadata = downloaded_metadata(recording);
        let cover = match (
            metadata.release_group_id.as_deref(),
            audio_file_has_picture(Path::new(&track.file_path)),
        ) {
            (Some(release_group_id), false) => {
                self.download_release_group_cover(release_group_id)?
            }
            _ => None,
        };
        let (outcome, fields) = {
            let _guard = self
                .file_write_lock
                .lock()
                .map_err(|_| AlbumArtError::StatePoisoned("file_write"))?;
            fill_track_metadata_atomic(Path::new(&track.file_path), &metadata, cover.as_deref())?
        };
        if outcome == WriteOutcome::Unchanged {
            return Ok(MetadataLookupItemResult {
                track_id: track.id,
                title: track.title.clone(),
                updated: false,
                message: "No empty metadata fields were available to fill.".to_owned(),
            });
        }
        let mut updated = track.clone();
        fields.apply(&mut updated, &metadata);
        let file_metadata = fs::metadata(&track.file_path)?;
        updated.file_size_bytes = i64::try_from(file_metadata.len()).unwrap_or(i64::MAX);
        updated.modified_at = file_metadata
            .modified()
            .ok()
            .and_then(super::system_time_to_timestamp);
        updated.indexed_at = now_timestamp();
        self.persist_updated_track(&updated)?;
        Ok(MetadataLookupItemResult {
            track_id: track.id,
            title: updated.title,
            updated: true,
            message: format!("Filled {} metadata field(s).", fields.count()),
        })
    }

    fn search_recording_candidates(
        &self,
        track: &LocalTrack,
    ) -> Result<Vec<MbRecording>, AlbumArtError> {
        let artist = track.artist.as_deref().unwrap_or("").trim();
        if track.title.trim().is_empty() || artist.is_empty() {
            return Ok(Vec::new());
        }
        self.wait_for_musicbrainz()?;
        let query = format!(
            "recording:\"{}\" AND artist:\"{}\"",
            escape_lucene(&track.title),
            escape_lucene(artist)
        );
        let limit = MAX_MUSICBRAINZ_CANDIDATES.to_string();
        let response = self
            .musicbrainz_client
            .get(format!("{MUSICBRAINZ_BASE_URL}/recording/"))
            .query(&[
                ("query", query.as_str()),
                ("fmt", "json"),
                ("limit", limit.as_str()),
            ])
            .send()?
            .error_for_status()?;
        let body: MbRecordingSearch = response.json()?;
        let title_key = normalize_match_text(&track.title);
        let artist_key = normalize_match_text(artist);
        let mut recordings = body
            .recordings
            .into_iter()
            .filter(|recording| normalize_match_text(&recording.title) == title_key)
            .filter(|recording| credit_matches(&recording.artist_credit, &artist_key))
            .collect::<Vec<_>>();
        recordings.sort_by(|left, right| {
            json_score(&right.score)
                .cmp(&json_score(&left.score))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(recordings)
    }

    fn persist_updated_track(&self, track: &LocalTrack) -> Result<(), AlbumArtError> {
        let db = self
            .db
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("db"))?;
        db.execute(
            "UPDATE local_tracks SET
                title = ?2,
                artist = ?3,
                album = ?4,
                album_artist = ?5,
                genre = ?6,
                year = ?7,
                track_number = ?8,
                disc_number = ?9,
                file_size_bytes = ?10,
                modified_at = ?11,
                indexed_at = ?12,
                metadata_version = ?13
             WHERE id = ?1",
            params![
                track.id,
                track.title,
                track.artist,
                track.album,
                track.album_artist,
                track.genre,
                track.year,
                track.track_number,
                track.disc_number,
                track.file_size_bytes,
                track.modified_at,
                track.indexed_at,
                LIBRARY_METADATA_VERSION,
            ],
        )?;
        Ok(())
    }

    fn reload_library(&self) -> Result<(), AlbumArtError> {
        let db = self
            .db
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("db"))?;
        let mut library = self
            .library
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("library"))?;
        library
            .reload(&db)
            .map_err(|error| AlbumArtError::InvalidResponse(error.to_string()))
    }

    fn lookup_cache(&self, group_id: &str) -> Result<Option<AlbumLookupCache>, AlbumArtError> {
        let db = self
            .db
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("db"))?;
        db.query_row(
            "SELECT status, release_group_id, candidates_json, message, checked_at,
                    written_tracks, failed_tracks
             FROM album_art_lookups WHERE group_id = ?1",
            [group_id],
            |row| {
                let candidates_json = row.get::<_, Option<String>>(2)?;
                Ok(AlbumLookupCache {
                    status: row.get(0)?,
                    candidates: candidates_json
                        .as_deref()
                        .and_then(|json| serde_json::from_str(json).ok())
                        .unwrap_or_default(),
                    message: row.get(3)?,
                    checked_at: row.get(4)?,
                    written_tracks: row
                        .get::<_, i64>(5)
                        .ok()
                        .and_then(|value| usize::try_from(value).ok())
                        .unwrap_or_default(),
                    failed_tracks: row
                        .get::<_, i64>(6)
                        .ok()
                        .and_then(|value| usize::try_from(value).ok())
                        .unwrap_or_default(),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    fn store_lookup(
        &self,
        group_id: &str,
        status: &str,
        release_group_id: Option<&str>,
        candidates: &[AlbumCoverCandidate],
        message: Option<&str>,
        counts: TrackWriteCounts,
    ) -> Result<(), AlbumArtError> {
        let candidates_json = if candidates.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(candidates)
                    .map_err(|error| AlbumArtError::InvalidResponse(error.to_string()))?,
            )
        };
        let db = self
            .db
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("db"))?;
        db.execute(
            "INSERT INTO album_art_lookups (
                group_id, status, release_group_id, candidates_json, message, checked_at,
                written_tracks, failed_tracks
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(group_id) DO UPDATE SET
                status = excluded.status,
                release_group_id = excluded.release_group_id,
                candidates_json = excluded.candidates_json,
                message = excluded.message,
                checked_at = excluded.checked_at,
                written_tracks = excluded.written_tracks,
                failed_tracks = excluded.failed_tracks",
            params![
                group_id,
                status,
                release_group_id,
                candidates_json,
                message,
                now_timestamp(),
                i64::try_from(counts.written).unwrap_or(i64::MAX),
                i64::try_from(counts.failed).unwrap_or(i64::MAX),
            ],
        )?;
        Ok(())
    }
}

struct InFlightGuard<'a> {
    state: &'a Mutex<HashSet<String>>,
    group_id: String,
}

impl<'a> InFlightGuard<'a> {
    fn acquire(
        state: &'a Mutex<HashSet<String>>,
        group_id: &str,
    ) -> Result<Option<Self>, AlbumArtError> {
        let mut groups = state
            .lock()
            .map_err(|_| AlbumArtError::StatePoisoned("album_art_in_flight"))?;
        if !groups.insert(group_id.to_owned()) {
            return Ok(None);
        }
        Ok(Some(Self {
            state,
            group_id: group_id.to_owned(),
        }))
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut groups) = self.state.lock() {
            groups.remove(&self.group_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteOutcome {
    Changed,
    Unchanged,
}

pub(crate) fn embedded_cover_data_url(path: &Path) -> Option<String> {
    embedded_cover_bytes(path)
        .and_then(|bytes| normalize_jpeg(&bytes, false).ok())
        .map(|jpeg| jpeg_data_url(&jpeg))
}

fn scan_album_embedded_cover(tracks: &[LocalTrack]) -> (Option<Vec<u8>>, bool) {
    let mut has_picture = false;
    let mut fallback = None;
    for track in tracks {
        let Ok(tagged_file) = lofty::read_from_path(&track.file_path) else {
            continue;
        };
        for picture in tagged_file
            .tags()
            .iter()
            .flat_map(|tag| tag.pictures().iter())
        {
            has_picture = true;
            if is_valid_embedded_picture(picture) {
                if let Ok(jpeg) = normalize_jpeg(picture.data(), false) {
                    if picture.pic_type() == PictureType::CoverFront {
                        return (Some(jpeg), true);
                    }
                    fallback.get_or_insert(jpeg);
                }
            }
        }
    }
    (fallback, has_picture)
}

fn embedded_cover_bytes(path: &Path) -> Option<Vec<u8>> {
    let tagged_file = lofty::read_from_path(path).ok()?;
    for tag in tagged_file.tags() {
        if let Some(picture) = tag.get_picture_type(PictureType::CoverFront) {
            if is_valid_embedded_picture(picture) {
                return Some(picture.data().to_vec());
            }
        }
    }
    tagged_file.tags().iter().find_map(|tag| {
        tag.pictures()
            .iter()
            .find(|picture| is_valid_embedded_picture(picture))
            .map(|picture| picture.data().to_vec())
    })
}

fn audio_file_has_picture(path: &Path) -> bool {
    lofty::read_from_path(path)
        .map(|tagged_file| {
            tagged_file
                .tags()
                .iter()
                .any(|tag| !tag.pictures().is_empty())
        })
        .unwrap_or(false)
}

fn is_valid_embedded_picture(picture: &Picture) -> bool {
    !picture.data().is_empty() && picture.data().len() <= MAX_EMBEDDED_IMAGE_BYTES
}

fn embed_cover_atomic(path: &Path, jpeg: &[u8]) -> Result<WriteOutcome, AlbumArtError> {
    edit_audio_file_atomic(
        path,
        |tagged_file| {
            if tagged_file
                .tags()
                .iter()
                .any(|tag| !tag.pictures().is_empty())
            {
                return Ok(false);
            }
            let tag = primary_tag_mut(tagged_file)?;
            let mut picture = Picture::from_reader(&mut Cursor::new(jpeg))?;
            picture.set_pic_type(PictureType::CoverFront);
            tag.push_picture(picture);
            Ok(true)
        },
        |tagged_file| tagged_file_contains_cover(tagged_file, jpeg),
    )
}

fn tagged_file_contains_cover(tagged_file: &TaggedFile, expected: &[u8]) -> bool {
    let Ok(expected) = image::load_from_memory(expected) else {
        return false;
    };
    let expected = expected.to_rgb8();
    tagged_file.tags().iter().any(|tag| {
        tag.pictures().iter().any(|picture| {
            image::load_from_memory(picture.data())
                .map(|image| image.to_rgb8() == expected)
                .unwrap_or(false)
        })
    })
}

fn edit_audio_file_atomic(
    path: &Path,
    edit: impl FnOnce(&mut TaggedFile) -> Result<bool, AlbumArtError>,
    verify: impl FnOnce(&TaggedFile) -> bool,
) -> Result<WriteOutcome, AlbumArtError> {
    if !path.is_file() {
        return Err(AlbumArtError::InvalidResponse(format!(
            "audio file does not exist: {}",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        AlbumArtError::InvalidResponse(format!("audio file has no parent: {}", path.display()))
    })?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_else(|| ".audio".to_owned());
    let temporary = TempFileBuilder::new()
        .prefix(".fika-metadata-")
        .suffix(&extension)
        .tempfile_in(parent)?
        .into_temp_path();
    fs::copy(path, &temporary)?;
    let mut tagged_file = lofty::read_from_path(&temporary)?;
    if !edit(&mut tagged_file)? {
        return Ok(WriteOutcome::Unchanged);
    }
    tagged_file.save_to_path(&temporary, WriteOptions::default())?;
    let verified = lofty::read_from_path(&temporary)?;
    if !verify(&verified) {
        return Err(AlbumArtError::InvalidResponse(
            "written metadata could not be read back".to_owned(),
        ));
    }
    fs::OpenOptions::new()
        .read(true)
        .open(&temporary)?
        .sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(WriteOutcome::Changed)
}

fn primary_tag_mut(tagged_file: &mut TaggedFile) -> Result<&mut Tag, AlbumArtError> {
    if tagged_file.primary_tag().is_none() {
        let tag_type = tagged_file.primary_tag_type();
        tagged_file.insert_tag(Tag::new(tag_type));
    }
    tagged_file.primary_tag_mut().ok_or_else(|| {
        AlbumArtError::InvalidResponse("audio format has no writable tag".to_owned())
    })
}

fn normalize_jpeg(bytes: &[u8], upscale: bool) -> Result<Vec<u8>, AlbumArtError> {
    if bytes.is_empty() || bytes.len() > MAX_EMBEDDED_IMAGE_BYTES {
        return Err(AlbumArtError::InvalidResponse(
            "cover image size is outside the supported range".to_owned(),
        ));
    }
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let (width, height) = reader.into_dimensions()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(AlbumArtError::InvalidResponse(
            "cover image dimensions are outside the supported range".to_owned(),
        ));
    }
    let image = image::load_from_memory(bytes)?;
    let normalized = if upscale || width > OUTPUT_IMAGE_EDGE || height > OUTPUT_IMAGE_EDGE {
        image.resize(OUTPUT_IMAGE_EDGE, OUTPUT_IMAGE_EDGE, FilterType::Lanczos3)
    } else {
        image
    };
    for edge in [OUTPUT_IMAGE_EDGE, 450, 400, 350, 300, 250] {
        let resized = if normalized.width() > edge || normalized.height() > edge {
            normalized.resize(edge, edge, FilterType::Lanczos3)
        } else {
            normalized.clone()
        };
        let mut output = Vec::new();
        JpegEncoder::new_with_quality(&mut output, OUTPUT_JPEG_QUALITY).encode_image(&resized)?;
        if output.len() <= MAX_OUTPUT_IMAGE_BYTES {
            return Ok(output);
        }
    }
    Err(AlbumArtError::InvalidResponse(
        "normalized cover exceeds 512 KiB".to_owned(),
    ))
}

fn read_bounded_response(mut response: Response, maximum: usize) -> Result<Vec<u8>, AlbumArtError> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(AlbumArtError::InvalidResponse(
            "cover response exceeds the download limit".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(AlbumArtError::InvalidResponse(
            "cover response exceeds the download limit".to_owned(),
        ));
    }
    Ok(bytes)
}

fn jpeg_data_url(jpeg: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(jpeg))
}

fn is_allowed_artwork_host(host: &str) -> bool {
    host == "coverartarchive.org" || host == "archive.org" || host.ends_with(".archive.org")
}

fn escape_lucene(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn normalize_match_text(value: &str) -> String {
    crate::library::normalize_text(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn year_from_date(value: Option<&str>) -> Option<i64> {
    value?.get(..4)?.parse().ok()
}

fn json_score(value: &Value) -> u32 {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .unwrap_or(0)
}

fn credit_name(credits: &[MbArtistCredit]) -> String {
    credits
        .iter()
        .map(|credit| {
            let name = credit
                .name
                .as_deref()
                .unwrap_or(credit.artist.name.as_str());
            format!("{name}{}", credit.joinphrase.as_deref().unwrap_or(""))
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn credit_matches(credits: &[MbArtistCredit], expected: &str) -> bool {
    if normalize_match_text(&credit_name(credits)) == expected {
        return true;
    }
    credits.iter().any(|credit| {
        normalize_match_text(credit.name.as_deref().unwrap_or("")) == expected
            || normalize_match_text(&credit.artist.name) == expected
            || credit
                .artist
                .aliases
                .iter()
                .any(|alias| normalize_match_text(&alias.name) == expected)
    })
}

fn is_uuid_like(value: &str) -> bool {
    value.len() == 36
        && value
            .chars()
            .enumerate()
            .all(|(index, character)| match index {
                8 | 13 | 18 | 23 => character == '-',
                _ => character.is_ascii_hexdigit(),
            })
}

fn pending_result(group_id: &str) -> AlbumCoverResult {
    AlbumCoverResult {
        group_id: group_id.to_owned(),
        status: AlbumCoverStatus::Pending,
        data_url: None,
        candidates: Vec::new(),
        message: None,
        written_tracks: 0,
        failed_tracks: 0,
    }
}

fn placeholder_result(group_id: &str, message: Option<&str>) -> AlbumCoverResult {
    AlbumCoverResult {
        group_id: group_id.to_owned(),
        status: AlbumCoverStatus::Placeholder,
        data_url: None,
        candidates: Vec::new(),
        message: message.map(str::to_owned),
        written_tracks: 0,
        failed_tracks: 0,
    }
}

fn review_result(
    group_id: &str,
    candidates: Vec<AlbumCoverCandidate>,
    message: Option<String>,
) -> AlbumCoverResult {
    AlbumCoverResult {
        group_id: group_id.to_owned(),
        status: AlbumCoverStatus::NeedsReview,
        data_url: None,
        candidates,
        message,
        written_tracks: 0,
        failed_tracks: 0,
    }
}

#[derive(Debug)]
struct DownloadedMetadata {
    title: String,
    artist: String,
    album: Option<String>,
    album_artist: Option<String>,
    genre: Option<String>,
    year: Option<i64>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    release_group_id: Option<String>,
}

#[derive(Debug, Default)]
struct FilledFields {
    title: bool,
    artist: bool,
    album: bool,
    album_artist: bool,
    genre: bool,
    year: bool,
    track_number: bool,
    disc_number: bool,
    cover: bool,
}

impl FilledFields {
    fn any(&self) -> bool {
        self.count() > 0
    }

    fn count(&self) -> usize {
        [
            self.title,
            self.artist,
            self.album,
            self.album_artist,
            self.genre,
            self.year,
            self.track_number,
            self.disc_number,
            self.cover,
        ]
        .into_iter()
        .filter(|filled| *filled)
        .count()
    }

    fn apply(&self, track: &mut LocalTrack, metadata: &DownloadedMetadata) {
        if self.title {
            track.title.clone_from(&metadata.title);
        }
        if self.artist {
            track.artist = Some(metadata.artist.clone());
        }
        if self.album {
            track.album.clone_from(&metadata.album);
        }
        if self.album_artist {
            track.album_artist.clone_from(&metadata.album_artist);
        }
        if self.genre {
            track.genre.clone_from(&metadata.genre);
        }
        if self.year {
            track.year = metadata.year;
        }
        if self.track_number {
            track.track_number = metadata.track_number;
        }
        if self.disc_number {
            track.disc_number = metadata.disc_number;
        }
    }
}

fn fill_track_metadata_atomic(
    path: &Path,
    metadata: &DownloadedMetadata,
    cover: Option<&[u8]>,
) -> Result<(WriteOutcome, FilledFields), AlbumArtError> {
    let fields = RefCell::new(FilledFields::default());
    let outcome = edit_audio_file_atomic(
        path,
        |tagged_file| {
            let mut fields = fields.borrow_mut();
            let has_picture = tagged_file
                .tags()
                .iter()
                .any(|tag| !tag.pictures().is_empty());
            let tag = primary_tag_mut(tagged_file)?;
            if tag.title().is_none_or(|value| value.trim().is_empty()) {
                tag.set_title(metadata.title.clone());
                fields.title = true;
            }
            if tag.artist().is_none_or(|value| value.trim().is_empty()) {
                tag.set_artist(metadata.artist.clone());
                fields.artist = true;
            }
            if tag.album().is_none_or(|value| value.trim().is_empty()) {
                if let Some(album) = metadata.album.as_ref() {
                    tag.set_album(album.clone());
                    fields.album = true;
                }
            }
            if tag
                .get_string(ItemKey::AlbumArtist)
                .is_none_or(|value| value.trim().is_empty())
            {
                if let Some(album_artist) = metadata.album_artist.as_ref() {
                    tag.insert_text(ItemKey::AlbumArtist, album_artist.clone());
                    fields.album_artist = true;
                }
            }
            if tag.genre().is_none_or(|value| value.trim().is_empty()) {
                if let Some(genre) = metadata.genre.as_ref() {
                    tag.set_genre(genre.clone());
                    fields.genre = true;
                }
            }
            let year_is_empty = tag.date().is_none()
                && tag_text_is_empty(tag, ItemKey::RecordingDate)
                && tag_text_is_empty(tag, ItemKey::Year);
            if year_is_empty {
                if let Some(year) = metadata.year.and_then(|year| u16::try_from(year).ok()) {
                    tag.set_date(Timestamp {
                        year,
                        ..Timestamp::default()
                    });
                    fields.year = true;
                }
            }
            if tag.track().is_none() && tag_text_is_empty(tag, ItemKey::TrackNumber) {
                if let Some(track_number) = metadata
                    .track_number
                    .and_then(|value| u32::try_from(value).ok())
                {
                    tag.set_track(track_number);
                    fields.track_number = true;
                }
            }
            if tag.disk().is_none() && tag_text_is_empty(tag, ItemKey::DiscNumber) {
                if let Some(disc_number) = metadata
                    .disc_number
                    .and_then(|value| u32::try_from(value).ok())
                {
                    tag.set_disk(disc_number);
                    fields.disc_number = true;
                }
            }
            if !has_picture {
                if let Some(cover) = cover {
                    let mut picture = Picture::from_reader(&mut Cursor::new(cover))?;
                    picture.set_pic_type(PictureType::CoverFront);
                    tag.push_picture(picture);
                    fields.cover = true;
                }
            }
            Ok(fields.any())
        },
        |tagged_file| {
            let fields = fields.borrow();
            let Some(tag) = tagged_file.primary_tag() else {
                return false;
            };
            let checks = [
                (
                    "title",
                    !fields.title || tag.title().as_deref() == Some(metadata.title.as_str()),
                ),
                (
                    "artist",
                    !fields.artist || tag.artist().as_deref() == Some(metadata.artist.as_str()),
                ),
                (
                    "album",
                    !fields.album || tag.album().as_deref() == metadata.album.as_deref(),
                ),
                (
                    "album artist",
                    !fields.album_artist
                        || tag.get_string(ItemKey::AlbumArtist) == metadata.album_artist.as_deref(),
                ),
                (
                    "genre",
                    !fields.genre || tag.genre().as_deref() == metadata.genre.as_deref(),
                ),
                (
                    "year",
                    !fields.year || tag.date().map(|date| i64::from(date.year)) == metadata.year,
                ),
                (
                    "track number",
                    !fields.track_number || tag.track().map(i64::from) == metadata.track_number,
                ),
                (
                    "disc number",
                    !fields.disc_number || tag.disk().map(i64::from) == metadata.disc_number,
                ),
                (
                    "cover",
                    !fields.cover
                        || cover
                            .is_some_and(|cover| tagged_file_contains_cover(tagged_file, cover)),
                ),
            ];
            let valid = checks.iter().all(|(_, valid)| *valid);
            #[cfg(test)]
            if !valid {
                eprintln!("metadata readback checks: {checks:?}");
            }
            valid
        },
    )?;
    Ok((outcome, fields.into_inner()))
}

fn tag_text_is_empty(tag: &Tag, key: ItemKey) -> bool {
    tag.get_string(key)
        .is_none_or(|value| value.trim().is_empty())
}

fn unique_highest_recording(recordings: &[MbRecording]) -> Option<&MbRecording> {
    let first = recordings.first()?;
    if recordings
        .get(1)
        .is_some_and(|second| json_score(&first.score) == json_score(&second.score))
    {
        return None;
    }
    Some(first)
}

fn downloaded_metadata(recording: &MbRecording) -> DownloadedMetadata {
    let artist = credit_name(&recording.artist_credit);
    let release = preferred_release(&recording.releases);
    let medium_and_track = release.and_then(|release| {
        release
            .media
            .iter()
            .find_map(|medium| medium.tracks.first().map(|track| (medium, track)))
    });
    let genre = recording
        .tags
        .iter()
        .max_by_key(|tag| tag.count)
        .map(|tag| tag.name.clone());
    DownloadedMetadata {
        title: recording.title.clone(),
        artist: artist.clone(),
        album: release.map(|release| release.title.clone()),
        album_artist: release
            .map(|release| credit_name(&release.artist_credit))
            .filter(|value| !value.is_empty())
            .or(Some(artist)),
        genre,
        year: release.and_then(|release| year_from_date(release.date.as_deref())),
        track_number: medium_and_track
            .and_then(|(_, track)| parse_track_number(&track.number))
            .or_else(|| {
                medium_and_track.and_then(|(medium, _)| {
                    medium
                        .track_offset
                        .and_then(|offset| i64::try_from(offset + 1).ok())
                })
            }),
        disc_number: medium_and_track.and_then(|(medium, _)| i64::try_from(medium.position).ok()),
        release_group_id: release
            .and_then(|release| release.release_group.as_ref())
            .map(|group| group.id.clone()),
    }
}

fn preferred_release(releases: &[MbRelease]) -> Option<&MbRelease> {
    releases.iter().max_by(|left, right| {
        release_score(left)
            .cmp(&release_score(right))
            .then_with(|| right.date.cmp(&left.date))
            .then_with(|| right.id.cmp(&left.id))
    })
}

fn release_score(release: &MbRelease) -> i32 {
    let mut score = 0;
    if release.status.as_deref() == Some("Official") {
        score += 40;
    }
    if release
        .release_group
        .as_ref()
        .and_then(|group| group.primary_type.as_deref())
        == Some("Album")
    {
        score += 30;
    }
    if !release.release_group.as_ref().is_some_and(|group| {
        group
            .secondary_types
            .iter()
            .any(|kind| kind == "Compilation")
    }) {
        score += 20;
    }
    if release
        .track_count
        .is_some_and(|count| (2..=40).contains(&count))
    {
        score += 10;
    }
    if release.date.is_some() {
        score += 2;
    }
    score
}

fn release_tracklist_matches(target: &LibraryAlbumTarget, release: &MbTracklistRelease) -> bool {
    let media_total = release
        .media
        .iter()
        .map(|medium| medium.track_count)
        .collect::<Option<Vec<_>>>()
        .map(|counts| counts.into_iter().sum::<usize>());
    if release.track_count.or(media_total) != Some(target.tracks.len()) {
        return false;
    }

    let mut local_disc_counts = Vec::<usize>::new();
    for track in &target.tracks {
        let Some(disc_number) = track
            .disc_number
            .and_then(|number| usize::try_from(number).ok())
            .filter(|number| *number > 0)
        else {
            return true;
        };
        if local_disc_counts.len() < disc_number {
            local_disc_counts.resize(disc_number, 0);
        }
        local_disc_counts[disc_number - 1] += 1;
    }
    if local_disc_counts.contains(&0) || release.media.len() != local_disc_counts.len() {
        return false;
    }
    release.media.iter().enumerate().all(|(index, medium)| {
        let position = medium.position.unwrap_or(index + 1);
        position > 0 && local_disc_counts.get(position - 1).copied() == medium.track_count
    })
}

fn parse_track_number(value: &str) -> Option<i64> {
    let digits = value
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits.parse().ok()
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupSearch {
    #[serde(default, rename = "release-groups")]
    release_groups: Vec<MbReleaseGroup>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroup {
    id: String,
    title: String,
    #[serde(default)]
    score: Value,
    #[serde(default, rename = "first-release-date")]
    first_release_date: Option<String>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<MbArtistCredit>,
}

#[derive(Debug, Clone, Deserialize)]
struct MbArtistCredit {
    #[serde(default)]
    name: Option<String>,
    artist: MbArtist,
    #[serde(default)]
    joinphrase: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MbArtist {
    name: String,
    #[serde(default)]
    aliases: Vec<MbAlias>,
}

#[derive(Debug, Clone, Deserialize)]
struct MbAlias {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbRecordingSearch {
    #[serde(default)]
    recordings: Vec<MbRecording>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseBrowse {
    #[serde(default)]
    releases: Vec<MbTracklistRelease>,
}

#[derive(Debug, Deserialize)]
struct MbTracklistRelease {
    #[serde(default, rename = "track-count")]
    track_count: Option<usize>,
    #[serde(default)]
    media: Vec<MbTracklistMedium>,
}

#[derive(Debug, Deserialize)]
struct MbTracklistMedium {
    #[serde(default)]
    position: Option<usize>,
    #[serde(default, rename = "track-count")]
    track_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MbRecording {
    id: String,
    title: String,
    #[serde(default)]
    score: Value,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    releases: Vec<MbRelease>,
    #[serde(default)]
    tags: Vec<MbTag>,
}

#[derive(Debug, Deserialize)]
struct MbRelease {
    id: String,
    title: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<MbArtistCredit>,
    #[serde(default, rename = "release-group")]
    release_group: Option<MbReleaseGroupReference>,
    #[serde(default, rename = "track-count")]
    track_count: Option<usize>,
    #[serde(default)]
    media: Vec<MbMedium>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroupReference {
    id: String,
    #[serde(default, rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(default, rename = "secondary-types")]
    secondary_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MbMedium {
    position: usize,
    #[serde(default, rename = "track")]
    tracks: Vec<MbTrack>,
    #[serde(default, rename = "track-offset")]
    track_offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MbTrack {
    number: String,
}

#[derive(Debug, Deserialize)]
struct MbTag {
    name: String,
    #[serde(default)]
    count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_fixture(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode_image(&image)
            .expect("fixture should encode");
        bytes
    }

    fn local_track(id: i64, disc_number: Option<i64>) -> LocalTrack {
        LocalTrack {
            id,
            file_path: format!("/music/{id}.flac"),
            file_name: format!("{id}.flac"),
            title: format!("Track {id}"),
            artist: Some("Artist".to_owned()),
            album: Some("Album".to_owned()),
            album_artist: Some("Artist".to_owned()),
            genre: None,
            year: Some(2024),
            codec: Some("FLAC".to_owned()),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_seconds: Some(180),
            track_number: Some(id),
            disc_number,
            file_size_bytes: 1,
            modified_at: None,
            indexed_at: 1,
            play_count: 0,
        }
    }

    #[test]
    fn release_group_search_should_accept_artist_aliases() {
        let credits = vec![MbArtistCredit {
            name: Some("Jay Chou".to_owned()),
            artist: MbArtist {
                name: "Jay Chou".to_owned(),
                aliases: vec![MbAlias {
                    name: "周杰伦".to_owned(),
                }],
            },
            joinphrase: None,
        }];

        assert!(credit_matches(&credits, &normalize_match_text("周杰伦")));
    }

    #[test]
    fn artwork_redirect_should_only_allow_archive_hosts() {
        assert!(is_allowed_artwork_host("ia801.example.archive.org"));
        assert!(!is_allowed_artwork_host("archive.org.example.com"));
    }

    #[test]
    fn uuid_validation_should_reject_path_injection() {
        assert!(!is_uuid_like("../../etc/passwd"));
    }

    #[test]
    fn album_auto_match_should_require_total_and_disc_track_counts() {
        let target = LibraryAlbumTarget {
            group_id: "album".to_owned(),
            title: "Album".to_owned(),
            album_artist: "Artist".to_owned(),
            year: Some(2024),
            tracks: vec![
                local_track(1, Some(1)),
                local_track(2, Some(1)),
                local_track(3, Some(2)),
                local_track(4, Some(2)),
            ],
        };
        let matching = MbTracklistRelease {
            track_count: Some(4),
            media: vec![
                MbTracklistMedium {
                    position: Some(1),
                    track_count: Some(2),
                },
                MbTracklistMedium {
                    position: Some(2),
                    track_count: Some(2),
                },
            ],
        };
        let wrong_disc_split = MbTracklistRelease {
            track_count: Some(4),
            media: vec![
                MbTracklistMedium {
                    position: Some(1),
                    track_count: Some(1),
                },
                MbTracklistMedium {
                    position: Some(2),
                    track_count: Some(3),
                },
            ],
        };

        assert!(release_tracklist_matches(&target, &matching));
        assert!(!release_tracklist_matches(&target, &wrong_disc_split));
    }

    #[test]
    fn normalize_jpeg_should_bound_dimensions_and_file_size() {
        let normalized =
            normalize_jpeg(&jpeg_fixture(1_200, 800), true).expect("cover should normalize");
        let image = image::load_from_memory(&normalized).expect("cover should decode");

        assert_eq!(
            (
                image.width().max(image.height()),
                normalized.len() <= MAX_OUTPUT_IMAGE_BYTES,
            ),
            (OUTPUT_IMAGE_EDGE, true),
        );
    }

    #[test]
    fn normalize_jpeg_should_keep_quality_85_and_shrink_noisy_images_to_the_size_limit() {
        let mut source = image::RgbImage::new(500, 500);
        for (index, pixel) in source.pixels_mut().enumerate() {
            let value = u32::try_from(index).unwrap_or_default();
            *pixel = image::Rgb([
                value.wrapping_mul(73) as u8,
                value.wrapping_mul(151) as u8,
                value.wrapping_mul(199) as u8,
            ]);
        }
        let mut input = Vec::new();
        JpegEncoder::new_with_quality(&mut input, 100)
            .encode_image(&source)
            .expect("noisy fixture should encode");

        let normalized = normalize_jpeg(&input, true).expect("cover should normalize");
        let image = image::load_from_memory(&normalized).expect("cover should decode");

        assert!(normalized.len() <= MAX_OUTPUT_IMAGE_BYTES);
        assert!(image.width().max(image.height()) <= OUTPUT_IMAGE_EDGE);
    }

    #[test]
    fn sidecar_image_should_not_be_used_as_an_embedded_cover() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let audio = directory.path().join("track.mp3");
        fs::write(&audio, b"not an audio file").expect("audio fixture should write");
        fs::write(directory.path().join("cover.jpg"), jpeg_fixture(32, 32))
            .expect("sidecar fixture should write");

        assert_eq!(embedded_cover_data_url(&audio), None);
    }

    #[test]
    fn failed_cover_write_should_leave_the_original_file_unchanged() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let audio = directory.path().join("track.mp3");
        let original = b"not an audio file";
        fs::write(&audio, original).expect("audio fixture should write");

        let result = embed_cover_atomic(&audio, &jpeg_fixture(32, 32));
        let actual = fs::read(audio).expect("original should remain readable");

        assert_eq!((result.is_err(), actual), (true, original.to_vec()));
    }

    #[test]
    fn cover_write_should_round_trip_all_indexed_audio_formats() {
        const FIXTURES: [(&str, &str); 4] = [
            ("mp3", "SUQzBAAAAAAAI1RTU0UAAAAPAAADTGF2ZjYyLjEyLjEwMgAAAAAAAAAAAAAA//tQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAASW5mbwAAAA8AAAADAAACCACZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZnMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMz///////////////////////////////////////////8AAAAATGF2YzYyLjI4AAAAAAAAAAAAAAAAJAKjAAAAAAAAAgjmxD9fAAAAAAAAAAAAAAAAAAAAAP/7EGQAD/AAAGkAAAAIAAANIAAAAQAAAaQAAAAgAAA0gAAABExBTUU0LjBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//sQZCIP8AAAaQAAAAgAAA0gAAABAAABpAAAACAAADSAAAAEVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVX/+xBkRA/wAABpAAAACAAADSAAAAEAAAGkAAAAIAAANIAAAARVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVQ=="),
            ("flac", "ZkxhQwAAACISABIAAAAQAAAQCsRC8AAACJ3qd1mm6BHm3yxYfVK3laE2hAAALg0AAABMYXZmNjIuMTIuMTAyAQAAABUAAABlbmNvZGVyPUxhdmY2Mi4xMi4xMDL/+HkYAAiccwAAAAAAAAUn"),
            ("m4a", "AAAAHGZ0eXBNNEEgAAACAE00QSBpc29taXNvMgAAAAhmcmVlAAAAMW1kYXTeAgBMYXZjNjIuMjguMTAyAEIgCMEYOCEQBGCMHCEQBGCMHCEQBGCMHAAAAwttb292AAAAbG12aGQAAAAAAAAAAAAAAAAAAAPoAAAAMgABAAABAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAACNXRyYWsAAABcdGtoZAAAAAMAAAAAAAAAAAAAAAEAAAAAAAAAMgAAAAAAAAAAAAAAAQEAAAAAAQAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAACRlZHRzAAAAHGVsc3QAAAAAAAAAAQAAADIAAAQAAAEAAAAAAa1tZGlhAAAAIG1kaGQAAAAAAAAAAAAAAAAAAKxEAAAMnVXEAAAAAAAtaGRscgAAAAAAAAAAc291bgAAAAAAAAAAAAAAAFNvdW5kSGFuZGxlcgAAAAFYbWluZgAAABBzbWhkAAAAAAAAAAAAAAAkZGluZgAAABxkcmVmAAAAAAAAAAEAAAAMdXJsIAAAAAEAAAEcc3RibAAAAGpzdHNkAAAAAAAAAAEAAABabXA0YQAAAAAAAAABAAAAAAAAAAAAAgAQAAAAAKxEAAAAAAA2ZXNkcwAAAAADgICAJQABAASAgIAXQBUAAAAAAPoAAAARfwWAgIAFEhBW5QAGgICAAQIAAAAgc3R0cwAAAAAAAAACAAAAAwAABAAAAAABAAAAnQAAABxzdHNjAAAAAAAAAAEAAAABAAAABAAAAAEAAAAkc3RzegAAAAAAAAAAAAAABAAAABcAAAAGAAAABgAAAAYAAAAUc3RjbwAAAAAAAAABAAAALAAAABpzZ3BkAQAAAHJvbGwAAAACAAAAAf//AAAAHHNiZ3AAAAAAcm9sbAAAAAEAAAAEAAAAAQAAAGJ1ZHRhAAAAWm1ldGEAAAAAAAAAIWhkbHIAAAAAAAAAAG1kaXJhcHBsAAAAAAAAAAAAAAAALWlsc3QAAAAlqXRvbwAAAB1kYXRhAAAAAQAAAABMYXZmNjIuMTIuMTAy"),
            ("aac", "//FQgAPf/N4CAExhdmM2Mi4yOC4xMDIAQiAIwRg4//FQgAG//CEQBGCMHP/xUIABv/whEARgjBz/8VCAAb/8IRAEYIwc"),
        ];
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let cover = jpeg_fixture(32, 32);
        let mut outcomes = Vec::new();
        for (extension, encoded) in FIXTURES {
            let path = directory.path().join(format!("track.{extension}"));
            let bytes = BASE64_STANDARD
                .decode(encoded)
                .expect("audio fixture should decode");
            fs::write(&path, bytes).expect("audio fixture should write");
            let result = embed_cover_atomic(&path, &cover);
            outcomes.push((
                extension,
                result.is_ok(),
                embedded_cover_bytes(&path).is_some(),
            ));
        }

        assert_eq!(
            outcomes,
            vec![
                ("mp3", true, true),
                ("flac", true, true),
                ("m4a", true, true),
                ("aac", true, true),
            ],
        );
    }

    #[test]
    fn metadata_write_should_fill_empty_fields_without_overwriting_existing_tags() {
        const MP3_FIXTURE: &str = "SUQzBAAAAAAAI1RTU0UAAAAPAAADTGF2ZjYyLjEyLjEwMgAAAAAAAAAAAAAA//tQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAASW5mbwAAAA8AAAADAAACCACZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZmZnMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMz///////////////////////////////////////////8AAAAATGF2YzYyLjI4AAAAAAAAAAAAAAAAJAKjAAAAAAAAAgjmxD9fAAAAAAAAAAAAAAAAAAAAAP/7EGQAD/AAAGkAAAAIAAANIAAAAQAAAaQAAAAgAAA0gAAABExBTUU0LjBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//sQZCIP8AAAaQAAAAgAAA0gAAABAAABpAAAACAAADSAAAAEVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVX/+xBkRA/wAABpAAAACAAADSAAAAEAAAGkAAAAIAAANIAAAARVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVQ==";
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let audio = directory.path().join("track.mp3");
        fs::write(
            &audio,
            BASE64_STANDARD
                .decode(MP3_FIXTURE)
                .expect("audio fixture should decode"),
        )
        .expect("audio fixture should write");
        edit_audio_file_atomic(
            &audio,
            |tagged_file| {
                primary_tag_mut(tagged_file)?.set_title("Existing title".to_owned());
                Ok(true)
            },
            |_| true,
        )
        .expect("existing title should write");
        let metadata = DownloadedMetadata {
            title: "Replacement title".to_owned(),
            artist: "New artist".to_owned(),
            album: Some("New album".to_owned()),
            album_artist: Some("New album artist".to_owned()),
            genre: Some("Pop".to_owned()),
            year: Some(2024),
            track_number: Some(2),
            disc_number: Some(1),
            release_group_id: None,
        };

        fill_track_metadata_atomic(&audio, &metadata, None).expect("empty metadata should fill");
        let tagged = lofty::read_from_path(audio).expect("written audio should parse");
        let tag = tagged.primary_tag().expect("primary tag should exist");

        assert_eq!(
            (
                tag.title().as_deref().map(str::to_owned),
                tag.artist().as_deref().map(str::to_owned),
                tag.album().as_deref().map(str::to_owned),
                tag.get_string(ItemKey::AlbumArtist).map(str::to_owned),
                tag.track(),
                tag.disk(),
            ),
            (
                Some("Existing title".to_owned()),
                Some("New artist".to_owned()),
                Some("New album".to_owned()),
                Some("New album artist".to_owned()),
                Some(2),
                Some(1),
            ),
        );
    }

    #[test]
    fn musicbrainz_recording_should_map_release_metadata() {
        let recording: MbRecording = serde_json::from_value(serde_json::json!({
            "id": "026fa041-3917-4c73-9079-ed16e36f20f8",
            "score": 100,
            "title": "Track title",
            "artist-credit": [{
                "name": "Track artist",
                "artist": { "name": "Track artist", "aliases": [] }
            }],
            "tags": [{ "name": "Pop", "count": 12 }],
            "releases": [{
                "id": "383be31c-37a0-4e08-8cda-cbcbbc587ae5",
                "title": "Album title",
                "status": "Official",
                "date": "2024-05-20",
                "artist-credit": [{
                    "name": "Album artist",
                    "artist": { "name": "Album artist", "aliases": [] }
                }],
                "release-group": {
                    "id": "4a45bfa5-eb1e-49eb-a20c-1021389b2121",
                    "primary-type": "Album",
                    "secondary-types": []
                },
                "track-count": 10,
                "media": [{
                    "position": 2,
                    "track-offset": 4,
                    "track": [{ "number": "5" }]
                }]
            }]
        }))
        .expect("MusicBrainz fixture should parse");

        let metadata = downloaded_metadata(&recording);

        assert_eq!(
            (
                metadata.title,
                metadata.artist,
                metadata.album,
                metadata.album_artist,
                metadata.genre,
                metadata.year,
                metadata.track_number,
                metadata.disc_number,
            ),
            (
                "Track title".to_owned(),
                "Track artist".to_owned(),
                Some("Album title".to_owned()),
                Some("Album artist".to_owned()),
                Some("Pop".to_owned()),
                Some(2024),
                Some(5),
                Some(2),
            ),
        );
    }

    #[test]
    fn network_permission_should_persist_in_app_settings() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        crate::database::initialize(&mut connection).expect("database should migrate");
        let db = Arc::new(Mutex::new(connection));
        let library = {
            let connection = db.lock().expect("database lock should open");
            LibraryService::load(&connection).expect("library should load")
        };
        let service = AlbumArtService::new(Arc::clone(&db), Arc::new(Mutex::new(library)))
            .expect("service should initialize");

        service
            .set_network_enabled(true)
            .expect("permission should persist");

        assert!(
            service
                .settings()
                .expect("settings should load")
                .network_enabled
        );
    }

    #[test]
    fn paused_background_tasks_should_require_resume_instead_of_replacing_pending_work() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        crate::database::initialize(&mut connection).expect("database should migrate");
        let db = Arc::new(Mutex::new(connection));
        let library = {
            let connection = db.lock().expect("database lock should open");
            LibraryService::load(&connection).expect("library should load")
        };
        let service = Arc::new(
            AlbumArtService::new(Arc::clone(&db), Arc::new(Mutex::new(library)))
                .expect("service should initialize"),
        );
        service
            .set_network_enabled(true)
            .expect("permission should persist");
        *service.album_task.lock().expect("album status should lock") = AlbumArtTaskStatus {
            state: LibraryTaskState::Paused,
            total: 1,
            ..AlbumArtTaskStatus::default()
        };
        *service
            .metadata_task
            .lock()
            .expect("metadata status should lock") = MetadataLookupTaskStatus {
            state: LibraryTaskState::Paused,
            total: 1,
            ..MetadataLookupTaskStatus::default()
        };

        let album_result = service.start_album_backfill(Vec::new(), |_| {});
        let metadata_result = service.start_metadata_lookup(Vec::new(), |_| {});

        assert!(matches!(
            album_result,
            Err(AlbumArtError::TaskAlreadyRunning)
        ));
        assert!(matches!(
            metadata_result,
            Err(AlbumArtError::TaskAlreadyRunning)
        ));
    }
}
