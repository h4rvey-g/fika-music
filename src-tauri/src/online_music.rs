use crate::source_runtime::{
    JsonScalar, MusicRecommendationKind, SourceAlbumSearchResult, SourceArtistBiography,
    SourceArtistSearchResult, SourcePlaylist, SourcePlaylistSearchResult, SourceQuality,
    SourceSearchResult,
};
use moka::sync::Cache;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use unicode_normalization::UnicodeNormalization;

const SETTINGS_KEY: &str = "online_music.settings.v1";
const MAX_SEARCH_HISTORY: usize = 10;
const RRF_K: f64 = 60.0;

#[derive(Debug)]
pub struct OnlineMusicCache {
    responses: Cache<String, crate::source_runtime::SourceRequestOutcome>,
}

impl Default for OnlineMusicCache {
    fn default() -> Self {
        Self {
            responses: Cache::builder()
                .max_capacity(512)
                .time_to_live(Duration::from_secs(10 * 60))
                .build(),
        }
    }
}

impl OnlineMusicCache {
    pub fn get(&self, key: &str) -> Option<crate::source_runtime::SourceRequestOutcome> {
        self.responses.get(key)
    }

    pub fn insert(&self, key: String, response: crate::source_runtime::SourceRequestOutcome) {
        self.responses.insert(key, response);
    }

    pub fn invalidate(&self) {
        self.responses.invalidate_all();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OnlineMusicError {
    #[error("online music database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("online music settings are invalid: {0}")]
    InvalidSettings(String),
    #[error("online music data could not be serialized: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum AudioSourceSelectionMode {
    #[default]
    Automatic,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineMusicSettings {
    pub excluded_channels: Vec<String>,
    pub channel_priority: Vec<String>,
    #[serde(default)]
    pub audio_source_selection_mode: AudioSourceSelectionMode,
    pub audio_source_priority: Vec<String>,
    pub layer_timeout_seconds: u64,
    pub playback_timeout_seconds: u64,
    pub preferred_quality: SourceQuality,
    pub search_history_enabled: bool,
    pub download_directory: Option<String>,
    pub filename_template: String,
    pub download_concurrency: u8,
    pub batch_notifications: bool,
}

impl Default for OnlineMusicSettings {
    fn default() -> Self {
        Self {
            excluded_channels: Vec::new(),
            channel_priority: Vec::new(),
            audio_source_selection_mode: AudioSourceSelectionMode::Automatic,
            audio_source_priority: Vec::new(),
            layer_timeout_seconds: 8,
            playback_timeout_seconds: 20,
            preferred_quality: SourceQuality::K320,
            search_history_enabled: true,
            download_directory: None,
            filename_template: "{artist} - {title}[ \\[{album}\\]]".to_owned(),
            download_concurrency: 2,
            batch_notifications: true,
        }
    }
}

impl OnlineMusicSettings {
    pub fn validate(&self) -> Result<(), OnlineMusicError> {
        validate_unique_ids(&self.excluded_channels, "excludedChannels")?;
        validate_unique_ids(&self.channel_priority, "channelPriority")?;
        validate_unique_ids(&self.audio_source_priority, "audioSourcePriority")?;
        if !(3..=30).contains(&self.layer_timeout_seconds) {
            return Err(OnlineMusicError::InvalidSettings(
                "layerTimeoutSeconds must be between 3 and 30".to_owned(),
            ));
        }
        if !(5..=60).contains(&self.playback_timeout_seconds) {
            return Err(OnlineMusicError::InvalidSettings(
                "playbackTimeoutSeconds must be between 5 and 60".to_owned(),
            ));
        }
        if !(1..=4).contains(&self.download_concurrency) {
            return Err(OnlineMusicError::InvalidSettings(
                "downloadConcurrency must be between 1 and 4".to_owned(),
            ));
        }
        if self.filename_template.trim().is_empty() || self.filename_template.len() > 512 {
            return Err(OnlineMusicError::InvalidSettings(
                "filenameTemplate must contain between 1 and 512 characters".to_owned(),
            ));
        }
        validate_filename_template(&self.filename_template)?;
        if self
            .download_directory
            .as_deref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(OnlineMusicError::InvalidSettings(
                "downloadDirectory must not be blank".to_owned(),
            ));
        }
        Ok(())
    }
}

fn validate_unique_ids(values: &[String], field: &str) -> Result<(), OnlineMusicError> {
    if values.len() > 256 {
        return Err(OnlineMusicError::InvalidSettings(format!(
            "{field} may contain at most 256 entries"
        )));
    }
    let mut unique = BTreeSet::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || value.len() > 256 {
            return Err(OnlineMusicError::InvalidSettings(format!(
                "{field} contains an invalid id"
            )));
        }
        if !unique.insert(value) {
            return Err(OnlineMusicError::InvalidSettings(format!(
                "{field} contains duplicate ids"
            )));
        }
    }
    Ok(())
}

fn validate_filename_template(template: &str) -> Result<(), OnlineMusicError> {
    const FIELDS: [&str; 5] = ["artist", "title", "album", "trackNumber", "channel"];
    let mut index = 0;
    let bytes = template.as_bytes();
    let mut optional_depth = 0_u8;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            b'[' => {
                optional_depth = optional_depth.saturating_add(1);
                if optional_depth > 1 {
                    return Err(OnlineMusicError::InvalidSettings(
                        "filenameTemplate optional groups may not be nested".to_owned(),
                    ));
                }
                index += 1;
            }
            b']' => {
                if optional_depth == 0 {
                    return Err(OnlineMusicError::InvalidSettings(
                        "filenameTemplate contains an unmatched ]".to_owned(),
                    ));
                }
                optional_depth -= 1;
                index += 1;
            }
            b'{' => {
                let Some(relative_end) = template[index + 1..].find('}') else {
                    return Err(OnlineMusicError::InvalidSettings(
                        "filenameTemplate contains an unmatched {".to_owned(),
                    ));
                };
                let end = index + 1 + relative_end;
                let field = &template[index + 1..end];
                if !FIELDS.contains(&field) {
                    return Err(OnlineMusicError::InvalidSettings(format!(
                        "filenameTemplate uses unsupported field {{{field}}}"
                    )));
                }
                index = end + 1;
            }
            b'}' => {
                return Err(OnlineMusicError::InvalidSettings(
                    "filenameTemplate contains an unmatched }".to_owned(),
                ));
            }
            _ => index += 1,
        }
    }
    if optional_depth != 0 {
        return Err(OnlineMusicError::InvalidSettings(
            "filenameTemplate contains an unmatched [".to_owned(),
        ));
    }
    if !template.contains("{title}") {
        return Err(OnlineMusicError::InvalidSettings(
            "filenameTemplate must include {title}".to_owned(),
        ));
    }
    Ok(())
}

