use moka::policy::EvictionPolicy;
use moka::sync::Cache;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

const MAX_DIAGNOSTICS: usize = 200;
const DEFAULT_NETWORK_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CACHE_ENTRIES: u32 = 256;
const MAX_CACHE_VALUE_BYTES: usize = 512 * 1024;
const MAX_CACHE_KEY_BYTES: usize = 4 * 1024;
const MAX_CACHE_WEIGHT_BYTES: u64 = 32 * 1024 * 1024;
const CACHE_ENTRY_MIN_WEIGHT: u32 = MAX_CACHE_WEIGHT_BYTES as u32 / MAX_CACHE_ENTRIES;
const CACHE_TIME_TO_IDLE: Duration = Duration::from_secs(30 * 60);

pub const LX_SOURCE_KIND_MUSIC: &str = "music";
pub const LX_SOURCE_KW: &str = "kw";
pub const LX_SOURCE_KG: &str = "kg";
pub const LX_SOURCE_TX: &str = "tx";
pub const LX_SOURCE_WY: &str = "wy";
pub const LX_SOURCE_MG: &str = "mg";
pub const LX_SOURCE_LOCAL: &str = "local";

pub const SOURCE_RUNTIME_API_VERSION: SourceRuntimeApiVersion = SourceRuntimeApiVersion::new(1, 5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "bindings.ts")]
pub struct SourceRuntimeApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl SourceRuntimeApiVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn is_compatible_with(self, runtime: Self) -> bool {
        self.major == runtime.major && self.minor <= runtime.minor
    }
}

impl fmt::Display for SourceRuntimeApiVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export_to = "bindings.ts")]
pub enum SourceCapability {
    #[serde(rename = "network:any")]
    NetworkAny,
    #[serde(rename = "account:ref")]
    AccountRef,
    #[serde(rename = "playlist:read")]
    PlaylistRead,
    #[serde(rename = "playlist:write")]
    PlaylistWrite,
    #[serde(rename = "metadata:read")]
    MetadataRead,
    #[serde(rename = "cache:read-write")]
    CacheReadWrite,
    #[serde(rename = "bridge:netease-api-enhanced")]
    BridgeNeteaseApiEnhanced,
    #[serde(rename = "bridge:kugou-music-api")]
    BridgeKugouMusicApi,
}

