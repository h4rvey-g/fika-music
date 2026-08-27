use crate::registry_support::sha256_hex;
use crate::source_runtime::SourceQuality;
use lofty::file::FileType;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::http::header::{
    ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_TYPE, RANGE,
};
use tauri::http::{Method, Request, Response, StatusCode};
use url::Url;

pub(crate) const PLAYBACK_CACHE_PROTOCOL: &str = "fika-cache";
pub(crate) const DEFAULT_PLAYBACK_CACHE_MAX_MB: u32 = 500;
pub(crate) const MAX_PLAYBACK_CACHE_MB: u32 = 10 * 1024;

const BYTES_PER_MB: u64 = 1024 * 1024;
const CACHE_ENTRY_VERSION: u8 = 1;
const CACHE_ENTRY_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_MEDIA_CHUNK_BYTES: u64 = 1024 * 1024;
const MAX_TRACK_KEY_BYTES: usize = 2048;
const MAX_SOURCE_URL_BYTES: usize = 64 * 1024;
const MEDIA_PREFIX_BYTES: u64 = 512;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlaybackCacheError {
    #[error("playback cache file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("playback cache network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("playback cache metadata error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("playback cache request is invalid: {0}")]
    Invalid(String),
    #[error("playback cache state lock was poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedPlaybackCandidate {
    id: String,
    plugin_id: String,
    source_id: String,
    channel_id: String,
    channel_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CacheOnlinePlaybackRequest {
    track_key: String,
    source_url: String,
    provider_name: String,
    candidate: CachedPlaybackCandidate,
    audio_source_id: String,
    quality: SourceQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedOnlinePlayback {
    cache_id: String,
    provider_name: String,
    candidate: CachedPlaybackCandidate,
    audio_source_id: String,
    quality: SourceQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackCacheEntry {
    version: u8,
    cache_id: String,
    track_key: String,
    content_type: String,
    file_size_bytes: u64,
    cached_at: i64,
    last_accessed_at: i64,
    playback: CachedOnlinePlayback,
}

pub(crate) struct PlaybackCache {
    root: PathBuf,
    client: Client,
    max_bytes: AtomicU64,
    index_lock: Mutex<()>,
    in_flight: Mutex<HashSet<String>>,
}

impl PlaybackCache {
    pub(crate) fn new(root: PathBuf, max_size_mb: u32) -> Result<Self, PlaybackCacheError> {
        fs::create_dir_all(&root)?;
        crate::restrict_path_to_current_user(&root, 0o700)?;
        let cache = Self {
            root,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(15 * 60))
                .user_agent("FikaMusic/0.2 PlaybackCache")
                .build()?,
            max_bytes: AtomicU64::new(cache_size_bytes(max_size_mb)),
            index_lock: Mutex::new(()),
            in_flight: Mutex::new(HashSet::new()),
        };
        let _ = cache.prune();
        Ok(cache)
    }

    pub(crate) fn set_max_size_mb(&self, max_size_mb: u32) {
        self.max_bytes
            .store(cache_size_bytes(max_size_mb), Ordering::Release);
    }

    pub(crate) fn lookup(
        &self,
        track_key: &str,
        qualities: &[SourceQuality],
    ) -> Result<Option<CachedOnlinePlayback>, PlaybackCacheError> {
        validate_track_key(track_key)?;
        if self.max_bytes.load(Ordering::Acquire) == 0 {
            return Ok(None);
        }
        let _guard = self
            .index_lock
            .lock()
            .map_err(|_| PlaybackCacheError::LockPoisoned)?;
        let now = now_timestamp();
        let mut seen = BTreeSet::new();
        for quality in qualities.iter().copied().take(4) {
            if !seen.insert(quality) {
                continue;
            }
            let cache_id = playback_cache_id(track_key, quality);
            let mut entry = match self.read_entry(&cache_id) {
                Ok(entry) => entry,
                Err(PlaybackCacheError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(_) => {
                    self.remove_entry_files(&cache_id)?;
                    continue;
                }
            };
            if !self.entry_is_valid(&entry, &cache_id, now)? {
                self.remove_entry_files(&cache_id)?;
                continue;
            }
            entry.last_accessed_at = now;
            self.write_entry(&entry)?;
            return Ok(Some(entry.playback));
        }
        Ok(None)
    }

    pub(crate) fn reserve(
        &self,
        request: &CacheOnlinePlaybackRequest,
    ) -> Result<Option<String>, PlaybackCacheError> {
        validate_request(request)?;
        if self.max_bytes.load(Ordering::Acquire) == 0 {
            return Ok(None);
        }
        let cache_id = playback_cache_id(&request.track_key, request.quality);
        let mut in_flight = self
            .in_flight
            .lock()
            .map_err(|_| PlaybackCacheError::LockPoisoned)?;
        Ok(in_flight.insert(cache_id.clone()).then_some(cache_id))
    }

    pub(crate) fn store(
        &self,
        request: &CacheOnlinePlaybackRequest,
        cache_id: &str,
    ) -> Result<(), PlaybackCacheError> {
        validate_request(request)?;
        let expected_cache_id = playback_cache_id(&request.track_key, request.quality);
        if cache_id != expected_cache_id {
            return Err(PlaybackCacheError::Invalid(
                "cache identity does not match the track".to_owned(),
            ));
        }
        let initial_limit = self.max_bytes.load(Ordering::Acquire);
        if initial_limit == 0 {
            return Ok(());
        }

        let source_url = Url::parse(&request.source_url)
            .map_err(|_| PlaybackCacheError::Invalid("source URL is invalid".to_owned()))?;
        let mut download = self.client.get(source_url.clone());
        if let Some(headers) = crate::youtube_media_proxy::registered_headers(source_url.as_str()) {
            download = download.headers(headers);
        }
        let mut response = download.send()?;
        if !response.status().is_success() {
            return Err(PlaybackCacheError::Invalid(format!(
                "media request returned HTTP {}",
                response.status().as_u16()
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > initial_limit)
        {
            return Ok(());
        }
        let supplied_content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let response_url = response.url().to_string();
        let mut prefix = Vec::with_capacity(MEDIA_PREFIX_BYTES as usize);
        response
            .by_ref()
            .take(MEDIA_PREFIX_BYTES)
            .read_to_end(&mut prefix)?;
        if prefix.is_empty()
            || !crate::media_response_is_plausible_audio(supplied_content_type.as_deref(), &prefix)
        {
            return Err(PlaybackCacheError::Invalid(
                "media response is not supported audio".to_owned(),
            ));
        }
        let content_type =
            canonical_content_type(supplied_content_type.as_deref(), &response_url, &prefix);
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        temporary.write_all(&prefix)?;
        let mut downloaded = prefix.len() as u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(count as u64);
            let current_limit = self.max_bytes.load(Ordering::Acquire);
            if current_limit == 0 || downloaded > current_limit {
                return Ok(());
            }
            temporary.write_all(&buffer[..count])?;
        }
        temporary.flush()?;
        temporary.as_file().sync_all()?;

        let current_limit = self.max_bytes.load(Ordering::Acquire);
        if current_limit == 0 || downloaded > current_limit {
            return Ok(());
        }
        let now = now_timestamp();
        let playback = CachedOnlinePlayback {
            cache_id: cache_id.to_owned(),
            provider_name: request.provider_name.clone(),
            candidate: request.candidate.clone(),
            audio_source_id: request.audio_source_id.clone(),
            quality: request.quality,
        };
        let entry = PlaybackCacheEntry {
            version: CACHE_ENTRY_VERSION,
            cache_id: cache_id.to_owned(),
            track_key: request.track_key.clone(),
            content_type,
            file_size_bytes: downloaded,
            cached_at: now,
            last_accessed_at: now,
            playback,
        };

        let _guard = self
            .index_lock
            .lock()
            .map_err(|_| PlaybackCacheError::LockPoisoned)?;
        let current_limit = self.max_bytes.load(Ordering::Acquire);
        if current_limit == 0 || downloaded > current_limit {
            return Ok(());
        }
        let audio_path = self.audio_path(cache_id);
        remove_file_if_present(&audio_path)?;
        temporary
            .persist(&audio_path)
            .map_err(|error| PlaybackCacheError::Io(error.error))?;
        crate::restrict_path_to_current_user(&audio_path, 0o600)?;
        if let Err(error) = self.write_entry(&entry) {
            let _ = remove_file_if_present(&audio_path);
            return Err(error);
        }
        self.prune_locked(current_limit, Some(cache_id))
    }

    pub(crate) fn release(&self, cache_id: &str) {
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.remove(cache_id);
        }
    }

    pub(crate) fn remove(&self, cache_id: &str) -> Result<(), PlaybackCacheError> {
        validate_cache_id(cache_id)?;
        let _guard = self
            .index_lock
            .lock()
            .map_err(|_| PlaybackCacheError::LockPoisoned)?;
        self.remove_entry_files(cache_id)
    }

    pub(crate) fn prune(&self) -> Result<(), PlaybackCacheError> {
        let _guard = self
            .index_lock
            .lock()
            .map_err(|_| PlaybackCacheError::LockPoisoned)?;
        self.prune_locked(self.max_bytes.load(Ordering::Acquire), None)
    }

    pub(crate) fn protocol_response(&self, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
        match self.read_media_chunk(&request) {
            Ok(chunk) => local_media_response(chunk),
            Err(error) => cache_error_response(error),
        }
    }

    fn read_media_chunk(
        &self,
        request: &Request<Vec<u8>>,
    ) -> Result<LocalMediaChunk, PlaybackCacheError> {
        if request.method() != Method::GET {
            return Err(PlaybackCacheError::Invalid(
                "cache protocol accepts only GET requests".to_owned(),
            ));
        }
        let cache_id = request
            .uri()
            .path()
            .strip_prefix('/')
            .ok_or_else(|| PlaybackCacheError::Invalid("cache path is invalid".to_owned()))?;
        validate_cache_id(cache_id)?;
        let _guard = self
            .index_lock
            .lock()
            .map_err(|_| PlaybackCacheError::LockPoisoned)?;
        let mut entry = self.read_entry(cache_id)?;
        let now = now_timestamp();
        if !self.entry_is_valid(&entry, cache_id, now)? {
            self.remove_entry_files(cache_id)?;
            return Err(PlaybackCacheError::Invalid(
                "cached audio is unavailable".to_owned(),
            ));
        }
        if now.saturating_sub(entry.last_accessed_at) >= 60 {
            entry.last_accessed_at = now;
            self.write_entry(&entry)?;
        }
        let byte_range = requested_byte_range(request, entry.file_size_bytes)?;
        let length = byte_range.end - byte_range.start + 1;
        let mut file = File::open(self.audio_path(cache_id))?;
        file.seek(SeekFrom::Start(byte_range.start))?;
        let mut body = Vec::with_capacity(length as usize);
        file.take(length).read_to_end(&mut body)?;
        if body.len() as u64 != length {
            return Err(PlaybackCacheError::Invalid(
                "cached audio is incomplete".to_owned(),
            ));
        }
        Ok(LocalMediaChunk {
            status: if byte_range.partial {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            },
            content_type: entry.content_type,
            content_range: byte_range.partial.then(|| {
                format!(
                    "bytes {}-{}/{}",
                    byte_range.start, byte_range.end, entry.file_size_bytes
                )
            }),
            body,
        })
    }

    fn entry_is_valid(
        &self,
        entry: &PlaybackCacheEntry,
        cache_id: &str,
        now: i64,
    ) -> Result<bool, PlaybackCacheError> {
        if entry.version != CACHE_ENTRY_VERSION
            || entry.cache_id != cache_id
            || entry.playback.cache_id != cache_id
            || playback_cache_id(&entry.track_key, entry.playback.quality) != cache_id
            || entry.file_size_bytes == 0
            || entry.content_type.is_empty()
            || now.saturating_sub(entry.last_accessed_at) > CACHE_ENTRY_TTL_SECONDS
        {
            return Ok(false);
        }
        let metadata = match fs::metadata(self.audio_path(cache_id)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        Ok(metadata.is_file() && metadata.len() == entry.file_size_bytes)
    }

    fn prune_locked(
        &self,
        max_bytes: u64,
        protected_cache_id: Option<&str>,
    ) -> Result<(), PlaybackCacheError> {
        let now = now_timestamp();
        let mut entries = Vec::new();
        let mut known_cache_ids = HashSet::new();
        for directory_entry in fs::read_dir(&self.root)? {
            let directory_entry = directory_entry?;
            let path = directory_entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(cache_id) = path.file_stem().and_then(|value| value.to_str()) else {
                remove_file_if_present(&path)?;
                continue;
            };
            let entry = match self.read_entry(cache_id) {
                Ok(entry) => entry,
                Err(_) => {
                    self.remove_entry_files(cache_id)?;
                    continue;
                }
            };
            if !self.entry_is_valid(&entry, cache_id, now)? {
                self.remove_entry_files(cache_id)?;
                continue;
            }
            known_cache_ids.insert(cache_id.to_owned());
            entries.push(entry);
        }
        for directory_entry in fs::read_dir(&self.root)? {
            let path = directory_entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("audio") {
                continue;
            }
            let cache_id = path.file_stem().and_then(|value| value.to_str());
            if cache_id.is_none_or(|cache_id| !known_cache_ids.contains(cache_id)) {
                remove_file_if_present(&path)?;
            }
        }

        entries.sort_by_key(|entry| (entry.last_accessed_at, entry.cached_at));
        let mut total_bytes = entries
            .iter()
            .map(|entry| entry.file_size_bytes)
            .sum::<u64>();
        for entry in entries {
            if total_bytes <= max_bytes {
                break;
            }
            if protected_cache_id == Some(entry.cache_id.as_str()) {
                continue;
            }
            self.remove_entry_files(&entry.cache_id)?;
            total_bytes = total_bytes.saturating_sub(entry.file_size_bytes);
        }
        Ok(())
    }

    fn read_entry(&self, cache_id: &str) -> Result<PlaybackCacheEntry, PlaybackCacheError> {
        let file = File::open(self.metadata_path(cache_id))?;
        Ok(serde_json::from_reader(file)?)
    }

    fn write_entry(&self, entry: &PlaybackCacheEntry) -> Result<(), PlaybackCacheError> {
        let path = self.metadata_path(&entry.cache_id);
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        serde_json::to_writer(temporary.as_file_mut(), entry)?;
        temporary.as_file_mut().flush()?;
        temporary.as_file().sync_all()?;
        remove_file_if_present(&path)?;
        temporary
            .persist(&path)
            .map_err(|error| PlaybackCacheError::Io(error.error))?;
        crate::restrict_path_to_current_user(&path, 0o600)?;
        Ok(())
    }

    fn remove_entry_files(&self, cache_id: &str) -> Result<(), PlaybackCacheError> {
        remove_file_if_present(&self.audio_path(cache_id))?;
        remove_file_if_present(&self.metadata_path(cache_id))?;
        Ok(())
    }

    fn audio_path(&self, cache_id: &str) -> PathBuf {
        self.root.join(format!("{cache_id}.audio"))
    }

    fn metadata_path(&self, cache_id: &str) -> PathBuf {
        self.root.join(format!("{cache_id}.json"))
    }
}

#[derive(Debug)]
struct RequestedByteRange {
    start: u64,
    end: u64,
    partial: bool,
}

#[derive(Debug)]
struct LocalMediaChunk {
    status: StatusCode,
    content_type: String,
    content_range: Option<String>,
    body: Vec<u8>,
}

fn requested_byte_range(
    request: &Request<Vec<u8>>,
    total_bytes: u64,
) -> Result<RequestedByteRange, PlaybackCacheError> {
    if total_bytes == 0 {
        return Err(PlaybackCacheError::Invalid(
            "cached audio is empty".to_owned(),
        ));
    }
    let Some(value) = request.headers().get(RANGE) else {
        let end = (MAX_MEDIA_CHUNK_BYTES - 1).min(total_bytes - 1);
        return Ok(RequestedByteRange {
            start: 0,
            end,
            partial: end + 1 < total_bytes,
        });
    };
    let value = value
        .to_str()
        .map_err(|_| PlaybackCacheError::Invalid("media range is invalid".to_owned()))?;
    let range = value
        .strip_prefix("bytes=")
        .filter(|range| !range.contains(','))
        .ok_or_else(|| PlaybackCacheError::Invalid("media range is invalid".to_owned()))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| PlaybackCacheError::Invalid("media range is invalid".to_owned()))?;
    if start.is_empty() {
        let length = end
            .parse::<u64>()
            .map_err(|_| PlaybackCacheError::Invalid("media range is invalid".to_owned()))?
            .clamp(1, MAX_MEDIA_CHUNK_BYTES)
            .min(total_bytes);
        return Ok(RequestedByteRange {
            start: total_bytes - length,
            end: total_bytes - 1,
            partial: true,
        });
    }
    let start = start
        .parse::<u64>()
        .map_err(|_| PlaybackCacheError::Invalid("media range is invalid".to_owned()))?;
    if start >= total_bytes {
        return Err(PlaybackCacheError::Invalid(
            "media range starts past the cached audio".to_owned(),
        ));
    }
    let maximum_end = start
        .saturating_add(MAX_MEDIA_CHUNK_BYTES - 1)
        .min(total_bytes - 1);
    let end = if end.is_empty() {
        maximum_end
    } else {
        end.parse::<u64>()
            .map_err(|_| PlaybackCacheError::Invalid("media range is invalid".to_owned()))?
            .min(maximum_end)
            .min(total_bytes - 1)
    };
    if end < start {
        return Err(PlaybackCacheError::Invalid(
            "media range is invalid".to_owned(),
        ));
    }
    Ok(RequestedByteRange {
        start,
        end,
        partial: true,
    })
}

fn local_media_response(chunk: LocalMediaChunk) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .status(chunk.status)
        .header(CONTENT_TYPE, chunk.content_type)
        .header(CONTENT_LENGTH, chunk.body.len())
        .header(ACCEPT_RANGES, "bytes")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header("Access-Control-Expose-Headers", "Content-Range")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .header(CACHE_CONTROL, "private, no-store");
    if let Some(content_range) = chunk.content_range {
        builder = builder.header(CONTENT_RANGE, content_range);
    }
    builder
        .body(chunk.body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn cache_error_response(error: PlaybackCacheError) -> Response<Vec<u8>> {
    let status = match error {
        PlaybackCacheError::Invalid(_) => StatusCode::RANGE_NOT_SATISFIABLE,
        _ => StatusCode::NOT_FOUND,
    };
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .header(CACHE_CONTROL, "no-store")
        .body(error.to_string().into_bytes())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn validate_request(request: &CacheOnlinePlaybackRequest) -> Result<(), PlaybackCacheError> {
    validate_track_key(&request.track_key)?;
    validate_text(&request.provider_name, "provider name")?;
    validate_text(&request.audio_source_id, "Audio Source id")?;
    validate_text(&request.candidate.id, "candidate id")?;
    validate_text(&request.candidate.plugin_id, "candidate plugin id")?;
    validate_text(&request.candidate.source_id, "candidate source id")?;
    validate_text(&request.candidate.channel_id, "candidate channel id")?;
    validate_text(&request.candidate.channel_name, "candidate channel name")?;
    if request.source_url.len() > MAX_SOURCE_URL_BYTES {
        return Err(PlaybackCacheError::Invalid(
            "source URL is too long".to_owned(),
        ));
    }
    let source_url = Url::parse(&request.source_url)
        .map_err(|_| PlaybackCacheError::Invalid("source URL is invalid".to_owned()))?;
    if !matches!(source_url.scheme(), "http" | "https")
        || source_url.host_str().is_none()
        || !source_url.username().is_empty()
        || source_url.password().is_some()
    {
        return Err(PlaybackCacheError::Invalid(
            "source URL must be an HTTP URL without credentials".to_owned(),
        ));
    }
    Ok(())
}

fn validate_track_key(track_key: &str) -> Result<(), PlaybackCacheError> {
    if track_key.trim().is_empty() || track_key.len() > MAX_TRACK_KEY_BYTES {
        return Err(PlaybackCacheError::Invalid(
            "track key is empty or too long".to_owned(),
        ));
    }
    Ok(())
}

fn validate_text(value: &str, field: &str) -> Result<(), PlaybackCacheError> {
    if value.trim().is_empty() || value.len() > 1024 {
        return Err(PlaybackCacheError::Invalid(format!(
            "{field} is empty or too long"
        )));
    }
    Ok(())
}

fn validate_cache_id(cache_id: &str) -> Result<(), PlaybackCacheError> {
    if cache_id.len() != 64 || !cache_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PlaybackCacheError::Invalid(
            "cache identity is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn playback_cache_id(track_key: &str, quality: SourceQuality) -> String {
    sha256_hex(format!("{track_key}\u{1f}{}", quality_key(quality)).as_bytes())
}

const fn quality_key(quality: SourceQuality) -> &'static str {
    match quality {
        SourceQuality::K128 => "128k",
        SourceQuality::K320 => "320k",
        SourceQuality::Flac => "flac",
        SourceQuality::Flac24Bit => "flac24bit",
    }
}

fn canonical_content_type(content_type: Option<&str>, url: &str, prefix: &[u8]) -> String {
    let normalized = crate::normalized_media_type(content_type);
    if normalized.starts_with("audio/") {
        return normalized;
    }
    if let Some(extension) = crate::media_extension(content_type, url) {
        return match extension {
            "mp3" => "audio/mpeg",
            "flac" => "audio/flac",
            "m4a" => "audio/mp4",
            "aac" => "audio/aac",
            "ogg" => "audio/ogg",
            _ => "application/octet-stream",
        }
        .to_owned();
    }
    match FileType::from_buffer(prefix) {
        Some(FileType::Flac) => "audio/flac".to_owned(),
        Some(FileType::Mp4) => "audio/mp4".to_owned(),
        Some(FileType::Aac) => "audio/aac".to_owned(),
        Some(FileType::Opus | FileType::Vorbis) => "audio/ogg".to_owned(),
        Some(FileType::Mpeg) => "audio/mpeg".to_owned(),
        _ if prefix.starts_with(b"ID3") => "audio/mpeg".to_owned(),
        _ if !normalized.is_empty() => normalized,
        _ => "application/octet-stream".to_owned(),
    }
}

fn cache_size_bytes(max_size_mb: u32) -> u64 {
    u64::from(max_size_mb).saturating_mul(BYTES_PER_MB)
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

fn remove_file_if_present(path: &Path) -> Result<(), std::io::Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn cache_request(source_url: String) -> CacheOnlinePlaybackRequest {
        CacheOnlinePlaybackRequest {
            track_key: "track-1".to_owned(),
            source_url,
            provider_name: "Test Source".to_owned(),
            candidate: CachedPlaybackCandidate {
                id: "song-1".to_owned(),
                plugin_id: "plugin-1".to_owned(),
                source_id: "wy".to_owned(),
                channel_id: "netease".to_owned(),
                channel_name: "NetEase".to_owned(),
            },
            audio_source_id: "audio-source-1".to_owned(),
            quality: SourceQuality::K320,
        }
    }

    fn spawn_audio_response(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener
            .local_addr()
            .expect("test server should have an address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("test server should accept");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(headers.as_bytes())
                .expect("test headers should write");
            stream.write_all(&body).expect("test body should write");
        });
        (format!("http://{address}/song.mp3"), handle)
    }

    fn write_test_entry(
        cache: &PlaybackCache,
        track_key: &str,
        quality: SourceQuality,
        last_accessed_at: i64,
        file_size_bytes: usize,
    ) -> String {
        let cache_id = playback_cache_id(track_key, quality);
        fs::write(cache.audio_path(&cache_id), vec![0_u8; file_size_bytes])
            .expect("test audio should write");
        let entry = PlaybackCacheEntry {
            version: CACHE_ENTRY_VERSION,
            cache_id: cache_id.clone(),
            track_key: track_key.to_owned(),
            content_type: "audio/mpeg".to_owned(),
            file_size_bytes: file_size_bytes as u64,
            cached_at: last_accessed_at,
            last_accessed_at,
            playback: CachedOnlinePlayback {
                cache_id: cache_id.clone(),
                provider_name: "Test Source".to_owned(),
                candidate: CachedPlaybackCandidate {
                    id: track_key.to_owned(),
                    plugin_id: "plugin-1".to_owned(),
                    source_id: "wy".to_owned(),
                    channel_id: "netease".to_owned(),
                    channel_name: "NetEase".to_owned(),
                },
                audio_source_id: "audio-source-1".to_owned(),
                quality,
            },
        };
        cache
            .write_entry(&entry)
            .expect("test metadata should write");
        cache_id
    }

    #[test]
    fn stored_audio_should_be_found_and_served_without_an_upstream_request() {
        let directory = tempfile::tempdir().expect("cache directory should create");
        let cache = PlaybackCache::new(
            directory.path().join("playback"),
            DEFAULT_PLAYBACK_CACHE_MAX_MB,
        )
        .expect("cache should initialize");
        let body = b"ID3cached-audio".to_vec();
        let (source_url, server) = spawn_audio_response(body.clone());
        let request = cache_request(source_url);
        let cache_id = cache
            .reserve(&request)
            .expect("cache request should validate")
            .expect("cache request should reserve");

        cache
            .store(&request, &cache_id)
            .expect("audio should be cached");
        cache.release(&cache_id);
        server.join().expect("test server should finish");

        let cached = cache
            .lookup("track-1", &[SourceQuality::K320])
            .expect("cache lookup should succeed")
            .expect("cached playback should exist");
        let protocol_request = Request::builder()
            .uri(format!("{PLAYBACK_CACHE_PROTOCOL}://localhost/{cache_id}"))
            .body(Vec::new())
            .expect("protocol request should build");
        let response = cache.protocol_response(protocol_request);

        assert_eq!(cached.cache_id, cache_id);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), &body);
    }

    #[test]
    fn protocol_should_honor_bounded_byte_ranges() {
        let directory = tempfile::tempdir().expect("cache directory should create");
        let cache = PlaybackCache::new(directory.path().join("playback"), 1)
            .expect("cache should initialize");
        let body = b"ID3cached-audio".to_vec();
        let (source_url, server) = spawn_audio_response(body);
        let request = cache_request(source_url);
        let cache_id = cache
            .reserve(&request)
            .expect("cache request should validate")
            .expect("cache request should reserve");
        cache
            .store(&request, &cache_id)
            .expect("audio should be cached");
        cache.release(&cache_id);
        server.join().expect("test server should finish");
        let protocol_request = Request::builder()
            .uri(format!("{PLAYBACK_CACHE_PROTOCOL}://localhost/{cache_id}"))
            .header(RANGE, "bytes=3-8")
            .body(Vec::new())
            .expect("protocol request should build");

        let response = cache.protocol_response(protocol_request);

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(CONTENT_RANGE).unwrap(),
            "bytes 3-8/15"
        );
        assert_eq!(response.body(), b"cached");
    }

    #[test]
    fn zero_size_limit_should_disable_new_cache_entries() {
        let directory = tempfile::tempdir().expect("cache directory should create");
        let cache = PlaybackCache::new(directory.path().join("playback"), 0)
            .expect("cache should initialize");
        let request = cache_request("https://media.test/song.mp3".to_owned());

        let reservation = cache
            .reserve(&request)
            .expect("cache request should validate");

        assert!(reservation.is_none());
    }

    #[test]
    fn prune_should_remove_the_least_recently_used_entry_when_over_limit() {
        let directory = tempfile::tempdir().expect("cache directory should create");
        let cache = PlaybackCache::new(directory.path().join("playback"), 1)
            .expect("cache should initialize");
        let now = now_timestamp();
        let oldest = write_test_entry(
            &cache,
            "old-track",
            SourceQuality::K320,
            now - 2,
            700 * 1024,
        );
        let newest = write_test_entry(
            &cache,
            "new-track",
            SourceQuality::K320,
            now - 1,
            700 * 1024,
        );

        cache.prune().expect("cache should prune");

        assert!(!cache.audio_path(&oldest).exists());
        assert!(cache.audio_path(&newest).exists());
    }
}