pub fn load_settings(connection: &Connection) -> Result<OnlineMusicSettings, OnlineMusicError> {
    let stored = connection
        .query_row(
            "SELECT setting_value FROM app_settings WHERE setting_key = ?1",
            [SETTINGS_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let settings: OnlineMusicSettings = stored
        .map(|value| serde_json::from_str(&value))
        .transpose()?
        .unwrap_or_default();
    settings.validate()?;
    Ok(settings)
}

pub fn save_settings(
    connection: &Connection,
    settings: &OnlineMusicSettings,
    updated_at: i64,
) -> Result<(), OnlineMusicError> {
    settings.validate()?;
    connection.execute(
        "INSERT INTO app_settings (setting_key, setting_value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(setting_key) DO UPDATE SET
            setting_value = excluded.setting_value,
            updated_at = excluded.updated_at",
        params![SETTINGS_KEY, serde_json::to_string(settings)?, updated_at],
    )?;
    if !settings.search_history_enabled {
        connection.execute("DELETE FROM online_search_history", [])?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineSearchHistoryEntry {
    pub query: String,
    pub searched_at: i64,
}

pub fn record_search(
    connection: &Connection,
    query: &str,
    searched_at: i64,
) -> Result<(), OnlineMusicError> {
    let settings = load_settings(connection)?;
    let query = query.trim();
    if !settings.search_history_enabled || query.is_empty() {
        return Ok(());
    }
    connection.execute(
        "INSERT INTO online_search_history (normalized_query, query, searched_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(normalized_query) DO UPDATE SET
            query = excluded.query,
            searched_at = excluded.searched_at",
        params![normalize_text(query), query, searched_at],
    )?;
    connection.execute(
        "DELETE FROM online_search_history
         WHERE normalized_query NOT IN (
            SELECT normalized_query FROM online_search_history
            ORDER BY searched_at DESC, normalized_query ASC LIMIT ?1
         )",
        [MAX_SEARCH_HISTORY as i64],
    )?;
    Ok(())
}

pub fn search_history(
    connection: &Connection,
) -> Result<Vec<OnlineSearchHistoryEntry>, OnlineMusicError> {
    if !load_settings(connection)?.search_history_enabled {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "SELECT query, searched_at FROM online_search_history
         ORDER BY searched_at DESC, normalized_query ASC LIMIT ?1",
    )?;
    let entries = statement
        .query_map([MAX_SEARCH_HISTORY as i64], |row| {
            Ok(OnlineSearchHistoryEntry {
                query: row.get(0)?,
                searched_at: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(OnlineMusicError::from)?;
    Ok(entries)
}

pub fn clear_search_history(connection: &Connection) -> Result<(), OnlineMusicError> {
    connection.execute("DELETE FROM online_search_history", [])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineChannel {
    pub id: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub provider_id: String,
    pub source_id: String,
    pub source_name: String,
    pub excluded: bool,
    pub actions: Vec<crate::source_runtime::SourceAction>,
}

pub fn channels_from_plugins(
    plugins: &[crate::plugin_system::PluginRecord],
    settings: &OnlineMusicSettings,
) -> Vec<OnlineChannel> {
    let excluded = settings
        .excluded_channels
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut channels = BTreeMap::<String, OnlineChannel>::new();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        for provider in plugin
            .providers
            .iter()
            .filter(|provider| provider.initialized)
        {
            for source in &provider.sources {
                let id = format!("{}::{}", plugin.id, source.id);
                let is_excluded = excluded.contains(id.as_str());
                let channel = channels.entry(id.clone()).or_insert_with(|| OnlineChannel {
                    id,
                    plugin_id: plugin.id.clone(),
                    plugin_name: plugin.name.clone(),
                    provider_id: provider.id.clone(),
                    source_id: source.id.clone(),
                    source_name: source.name.clone(),
                    excluded: is_excluded,
                    actions: Vec::new(),
                });
                for action in &source.actions {
                    if !channel.actions.contains(action) {
                        channel.actions.push(*action);
                    }
                }
            }
        }
    }
    let mut channels = channels.into_values().collect::<Vec<_>>();
    channels.retain(|channel| {
        channel.actions.iter().any(|action| {
            matches!(
                action,
                crate::source_runtime::SourceAction::MusicSearch
                    | crate::source_runtime::SourceAction::ArtistSearch
                    | crate::source_runtime::SourceAction::AlbumSearch
                    | crate::source_runtime::SourceAction::PlaylistSearch
            )
        })
    });
    channels.sort_by(|left, right| {
        channel_rank(&left.id, &settings.channel_priority)
            .cmp(&channel_rank(&right.id, &settings.channel_priority))
            .then_with(|| left.id.cmp(&right.id))
    });
    channels
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum OnlineSearchSection {
    Songs,
    Artists,
    Albums,
    Playlists,
}

impl OnlineSearchSection {
    pub const fn action(self) -> crate::source_runtime::SourceAction {
        match self {
            Self::Songs => crate::source_runtime::SourceAction::MusicSearch,
            Self::Artists => crate::source_runtime::SourceAction::ArtistSearch,
            Self::Albums => crate::source_runtime::SourceAction::AlbumSearch,
            Self::Playlists => crate::source_runtime::SourceAction::PlaylistSearch,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineChannelFailure {
    pub channel_id: String,
    pub channel_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(tag = "section", content = "items", rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum OnlineSearchData {
    Songs(Vec<OnlineTrack>),
    Artists(Vec<OnlineArtist>),
    Albums(Vec<OnlineAlbum>),
    Playlists(Vec<OnlinePlaylist>),
}

impl OnlineSearchData {
    pub fn truncate(&mut self, length: usize) {
        match self {
            Self::Songs(items) => items.truncate(length),
            Self::Artists(items) => items.truncate(length),
            Self::Albums(items) => items.truncate(length),
            Self::Playlists(items) => items.truncate(length),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Songs(items) => items.len(),
            Self::Artists(items) => items.len(),
            Self::Albums(items) => items.len(),
            Self::Playlists(items) => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineSearchSectionResult {
    pub section: OnlineSearchSection,
    pub data: OnlineSearchData,
    pub failures: Vec<OnlineChannelFailure>,
    pub supported_channels: u32,
    pub completed_channels: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineSearchSectionEvent {
    pub search_id: String,
    pub result: OnlineSearchSectionResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineSuggestionsResult {
    pub suggestions: Vec<String>,
    pub failures: Vec<OnlineChannelFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineTrackPage {
    pub items: Vec<OnlineTrack>,
    pub has_more: bool,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineRecommendationsResult {
    pub kind: MusicRecommendationKind,
    pub items: Vec<OnlineTrack>,
    pub failures: Vec<OnlineChannelFailure>,
    pub supported_channels: u32,
    pub completed_channels: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlinePlaylistsResult {
    pub items: Vec<OnlinePlaylist>,
    pub failures: Vec<OnlineChannelFailure>,
    pub supported_channels: u32,
    pub completed_channels: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlinePlaylistDetailError {
    pub code: String,
    pub message: String,
    pub plugin_id: String,
    pub channel_name: String,
}

pub fn merge_suggestions(
    query: &str,
    history: &[OnlineSearchHistoryEntry],
    online: &[(OnlineChannel, Vec<String>)],
) -> Vec<String> {
    let normalized_query = normalize_text(query);
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in history {
        let normalized = normalize_text(&entry.query);
        if normalized.starts_with(&normalized_query) && seen.insert(normalized) {
            output.push(entry.query.clone());
        }
    }

    let mut scores = BTreeMap::<String, (f64, usize, String, BTreeSet<String>)>::new();
    for (channel_index, (channel, suggestions)) in online.iter().enumerate() {
        for (rank, suggestion) in suggestions.iter().enumerate() {
            let key = normalize_text(suggestion);
            if key.is_empty() || seen.contains(&key) {
                continue;
            }
            let contribution = 1.0 / (RRF_K + rank as f64 + 1.0);
            scores
                .entry(key)
                .and_modify(|entry| {
                    if entry.3.insert(channel.source_id.clone()) {
                        entry.0 += contribution;
                    }
                    entry.1 = entry.1.min(channel_index);
                })
                .or_insert_with(|| {
                    (
                        contribution,
                        channel_index,
                        suggestion.clone(),
                        BTreeSet::from([channel.source_id.clone()]),
                    )
                });
        }
    }
    let mut scores = scores.into_values().collect::<Vec<_>>();
    scores.sort_by(|left, right| {
        compare_score(right.0, left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    for (_, _, suggestion, _) in scores {
        let key = normalize_text(&suggestion);
        if seen.insert(key) {
            output.push(suggestion);
        }
        if output.len() == 8 {
            break;
        }
    }
    output.truncate(8);
    output
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineTrackCandidate {
    pub channel_id: String,
    pub plugin_id: String,
    pub source_id: String,
    pub channel_name: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_seconds: Option<u64>,
    pub cover_url: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
    pub rank: u32,
}

impl OnlineTrackCandidate {
    pub fn from_source(channel: &OnlineChannel, track: SourceSearchResult, rank: u32) -> Self {
        Self {
            channel_id: channel.id.clone(),
            plugin_id: channel.plugin_id.clone(),
            source_id: channel.source_id.clone(),
            channel_name: channel.source_name.clone(),
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            duration_seconds: track.duration_seconds,
            cover_url: track.cover_url,
            track_number: track.track_number,
            disc_number: track.disc_number,
            platform_ids: track.platform_ids,
            raw_info: track.raw_info,
            rank,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineTrack {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_seconds: Option<u64>,
    pub cover_url: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub candidates: Vec<OnlineTrackCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineArtistCandidate {
    pub channel_id: String,
    pub plugin_id: String,
    pub source_id: String,
    pub channel_name: String,
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
    pub rank: u32,
}

impl OnlineArtistCandidate {
    pub fn from_source(
        channel: &OnlineChannel,
        artist: SourceArtistSearchResult,
        rank: u32,
    ) -> Self {
        Self {
            channel_id: channel.id.clone(),
            plugin_id: channel.plugin_id.clone(),
            source_id: channel.source_id.clone(),
            channel_name: channel.source_name.clone(),
            id: artist.id,
            name: artist.name,
            cover_url: artist.cover_url,
            platform_ids: artist.platform_ids,
            raw_info: artist.raw_info,
            rank,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineArtist {
    pub key: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub candidates: Vec<OnlineArtistCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineArtistBiographySection {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineArtistBiography {
    pub summary: Option<String>,
    pub sections: Vec<OnlineArtistBiographySection>,
    pub source_name: String,
}

impl OnlineArtistBiography {
    pub fn from_source(source_name: String, biography: SourceArtistBiography) -> Self {
        Self {
            summary: biography.summary,
            sections: biography
                .sections
                .into_iter()
                .map(|section| OnlineArtistBiographySection {
                    title: section.title,
                    text: section.text,
                })
                .collect(),
            source_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineAlbumCandidate {
    pub channel_id: String,
    pub plugin_id: String,
    pub source_id: String,
    pub channel_name: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub release_year: Option<u32>,
    pub cover_url: Option<String>,
    pub track_count: Option<u64>,
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
    pub rank: u32,
}

impl OnlineAlbumCandidate {
    pub fn from_source(channel: &OnlineChannel, album: SourceAlbumSearchResult, rank: u32) -> Self {
        Self {
            channel_id: channel.id.clone(),
            plugin_id: channel.plugin_id.clone(),
            source_id: channel.source_id.clone(),
            channel_name: channel.source_name.clone(),
            id: album.id,
            title: album.title,
            artist: album.artist,
            release_year: album.release_year,
            cover_url: album.cover_url,
            track_count: album.track_count,
            platform_ids: album.platform_ids,
            raw_info: album.raw_info,
            rank,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineAlbum {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub release_year: Option<u32>,
    pub cover_url: Option<String>,
    pub track_count: Option<u64>,
    pub candidates: Vec<OnlineAlbumCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineAlbumPage {
    pub items: Vec<OnlineAlbum>,
    pub has_more: bool,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlinePlaylist {
    pub key: String,
    pub channel_id: String,
    pub plugin_id: String,
    pub source_id: String,
    pub channel_name: String,
    #[serde(default)]
    pub account_ref: Option<String>,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub track_count: Option<u64>,
    pub owner_name: Option<String>,
    pub can_mutate: bool,
    pub is_favorite: bool,
    #[ts(type = "Record<string, string | number>")]
    pub platform_ids: BTreeMap<String, JsonScalar>,
    #[ts(type = "Record<string, unknown>")]
    pub raw_info: JsonValue,
    pub rank: u32,
}

impl OnlinePlaylist {
    pub fn from_source(
        channel: &OnlineChannel,
        playlist: SourcePlaylistSearchResult,
        rank: u32,
    ) -> Self {
        Self {
            key: format!("{}:{}", channel.id, playlist.id),
            channel_id: channel.id.clone(),
            plugin_id: channel.plugin_id.clone(),
            source_id: channel.source_id.clone(),
            channel_name: channel.source_name.clone(),
            account_ref: None,
            id: playlist.id,
            name: playlist.name,
            description: playlist.description,
            cover_url: playlist.cover_url,
            track_count: playlist.track_count,
            owner_name: playlist.owner_name,
            can_mutate: false,
            is_favorite: false,
            platform_ids: playlist.platform_ids,
            raw_info: playlist.raw_info,
            rank,
        }
    }

    pub fn from_account(
        channel: &OnlineChannel,
        account_ref: &str,
        playlist: SourcePlaylist,
        rank: u32,
    ) -> Self {
        let id = playlist.id;
        Self {
            key: format!("{}:{account_ref}:{id}", channel.id),
            channel_id: channel.id.clone(),
            plugin_id: channel.plugin_id.clone(),
            source_id: channel.source_id.clone(),
            channel_name: channel.source_name.clone(),
            account_ref: Some(account_ref.to_owned()),
            platform_ids: BTreeMap::from([("id".to_owned(), JsonScalar::String(id.clone()))]),
            raw_info: JsonValue::Object(Default::default()),
            id,
            name: playlist.name,
            description: playlist.description,
            cover_url: playlist.cover_url,
            track_count: Some(playlist.track_count),
            owner_name: Some(playlist.owner_name),
            can_mutate: playlist.can_mutate,
            is_favorite: playlist.is_favorite,
            rank,
        }
    }
}

pub fn merge_tracks(
    candidates: Vec<OnlineTrackCandidate>,
    channel_priority: &[String],
) -> Vec<OnlineTrack> {
    let effective_channel_priority = effective_channel_priority(
        channel_priority,
        candidates
            .iter()
            .map(|candidate| candidate.channel_id.as_str()),
    );
    let mut groups: Vec<Vec<OnlineTrackCandidate>> = Vec::new();
    for candidate in candidates {
        if let Some(group) = groups.iter_mut().find(|group| {
            group
                .first()
                .is_some_and(|existing| tracks_match(existing, &candidate))
        }) {
            group.push(candidate);
        } else {
            groups.push(vec![candidate]);
        }
    }

    let mut ranked = groups
        .into_iter()
        .filter_map(|mut group| {
            sort_by_channel_priority(&mut group, &effective_channel_priority, |item| {
                &item.channel_id
            });
            let primary = group.first()?.clone();
            let score = rrf_score(
                group
                    .iter()
                    .map(|candidate| (&candidate.source_id, candidate.rank)),
            );
            let key = track_key(&primary);
            let track = OnlineTrack {
                key,
                title: first_non_empty(group.iter().map(|item| item.title.as_str()))
                    .unwrap_or_default()
                    .to_owned(),
                artist: first_non_empty(group.iter().map(|item| item.artist.as_str()))
                    .unwrap_or_default()
                    .to_owned(),
                album: group.iter().find_map(|item| item.album.clone()),
                duration_seconds: group.iter().find_map(|item| item.duration_seconds),
                cover_url: group.iter().find_map(|item| item.cover_url.clone()),
                track_number: group.iter().find_map(|item| item.track_number),
                disc_number: group.iter().find_map(|item| item.disc_number),
                candidates: group,
            };
            Some((
                channel_rank(&primary.channel_id, &effective_channel_priority),
                score,
                track,
            ))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| compare_score(right.1, left.1))
            .then_with(|| left.2.key.cmp(&right.2.key))
    });
    ranked.into_iter().map(|(_, _, track)| track).collect()
}

pub fn merge_artists(
    groups: Vec<Vec<OnlineArtistCandidate>>,
    channel_priority: &[String],
) -> Vec<OnlineArtist> {
    let effective_channel_priority = effective_channel_priority(
        channel_priority,
        groups
            .iter()
            .flat_map(|group| group.iter().map(|candidate| candidate.channel_id.as_str())),
    );
    let mut ranked = groups
        .into_iter()
        .filter_map(|mut group| {
            sort_by_channel_priority(&mut group, &effective_channel_priority, |item| {
                &item.channel_id
            });
            let primary = group.first()?.clone();
            let score = rrf_score(
                group
                    .iter()
                    .map(|candidate| (&candidate.source_id, candidate.rank)),
            );
            let artist = OnlineArtist {
                key: format!(
                    "artist:{}:{}",
                    normalize_text(&primary.name),
                    group
                        .iter()
                        .map(|candidate| candidate.channel_id.as_str())
                        .collect::<Vec<_>>()
                        .join("+")
                ),
                name: primary.name.clone(),
                cover_url: group.iter().find_map(|item| item.cover_url.clone()),
                candidates: group,
            };
            Some((
                channel_rank(&primary.channel_id, &effective_channel_priority),
                score,
                artist,
            ))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| compare_score(right.1, left.1))
            .then_with(|| left.2.key.cmp(&right.2.key))
    });
    ranked.into_iter().map(|(_, _, artist)| artist).collect()
}

pub fn merge_albums(
    groups: Vec<Vec<OnlineAlbumCandidate>>,
    channel_priority: &[String],
) -> Vec<OnlineAlbum> {
    let effective_channel_priority = effective_channel_priority(
        channel_priority,
        groups
            .iter()
            .flat_map(|group| group.iter().map(|candidate| candidate.channel_id.as_str())),
    );
    let mut ranked = groups
        .into_iter()
        .filter_map(|mut group| {
            sort_by_channel_priority(&mut group, &effective_channel_priority, |item| {
                &item.channel_id
            });
            let primary = group.first()?.clone();
            let score = rrf_score(
                group
                    .iter()
                    .map(|candidate| (&candidate.source_id, candidate.rank)),
            );
            let album = OnlineAlbum {
                key: format!(
                    "album:{}:{}:{}",
                    normalize_text(&primary.title),
                    artist_set_key(&primary.artist),
                    group
                        .iter()
                        .map(|candidate| candidate.channel_id.as_str())
                        .collect::<Vec<_>>()
                        .join("+")
                ),
                title: primary.title.clone(),
                artist: primary.artist.clone(),
                release_year: group.iter().find_map(|item| item.release_year),
                cover_url: group.iter().find_map(|item| item.cover_url.clone()),
                track_count: group.iter().find_map(|item| item.track_count),
                candidates: group,
            };
            Some((
                channel_rank(&primary.channel_id, &effective_channel_priority),
                score,
                album,
            ))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| compare_score(right.1, left.1))
            .then_with(|| left.2.key.cmp(&right.2.key))
    });
    ranked.into_iter().map(|(_, _, album)| album).collect()
}

pub fn sort_playlists(playlists: &mut [OnlinePlaylist], channel_priority: &[String]) {
    let effective_channel_priority = effective_channel_priority(
        channel_priority,
        playlists
            .iter()
            .map(|playlist| playlist.channel_id.as_str()),
    );
    playlists.sort_by(|left, right| {
        let left_score = 1.0 / (RRF_K + f64::from(left.rank));
        let right_score = 1.0 / (RRF_K + f64::from(right.rank));
        channel_rank(&left.channel_id, &effective_channel_priority)
            .cmp(&channel_rank(
                &right.channel_id,
                &effective_channel_priority,
            ))
            .then_with(|| compare_score(right_score, left_score))
            .then_with(|| left.key.cmp(&right.key))
    });
}

pub fn group_artist_candidates(
    candidates: Vec<OnlineArtistCandidate>,
) -> BTreeMap<String, Vec<OnlineArtistCandidate>> {
    let mut groups = BTreeMap::new();
    for candidate in candidates {
        groups
            .entry(normalize_text(&candidate.name))
            .or_insert_with(Vec::new)
            .push(candidate);
    }
    groups
}

pub fn group_album_candidates(
    candidates: Vec<OnlineAlbumCandidate>,
) -> BTreeMap<String, Vec<OnlineAlbumCandidate>> {
    let mut groups = BTreeMap::new();
    for candidate in candidates {
        let key = format!(
            "{}\u{1f}{}",
            normalize_text(&candidate.title),
            artist_set_key(&candidate.artist)
        );
        groups.entry(key).or_insert_with(Vec::new).push(candidate);
    }
    groups
}

pub fn artist_samples_match(left: &[OnlineTrack], right: &[OnlineTrack]) -> bool {
    let left_titles = left
        .iter()
        .map(|track| normalize_text(&track.title))
        .collect::<BTreeSet<_>>();
    let right_titles = right
        .iter()
        .map(|track| normalize_text(&track.title))
        .collect::<BTreeSet<_>>();
    if left_titles.intersection(&right_titles).take(2).count() >= 2 {
        return true;
    }
    left.iter().any(|left_track| {
        right.iter().any(|right_track| {
            normalize_text(&left_track.title) == normalize_text(&right_track.title)
                && left_track.album.is_some()
                && right_track.album.is_some()
                && left_track.album.as_deref().map(normalize_text)
                    == right_track.album.as_deref().map(normalize_text)
        })
    })
}

pub fn album_samples_match(
    left_year: Option<u32>,
    left: &[OnlineTrack],
    right_year: Option<u32>,
    right: &[OnlineTrack],
) -> bool {
    if left_year.is_some() && right_year.is_some() && left_year != right_year {
        return false;
    }
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let left_keys = left.iter().map(track_display_key).collect::<BTreeSet<_>>();
    let right_keys = right.iter().map(track_display_key).collect::<BTreeSet<_>>();
    let intersection = left_keys.intersection(&right_keys).count();
    let denominator = left_keys.len().min(right_keys.len());
    intersection.saturating_mul(2) >= denominator
}

pub fn track_matches_snapshot(left: &OnlineTrack, right: &OnlineTrack) -> bool {
    if normalize_text(&left.title) != normalize_text(&right.title)
        || artist_set_key(&left.artist) != artist_set_key(&right.artist)
    {
        return false;
    }
    match (&left.album, &right.album) {
        (Some(left_album), Some(right_album))
            if normalize_text(left_album) == normalize_text(right_album) => {}
        (None, None) => {}
        _ => return false,
    }
    !matches!(
        (left.duration_seconds, right.duration_seconds),
        (Some(left_duration), Some(right_duration)) if left_duration.abs_diff(right_duration) > 5
    )
}

fn tracks_match(left: &OnlineTrackCandidate, right: &OnlineTrackCandidate) -> bool {
    if normalize_text(&left.title) != normalize_text(&right.title)
        || artist_set_key(&left.artist) != artist_set_key(&right.artist)
    {
        return false;
    }
    match (&left.album, &right.album) {
        (Some(left_album), Some(right_album))
            if normalize_text(left_album) == normalize_text(right_album) => {}
        (None, None) => {}
        _ => return false,
    }
    !matches!(
        (left.duration_seconds, right.duration_seconds),
        (Some(left_duration), Some(right_duration)) if left_duration.abs_diff(right_duration) > 5
    )
}

fn track_key(candidate: &OnlineTrackCandidate) -> String {
    format!(
        "track:{}\u{1f}{}\u{1f}{}\u{1f}{}",
        normalize_text(&candidate.title),
        candidate
            .album
            .as_deref()
            .map(normalize_text)
            .unwrap_or_default(),
        artist_set_key(&candidate.artist),
        candidate.duration_seconds.unwrap_or_default() / 6
    )
}

fn track_display_key(track: &OnlineTrack) -> String {
    format!(
        "{}\u{1f}{}",
        normalize_text(&track.title),
        track
            .album
            .as_deref()
            .map(normalize_text)
            .unwrap_or_default()
    )
}

pub fn normalize_text(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn artist_set_key(value: &str) -> String {
    let normalized = value
        .replace(['＆', '&', '、', '，', ','], "/")
        .replace(" feat. ", "/")
        .replace(" feat ", "/")
        .replace(" ft. ", "/")
        .replace(" ft ", "/");
    let mut artists = normalized
        .split('/')
        .map(normalize_text)
        .filter(|artist| !artist.is_empty())
        .collect::<Vec<_>>();
    artists.sort();
    artists.dedup();
    artists.join("\u{1f}")
}

fn rrf_score<'a>(rankings: impl Iterator<Item = (&'a String, u32)>) -> f64 {
    let mut best_by_source = BTreeMap::<&str, u32>::new();
    for (source_id, rank) in rankings {
        best_by_source
            .entry(source_id.as_str())
            .and_modify(|best| *best = (*best).min(rank))
            .or_insert(rank);
    }
    best_by_source
        .values()
        .map(|rank| 1.0 / (RRF_K + f64::from(*rank)))
        .sum()
}

fn sort_by_channel_priority<T, F>(items: &mut [T], priority: &[String], channel: F)
where
    F: Fn(&T) -> &String,
{
    items.sort_by(|left, right| {
        channel_rank(channel(left), priority)
            .cmp(&channel_rank(channel(right), priority))
            .then_with(|| channel(left).cmp(channel(right)))
    });
}

fn effective_channel_priority<'a>(
    configured: &[String],
    channel_ids: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let available = channel_ids.map(str::to_owned).collect::<BTreeSet<_>>();
    let configured_ids = configured
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    configured
        .iter()
        .cloned()
        .chain(
            available
                .into_iter()
                .filter(|channel_id| !configured_ids.contains(channel_id.as_str())),
        )
        .collect()
}

fn channel_rank(channel_id: &str, priority: &[String]) -> usize {
    priority
        .iter()
        .position(|configured| configured == channel_id)
        .unwrap_or(priority.len())
}

fn compare_score(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn first_non_empty<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    values.into_iter().find(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_without_source_selection_mode_should_default_to_automatic() {
        let value = serde_json::json!({
            "excludedChannels": [],
            "channelPriority": [],
            "audioSourcePriority": [],
            "layerTimeoutSeconds": 8,
            "playbackTimeoutSeconds": 20,
            "preferredQuality": "320k",
            "searchHistoryEnabled": true,
            "downloadDirectory": null,
            "filenameTemplate": "{artist} - {title}",
            "downloadConcurrency": 2,
            "batchNotifications": true
        });

        let settings: OnlineMusicSettings = serde_json::from_value(value).unwrap();

        assert_eq!(
            settings.audio_source_selection_mode,
            AudioSourceSelectionMode::Automatic
        );
    }

    fn candidate(
        channel_id: &str,
        source_id: &str,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration: Option<u64>,
        rank: u32,
    ) -> OnlineTrackCandidate {
        OnlineTrackCandidate {
            channel_id: channel_id.to_owned(),
            plugin_id: channel_id.to_owned(),
            source_id: source_id.to_owned(),
            channel_name: channel_id.to_owned(),
            id: format!("{channel_id}-{rank}"),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: album.map(str::to_owned),
            duration_seconds: duration,
            cover_url: None,
            track_number: None,
            disc_number: None,
            platform_ids: BTreeMap::new(),
            raw_info: JsonValue::Object(Default::default()),
            rank,
        }
    }

    #[test]
    fn merge_tracks_should_prioritize_configured_channel_over_provider_rank() {
        let merged = merge_tracks(
            vec![
                candidate(
                    "netease",
                    "wy",
                    "NetEase result",
                    "Jay Chou",
                    Some("NetEase album"),
                    Some(180),
                    1,
                ),
                candidate(
                    "kugou",
                    "kg",
                    "KuGou result",
                    "Jay Chou",
                    Some("KuGou album"),
                    Some(180),
                    20,
                ),
            ],
            &["kugou".to_owned(), "netease".to_owned()],
        );

        assert_eq!(
            merged.first().map(|track| track.title.as_str()),
            Some("KuGou result")
        );
    }

    #[test]
    fn merge_tracks_should_use_default_channel_order_before_provider_rank() {
        let merged = merge_tracks(
            vec![
                candidate(
                    "fika.netease::wy",
                    "wy",
                    "NetEase result",
                    "Jay Chou",
                    Some("NetEase album"),
                    Some(180),
                    1,
                ),
                candidate(
                    "fika.kugou::kg",
                    "kg",
                    "KuGou result",
                    "Jay Chou",
                    Some("KuGou album"),
                    Some(180),
                    20,
                ),
            ],
            &[],
        );

        assert_eq!(
            merged.first().map(|track| track.title.as_str()),
            Some("KuGou result")
        );
    }

    #[test]
    fn merge_tracks_should_require_title_album_and_complete_artist_set() {
        let merged = merge_tracks(
            vec![
                candidate("one", "wy", "Song", "A / B", Some("Album"), Some(180), 1),
                candidate("two", "kg", " song ", "B & A", Some("album"), Some(183), 2),
                candidate("three", "tx", "Song", "A", Some("Album"), Some(180), 1),
            ],
            &[],
        );

        assert_eq!(
            merged
                .iter()
                .map(|track| track.candidates.len())
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn merge_tracks_should_not_merge_missing_album_with_named_album() {
        let merged = merge_tracks(
            vec![
                candidate("one", "wy", "Song", "Artist", None, Some(180), 1),
                candidate("two", "kg", "Song", "Artist", Some("Album"), Some(180), 1),
            ],
            &[],
        );

        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_tracks_should_split_duration_differences_over_five_seconds() {
        let merged = merge_tracks(
            vec![
                candidate("one", "wy", "Song", "Artist", Some("Album"), Some(180), 1),
                candidate("two", "kg", "Song", "Artist", Some("Album"), Some(186), 1),
            ],
            &[],
        );

        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn track_snapshot_matching_should_use_the_strict_merge_identity() {
        let left = merge_tracks(
            vec![candidate(
                "one",
                "wy",
                "Song",
                "A / B",
                Some("Album"),
                Some(180),
                1,
            )],
            &[],
        )
        .remove(0);
        let matching = merge_tracks(
            vec![candidate(
                "two",
                "kg",
                " song ",
                "B & A",
                Some("album"),
                Some(185),
                1,
            )],
            &[],
        )
        .remove(0);
        let different_version = merge_tracks(
            vec![candidate(
                "three",
                "tx",
                "Song",
                "A / B",
                Some("Album"),
                Some(186),
                1,
            )],
            &[],
        )
        .remove(0);

        assert!(track_matches_snapshot(&left, &matching));
        assert!(!track_matches_snapshot(&left, &different_version));
    }

    #[test]
    fn rrf_should_count_duplicate_source_implementations_once() {
        let duplicate_source =
            rrf_score([(&"wy".to_owned(), 1), (&"wy".to_owned(), 2)].into_iter());
        let one_source = rrf_score([(&"wy".to_owned(), 1)].into_iter());

        assert_eq!(duplicate_source, one_source);
    }

    #[test]
    fn filename_template_should_require_a_title_and_balanced_optional_groups() {
        let invalid = ["{artist}", "{title}[ [{album}]", "{title} {unknown}"];

        assert!(invalid
            .into_iter()
            .all(|template| validate_filename_template(template).is_err()));
    }
}