impl SourceCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NetworkAny => "network:any",
            Self::AccountRef => "account:ref",
            Self::PlaylistRead => "playlist:read",
            Self::PlaylistWrite => "playlist:write",
            Self::MetadataRead => "metadata:read",
            Self::CacheReadWrite => "cache:read-write",
            Self::BridgeNeteaseApiEnhanced => "bridge:netease-api-enhanced",
            Self::BridgeKugouMusicApi => "bridge:kugou-music-api",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum DiagnosticLevel {
    Info,
    Warn,
    Error,
    Security,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceDiagnostic {
    pub source_id: String,
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SourceAction {
    MusicSearch,
    ArtistSearch,
    AlbumSearch,
    PlaylistSearch,
    SearchSuggestions,
    ArtistTopTracks,
    ArtistAlbums,
    ArtistBiography,
    AlbumRead,
    PlaylistReadPublic,
    MusicUrl,
    Lyric,
    Pic,
    MusicComments,
    MusicRecommendations,
    PlaylistList,
    PlaylistRead,
    PlaylistAddTrack,
    PlaylistRemoveTrack,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum MusicRecommendationKind {
    #[default]
    Daily,
    Roaming,
    Radar,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ts_rs::TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export_to = "bindings.ts")]
pub enum SourceQuality {
    #[serde(rename = "128k")]
    #[default]
    K128,
    #[serde(rename = "320k")]
    K320,
    Flac,
    #[serde(rename = "flac24bit")]
    Flac24Bit,
}

impl SourceQuality {
    pub fn from_lx_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "128k" => Some(Self::K128),
            "320k" => Some(Self::K320),
            "flac" => Some(Self::Flac),
            "flac24bit" | "24bit" => Some(Self::Flac24Bit),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::K128 => "128k",
            Self::K320 => "320k",
            Self::Flac => "flac",
            Self::Flac24Bit => "flac24bit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SourceKind {
    Music,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "bindings.ts")]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: SourceKind,
    pub actions: Vec<SourceAction>,
    pub qualities: Vec<SourceQuality>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceRuntimeReport {
    pub source_id: String,
    pub initialized: bool,
    pub runtime_api_version: SourceRuntimeApiVersion,
    pub provider_api_version: SourceRuntimeApiVersion,
    pub declared_capabilities: BTreeSet<SourceCapability>,
    pub granted_capabilities: BTreeSet<SourceCapability>,
    pub sources: BTreeMap<String, SourceInfo>,
    pub diagnostics: Vec<SourceDiagnostic>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(tag = "action", rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SourceRequest {
    MusicSearch {
        source: String,
        keyword: String,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    ArtistSearch {
        source: String,
        keyword: String,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    AlbumSearch {
        source: String,
        keyword: String,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    PlaylistSearch {
        source: String,
        keyword: String,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    SearchSuggestions {
        source: String,
        keyword: String,
        limit: u64,
    },
    ArtistTopTracks {
        source: String,
        artist: SourceEntityRef,
        limit: u64,
    },
    ArtistAlbums {
        source: String,
        artist: SourceEntityRef,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    ArtistBiography {
        source: String,
        artist: SourceEntityRef,
    },
    AlbumRead {
        source: String,
        album: SourceEntityRef,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    PlaylistReadPublic {
        source: String,
        playlist: SourceEntityRef,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    MusicUrl {
        source: String,
        #[serde(rename = "musicInfo")]
        #[ts(type = "Record<string, unknown>")]
        music_info: JsonValue,
        #[serde(default)]
        quality: SourceQuality,
    },
    Lyric {
        source: String,
        #[serde(rename = "musicInfo")]
        #[ts(type = "Record<string, unknown>")]
        music_info: JsonValue,
    },
    Pic {
        source: String,
        #[serde(rename = "musicInfo")]
        #[ts(type = "Record<string, unknown>")]
        music_info: JsonValue,
    },
    MusicComments {
        source: String,
        #[serde(rename = "musicInfo")]
        #[ts(type = "Record<string, unknown>")]
        music_info: JsonValue,
        page: u64,
        #[serde(rename = "pageSize")]
        page_size: u64,
    },
    MusicRecommendations {
        source: String,
        #[serde(rename = "accountRef")]
        account_ref: String,
        #[serde(default)]
        kind: MusicRecommendationKind,
        limit: u64,
    },
    PlaylistList {
        source: String,
        #[serde(rename = "accountRef")]
        account_ref: String,
    },
    PlaylistRead {
        source: String,
        #[serde(rename = "accountRef")]
        account_ref: String,
        #[serde(rename = "playlistId")]
        playlist_id: String,
    },
    PlaylistAddTrack {
        source: String,
        #[serde(rename = "accountRef")]
        account_ref: String,
        #[serde(rename = "playlistId")]
        playlist_id: String,
        track: SourceTrackRef,
    },
    PlaylistRemoveTrack {
        source: String,
        #[serde(rename = "accountRef")]
        account_ref: String,
        #[serde(rename = "playlistId")]
        playlist_id: String,
        track: SourceTrackRef,
    },
}

impl SourceRequest {
    pub fn source(&self) -> &str {
        match self {
            Self::MusicSearch { source, .. }
            | Self::ArtistSearch { source, .. }
            | Self::AlbumSearch { source, .. }
            | Self::PlaylistSearch { source, .. }
            | Self::SearchSuggestions { source, .. }
            | Self::ArtistTopTracks { source, .. }
            | Self::ArtistAlbums { source, .. }
            | Self::ArtistBiography { source, .. }
            | Self::AlbumRead { source, .. }
            | Self::PlaylistReadPublic { source, .. }
            | Self::MusicUrl { source, .. }
            | Self::Lyric { source, .. }
            | Self::Pic { source, .. }
            | Self::MusicComments { source, .. }
            | Self::MusicRecommendations { source, .. }
            | Self::PlaylistList { source, .. }
            | Self::PlaylistRead { source, .. }
            | Self::PlaylistAddTrack { source, .. }
            | Self::PlaylistRemoveTrack { source, .. } => source,
        }
    }

    pub const fn action(&self) -> SourceAction {
        match self {
            Self::MusicSearch { .. } => SourceAction::MusicSearch,
            Self::ArtistSearch { .. } => SourceAction::ArtistSearch,
            Self::AlbumSearch { .. } => SourceAction::AlbumSearch,
            Self::PlaylistSearch { .. } => SourceAction::PlaylistSearch,
            Self::SearchSuggestions { .. } => SourceAction::SearchSuggestions,
            Self::ArtistTopTracks { .. } => SourceAction::ArtistTopTracks,
            Self::ArtistAlbums { .. } => SourceAction::ArtistAlbums,
            Self::ArtistBiography { .. } => SourceAction::ArtistBiography,
            Self::AlbumRead { .. } => SourceAction::AlbumRead,
            Self::PlaylistReadPublic { .. } => SourceAction::PlaylistReadPublic,
            Self::MusicUrl { .. } => SourceAction::MusicUrl,
            Self::Lyric { .. } => SourceAction::Lyric,
            Self::Pic { .. } => SourceAction::Pic,
            Self::MusicComments { .. } => SourceAction::MusicComments,
            Self::MusicRecommendations { .. } => SourceAction::MusicRecommendations,
            Self::PlaylistList { .. } => SourceAction::PlaylistList,
            Self::PlaylistRead { .. } => SourceAction::PlaylistRead,
            Self::PlaylistAddTrack { .. } => SourceAction::PlaylistAddTrack,
            Self::PlaylistRemoveTrack { .. } => SourceAction::PlaylistRemoveTrack,
        }
    }

    pub const fn requested_quality(&self) -> Option<SourceQuality> {
        match self {
            Self::MusicUrl { quality, .. } => Some(*quality),
            _ => None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.source().trim().is_empty() {
            return Err("source key must not be empty".to_owned());
        }

        match self {
            Self::MusicSearch {
                keyword,
                page,
                page_size,
                ..
            }
            | Self::ArtistSearch {
                keyword,
                page,
                page_size,
                ..
            }
            | Self::AlbumSearch {
                keyword,
                page,
                page_size,
                ..
            }
            | Self::PlaylistSearch {
                keyword,
                page,
                page_size,
                ..
            } => {
                if keyword.trim().is_empty() {
                    return Err("search keyword must not be empty".to_owned());
                }
                if *page == 0 {
                    return Err("search page must be at least 1".to_owned());
                }
                if !(1..=100).contains(page_size) {
                    return Err("search pageSize must be between 1 and 100".to_owned());
                }
            }
            Self::SearchSuggestions { keyword, limit, .. } => {
                if keyword.trim().is_empty() {
                    return Err("searchSuggestions keyword must not be empty".to_owned());
                }
                if !(1..=20).contains(limit) {
                    return Err("searchSuggestions limit must be between 1 and 20".to_owned());
                }
            }
            Self::ArtistTopTracks { artist, limit, .. } => {
                artist.validate("artist")?;
                if !(1..=50).contains(limit) {
                    return Err("artistTopTracks limit must be between 1 and 50".to_owned());
                }
            }
            Self::ArtistAlbums {
                artist,
                page,
                page_size,
                ..
            } => {
                artist.validate("artist")?;
                validate_detail_page(*page, *page_size, "artistAlbums")?;
            }
            Self::ArtistBiography { artist, .. } => artist.validate("artist")?,
            Self::AlbumRead {
                album,
                page,
                page_size,
                ..
            } => {
                album.validate("album")?;
                validate_detail_page(*page, *page_size, "albumRead")?;
            }
            Self::PlaylistReadPublic {
                playlist,
                page,
                page_size,
                ..
            } => {
                playlist.validate("playlist")?;
                validate_detail_page(*page, *page_size, "playlistReadPublic")?;
            }
            Self::MusicUrl { music_info, .. }
            | Self::Lyric { music_info, .. }
            | Self::Pic { music_info, .. } => {
                if !music_info.is_object() {
                    return Err("musicInfo must be a JSON object".to_owned());
                }
            }
            Self::MusicComments {
                music_info,
                page,
                page_size,
                ..
            } => {
                if !music_info.is_object() {
                    return Err("musicComments musicInfo must be a JSON object".to_owned());
                }
                if *page == 0 {
                    return Err("musicComments page must be at least 1".to_owned());
                }
                if !(1..=50).contains(page_size) {
                    return Err("musicComments pageSize must be between 1 and 50".to_owned());
                }
            }
            Self::MusicRecommendations {
                account_ref, limit, ..
            } => {
                validate_account_ref(account_ref)?;
                if !(1..=100).contains(limit) {
                    return Err("musicRecommendations limit must be between 1 and 100".to_owned());
                }
            }
            Self::PlaylistList { account_ref, .. } => validate_account_ref(account_ref)?,
            Self::PlaylistRead {
                account_ref,
                playlist_id,
                ..
            } => {
                validate_account_ref(account_ref)?;
                validate_playlist_id(playlist_id)?;
            }
            Self::PlaylistAddTrack {
                account_ref,
                playlist_id,
                track,
                ..
            }
            | Self::PlaylistRemoveTrack {
                account_ref,
                playlist_id,
                track,
                ..
            } => {
                validate_account_ref(account_ref)?;
                validate_playlist_id(playlist_id)?;
                track.validate()?;
            }
        }

        Ok(())
    }
}

fn validate_detail_page(page: u64, page_size: u64, action: &str) -> Result<(), String> {
    if page == 0 {
        return Err(format!("{action} page must be at least 1"));
    }
    if !(1..=200).contains(&page_size) {
        return Err(format!("{action} pageSize must be between 1 and 200"));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceEntityRef {
    pub id: String,
    #[serde(default)]
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
}

impl SourceEntityRef {
    fn validate(&self, kind: &str) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err(format!("{kind} id must not be empty"));
        }
        validate_platform_ids(&self.platform_ids)?;
        if !self.raw_info.is_object() {
            return Err(format!("{kind} rawInfo must be a JSON object"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(untagged)]
#[ts(export_to = "bindings.ts")]
pub enum JsonScalar {
    String(String),
    Number(i64),
}

fn validate_platform_ids(platform_ids: &BTreeMap<String, JsonScalar>) -> Result<(), String> {
    const MAX_PLATFORM_IDS: usize = 16;
    const MAX_PLATFORM_IDS_BYTES: usize = 2 * 1024;
    if platform_ids.len() > MAX_PLATFORM_IDS {
        return Err(format!(
            "platformIds may contain at most {MAX_PLATFORM_IDS} entries"
        ));
    }
    if platform_ids.keys().any(|key| {
        key.is_empty()
            || key.len() > 64
            || !key.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
    }) {
        return Err("platformIds contains an invalid key".to_owned());
    }
    let serialized = serde_json::to_vec(platform_ids)
        .map_err(|error| format!("platformIds could not be serialized: {error}"))?;
    if serialized.len() > MAX_PLATFORM_IDS_BYTES {
        return Err(format!(
            "platformIds must not exceed {MAX_PLATFORM_IDS_BYTES} bytes"
        ));
    }
    Ok(())
}

fn validate_account_ref(account_ref: &str) -> Result<(), String> {
    if account_ref.trim().is_empty() {
        Err("accountRef must not be empty".to_owned())
    } else {
        Ok(())
    }
}

fn validate_playlist_id(playlist_id: &str) -> Result<(), String> {
    if playlist_id.trim().is_empty() {
        Err("playlistId must not be empty".to_owned())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceTrackRef {
    pub id: String,
    pub source: String,
    #[serde(default)]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
}

impl SourceTrackRef {
    fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("track id must not be empty".to_owned());
        }
        if self.source.trim().is_empty() {
            return Err("track source must not be empty".to_owned());
        }
        if self
            .title
            .as_deref()
            .is_some_and(|title| title.trim().is_empty())
        {
            return Err("track title must not be empty when provided".to_owned());
        }
        validate_platform_ids(&self.platform_ids)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct LyricResponse {
    pub lyric: Option<String>,
    pub tlyric: Option<String>,
    pub rlyric: Option<String>,
    pub lxlyric: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceComment {
    pub id: String,
    pub user_name: String,
    pub avatar_url: Option<String>,
    pub content: String,
    pub timestamp_ms: Option<u64>,
    pub time_label: Option<String>,
    pub liked_count: u64,
    pub reply_count: u64,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceCommentsResponse {
    pub hot_comments: Vec<SourceComment>,
    pub comments: Vec<SourceComment>,
    pub total: Option<u64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceSearchResult {
    pub id: String,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_seconds: Option<u64>,
    pub cover_url: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub track_number: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub disc_number: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceSearchResponse {
    pub is_end: bool,
    pub total: Option<u64>,
    pub list: Vec<SourceSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceArtistSearchResult {
    pub id: String,
    pub source: String,
    pub name: String,
    pub cover_url: Option<String>,
    #[serde(default)]
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceArtistSearchResponse {
    pub is_end: bool,
    pub total: Option<u64>,
    pub list: Vec<SourceArtistSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceArtistBiographySection {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceArtistBiography {
    pub summary: Option<String>,
    pub sections: Vec<SourceArtistBiographySection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceAlbumSearchResult {
    pub id: String,
    pub source: String,
    pub title: String,
    pub artist: String,
    pub release_year: Option<u32>,
    pub cover_url: Option<String>,
    pub track_count: Option<u64>,
    #[serde(default)]
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceAlbumSearchResponse {
    pub is_end: bool,
    pub total: Option<u64>,
    pub list: Vec<SourceAlbumSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourcePlaylistSearchResult {
    pub id: String,
    pub source: String,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub track_count: Option<u64>,
    pub owner_name: Option<String>,
    #[serde(default)]
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[serde(default)]
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourcePlaylistSearchResponse {
    pub is_end: bool,
    pub total: Option<u64>,
    pub list: Vec<SourcePlaylistSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceSuggestionsResponse {
    pub list: Vec<String>,
}

pub type RemoteTrack = SourceSearchResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceRecommendationsResponse {
    pub list: Vec<RemoteTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourcePlaylist {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mutation_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub track_count: u64,
    pub owner_name: String,
    pub can_mutate: bool,
    #[serde(default)]
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourcePlaylistDetail {
    pub playlist: SourcePlaylist,
    pub tracks: Vec<RemoteTrack>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SourcePlaylistMutationKind {
    Add,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourcePlaylistMutation {
    pub audit_id: i64,
    pub operation: SourcePlaylistMutationKind,
    pub playlist_id: String,
    pub track_id: String,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(tag = "action", content = "data", rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SourceResponse {
    MusicSearch(SourceSearchResponse),
    ArtistSearch(SourceArtistSearchResponse),
    AlbumSearch(SourceAlbumSearchResponse),
    PlaylistSearch(SourcePlaylistSearchResponse),
    SearchSuggestions(SourceSuggestionsResponse),
    ArtistTopTracks(SourceSearchResponse),
    ArtistAlbums(SourceAlbumSearchResponse),
    ArtistBiography(SourceArtistBiography),
    AlbumRead(SourceSearchResponse),
    PlaylistReadPublic(SourceSearchResponse),
    MusicUrl(String),
    Lyric(LyricResponse),
    Pic(String),
    MusicComments(SourceCommentsResponse),
    MusicRecommendations(SourceRecommendationsResponse),
    PlaylistList(Vec<SourcePlaylist>),
    PlaylistRead(SourcePlaylistDetail),
    PlaylistAddTrack(SourcePlaylistMutation),
    PlaylistRemoveTrack(SourcePlaylistMutation),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SourceRequestOutcome {
    pub response: SourceResponse,
    pub diagnostics: Vec<SourceDiagnostic>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl SourceCancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SourceAccountRef(String);

impl SourceAccountRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl SourceHttpMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHttpRequest {
    pub method: SourceHttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub json_body: Option<JsonValue>,
    pub timeout: Option<Duration>,
}

impl SourceHttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: SourceHttpMethod::Get,
            url: url.into(),
            headers: BTreeMap::new(),
            body: None,
            json_body: None,
            timeout: None,
        }
    }

    pub fn post_json(url: impl Into<String>, body: JsonValue) -> Self {
        Self {
            method: SourceHttpMethod::Post,
            url: url.into(),
            headers: BTreeMap::new(),
            body: None,
            json_body: Some(body),
            timeout: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHttpResponse {
    pub status: u16,
    pub final_url: String,
    pub headers: BTreeMap<String, String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl SourceHttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> Result<String, SourceHostError> {
        String::from_utf8(self.body.clone()).map_err(|error| SourceHostError::InvalidResponse {
            message: error.to_string(),
        })
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, SourceHostError> {
        serde_json::from_slice(&self.body).map_err(|error| SourceHostError::InvalidResponse {
            message: error.to_string(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceHostError {
    #[error("host operation was cancelled")]
    Cancelled,
    #[error("network request timed out for {url}")]
    Timeout { url: String },
    #[error("network request failed for {url}: {message}")]
    Network { url: String, message: String },
    #[error("unsupported network URL: {url}")]
    InvalidUrl { url: String },
    #[error("host response exceeded the {max_bytes} byte limit")]
    ResponseTooLarge { max_bytes: usize },
    #[error("invalid host response: {message}")]
    InvalidResponse { message: String },
    #[error("host cache failed: {message}")]
    Cache { message: String },
    #[error("host account service failed: {message}")]
    Account { message: String },
    #[error("account reference is not available")]
    InvalidAccountRef,
    #[error("host service is unavailable: {service}")]
    Unavailable { service: &'static str },
}

impl SourceHostError {
    fn diagnostic_message(&self) -> String {
        match self {
            Self::Timeout { url } => {
                format!(
                    "network request timed out for {}",
                    network_diagnostic_target(url)
                )
            }
            Self::Network { url, message } => {
                let target = network_diagnostic_target(url);
                format!(
                    "network request failed for {target}: {}",
                    message.replace(url, &target)
                )
            }
            Self::InvalidUrl { url } => {
                format!(
                    "unsupported network URL: {}",
                    network_diagnostic_target(url)
                )
            }
            _ => self.to_string(),
        }
    }
}

fn network_diagnostic_target(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return "<invalid URL>".to_owned();
    };
    let host = parsed.host_str().unwrap_or("<unknown>");
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    format!("{}://{}{port}", parsed.scheme(), host)
}

pub trait SourceHost: Send + Sync {
    fn http_request(
        &self,
        source_id: &str,
        request: &SourceHttpRequest,
        cancellation: &SourceCancellationToken,
    ) -> Result<SourceHttpResponse, SourceHostError>;

    fn cache_read(
        &self,
        _source_id: &str,
        _key: &str,
        _cancellation: &SourceCancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceHostError> {
        Err(SourceHostError::Unavailable { service: "cache" })
    }

    fn cache_write(
        &self,
        _source_id: &str,
        _key: &str,
        _value: &[u8],
        _cancellation: &SourceCancellationToken,
    ) -> Result<(), SourceHostError> {
        Err(SourceHostError::Unavailable { service: "cache" })
    }

    /// Resolves a provider-supplied lookup key to a host-issued opaque reference.
    fn resolve_account_ref(
        &self,
        _source_id: &str,
        _requested_ref: &str,
        _cancellation: &SourceCancellationToken,
    ) -> Result<String, SourceHostError> {
        Err(SourceHostError::Unavailable {
            service: "account refs",
        })
    }
}

type SourceCache = Cache<(String, String), Vec<u8>>;
type SourceAccountRefs = Arc<RwLock<BTreeMap<(String, String), String>>>;

#[derive(Clone)]
pub struct DefaultSourceHost {
    http_client: reqwest::blocking::Client,
    network_timeout: Duration,
    max_response_bytes: usize,
    cache: SourceCache,
    account_refs: SourceAccountRefs,
}

impl fmt::Debug for DefaultSourceHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DefaultSourceHost")
            .field("network_timeout", &self.network_timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl DefaultSourceHost {
    pub fn new(network_timeout: Duration, max_response_bytes: usize) -> Self {
        Self::with_client(
            reqwest::blocking::Client::new(),
            network_timeout,
            max_response_bytes,
        )
    }

    pub fn with_client(
        http_client: reqwest::blocking::Client,
        network_timeout: Duration,
        max_response_bytes: usize,
    ) -> Self {
        Self {
            http_client,
            network_timeout,
            max_response_bytes,
            cache: Cache::builder()
                .max_capacity(MAX_CACHE_WEIGHT_BYTES)
                .weigher(|(source_id, key): &(String, String), value: &Vec<u8>| {
                    let bytes = source_id
                        .len()
                        .saturating_add(key.len())
                        .saturating_add(value.len());
                    u32::try_from(bytes)
                        .unwrap_or(u32::MAX)
                        .max(CACHE_ENTRY_MIN_WEIGHT)
                })
                .eviction_policy(EvictionPolicy::lru())
                .time_to_idle(CACHE_TIME_TO_IDLE)
                .build(),
            account_refs: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Registers an opaque account reference for one Provider.
    pub fn register_account_ref(
        &self,
        source_id: impl Into<String>,
        requested_ref: impl Into<String>,
        opaque_ref: impl Into<String>,
    ) -> Result<(), SourceHostError> {
        let source_id = source_id.into();
        let requested_ref = requested_ref.into();
        let opaque_ref = opaque_ref.into();
        if source_id.trim().is_empty()
            || requested_ref.trim().is_empty()
            || opaque_ref.trim().is_empty()
        {
            return Err(SourceHostError::InvalidAccountRef);
        }

        self.account_refs
            .write()
            .map_err(|_| SourceHostError::Account {
                message: "account reference lock was poisoned".to_owned(),
            })?
            .insert((source_id, requested_ref), opaque_ref);
        Ok(())
    }

    /// Revokes an opaque account reference for one Provider.
    pub fn revoke_account_ref(
        &self,
        source_id: &str,
        requested_ref: &str,
    ) -> Result<bool, SourceHostError> {
        self.account_refs
            .write()
            .map_err(|_| SourceHostError::Account {
                message: "account reference lock was poisoned".to_owned(),
            })
            .map(|mut refs| {
                refs.remove(&(source_id.to_owned(), requested_ref.to_owned()))
                    .is_some()
            })
    }
}

impl SourceHost for DefaultSourceHost {
    fn http_request(
        &self,
        _source_id: &str,
        request: &SourceHttpRequest,
        cancellation: &SourceCancellationToken,
    ) -> Result<SourceHttpResponse, SourceHostError> {
        if cancellation.is_cancelled() {
            return Err(SourceHostError::Cancelled);
        }
        let parsed_url =
            reqwest::Url::parse(&request.url).map_err(|_| SourceHostError::InvalidUrl {
                url: network_diagnostic_target(&request.url),
            })?;
        if !matches!(parsed_url.scheme(), "http" | "https") || parsed_url.host_str().is_none() {
            return Err(SourceHostError::InvalidUrl {
                url: network_diagnostic_target(&request.url),
            });
        }

        let diagnostic_target = network_diagnostic_target(&request.url);

        let mut request_builder = match request.method {
            SourceHttpMethod::Get => self.http_client.get(parsed_url.clone()),
            SourceHttpMethod::Post => self.http_client.post(parsed_url),
            SourceHttpMethod::Put => self.http_client.put(parsed_url),
            SourceHttpMethod::Patch => self.http_client.patch(parsed_url),
            SourceHttpMethod::Delete => self.http_client.delete(parsed_url),
            SourceHttpMethod::Head => self.http_client.head(parsed_url),
        }
        .timeout(
            request
                .timeout
                .unwrap_or(self.network_timeout)
                .min(self.network_timeout),
        );
        if !request
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("user-agent"))
        {
            request_builder =
                request_builder.header(reqwest::header::USER_AGENT, "FikaMusic/0.1 source-runtime");
        }
        for (name, value) in &request.headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                SourceHostError::InvalidResponse {
                    message: "network request contains an invalid header name".to_owned(),
                }
            })?;
            let value = reqwest::header::HeaderValue::from_str(value).map_err(|_| {
                SourceHostError::InvalidResponse {
                    message: "network request contains an invalid header value".to_owned(),
                }
            })?;
            request_builder = request_builder.header(name, value);
        }
        if let Some(body) = &request.json_body {
            request_builder = request_builder.json(body);
        } else if let Some(body) = &request.body {
            request_builder = request_builder.body(body.clone());
        }

        let mut response = request_builder.send().map_err(|error| {
            if error.is_timeout() {
                SourceHostError::Timeout {
                    url: diagnostic_target.clone(),
                }
            } else {
                SourceHostError::Network {
                    url: diagnostic_target.clone(),
                    message: error.to_string().replace(&request.url, &diagnostic_target),
                }
            }
        })?;
        if cancellation.is_cancelled() {
            return Err(SourceHostError::Cancelled);
        }

        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let mut body = Vec::new();
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            if cancellation.is_cancelled() {
                return Err(SourceHostError::Cancelled);
            }
            let bytes_read =
                response
                    .read(&mut chunk)
                    .map_err(|error| SourceHostError::Network {
                        url: diagnostic_target.clone(),
                        message: error.to_string().replace(&request.url, &diagnostic_target),
                    })?;
            if bytes_read == 0 {
                break;
            }
            if body.len().saturating_add(bytes_read) > self.max_response_bytes {
                return Err(SourceHostError::ResponseTooLarge {
                    max_bytes: self.max_response_bytes,
                });
            }
            body.extend_from_slice(&chunk[..bytes_read]);
        }

        Ok(SourceHttpResponse {
            status,
            final_url,
            headers,
            content_type,
            body,
        })
    }

    fn cache_read(
        &self,
        source_id: &str,
        key: &str,
        cancellation: &SourceCancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceHostError> {
        if cancellation.is_cancelled() {
            return Err(SourceHostError::Cancelled);
        }
        if key.len() > MAX_CACHE_KEY_BYTES {
            return Err(SourceHostError::Cache {
                message: format!("cache key exceeds the {} byte limit", MAX_CACHE_KEY_BYTES),
            });
        }
        Ok(self.cache.get(&(source_id.to_owned(), key.to_owned())))
    }

    fn cache_write(
        &self,
        source_id: &str,
        key: &str,
        value: &[u8],
        cancellation: &SourceCancellationToken,
    ) -> Result<(), SourceHostError> {
        if cancellation.is_cancelled() {
            return Err(SourceHostError::Cancelled);
        }
        if value.len() > MAX_CACHE_VALUE_BYTES {
            return Err(SourceHostError::Cache {
                message: format!(
                    "cache value exceeds the {} byte limit",
                    MAX_CACHE_VALUE_BYTES
                ),
            });
        }
        if key.len() > MAX_CACHE_KEY_BYTES {
            return Err(SourceHostError::Cache {
                message: format!("cache key exceeds the {} byte limit", MAX_CACHE_KEY_BYTES),
            });
        }
        self.cache
            .insert((source_id.to_owned(), key.to_owned()), value.to_vec());
        Ok(())
    }

    fn resolve_account_ref(
        &self,
        source_id: &str,
        requested_ref: &str,
        cancellation: &SourceCancellationToken,
    ) -> Result<String, SourceHostError> {
        if cancellation.is_cancelled() {
            return Err(SourceHostError::Cancelled);
        }
        self.account_refs
            .read()
            .map_err(|_| SourceHostError::Account {
                message: "account reference lock was poisoned".to_owned(),
            })?
            .get(&(source_id.to_owned(), requested_ref.to_owned()))
            .cloned()
            .ok_or(SourceHostError::InvalidAccountRef)
    }
}

pub struct SourceRuntimeContext {
    source_id: String,
    declared_capabilities: BTreeSet<SourceCapability>,
    granted_capabilities: BTreeSet<SourceCapability>,
    host: Arc<dyn SourceHost>,
    cancellation: SourceCancellationToken,
    diagnostics: Vec<SourceDiagnostic>,
}

impl fmt::Debug for SourceRuntimeContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceRuntimeContext")
            .field("source_id", &self.source_id)
            .field("declared_capabilities", &self.declared_capabilities)
            .field("granted_capabilities", &self.granted_capabilities)
            .field("cancelled", &self.cancellation.is_cancelled())
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

impl SourceRuntimeContext {
    fn new(
        source_id: impl Into<String>,
        declared_capabilities: BTreeSet<SourceCapability>,
        granted_capabilities: BTreeSet<SourceCapability>,
        host: Arc<dyn SourceHost>,
        cancellation: SourceCancellationToken,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            declared_capabilities,
            granted_capabilities,
            host,
            cancellation,
            diagnostics: Vec::new(),
        }
    }

    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    pub fn declared_capabilities(&self) -> &BTreeSet<SourceCapability> {
        &self.declared_capabilities
    }

    pub fn granted_capabilities(&self) -> &BTreeSet<SourceCapability> {
        &self.granted_capabilities
    }

    pub fn has_capability(&self, capability: SourceCapability) -> bool {
        self.granted_capabilities.contains(&capability)
    }

    pub(crate) fn cancellation_token(&self) -> SourceCancellationToken {
        self.cancellation.clone()
    }

    pub(crate) fn fork_for_host_calls(&self) -> Self {
        Self {
            source_id: self.source_id.clone(),
            declared_capabilities: self.declared_capabilities.clone(),
            granted_capabilities: self.granted_capabilities.clone(),
            host: Arc::clone(&self.host),
            cancellation: self.cancellation.clone(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn append_nested_diagnostics(&mut self, diagnostics: &[SourceDiagnostic]) {
        for diagnostic in diagnostics {
            self.push_diagnostic(diagnostic.level, diagnostic.message.clone());
        }
    }

    pub fn require_capability(
        &mut self,
        capability: SourceCapability,
        operation: &str,
    ) -> Result<(), SourceRuntimeError> {
        if self.has_capability(capability) {
            return Ok(());
        }

        let reason = if self.declared_capabilities.contains(&capability) {
            "host permission was not granted"
        } else {
            "provider did not declare the capability"
        };
        let message = format!(
            "denied {operation}: missing {} ({reason})",
            capability.as_str()
        );
        self.push_diagnostic(DiagnosticLevel::Security, message);
        Err(SourceRuntimeError::CapabilityDenied {
            source_id: self.source_id.clone(),
            capability,
            operation: operation.to_owned(),
            diagnostics: self.diagnostics.clone(),
        })
    }

    pub fn ensure_not_cancelled(&mut self, operation: &str) -> Result<(), SourceRuntimeError> {
        if !self.cancellation.is_cancelled() {
            return Ok(());
        }

        self.push_diagnostic(DiagnosticLevel::Warn, format!("cancelled {operation}"));
        Err(SourceRuntimeError::Cancelled {
            source_id: self.source_id.clone(),
            operation: operation.to_owned(),
            diagnostics: self.diagnostics.clone(),
        })
    }

    pub fn http_request(
        &mut self,
        request: SourceHttpRequest,
        operation: &str,
    ) -> Result<SourceHttpResponse, SourceRuntimeError> {
        self.require_capability(SourceCapability::NetworkAny, operation)?;
        self.ensure_not_cancelled(operation)?;
        let target = network_diagnostic_target(&request.url);
        self.info(format!(
            "host network started {operation} ({}, target {target})",
            request.method.as_str(),
        ));

        match self
            .host
            .http_request(&self.source_id, &request, &self.cancellation)
        {
            Ok(response) => {
                let final_target = network_diagnostic_target(&response.final_url);
                self.info(format!(
                    "host network completed {operation} with HTTP {} (target {final_target})",
                    response.status,
                ));
                Ok(response)
            }
            Err(SourceHostError::Cancelled) => {
                self.cancellation.cancel();
                self.push_diagnostic(DiagnosticLevel::Warn, format!("cancelled {operation}"));
                Err(SourceRuntimeError::Cancelled {
                    source_id: self.source_id.clone(),
                    operation: operation.to_owned(),
                    diagnostics: self.diagnostics.clone(),
                })
            }
            Err(error) => {
                let message = format!(
                    "host network failed {operation}: {}",
                    error.diagnostic_message()
                );
                self.push_diagnostic(DiagnosticLevel::Error, message.clone());
                Err(SourceRuntimeError::Host {
                    source_id: self.source_id.clone(),
                    operation: operation.to_owned(),
                    message,
                    diagnostics: self.diagnostics.clone(),
                })
            }
        }
    }

    pub fn cache_read(
        &mut self,
        key: &str,
        operation: &str,
    ) -> Result<Option<Vec<u8>>, SourceRuntimeError> {
        self.require_capability(SourceCapability::CacheReadWrite, operation)?;
        self.ensure_not_cancelled(operation)?;
        match self
            .host
            .cache_read(&self.source_id, key, &self.cancellation)
        {
            Ok(value) => Ok(value),
            Err(error) => Err(self.host_error(operation, error)),
        }
    }

    pub fn cache_write(
        &mut self,
        key: &str,
        value: &[u8],
        operation: &str,
    ) -> Result<(), SourceRuntimeError> {
        self.require_capability(SourceCapability::CacheReadWrite, operation)?;
        self.ensure_not_cancelled(operation)?;
        match self
            .host
            .cache_write(&self.source_id, key, value, &self.cancellation)
        {
            Ok(()) => Ok(()),
            Err(error) => Err(self.host_error(operation, error)),
        }
    }

    pub fn account_ref(
        &mut self,
        requested_ref: &str,
        operation: &str,
    ) -> Result<SourceAccountRef, SourceRuntimeError> {
        self.require_capability(SourceCapability::AccountRef, operation)?;
        self.ensure_not_cancelled(operation)?;
        if requested_ref.trim().is_empty() {
            return Err(self.provider_error("account ref request must not be empty"));
        }

        let resolved_ref = self
            .host
            .resolve_account_ref(&self.source_id, requested_ref, &self.cancellation)
            .map_err(|error| self.host_error(operation, error))?;
        if resolved_ref.trim().is_empty() {
            return Err(self.provider_error("host returned an empty account ref"));
        }
        self.info(format!("host resolved opaque account ref for {operation}"));
        Ok(SourceAccountRef(resolved_ref))
    }

    pub fn provider_error(&mut self, message: impl Into<String>) -> SourceRuntimeError {
        self.provider_error_with_code("provider-failure", message)
    }

    pub fn provider_error_with_code(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> SourceRuntimeError {
        let code = code.into();
        let message = message.into();
        self.push_diagnostic(DiagnosticLevel::Error, message.clone());
        SourceRuntimeError::Provider {
            source_id: self.source_id.clone(),
            code,
            message,
            diagnostics: self.diagnostics.clone(),
        }
    }

    pub fn unsupported_action(
        &mut self,
        source_key: impl Into<String>,
        action: SourceAction,
    ) -> SourceRuntimeError {
        let source_key = source_key.into();
        self.push_diagnostic(
            DiagnosticLevel::Error,
            format!("source {source_key} does not support {action:?}"),
        );
        SourceRuntimeError::UnsupportedAction {
            source_id: self.source_id.clone(),
            source_key,
            action,
            diagnostics: self.diagnostics.clone(),
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.push_diagnostic(DiagnosticLevel::Info, message);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.push_diagnostic(DiagnosticLevel::Warn, message);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push_diagnostic(DiagnosticLevel::Error, message);
    }

    pub fn diagnostics(&self) -> &[SourceDiagnostic] {
        &self.diagnostics
    }

    fn host_error(&mut self, operation: &str, error: SourceHostError) -> SourceRuntimeError {
        if matches!(error, SourceHostError::Cancelled) {
            self.cancellation.cancel();
            self.push_diagnostic(DiagnosticLevel::Warn, format!("cancelled {operation}"));
            return SourceRuntimeError::Cancelled {
                source_id: self.source_id.clone(),
                operation: operation.to_owned(),
                diagnostics: self.diagnostics.clone(),
            };
        }

        let message = format!("host operation failed {operation}: {error}");
        self.push_diagnostic(DiagnosticLevel::Error, message.clone());
        SourceRuntimeError::Host {
            source_id: self.source_id.clone(),
            operation: operation.to_owned(),
            message,
            diagnostics: self.diagnostics.clone(),
        }
    }

    fn into_diagnostics(self) -> Vec<SourceDiagnostic> {
        self.diagnostics
    }

    fn push_diagnostic(&mut self, level: DiagnosticLevel, message: impl Into<String>) {
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.remove(0);
        }

        self.diagnostics.push(SourceDiagnostic {
            source_id: self.source_id.clone(),
            level,
            message: message.into(),
        });
    }
}

/// Host-executed implementation behind a registered Plugin Provider entrypoint.
///
/// Implementations are compiled into Fika Music and registered through
/// [`crate::plugin_system::PluginProviderCatalog`]. The runtime catches panics,
/// validates catalogs, requests, and responses, and exposes only capability-
/// checked operations through [`SourceRuntimeContext`].
///
/// Implementations must return stable metadata: `id`, `api_version`, and
/// `required_capabilities` are read during initialization and dispatch. The ID
/// and capabilities must match the corresponding Plugin manifest entrypoint.
pub trait SourceProvider: Send + Sync {
    /// Returns the globally unique Provider ID declared by `plugin.json`.
    fn id(&self) -> &str;

    /// Returns the oldest Source Runtime contract required by this Provider.
    fn api_version(&self) -> SourceRuntimeApiVersion {
        SOURCE_RUNTIME_API_VERSION
    }

    /// Returns every host capability the Provider may request at runtime.
    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        BTreeSet::new()
    }

    /// Initializes the Provider and returns its complete source/action catalog.
    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError>;

    /// Handles one validated request and returns the matching response variant.
    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError>;
}

#[derive(Debug, Clone)]
pub struct SourceRuntimeConfig {
    api_version: SourceRuntimeApiVersion,
    granted_capabilities: BTreeSet<SourceCapability>,
    provider_granted_capabilities: BTreeMap<String, BTreeSet<SourceCapability>>,
    network_timeout: Duration,
    max_response_bytes: usize,
}

impl Default for SourceRuntimeConfig {
    fn default() -> Self {
        Self {
            api_version: SOURCE_RUNTIME_API_VERSION,
            granted_capabilities: BTreeSet::new(),
            provider_granted_capabilities: BTreeMap::new(),
            network_timeout: DEFAULT_NETWORK_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

impl SourceRuntimeConfig {
    pub fn with_api_version(mut self, api_version: SourceRuntimeApiVersion) -> Self {
        self.api_version = api_version;
        self
    }

    pub fn with_granted_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Self {
        self.granted_capabilities = capabilities.into_iter().collect();
        self
    }

    pub fn with_provider_granted_capabilities(
        mut self,
        provider_id: impl Into<String>,
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Self {
        self.provider_granted_capabilities
            .insert(provider_id.into(), capabilities.into_iter().collect());
        self
    }

    pub fn with_network_limits(mut self, timeout: Duration, max_response_bytes: usize) -> Self {
        self.network_timeout = timeout;
        self.max_response_bytes = max_response_bytes;
        self
    }
}

pub struct SourceRuntime {
    config: SourceRuntimeConfig,
    host: Arc<dyn SourceHost>,
    granted_capabilities: RwLock<BTreeSet<SourceCapability>>,
    provider_granted_capabilities: RwLock<BTreeMap<String, BTreeSet<SourceCapability>>>,
    initialized_sources: Arc<RwLock<BTreeMap<String, BTreeMap<String, SourceInfo>>>>,
}

impl fmt::Debug for SourceRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceRuntime")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Default for SourceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceRuntime {
    pub fn new() -> Self {
        Self::with_config(SourceRuntimeConfig::default())
    }

    pub fn with_config(config: SourceRuntimeConfig) -> Self {
        let host = Arc::new(DefaultSourceHost::new(
            config.network_timeout,
            config.max_response_bytes,
        ));
        Self::with_config_and_host(config, host)
    }

    pub fn with_granted_capabilities(
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Self {
        Self::with_config(SourceRuntimeConfig::default().with_granted_capabilities(capabilities))
    }

    pub fn with_provider_granted_capabilities(
        provider_id: impl Into<String>,
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Self {
        Self::with_config(
            SourceRuntimeConfig::default()
                .with_provider_granted_capabilities(provider_id, capabilities),
        )
    }

    pub fn with_host(
        host: Arc<dyn SourceHost>,
        granted_capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Self {
        let config = SourceRuntimeConfig::default().with_granted_capabilities(granted_capabilities);
        Self::with_config_and_host(config, host)
    }

    pub fn with_config_and_host(config: SourceRuntimeConfig, host: Arc<dyn SourceHost>) -> Self {
        let granted_capabilities = config.granted_capabilities.clone();
        let provider_granted_capabilities = config.provider_granted_capabilities.clone();
        Self {
            config,
            host,
            granted_capabilities: RwLock::new(granted_capabilities),
            provider_granted_capabilities: RwLock::new(provider_granted_capabilities),
            initialized_sources: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn api_version(&self) -> SourceRuntimeApiVersion {
        self.config.api_version
    }

    pub fn granted_capabilities(&self) -> Result<BTreeSet<SourceCapability>, SourceRuntimeError> {
        self.granted_capabilities
            .read()
            .map_err(|_| runtime_state_error("runtime", "capability grant lock was poisoned"))
            .map(|capabilities| capabilities.clone())
    }

    pub fn replace_granted_capabilities(
        &self,
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Result<(), SourceRuntimeError> {
        let mut granted = self
            .granted_capabilities
            .write()
            .map_err(|_| runtime_state_error("runtime", "capability grant lock was poisoned"))?;
        *granted = capabilities.into_iter().collect();
        Ok(())
    }

    pub fn granted_capabilities_for(
        &self,
        provider_id: &str,
    ) -> Result<BTreeSet<SourceCapability>, SourceRuntimeError> {
        let provider_grants = self
            .provider_granted_capabilities
            .read()
            .map_err(|_| runtime_state_error(provider_id, "capability grant lock was poisoned"))?;
        if let Some(capabilities) = provider_grants.get(provider_id) {
            return Ok(capabilities.clone());
        }
        self.granted_capabilities()
    }

    pub fn replace_provider_granted_capabilities(
        &self,
        provider_id: impl Into<String>,
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Result<(), SourceRuntimeError> {
        let provider_id = provider_id.into();
        self.provider_granted_capabilities
            .write()
            .map_err(|_| runtime_state_error(&provider_id, "capability grant lock was poisoned"))?
            .insert(provider_id, capabilities.into_iter().collect());
        Ok(())
    }

    /// Installs default grants only when no provider-specific policy exists.
    /// Existing entries, including explicit empty grants, remain unchanged.
    pub fn ensure_provider_granted_capabilities(
        &self,
        provider_id: impl Into<String>,
        capabilities: impl IntoIterator<Item = SourceCapability>,
    ) -> Result<bool, SourceRuntimeError> {
        let provider_id = provider_id.into();
        let mut grants = self
            .provider_granted_capabilities
            .write()
            .map_err(|_| runtime_state_error(&provider_id, "capability grant lock was poisoned"))?;
        if grants.contains_key(&provider_id) {
            return Ok(false);
        }
        grants.insert(provider_id, capabilities.into_iter().collect());
        Ok(true)
    }

    pub fn revoke_capability(
        &self,
        capability: SourceCapability,
    ) -> Result<bool, SourceRuntimeError> {
        self.granted_capabilities
            .write()
            .map_err(|_| runtime_state_error("runtime", "capability grant lock was poisoned"))
            .map(|mut capabilities| capabilities.remove(&capability))
    }

    pub fn revoke_provider_capability(
        &self,
        provider_id: &str,
        capability: SourceCapability,
    ) -> Result<bool, SourceRuntimeError> {
        let mut grants = self
            .provider_granted_capabilities
            .write()
            .map_err(|_| runtime_state_error(provider_id, "capability grant lock was poisoned"))?;
        // A Provider-specific revoke derives a replacement policy from its current grants,
        // preserving unrelated global permissions.
        if let Some(provider_grants) = grants.get_mut(provider_id) {
            return Ok(provider_grants.remove(&capability));
        }

        let mut provider_grants = self
            .granted_capabilities
            .read()
            .map_err(|_| runtime_state_error(provider_id, "capability grant lock was poisoned"))?
            .clone();
        let was_granted = provider_grants.remove(&capability);
        grants.insert(provider_id.to_owned(), provider_grants);
        Ok(was_granted)
    }

    pub fn clear_provider_granted_capabilities(
        &self,
        provider_id: &str,
    ) -> Result<bool, SourceRuntimeError> {
        self.provider_granted_capabilities
            .write()
            .map_err(|_| runtime_state_error(provider_id, "capability grant lock was poisoned"))
            .map(|mut grants| grants.remove(provider_id).is_some())
    }

    pub fn initialize_provider(
        &self,
        provider: &dyn SourceProvider,
    ) -> Result<SourceRuntimeReport, SourceRuntimeError> {
        self.initialize_provider_with_cancellation(provider, SourceCancellationToken::default())
    }

    pub fn initialize_provider_with_cancellation(
        &self,
        provider: &dyn SourceProvider,
        cancellation: SourceCancellationToken,
    ) -> Result<SourceRuntimeReport, SourceRuntimeError> {
        let (provider_id, provider_api_version, declared_capabilities) =
            provider_metadata(provider)?;
        ensure_compatible(&provider_id, provider_api_version, self.config.api_version)?;
        let granted_capabilities = self.granted_for(&provider_id, &declared_capabilities)?;
        let mut context = SourceRuntimeContext::new(
            &provider_id,
            declared_capabilities.clone(),
            granted_capabilities.clone(),
            Arc::clone(&self.host),
            cancellation,
        );
        for capability in declared_capabilities.difference(&granted_capabilities) {
            context.warn(format!(
                "declared capability {} is not granted by the host",
                capability.as_str()
            ));
        }
        context.ensure_not_cancelled("initialize provider")?;

        let sources = match catch_unwind(AssertUnwindSafe(|| provider.initialize(&mut context))) {
            Ok(Ok(sources)) => sources,
            Ok(Err(error)) => {
                return Err(enrich_error(error, &mut context, "provider initialization"));
            }
            Err(payload) => {
                return Err(provider_panic_error(
                    &mut context,
                    "provider initialization",
                    payload,
                ));
            }
        };
        validate_catalog(&provider_id, &sources, &mut context)?;

        self.initialized_sources
            .write()
            .map_err(|_| runtime_state_error(&provider_id, "provider registry lock was poisoned"))?
            .insert(provider_id.clone(), sources.clone());

        Ok(SourceRuntimeReport {
            source_id: provider_id,
            initialized: true,
            runtime_api_version: self.config.api_version,
            provider_api_version,
            declared_capabilities,
            granted_capabilities,
            sources,
            diagnostics: context.into_diagnostics(),
        })
    }

    pub fn dispatch_request(
        &self,
        provider: &dyn SourceProvider,
        request: SourceRequest,
    ) -> Result<SourceRequestOutcome, SourceRuntimeError> {
        self.dispatch_request_with_cancellation(
            provider,
            request,
            SourceCancellationToken::default(),
        )
    }

    pub fn dispatch_request_with_cancellation(
        &self,
        provider: &dyn SourceProvider,
        request: SourceRequest,
        cancellation: SourceCancellationToken,
    ) -> Result<SourceRequestOutcome, SourceRuntimeError> {
        let (provider_id, provider_api_version, declared_capabilities) =
            provider_metadata(provider)?;
        ensure_compatible(&provider_id, provider_api_version, self.config.api_version)?;
        let granted_capabilities = self.granted_for(&provider_id, &declared_capabilities)?;
        let mut context = SourceRuntimeContext::new(
            &provider_id,
            declared_capabilities,
            granted_capabilities,
            Arc::clone(&self.host),
            cancellation,
        );
        context.ensure_not_cancelled("dispatch source request")?;

        let source_info = self.source_info(&provider_id, request.source(), &mut context)?;
        validate_request(&request, &source_info, &mut context)?;
        let action = request.action();
        let source_key = request.source().to_owned();
        let request_contract = request.clone();

        let response = match catch_unwind(AssertUnwindSafe(|| {
            provider.handle_request(&mut context, request)
        })) {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                return Err(enrich_error(error, &mut context, "provider request"));
            }
            Err(payload) => {
                return Err(provider_panic_error(
                    &mut context,
                    "provider request",
                    payload,
                ));
            }
        };
        validate_response(
            &source_key,
            action,
            &request_contract,
            &response,
            &mut context,
        )?;

        Ok(SourceRequestOutcome {
            response,
            diagnostics: context.into_diagnostics(),
        })
    }

    pub fn uninitialize_provider(&self, provider_id: &str) -> Result<bool, SourceRuntimeError> {
        self.initialized_sources
            .write()
            .map_err(|_| runtime_state_error(provider_id, "provider registry lock was poisoned"))
            .map(|mut providers| providers.remove(provider_id).is_some())
    }

    fn granted_for(
        &self,
        source_id: &str,
        declared_capabilities: &BTreeSet<SourceCapability>,
    ) -> Result<BTreeSet<SourceCapability>, SourceRuntimeError> {
        let provider_grants = self
            .provider_granted_capabilities
            .read()
            .map_err(|_| runtime_state_error(source_id, "capability grant lock was poisoned"))?;
        let global_grants = self
            .granted_capabilities
            .read()
            .map_err(|_| runtime_state_error(source_id, "capability grant lock was poisoned"))?;
        let granted = provider_grants.get(source_id).unwrap_or(&global_grants);
        Ok(declared_capabilities
            .intersection(granted)
            .copied()
            .collect())
    }

    fn source_info(
        &self,
        provider_id: &str,
        source_key: &str,
        context: &mut SourceRuntimeContext,
    ) -> Result<SourceInfo, SourceRuntimeError> {
        let providers = self
            .initialized_sources
            .read()
            .map_err(|_| runtime_state_error(provider_id, "provider registry lock was poisoned"))?;
        let Some(sources) = providers.get(provider_id) else {
            context.error("provider must be initialized before dispatch");
            return Err(SourceRuntimeError::NotInitialized {
                source_id: provider_id.to_owned(),
                diagnostics: context.diagnostics().to_vec(),
            });
        };
        let Some(source) = sources.get(source_key) else {
            context.error(format!("unknown source key {source_key}"));
            return Err(SourceRuntimeError::InvalidRequest {
                source_id: provider_id.to_owned(),
                source_key: source_key.to_owned(),
                action: None,
                message: "source key is not present in the initialized catalog".to_owned(),
                diagnostics: context.diagnostics().to_vec(),
            });
        };
        Ok(source.clone())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceRuntimeError {
    #[error(
        "source {source_id} provider API {provider_version} is incompatible with runtime API {runtime_version}"
    )]
    Compatibility {
        source_id: String,
        runtime_version: SourceRuntimeApiVersion,
        provider_version: SourceRuntimeApiVersion,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} has not been initialized")]
    NotInitialized {
        source_id: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} published an invalid catalog: {message}")]
    InvalidCatalog {
        source_id: String,
        message: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} request for {source_key} is invalid: {message}")]
    InvalidRequest {
        source_id: String,
        source_key: String,
        action: Option<SourceAction>,
        message: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} denied capability {capability:?} for {operation}")]
    CapabilityDenied {
        source_id: String,
        capability: SourceCapability,
        operation: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} cancelled {operation}")]
    Cancelled {
        source_id: String,
        operation: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} does not support {action:?} for {source_key}")]
    UnsupportedAction {
        source_id: String,
        source_key: String,
        action: SourceAction,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} host operation {operation} failed: {message}")]
    Host {
        source_id: String,
        operation: String,
        message: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} failed: {message}")]
    Provider {
        source_id: String,
        code: String,
        message: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source {source_id} panicked during {operation}: {message}")]
    ProviderPanicked {
        source_id: String,
        operation: String,
        message: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
    #[error("source runtime state failed for {source_id}: {message}")]
    RuntimeState {
        source_id: String,
        message: String,
        diagnostics: Vec<SourceDiagnostic>,
    },
}

impl SourceRuntimeError {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Provider { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn diagnostics(&self) -> &[SourceDiagnostic] {
        match self {
            Self::Compatibility { diagnostics, .. }
            | Self::NotInitialized { diagnostics, .. }
            | Self::InvalidCatalog { diagnostics, .. }
            | Self::InvalidRequest { diagnostics, .. }
            | Self::CapabilityDenied { diagnostics, .. }
            | Self::Cancelled { diagnostics, .. }
            | Self::UnsupportedAction { diagnostics, .. }
            | Self::Host { diagnostics, .. }
            | Self::Provider { diagnostics, .. }
            | Self::ProviderPanicked { diagnostics, .. }
            | Self::RuntimeState { diagnostics, .. } => diagnostics,
        }
    }

    pub fn into_diagnostics(self) -> Vec<SourceDiagnostic> {
        match self {
            Self::Compatibility { diagnostics, .. }
            | Self::NotInitialized { diagnostics, .. }
            | Self::InvalidCatalog { diagnostics, .. }
            | Self::InvalidRequest { diagnostics, .. }
            | Self::CapabilityDenied { diagnostics, .. }
            | Self::Cancelled { diagnostics, .. }
            | Self::UnsupportedAction { diagnostics, .. }
            | Self::Host { diagnostics, .. }
            | Self::Provider { diagnostics, .. }
            | Self::ProviderPanicked { diagnostics, .. }
            | Self::RuntimeState { diagnostics, .. } => diagnostics,
        }
    }

    fn with_diagnostics(mut self, diagnostics: Vec<SourceDiagnostic>) -> Self {
        match &mut self {
            Self::Compatibility {
                diagnostics: current,
                ..
            }
            | Self::NotInitialized {
                diagnostics: current,
                ..
            }
            | Self::InvalidCatalog {
                diagnostics: current,
                ..
            }
            | Self::InvalidRequest {
                diagnostics: current,
                ..
            }
            | Self::CapabilityDenied {
                diagnostics: current,
                ..
            }
            | Self::Cancelled {
                diagnostics: current,
                ..
            }
            | Self::UnsupportedAction {
                diagnostics: current,
                ..
            }
            | Self::Host {
                diagnostics: current,
                ..
            }
            | Self::Provider {
                diagnostics: current,
                ..
            }
            | Self::ProviderPanicked {
                diagnostics: current,
                ..
            }
            | Self::RuntimeState {
                diagnostics: current,
                ..
            } => *current = diagnostics,
        }
        self
    }
}

fn provider_metadata(
    provider: &dyn SourceProvider,
) -> Result<(String, SourceRuntimeApiVersion, BTreeSet<SourceCapability>), SourceRuntimeError> {
    let provider_id = catch_unwind(AssertUnwindSafe(|| provider.id().to_owned()))
        .map_err(|payload| metadata_panic_error("unknown", "provider id", payload))?;
    let api_version = catch_unwind(AssertUnwindSafe(|| provider.api_version()))
        .map_err(|payload| metadata_panic_error(&provider_id, "provider API version", payload))?;
    let capabilities = catch_unwind(AssertUnwindSafe(|| provider.required_capabilities()))
        .map_err(|payload| metadata_panic_error(&provider_id, "provider capabilities", payload))?;
    Ok((provider_id, api_version, capabilities))
}

fn ensure_compatible(
    provider_id: &str,
    provider_version: SourceRuntimeApiVersion,
    runtime_version: SourceRuntimeApiVersion,
) -> Result<(), SourceRuntimeError> {
    if provider_version.is_compatible_with(runtime_version) {
        return Ok(());
    }

    let message = format!(
        "provider API {provider_version} is incompatible with runtime API {runtime_version}"
    );
    Err(SourceRuntimeError::Compatibility {
        source_id: provider_id.to_owned(),
        runtime_version,
        provider_version,
        diagnostics: vec![SourceDiagnostic {
            source_id: provider_id.to_owned(),
            level: DiagnosticLevel::Error,
            message,
        }],
    })
}

fn enrich_error(
    error: SourceRuntimeError,
    context: &mut SourceRuntimeContext,
    operation: &str,
) -> SourceRuntimeError {
    let message = error.to_string();
    if !context
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.message == message)
    {
        context.error(format!("{operation} failed: {message}"));
    }
    error.with_diagnostics(context.diagnostics().to_vec())
}

fn metadata_panic_error(
    source_id: &str,
    operation: &str,
    payload: Box<dyn Any + Send>,
) -> SourceRuntimeError {
    let message = panic_payload_message(payload);
    SourceRuntimeError::ProviderPanicked {
        source_id: source_id.to_owned(),
        operation: operation.to_owned(),
        message: message.clone(),
        diagnostics: vec![SourceDiagnostic {
            source_id: source_id.to_owned(),
            level: DiagnosticLevel::Error,
            message: format!("{operation} panicked: {message}"),
        }],
    }
}

fn provider_panic_error(
    context: &mut SourceRuntimeContext,
    operation: &str,
    payload: Box<dyn Any + Send>,
) -> SourceRuntimeError {
    let message = panic_payload_message(payload);
    context.error(format!("{operation} panicked: {message}"));
    SourceRuntimeError::ProviderPanicked {
        source_id: context.source_id().to_owned(),
        operation: operation.to_owned(),
        message,
        diagnostics: context.diagnostics().to_vec(),
    }
}

fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn runtime_state_error(source_id: &str, message: &str) -> SourceRuntimeError {
    SourceRuntimeError::RuntimeState {
        source_id: source_id.to_owned(),
        message: message.to_owned(),
        diagnostics: vec![SourceDiagnostic {
            source_id: source_id.to_owned(),
            level: DiagnosticLevel::Error,
            message: message.to_owned(),
        }],
    }
}

fn validate_catalog(
    provider_id: &str,
    sources: &BTreeMap<String, SourceInfo>,
    context: &mut SourceRuntimeContext,
) -> Result<(), SourceRuntimeError> {
    let invalid = if sources.is_empty() {
        Some("catalog must contain at least one source".to_owned())
    } else {
        sources.iter().find_map(|(source_key, source)| {
            if source_key.trim().is_empty() || source_key.chars().any(char::is_whitespace) {
                return Some("source keys must be non-empty and contain no whitespace".to_owned());
            }
            if source.id != *source_key {
                return Some(format!(
                    "catalog key {source_key} does not match source id {}",
                    source.id
                ));
            }
            if source.name.trim().is_empty() {
                return Some(format!("source {source_key} must have a display name"));
            }
            if source.actions.is_empty() {
                return Some(format!(
                    "source {source_key} must declare at least one action"
                ));
            }
            let actions = source.actions.iter().copied().collect::<BTreeSet<_>>();
            if actions.len() != source.actions.len() {
                return Some(format!("source {source_key} declares duplicate actions"));
            }
            let qualities = source.qualities.iter().copied().collect::<BTreeSet<_>>();
            (qualities.len() != source.qualities.len())
                .then(|| format!("source {source_key} declares duplicate qualities"))
        })
    };

    if let Some(message) = invalid {
        context.error(message.clone());
        return Err(SourceRuntimeError::InvalidCatalog {
            source_id: provider_id.to_owned(),
            message,
            diagnostics: context.diagnostics().to_vec(),
        });
    }
    Ok(())
}

fn validate_request(
    request: &SourceRequest,
    source: &SourceInfo,
    context: &mut SourceRuntimeContext,
) -> Result<(), SourceRuntimeError> {
    if let Err(message) = request.validate() {
        context.error(message.clone());
        return Err(SourceRuntimeError::InvalidRequest {
            source_id: context.source_id().to_owned(),
            source_key: request.source().to_owned(),
            action: Some(request.action()),
            message,
            diagnostics: context.diagnostics().to_vec(),
        });
    }
    if !source.actions.contains(&request.action()) {
        return Err(context.unsupported_action(request.source(), request.action()));
    }
    if let Some(quality) = request.requested_quality() {
        if !source.qualities.is_empty() && !source.qualities.contains(&quality) {
            let message = format!(
                "quality {} is not declared by source {}",
                quality.as_str(),
                request.source()
            );
            context.error(message.clone());
            return Err(SourceRuntimeError::InvalidRequest {
                source_id: context.source_id().to_owned(),
                source_key: request.source().to_owned(),
                action: Some(request.action()),
                message,
                diagnostics: context.diagnostics().to_vec(),
            });
        }
    }
    Ok(())
}

fn validate_response(
    source_key: &str,
    action: SourceAction,
    request: &SourceRequest,
    response: &SourceResponse,
    context: &mut SourceRuntimeContext,
) -> Result<(), SourceRuntimeError> {
    let matches_action = matches!(
        (action, response),
        (SourceAction::MusicSearch, SourceResponse::MusicSearch(_))
            | (SourceAction::ArtistSearch, SourceResponse::ArtistSearch(_))
            | (SourceAction::AlbumSearch, SourceResponse::AlbumSearch(_))
            | (
                SourceAction::PlaylistSearch,
                SourceResponse::PlaylistSearch(_)
            )
            | (
                SourceAction::SearchSuggestions,
                SourceResponse::SearchSuggestions(_)
            )
            | (
                SourceAction::ArtistTopTracks,
                SourceResponse::ArtistTopTracks(_)
            )
            | (SourceAction::ArtistAlbums, SourceResponse::ArtistAlbums(_))
            | (
                SourceAction::ArtistBiography,
                SourceResponse::ArtistBiography(_)
            )
            | (SourceAction::AlbumRead, SourceResponse::AlbumRead(_))
            | (
                SourceAction::PlaylistReadPublic,
                SourceResponse::PlaylistReadPublic(_)
            )
            | (SourceAction::MusicUrl, SourceResponse::MusicUrl(_))
            | (SourceAction::Lyric, SourceResponse::Lyric(_))
            | (SourceAction::Pic, SourceResponse::Pic(_))
            | (
                SourceAction::MusicComments,
                SourceResponse::MusicComments(_)
            )
            | (
                SourceAction::MusicRecommendations,
                SourceResponse::MusicRecommendations(_)
            )
            | (SourceAction::PlaylistList, SourceResponse::PlaylistList(_))
            | (SourceAction::PlaylistRead, SourceResponse::PlaylistRead(_))
            | (
                SourceAction::PlaylistAddTrack,
                SourceResponse::PlaylistAddTrack(_)
            )
            | (
                SourceAction::PlaylistRemoveTrack,
                SourceResponse::PlaylistRemoveTrack(_)
            )
    );
    if !matches_action {
        return Err(context.provider_error(format!(
            "provider returned a response that does not match {action:?}"
        )));
    }

    match response {
        SourceResponse::MusicSearch(search) => {
            validate_track_page(source_key, search, "musicSearch", context)?;
        }
        SourceResponse::ArtistSearch(search) => {
            if search.list.iter().any(|item| {
                item.source != source_key
                    || item.id.trim().is_empty()
                    || item.name.trim().is_empty()
                    || validate_platform_ids(&item.platform_ids).is_err()
                    || !item.raw_info.is_object()
            }) {
                return Err(context.provider_error("artistSearch returned an invalid entity"));
            }
        }
        SourceResponse::AlbumSearch(search) => {
            if search.list.iter().any(|item| {
                item.source != source_key
                    || item.id.trim().is_empty()
                    || item.title.trim().is_empty()
                    || item.artist.trim().is_empty()
                    || validate_platform_ids(&item.platform_ids).is_err()
                    || !item.raw_info.is_object()
            }) {
                return Err(context.provider_error("albumSearch returned an invalid entity"));
            }
        }
        SourceResponse::PlaylistSearch(search) => {
            if search.list.iter().any(|item| {
                item.source != source_key
                    || item.id.trim().is_empty()
                    || item.name.trim().is_empty()
                    || validate_platform_ids(&item.platform_ids).is_err()
                    || !item.raw_info.is_object()
            }) {
                return Err(context.provider_error("playlistSearch returned an invalid entity"));
            }
        }
        SourceResponse::SearchSuggestions(suggestions) => {
            if suggestions
                .list
                .iter()
                .any(|suggestion| suggestion.trim().is_empty())
            {
                return Err(context.provider_error("searchSuggestions returned an empty value"));
            }
        }
        SourceResponse::ArtistTopTracks(search) => {
            validate_track_page(source_key, search, "artistTopTracks", context)?;
        }
        SourceResponse::ArtistAlbums(search) => {
            if search.list.iter().any(|item| {
                item.source != source_key
                    || item.id.trim().is_empty()
                    || item.title.trim().is_empty()
                    || item.artist.trim().is_empty()
                    || validate_platform_ids(&item.platform_ids).is_err()
                    || !item.raw_info.is_object()
            }) {
                return Err(context.provider_error("artistAlbums returned an invalid entity"));
            }
        }
        SourceResponse::ArtistBiography(biography) => {
            if biography
                .summary
                .as_ref()
                .is_some_and(|summary| summary.trim().is_empty())
                || biography.sections.iter().any(|section| {
                    section.title.trim().is_empty() || section.text.trim().is_empty()
                })
            {
                return Err(context.provider_error("artistBiography returned invalid text"));
            }
        }
        SourceResponse::AlbumRead(search) => {
            validate_track_page(source_key, search, "albumRead", context)?;
        }
        SourceResponse::PlaylistReadPublic(search) => {
            validate_track_page(source_key, search, "playlistReadPublic", context)?;
        }
        SourceResponse::MusicComments(response) => {
            if response
                .hot_comments
                .iter()
                .chain(&response.comments)
                .any(invalid_source_comment)
            {
                return Err(context.provider_error("musicComments returned an invalid comment"));
            }
        }
        SourceResponse::MusicRecommendations(recommendations) => {
            if recommendations
                .list
                .iter()
                .any(|item| item.source != source_key)
            {
                return Err(context
                    .provider_error("recommendation source does not match the request source"));
            }
        }
        SourceResponse::PlaylistList(playlists) => {
            if playlists
                .iter()
                .any(|playlist| playlist.id.trim().is_empty() || playlist.name.trim().is_empty())
            {
                return Err(
                    context.provider_error("provider returned a playlist without an id or name")
                );
            }
        }
        SourceResponse::PlaylistRead(detail) => {
            let matches_playlist = matches!(
                request,
                SourceRequest::PlaylistRead { playlist_id, .. }
                    if playlist_id == &detail.playlist.id
            );
            if detail.playlist.id.trim().is_empty()
                || detail.playlist.name.trim().is_empty()
                || !matches_playlist
                || detail.tracks.iter().any(|track| track.source != source_key)
            {
                return Err(
                    context.provider_error("provider returned an invalid playlist detail response")
                );
            }
        }
        SourceResponse::PlaylistAddTrack(mutation) => {
            let matches_request = matches!(
                request,
                SourceRequest::PlaylistAddTrack {
                    playlist_id,
                    track,
                    ..
                } if playlist_id == &mutation.playlist_id && track.id == mutation.track_id
            );
            if mutation.operation != SourcePlaylistMutationKind::Add || !matches_request {
                return Err(
                    context.provider_error("provider returned a mismatched playlist add mutation")
                );
            }
        }
        SourceResponse::PlaylistRemoveTrack(mutation) => {
            let matches_request = matches!(
                request,
                SourceRequest::PlaylistRemoveTrack {
                    playlist_id,
                    track,
                    ..
                } if playlist_id == &mutation.playlist_id && track.id == mutation.track_id
            );
            if mutation.operation != SourcePlaylistMutationKind::Remove || !matches_request {
                return Err(context
                    .provider_error("provider returned a mismatched playlist remove mutation"));
            }
        }
        SourceResponse::MusicUrl(url) | SourceResponse::Pic(url) => {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(context.provider_error("provider returned a non-HTTP media URL"));
            }
        }
        SourceResponse::Lyric(_) => {}
    }
    Ok(())
}

fn invalid_source_comment(comment: &SourceComment) -> bool {
    comment.id.trim().is_empty()
        || comment.user_name.trim().is_empty()
        || comment.content.trim().is_empty()
        || comment
            .avatar_url
            .as_deref()
            .is_some_and(|url| !url.starts_with("http://") && !url.starts_with("https://"))
        || comment
            .time_label
            .as_deref()
            .is_some_and(|label| label.trim().is_empty())
        || comment
            .location
            .as_deref()
            .is_some_and(|location| location.trim().is_empty())
}

fn validate_track_page(
    source_key: &str,
    page: &SourceSearchResponse,
    action: &str,
    context: &mut SourceRuntimeContext,
) -> Result<(), SourceRuntimeError> {
    if page.list.iter().any(|item| {
        item.source != source_key
            || item.id.trim().is_empty()
            || item.title.trim().is_empty()
            || item.artist.trim().is_empty()
            || validate_platform_ids(&item.platform_ids).is_err()
            || !item.raw_info.is_object()
    }) {
        return Err(context.provider_error(format!("{action} returned an invalid track")));
    }
    Ok(())
}

pub fn lx_music_source(
    id: impl Into<String>,
    name: impl Into<String>,
    actions: Vec<SourceAction>,
    qualities: Vec<SourceQuality>,
) -> SourceInfo {
    SourceInfo {
        id: id.into(),
        name: name.into(),
        kind: SourceKind::Music,
        actions,
        qualities,
    }
}

pub fn standard_lx_qualities() -> Vec<SourceQuality> {
    vec![
        SourceQuality::K128,
        SourceQuality::K320,
        SourceQuality::Flac,
        SourceQuality::Flac24Bit,
    ]
}

#[cfg(test)]
fn standard_lx_music_actions() -> Vec<SourceAction> {
    vec![
        SourceAction::MusicSearch,
        SourceAction::MusicUrl,
        SourceAction::Lyric,
        SourceAction::Pic,
    ]
}

#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct MockRustSourceProvider {
    required_capabilities: BTreeSet<SourceCapability>,
}

#[cfg(test)]
impl MockRustSourceProvider {
    pub fn new(capabilities: impl IntoIterator<Item = SourceCapability>) -> Self {
        Self {
            required_capabilities: capabilities.into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl SourceProvider for MockRustSourceProvider {
    fn id(&self) -> &str {
        "mock-rust-source"
    }

    fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
        self.required_capabilities.clone()
    }

    fn initialize(
        &self,
        context: &mut SourceRuntimeContext,
    ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
        context.info("initialized Rust-native LX-compatible source provider");

        let mut sources = BTreeMap::new();
        for (id, name) in [
            (LX_SOURCE_KW, "Kuwo"),
            (LX_SOURCE_KG, "Kugou"),
            (LX_SOURCE_TX, "QQ Music"),
            (LX_SOURCE_WY, "NetEase"),
            (LX_SOURCE_MG, "Migu"),
        ] {
            sources.insert(
                id.to_owned(),
                lx_music_source(
                    id,
                    name,
                    standard_lx_music_actions(),
                    standard_lx_qualities(),
                ),
            );
        }

        sources.insert(
            LX_SOURCE_LOCAL.to_owned(),
            lx_music_source(
                LX_SOURCE_LOCAL,
                "Local Music",
                standard_lx_music_actions(),
                Vec::new(),
            ),
        );

        Ok(sources)
    }

    fn handle_request(
        &self,
        context: &mut SourceRuntimeContext,
        request: SourceRequest,
    ) -> Result<SourceResponse, SourceRuntimeError> {
        match request {
            SourceRequest::MusicUrl {
                source, music_info, ..
            } => {
                context.require_capability(SourceCapability::NetworkAny, "resolve musicUrl")?;
                let track_id = music_info
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("unknown");
                context.info(format!(
                    "resolved Rust-native mock musicUrl for {source}:{track_id}"
                ));
                Ok(SourceResponse::MusicUrl(format!(
                    "https://example.invalid/{source}/{track_id}.mp3"
                )))
            }
            SourceRequest::MusicSearch { .. } => {
                Ok(SourceResponse::MusicSearch(SourceSearchResponse {
                    is_end: true,
                    total: Some(0),
                    list: Vec::new(),
                }))
            }
            SourceRequest::Lyric { .. } => Ok(SourceResponse::Lyric(LyricResponse {
                lyric: Some("[00:00.00]Mock lyric from Rust provider".to_owned()),
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
pub(crate) fn mock_music_url_request(
    source: impl Into<String>,
    track_id: impl Into<String>,
) -> SourceRequest {
    SourceRequest::MusicUrl {
        source: source.into(),
        music_info: serde_json::json!({ "id": track_id.into() }),
        quality: SourceQuality::K128,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    const LX_JS_REFERENCE_SOURCES: &[(&str, &str)] = &[
        ("quantouya-aggregate-v4.1.js", "全豆要"),
        ("nianxin-v1.0.1.js", "念心音源"),
        ("changqing-svip-v1.2.0.js", "长青SVIP音源"),
    ];

    #[derive(Debug, Default)]
    struct RecordingHost {
        requests: AtomicUsize,
    }

    impl SourceHost for RecordingHost {
        fn http_request(
            &self,
            _source_id: &str,
            request: &SourceHttpRequest,
            cancellation: &SourceCancellationToken,
        ) -> Result<SourceHttpResponse, SourceHostError> {
            if cancellation.is_cancelled() {
                return Err(SourceHostError::Cancelled);
            }
            self.requests.fetch_add(1, Ordering::Relaxed);
            Ok(SourceHttpResponse {
                status: 200,
                final_url: request.url.clone(),
                headers: BTreeMap::new(),
                content_type: Some("application/json".to_owned()),
                body: br#"{"ok":true}"#.to_vec(),
            })
        }
    }

    fn spawn_http_test_server(body: &[u8], delay: Duration) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server should bind");
        let address = listener
            .local_addr()
            .expect("test HTTP server should have an address");
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);

        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let _ = stream.write_all(&response);
        });

        (format!("http://{address}/test"), handle)
    }

    fn spawn_http_request_capture_server(
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP server should bind");
        let address = listener
            .local_addr()
            .expect("test HTTP server should have an address");
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0_u8; 4096];
            let length = stream.read(&mut request).unwrap_or_default();
            let _ = sender.send(String::from_utf8_lossy(&request[..length]).into_owned());
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        });
        (format!("http://{address}/test"), receiver, handle)
    }

    #[derive(Debug)]
    struct HostApiProvider;

    impl SourceProvider for HostApiProvider {
        fn id(&self) -> &str {
            "host-api-provider"
        }

        fn required_capabilities(&self) -> BTreeSet<SourceCapability> {
            BTreeSet::from([SourceCapability::NetworkAny])
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            Ok(BTreeMap::from([(
                LX_SOURCE_WY.to_owned(),
                lx_music_source(
                    LX_SOURCE_WY,
                    "NetEase",
                    vec![SourceAction::MusicSearch],
                    standard_lx_qualities(),
                ),
            )]))
        }

        fn handle_request(
            &self,
            context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            let response = context.http_request(
                SourceHttpRequest::get("https://example.invalid/search"),
                "search test source",
            )?;
            let _: JsonValue = response
                .json()
                .map_err(|error| context.provider_error(error.to_string()))?;
            Ok(SourceResponse::MusicSearch(SourceSearchResponse {
                is_end: true,
                total: Some(0),
                list: Vec::new(),
            }))
        }
    }

    #[derive(Debug)]
    struct PanickingProvider;

    impl SourceProvider for PanickingProvider {
        fn id(&self) -> &str {
            "panicking-provider"
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            Ok(BTreeMap::from([(
                LX_SOURCE_WY.to_owned(),
                lx_music_source(
                    LX_SOURCE_WY,
                    "NetEase",
                    vec![SourceAction::Lyric],
                    Vec::new(),
                ),
            )]))
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            panic!("intentional provider panic")
        }
    }

    #[derive(Debug)]
    struct IncompatibleProvider;

    impl SourceProvider for IncompatibleProvider {
        fn id(&self) -> &str {
            "incompatible-provider"
        }

        fn api_version(&self) -> SourceRuntimeApiVersion {
            SourceRuntimeApiVersion::new(2, 0)
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            unreachable!("incompatible provider must not initialize")
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            unreachable!("incompatible provider must not dispatch")
        }
    }

    #[derive(Debug)]
    struct InvalidCatalogProvider;

    impl SourceProvider for InvalidCatalogProvider {
        fn id(&self) -> &str {
            "invalid-catalog-provider"
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            Ok(BTreeMap::from([(
                "wrong-key".to_owned(),
                lx_music_source(
                    "different-id",
                    "Broken",
                    vec![SourceAction::MusicSearch],
                    vec![],
                ),
            )]))
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            unreachable!("invalid catalog provider must not dispatch")
        }
    }

    #[derive(Debug)]
    struct InitializationErrorProvider;

    impl SourceProvider for InitializationErrorProvider {
        fn id(&self) -> &str {
            "initialization-error-provider"
        }

        fn initialize(
            &self,
            context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            Err(context.provider_error("intentional initialization failure"))
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            unreachable!("initialization error provider must not dispatch")
        }
    }

    #[derive(Debug)]
    struct InitializationPanickingProvider;

    impl SourceProvider for InitializationPanickingProvider {
        fn id(&self) -> &str {
            "initialization-panicking-provider"
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            panic!("intentional initialization panic")
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            unreachable!("initialization panic provider must not dispatch")
        }
    }

    #[derive(Debug)]
    struct MismatchedResponseProvider;

    impl SourceProvider for MismatchedResponseProvider {
        fn id(&self) -> &str {
            "mismatched-response-provider"
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            Ok(BTreeMap::from([(
                LX_SOURCE_WY.to_owned(),
                lx_music_source(
                    LX_SOURCE_WY,
                    "NetEase",
                    vec![SourceAction::MusicSearch],
                    vec![],
                ),
            )]))
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            Ok(SourceResponse::MusicUrl(
                "https://example.invalid/mismatched.mp3".to_owned(),
            ))
        }
    }

    #[derive(Debug)]
    struct QualityLimitedProvider;

    impl SourceProvider for QualityLimitedProvider {
        fn id(&self) -> &str {
            "quality-limited-provider"
        }

        fn initialize(
            &self,
            _context: &mut SourceRuntimeContext,
        ) -> Result<BTreeMap<String, SourceInfo>, SourceRuntimeError> {
            Ok(BTreeMap::from([(
                LX_SOURCE_WY.to_owned(),
                lx_music_source(
                    LX_SOURCE_WY,
                    "NetEase",
                    vec![SourceAction::MusicUrl],
                    vec![SourceQuality::K128],
                ),
            )]))
        }

        fn handle_request(
            &self,
            _context: &mut SourceRuntimeContext,
            _request: SourceRequest,
        ) -> Result<SourceResponse, SourceRuntimeError> {
            Ok(SourceResponse::MusicUrl(
                "https://example.invalid/quality-limited.mp3".to_owned(),
            ))
        }
    }

    #[derive(Debug)]
    struct BlockingHost {
        started: Arc<AtomicBool>,
    }

    impl SourceHost for BlockingHost {
        fn http_request(
            &self,
            _source_id: &str,
            _request: &SourceHttpRequest,
            cancellation: &SourceCancellationToken,
        ) -> Result<SourceHttpResponse, SourceHostError> {
            self.started.store(true, Ordering::Release);
            while !cancellation.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(SourceHostError::Cancelled)
        }
    }

    fn lx_js_reference_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/lx-js-sources")
    }

    fn read_lx_js_reference(file_name: &str) -> String {
        fs::read_to_string(lx_js_reference_dir().join(file_name))
            .expect("LX JS reference fixture should be readable")
    }

    fn search_request() -> SourceRequest {
        SourceRequest::MusicSearch {
            source: LX_SOURCE_WY.to_owned(),
            keyword: "test".to_owned(),
            page: 1,
            page_size: 20,
        }
    }

    #[test]
    fn initialize_provider_should_publish_versioned_lx_catalog() {
        let provider = MockRustSourceProvider::default();
        let report = SourceRuntime::new()
            .initialize_provider(&provider)
            .expect("provider should initialize");

        assert!(report.initialized);
        assert_eq!(report.runtime_api_version, SOURCE_RUNTIME_API_VERSION);
        assert_eq!(report.provider_api_version, SOURCE_RUNTIME_API_VERSION);
        assert_eq!(report.sources.len(), 6);
        assert_eq!(report.sources[LX_SOURCE_WY].kind, SourceKind::Music);
        assert!(report.sources[LX_SOURCE_WY]
            .actions
            .contains(&SourceAction::MusicSearch));
        assert_eq!(report.sources[LX_SOURCE_LOCAL].qualities, Vec::new());
    }

    #[test]
    fn initialize_provider_should_reject_invalid_catalogs() {
        let error = SourceRuntime::new()
            .initialize_provider(&InvalidCatalogProvider)
            .expect_err("invalid catalog should be rejected");

        assert!(matches!(error, SourceRuntimeError::InvalidCatalog { .. }));
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error));
    }

    #[test]
    fn initialize_provider_should_isolate_provider_errors_and_panics() {
        let error = SourceRuntime::new()
            .initialize_provider(&InitializationErrorProvider)
            .expect_err("provider initialization error should be returned");
        assert!(matches!(error, SourceRuntimeError::Provider { .. }));
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("initialization failure")));

        let error = SourceRuntime::new()
            .initialize_provider(&InitializationPanickingProvider)
            .expect_err("provider initialization panic should be isolated");
        assert!(matches!(error, SourceRuntimeError::ProviderPanicked { .. }));
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("initialization panic")));
    }

    #[test]
    fn dispatch_request_should_require_provider_initialization() {
        let provider = MockRustSourceProvider::default();
        let error = SourceRuntime::new()
            .dispatch_request(
                &provider,
                SourceRequest::Lyric {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "track-1" }),
                },
            )
            .expect_err("uninitialized provider should not dispatch");

        assert!(matches!(error, SourceRuntimeError::NotInitialized { .. }));
    }

    #[test]
    fn dispatch_request_should_reject_invalid_payloads_actions_and_qualities() {
        let search_provider = HostApiProvider;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&search_provider)
            .expect("provider should initialize");

        let error = runtime
            .dispatch_request(
                &search_provider,
                SourceRequest::MusicSearch {
                    source: LX_SOURCE_WY.to_owned(),
                    keyword: " ".to_owned(),
                    page: 1,
                    page_size: 20,
                },
            )
            .expect_err("empty search keyword should be rejected");
        assert!(matches!(error, SourceRuntimeError::InvalidRequest { .. }));

        let error = runtime
            .dispatch_request(
                &search_provider,
                SourceRequest::MusicSearch {
                    source: LX_SOURCE_WY.to_owned(),
                    keyword: "test".to_owned(),
                    page: 0,
                    page_size: 20,
                },
            )
            .expect_err("zero search page should be rejected");
        assert!(matches!(error, SourceRuntimeError::InvalidRequest { .. }));

        let error = runtime
            .dispatch_request(
                &search_provider,
                SourceRequest::Lyric {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "track-1" }),
                },
            )
            .expect_err("unsupported action should be rejected");
        assert!(matches!(
            error,
            SourceRuntimeError::UnsupportedAction { .. }
        ));

        let quality_provider = QualityLimitedProvider;
        let quality_runtime = SourceRuntime::new();
        quality_runtime
            .initialize_provider(&quality_provider)
            .expect("quality provider should initialize");
        let error = quality_runtime
            .dispatch_request(
                &quality_provider,
                SourceRequest::MusicUrl {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "track-1" }),
                    quality: SourceQuality::K320,
                },
            )
            .expect_err("undeclared quality should be rejected");
        assert!(matches!(error, SourceRuntimeError::InvalidRequest { .. }));
    }

    #[test]
    fn dispatch_request_should_reject_mismatched_provider_responses() {
        let provider = MismatchedResponseProvider;
        let runtime = SourceRuntime::new();
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let error = runtime
            .dispatch_request(&provider, search_request())
            .expect_err("response action must match request action");

        assert!(matches!(error, SourceRuntimeError::Provider { .. }));
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("does not match")));
    }

    #[test]
    fn dispatch_request_should_return_typed_lyric_response() {
        let provider = MockRustSourceProvider::default();
        let runtime = SourceRuntime::new();
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::Lyric {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "track-1" }),
                },
            )
            .expect("lyric request should dispatch");

        assert_eq!(
            outcome.response,
            SourceResponse::Lyric(LyricResponse {
                lyric: Some("[00:00.00]Mock lyric from Rust provider".to_owned()),
                tlyric: None,
                rlyric: None,
                lxlyric: None,
            })
        );
    }

    #[test]
    fn dispatch_request_should_deny_declared_but_ungranted_network_capability() {
        let provider = MockRustSourceProvider::new([SourceCapability::NetworkAny]);
        let runtime = SourceRuntime::new();
        runtime
            .initialize_provider(&provider)
            .expect("provider catalog should initialize without a network grant");
        let error = runtime
            .dispatch_request(&provider, mock_music_url_request(LX_SOURCE_WY, "track-1"))
            .expect_err("musicUrl should require a host network grant");

        assert!(matches!(
            error,
            SourceRuntimeError::CapabilityDenied {
                capability: SourceCapability::NetworkAny,
                ..
            }
        ));
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.level == DiagnosticLevel::Security));
    }

    #[test]
    fn dispatch_request_should_resolve_music_url_with_explicit_host_grant() {
        let provider = MockRustSourceProvider::new([SourceCapability::NetworkAny]);
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let outcome = runtime
            .dispatch_request(&provider, mock_music_url_request(LX_SOURCE_WY, "track-1"))
            .expect("musicUrl should resolve with a host grant");

        assert_eq!(
            outcome.response,
            SourceResponse::MusicUrl("https://example.invalid/wy/track-1.mp3".to_owned())
        );
    }

    #[test]
    fn dispatch_request_should_return_mock_pic_response() {
        let provider = MockRustSourceProvider::default();
        let runtime = SourceRuntime::new();
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let outcome = runtime
            .dispatch_request(
                &provider,
                SourceRequest::Pic {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "track-1" }),
                },
            )
            .expect("pic request should dispatch");

        assert_eq!(
            outcome.response,
            SourceResponse::Pic("https://example.invalid/wy/cover.jpg".to_owned())
        );
    }

    #[test]
    fn host_network_should_only_run_after_runtime_grants_capability() {
        let host = Arc::new(RecordingHost::default());
        let provider = HostApiProvider;
        let denied_runtime = SourceRuntime::with_host(host.clone(), []);
        denied_runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let error = denied_runtime
            .dispatch_request(&provider, search_request())
            .expect_err("network should be denied");
        assert!(matches!(error, SourceRuntimeError::CapabilityDenied { .. }));
        assert_eq!(host.requests.load(Ordering::Relaxed), 0);

        let granted_runtime =
            SourceRuntime::with_host(host.clone(), [SourceCapability::NetworkAny]);
        granted_runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        granted_runtime
            .dispatch_request(&provider, search_request())
            .expect("granted network request should dispatch");
        assert_eq!(host.requests.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dispatch_request_should_stop_when_cancelled_before_provider_call() {
        let provider = HostApiProvider;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let cancellation = SourceCancellationToken::default();
        cancellation.cancel();
        let error = runtime
            .dispatch_request_with_cancellation(&provider, search_request(), cancellation)
            .expect_err("cancelled request should stop");

        assert!(matches!(error, SourceRuntimeError::Cancelled { .. }));
    }

    #[test]
    fn dispatch_request_should_stop_when_cancelled_during_host_operation() {
        let started = Arc::new(AtomicBool::new(false));
        let host = Arc::new(BlockingHost {
            started: Arc::clone(&started),
        });
        let runtime = SourceRuntime::with_host(host, [SourceCapability::NetworkAny]);
        let provider = HostApiProvider;
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let cancellation = SourceCancellationToken::default();
        let cancellation_for_thread = cancellation.clone();
        let handle = thread::spawn(move || {
            runtime.dispatch_request_with_cancellation(
                &provider,
                search_request(),
                cancellation_for_thread,
            )
        });

        for _ in 0..1_000 {
            if started.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            started.load(Ordering::Acquire),
            "host operation should have started before cancellation"
        );
        cancellation.cancel();

        let error = handle
            .join()
            .expect("dispatch worker should not panic")
            .expect_err("in-flight cancellation should stop the request");
        assert!(matches!(error, SourceRuntimeError::Cancelled { .. }));
    }

    #[test]
    fn dispatch_request_should_isolate_provider_panic_and_keep_diagnostics() {
        let provider = PanickingProvider;
        let runtime = SourceRuntime::new();
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        let error = runtime
            .dispatch_request(
                &provider,
                SourceRequest::Lyric {
                    source: LX_SOURCE_WY.to_owned(),
                    music_info: json!({ "id": "track-1" }),
                },
            )
            .expect_err("provider panic should become a runtime error");

        assert!(matches!(error, SourceRuntimeError::ProviderPanicked { .. }));
        assert!(error
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("intentional provider panic")));
    }

    #[test]
    fn initialize_provider_should_reject_incompatible_api_major_version() {
        let error = SourceRuntime::new()
            .initialize_provider(&IncompatibleProvider)
            .expect_err("incompatible provider should be rejected");

        assert!(matches!(error, SourceRuntimeError::Compatibility { .. }));
    }

    #[test]
    fn request_serialization_should_preserve_lx_action_and_quality_names() {
        let serialized = serde_json::to_value(mock_music_url_request(LX_SOURCE_WY, "track-1"))
            .expect("request should serialize");

        assert_eq!(serialized["action"], "musicUrl");
        assert_eq!(serialized["quality"], "128k");
        assert_eq!(serialized["musicInfo"]["id"], "track-1");
    }

    #[test]
    fn music_comments_request_should_validate_and_serialize_pagination() {
        let request = SourceRequest::MusicComments {
            source: LX_SOURCE_WY.to_owned(),
            music_info: json!({ "id": "track-1" }),
            page: 2,
            page_size: 20,
        };

        request.validate().expect("comment request should be valid");
        let serialized = serde_json::to_value(&request).expect("comment request should serialize");
        assert_eq!(serialized["action"], "musicComments");
        assert_eq!(serialized["musicInfo"]["id"], "track-1");
        assert_eq!(serialized["page"], 2);
        assert_eq!(serialized["pageSize"], 20);

        let invalid = SourceRequest::MusicComments {
            source: LX_SOURCE_WY.to_owned(),
            music_info: json!({ "id": "track-1" }),
            page: 0,
            page_size: 51,
        };
        assert_eq!(
            invalid
                .validate()
                .expect_err("zero comment page should be rejected"),
            "musicComments page must be at least 1"
        );
    }

    #[test]
    fn legacy_recommendation_request_should_default_to_daily() {
        let request = serde_json::from_value::<SourceRequest>(json!({
            "action": "musicRecommendations",
            "source": "wy",
            "accountRef": "account",
            "limit": 50
        }))
        .expect("legacy request should deserialize");

        assert!(matches!(
            request,
            SourceRequest::MusicRecommendations {
                kind: MusicRecommendationKind::Daily,
                ..
            }
        ));
    }

    #[test]
    fn recommendation_request_should_serialize_the_selected_kind() {
        let serialized = serde_json::to_value(SourceRequest::MusicRecommendations {
            source: LX_SOURCE_WY.to_owned(),
            account_ref: "account".to_owned(),
            kind: MusicRecommendationKind::Radar,
            limit: 50,
        })
        .expect("recommendation request should serialize");

        assert_eq!(serialized["kind"], "radar");
    }

    #[test]
    fn cache_and_account_refs_should_require_separate_host_grants() {
        let provider = MockRustSourceProvider::new([
            SourceCapability::CacheReadWrite,
            SourceCapability::AccountRef,
        ]);
        let host = Arc::new(DefaultSourceHost::new(Duration::from_secs(1), 1024));
        host.register_account_ref(provider.id(), "account-1", "opaque-account-1")
            .expect("test account ref should register");
        let runtime = SourceRuntime::with_config_and_host(
            SourceRuntimeConfig::default().with_granted_capabilities([
                SourceCapability::CacheReadWrite,
                SourceCapability::AccountRef,
            ]),
            host,
        );
        let declared = provider.required_capabilities();
        let granted = runtime
            .granted_for(provider.id(), &declared)
            .expect("capability grants should be readable");
        let mut context = SourceRuntimeContext::new(
            provider.id(),
            declared,
            granted,
            Arc::clone(&runtime.host),
            SourceCancellationToken::default(),
        );

        context
            .cache_write("key", b"value", "write test cache")
            .expect("cache write should be granted");
        assert_eq!(
            context
                .cache_read("key", "read test cache")
                .expect("cache read should be granted"),
            Some(b"value".to_vec())
        );
        assert_eq!(
            context
                .account_ref("account-1", "use test account")
                .expect("account ref should be granted")
                .as_str(),
            "opaque-account-1"
        );

        let error = context
            .account_ref("missing-account", "use missing account")
            .expect_err("unknown account ref should be rejected by the host");
        assert!(matches!(error, SourceRuntimeError::Host { .. }));
    }

    #[test]
    fn account_refs_should_be_isolated_by_provider_id() {
        let host = DefaultSourceHost::new(Duration::from_secs(1), 1024);
        let cancellation = SourceCancellationToken::default();
        host.register_account_ref("provider-a", "shared-key", "opaque-a")
            .expect("provider A account ref should register");
        host.register_account_ref("provider-b", "shared-key", "opaque-b")
            .expect("provider B account ref should register");

        assert_eq!(
            host.resolve_account_ref("provider-a", "shared-key", &cancellation)
                .expect("provider A should resolve its account ref"),
            "opaque-a"
        );
        assert_eq!(
            host.resolve_account_ref("provider-b", "shared-key", &cancellation)
                .expect("provider B should resolve its account ref"),
            "opaque-b"
        );
        assert!(matches!(
            host.resolve_account_ref("provider-c", "shared-key", &cancellation),
            Err(SourceHostError::InvalidAccountRef)
        ));
    }

    #[test]
    fn network_diagnostics_should_include_target_without_query_parameters() {
        let host = Arc::new(RecordingHost::default());
        let capabilities = BTreeSet::from([SourceCapability::NetworkAny]);
        let mut context = SourceRuntimeContext::new(
            "diagnostic-provider",
            capabilities.clone(),
            capabilities,
            host,
            SourceCancellationToken::default(),
        );

        context
            .http_request(
                SourceHttpRequest::get("https://example.invalid/search?token=secret"),
                "diagnostic request",
            )
            .expect("diagnostic host request should succeed");

        assert!(context.diagnostics().iter().any(|diagnostic| diagnostic
            .message
            .contains("target https://example.invalid")));
        assert!(context
            .diagnostics()
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("secret")));
    }

    #[test]
    fn cache_keys_should_be_bounded_as_well_as_cache_values() {
        let host = DefaultSourceHost::new(Duration::from_secs(1), 1024);
        let cancellation = SourceCancellationToken::default();
        let oversized_key = "k".repeat(MAX_CACHE_KEY_BYTES + 1);
        let error = host
            .cache_write("test-source", &oversized_key, b"value", &cancellation)
            .expect_err("oversized cache key should be rejected");

        assert!(matches!(error, SourceHostError::Cache { .. }));
    }

    #[test]
    fn default_host_cache_should_bound_entries_and_keep_recent_values() {
        let host = DefaultSourceHost::new(Duration::from_secs(1), 1024);
        let cancellation = SourceCancellationToken::default();
        for index in 0..MAX_CACHE_ENTRIES {
            host.cache_write(
                "test-source",
                &format!("key-{index}"),
                b"value",
                &cancellation,
            )
            .expect("cache entry should write");
        }
        host.cache_read("test-source", "key-0", &cancellation)
            .expect("recent cache entry should read");
        host.cache_write(
            "test-source",
            "overflow-key",
            b"overflow-value",
            &cancellation,
        )
        .expect("overflow cache entry should write");
        host.cache.run_pending_tasks();

        assert!(host.cache.entry_count() <= u64::from(MAX_CACHE_ENTRIES));
        assert!(host
            .cache_read("test-source", "key-0", &cancellation)
            .expect("recent cache entry should read")
            .is_some());
        assert!(host
            .cache_read("test-source", "overflow-key", &cancellation)
            .expect("new cache entry should read")
            .is_some());
    }

    #[test]
    fn default_host_cache_should_bound_total_weight() {
        let host = DefaultSourceHost::new(Duration::from_secs(1), 1024);
        let cancellation = SourceCancellationToken::default();
        let value = vec![0_u8; MAX_CACHE_VALUE_BYTES];
        for index in 0..100 {
            host.cache_write(
                "test-source",
                &format!("large-{index}"),
                &value,
                &cancellation,
            )
            .expect("large cache entry should write");
        }
        host.cache.run_pending_tasks();

        assert!(host.cache.weighted_size() <= MAX_CACHE_WEIGHT_BYTES);
    }

    #[test]
    fn default_host_should_reject_responses_over_the_configured_limit() {
        let (url, server) = spawn_http_test_server(b"123456", Duration::ZERO);
        let host = DefaultSourceHost::new(Duration::from_secs(1), 5);
        let error = host
            .http_request(
                "test-source",
                &SourceHttpRequest::get(url),
                &SourceCancellationToken::default(),
            )
            .expect_err("oversized HTTP response should be rejected");
        server.join().expect("test HTTP server should finish");

        assert!(matches!(
            error,
            SourceHostError::ResponseTooLarge { max_bytes: 5 }
        ));
    }

    #[test]
    fn default_host_should_timeout_slow_responses() {
        let (url, server) = spawn_http_test_server(b"ok", Duration::from_millis(250));
        let host = DefaultSourceHost::new(Duration::from_millis(50), 1024);
        let error = host
            .http_request(
                "test-source",
                &SourceHttpRequest::get(url),
                &SourceCancellationToken::default(),
            )
            .expect_err("slow HTTP response should time out");
        server.join().expect("test HTTP server should finish");

        assert!(matches!(error, SourceHostError::Timeout { .. }));
    }

    #[test]
    fn default_host_should_use_an_explicit_user_agent_instead_of_its_default() {
        let (url, request_receiver, server) = spawn_http_request_capture_server();
        let host = DefaultSourceHost::new(Duration::from_secs(1), 1024);
        let mut request = SourceHttpRequest::get(url);
        request
            .headers
            .insert("User-Agent".to_owned(), "ProviderBrowser/1.0".to_owned());

        host.http_request("test-source", &request, &SourceCancellationToken::default())
            .expect("HTTP request should complete");
        let captured = request_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("server should capture the request");
        server.join().expect("test HTTP server should finish");
        let user_agents = captured
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
            .map(|(_, value)| value.trim())
            .collect::<Vec<_>>();

        assert_eq!(user_agents, ["ProviderBrowser/1.0"]);
    }

    #[test]
    fn revoked_capability_should_be_denied_on_the_next_dispatch() {
        let provider = MockRustSourceProvider::new([SourceCapability::NetworkAny]);
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        assert!(runtime
            .revoke_capability(SourceCapability::NetworkAny)
            .expect("capability grant should be writable"));

        let error = runtime
            .dispatch_request(&provider, mock_music_url_request(LX_SOURCE_WY, "track-1"))
            .expect_err("revoked capability should deny the next request");
        assert!(matches!(
            error,
            SourceRuntimeError::CapabilityDenied {
                capability: SourceCapability::NetworkAny,
                ..
            }
        ));
    }

    #[test]
    fn provider_specific_grant_should_not_leak_to_other_provider_ids() {
        let provider = HostApiProvider;
        let runtime = SourceRuntime::with_provider_granted_capabilities(
            "approved-provider",
            [SourceCapability::NetworkAny],
        );
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize without its own grant");

        assert!(runtime
            .granted_capabilities_for("approved-provider")
            .expect("provider grant should be readable")
            .contains(&SourceCapability::NetworkAny));
        assert!(!runtime
            .granted_capabilities_for(provider.id())
            .expect("fallback grant should be readable")
            .contains(&SourceCapability::NetworkAny));
        let error = runtime
            .dispatch_request(&provider, search_request())
            .expect_err("a grant for another provider must not authorize this request");
        assert!(matches!(
            error,
            SourceRuntimeError::CapabilityDenied {
                capability: SourceCapability::NetworkAny,
                ..
            }
        ));
    }

    #[test]
    fn provider_capability_revocation_should_override_global_grants() {
        let provider = HostApiProvider;
        let runtime = SourceRuntime::with_granted_capabilities([SourceCapability::NetworkAny]);
        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");

        assert!(runtime
            .granted_capabilities_for(provider.id())
            .expect("global grant should be visible")
            .contains(&SourceCapability::NetworkAny));
        assert!(runtime
            .revoke_provider_capability(provider.id(), SourceCapability::NetworkAny)
            .expect("provider capability should be revocable"));
        assert!(!runtime
            .granted_capabilities_for(provider.id())
            .expect("provider deny override should be visible")
            .contains(&SourceCapability::NetworkAny));
        assert!(!runtime
            .ensure_provider_granted_capabilities(provider.id(), [SourceCapability::NetworkAny],)
            .expect("default grant should not overwrite an explicit revoke"));

        let error = runtime
            .dispatch_request(&provider, search_request())
            .expect_err("provider-specific revoke must deny a global capability");
        assert!(matches!(
            error,
            SourceRuntimeError::CapabilityDenied {
                capability: SourceCapability::NetworkAny,
                ..
            }
        ));

        assert!(runtime
            .clear_provider_granted_capabilities(provider.id())
            .expect("provider override should be clearable"));
        assert!(runtime
            .granted_capabilities_for(provider.id())
            .expect("global grant should be restored")
            .contains(&SourceCapability::NetworkAny));
    }

    #[test]
    fn provider_capability_revocation_should_preserve_other_global_grants() {
        let provider = MockRustSourceProvider::new([
            SourceCapability::NetworkAny,
            SourceCapability::CacheReadWrite,
        ]);
        let runtime = SourceRuntime::with_granted_capabilities([
            SourceCapability::NetworkAny,
            SourceCapability::CacheReadWrite,
        ]);

        runtime
            .initialize_provider(&provider)
            .expect("provider should initialize");
        assert!(runtime
            .revoke_provider_capability(provider.id(), SourceCapability::NetworkAny)
            .expect("network capability should be revocable"));

        let grants = runtime
            .granted_capabilities_for(provider.id())
            .expect("provider grants should be readable");
        assert!(!grants.contains(&SourceCapability::NetworkAny));
        assert!(grants.contains(&SourceCapability::CacheReadWrite));
    }

    #[test]
    fn diagnostics_should_keep_the_most_recent_entries() {
        let runtime = SourceRuntime::new();
        let mut context = SourceRuntimeContext::new(
            "diagnostic-source",
            BTreeSet::new(),
            BTreeSet::new(),
            Arc::clone(&runtime.host),
            SourceCancellationToken::default(),
        );
        for index in 0..MAX_DIAGNOSTICS + 5 {
            context.info(format!("message {index}"));
        }

        assert_eq!(context.diagnostics().len(), MAX_DIAGNOSTICS);
        assert_eq!(context.diagnostics()[0].message, "message 5");
    }

    #[test]
    fn provider_errors_should_preserve_stable_codes() {
        let runtime = SourceRuntime::new();
        let mut context = SourceRuntimeContext::new(
            "coded-provider",
            BTreeSet::new(),
            BTreeSet::new(),
            Arc::clone(&runtime.host),
            SourceCancellationToken::default(),
        );

        let error =
            context.provider_error_with_code("credential-expired", "account session expired");

        assert_eq!(error.code(), Some("credential-expired"));
        assert!(matches!(
            error,
            SourceRuntimeError::Provider { ref code, .. } if code == "credential-expired"
        ));
    }

    #[test]
    fn lx_js_reference_sources_should_be_checked_in_for_sandbox_regressions() {
        for (file_name, display_name) in LX_JS_REFERENCE_SOURCES {
            let path = lx_js_reference_dir().join(file_name);
            assert!(Path::new(&path).is_file(), "missing {file_name}");

            let script = read_lx_js_reference(file_name);
            assert!(script.contains(display_name), "{file_name}");
            assert!(
                script.contains("globalThis['lx']") || script.contains("globalThis.lx"),
                "{file_name}"
            );
            assert!(script.contains("EVENT_NAMES"), "{file_name}");
            assert!(script.contains("musicUrl"), "{file_name}");
        }
    }

    #[test]
    fn rust_native_providers_should_initialize_without_javascript_inputs() {
        let provider = MockRustSourceProvider::default();
        let report = SourceRuntime::new()
            .initialize_provider(&provider)
            .expect("provider should initialize without JS fixtures");

        assert_eq!(report.source_id, "mock-rust-source");
    }
}
