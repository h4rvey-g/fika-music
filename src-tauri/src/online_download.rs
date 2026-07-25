use crate::online_music::OnlineTrack;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::tag::{Accessor, Tag};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum OnlineDownloadError {
    #[error("download database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("download data could not be serialized: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("download request is invalid: {0}")]
    Invalid(String),
    #[error("download file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum OnlineDownloadState {
    Queued,
    Running,
    Paused,
    Completed,
    CompletedWithErrors,
    Cancelled,
}

impl OnlineDownloadState {
    const fn as_db(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::CompletedWithErrors => "completed_with_errors",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_db(value: &str) -> Result<Self, OnlineDownloadError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "completed_with_errors" => Ok(Self::CompletedWithErrors),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(OnlineDownloadError::Invalid(format!(
                "unknown task state {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum OnlineDownloadItemState {
    Queued,
    Resolving,
    Downloading,
    Paused,
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

impl OnlineDownloadItemState {
    const fn as_db(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_db(value: &str) -> Result<Self, OnlineDownloadError> {
        match value {
            "queued" => Ok(Self::Queued),
            "resolving" => Ok(Self::Resolving),
            "downloading" => Ok(Self::Downloading),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "skipped" => Ok(Self::Skipped),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(OnlineDownloadError::Invalid(format!(
                "unknown item state {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineDownloadItem {
    pub item_id: String,
    pub position: u32,
    pub state: OnlineDownloadItemState,
    pub track: OnlineTrack,
    pub target_path: Option<String>,
    pub message: Option<String>,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct OnlineDownloadTask {
    pub task_id: String,
    pub kind: String,
    pub title: String,
    pub state: OnlineDownloadState,
    pub destination: String,
    pub selected_audio_source_id: Option<String>,
    pub total_items: u32,
    pub completed_items: u32,
    pub skipped_items: u32,
    pub failed_items: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub items: Vec<OnlineDownloadItem>,
}

pub fn recover_interrupted_tasks(
    connection: &Connection,
    updated_at: i64,
) -> Result<(), OnlineDownloadError> {
    let mut statement = connection.prepare(
        "SELECT temporary_path FROM online_download_items
         WHERE temporary_path IS NOT NULL AND state IN ('resolving', 'downloading')",
    )?;
    let temporary_paths = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for path in temporary_paths {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    connection.execute(
        "UPDATE online_download_tasks
         SET state = 'paused', updated_at = ?1
         WHERE state IN ('queued', 'running')",
        [updated_at],
    )?;
    connection.execute(
        "UPDATE online_download_items
         SET state = 'paused', message = 'Paused after application restart',
             temporary_path = NULL, bytes_downloaded = 0
         WHERE state IN ('queued', 'resolving', 'downloading')",
        [],
    )?;
    Ok(())
}

pub fn create_task(
    connection: &Connection,
    kind: &str,
    title: &str,
    destination: &Path,
    tracks: &[OnlineTrack],
    selected_audio_source_id: Option<&str>,
    created_at: i64,
) -> Result<OnlineDownloadTask, OnlineDownloadError> {
    if tracks.is_empty() {
        return Err(OnlineDownloadError::Invalid(
            "a download task requires at least one track".to_owned(),
        ));
    }
    if !destination.is_dir() {
        return Err(OnlineDownloadError::Invalid(format!(
            "download destination is not a directory: {}",
            destination.display()
        )));
    }
    let title = title.trim();
    if title.is_empty() || title.len() > 512 {
        return Err(OnlineDownloadError::Invalid(
            "download title must contain between 1 and 512 characters".to_owned(),
        ));
    }
    let task_id = Uuid::new_v4().to_string();
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "INSERT INTO online_download_tasks
         (task_id, kind, title, state, destination, total_items, created_at, updated_at,
          selected_audio_source_id)
         VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6, ?6, ?7)",
        params![
            task_id,
            kind.trim(),
            title,
            destination.to_string_lossy(),
            i64::try_from(tracks.len()).unwrap_or(i64::MAX),
            created_at,
            selected_audio_source_id
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        ],
    )?;
    for (position, track) in tracks.iter().enumerate() {
        let mut snapshot = track.clone();
        for candidate in &mut snapshot.candidates {
            candidate.raw_info = serde_json::json!({});
        }
        transaction.execute(
            "INSERT INTO online_download_items
             (item_id, task_id, position, state, track_json)
             VALUES (?1, ?2, ?3, 'queued', ?4)",
            params![
                Uuid::new_v4().to_string(),
                task_id,
                i64::try_from(position).unwrap_or(i64::MAX),
                serde_json::to_string(&snapshot)?,
            ],
        )?;
    }
    transaction.commit()?;
    task(connection, &task_id)?.ok_or_else(|| {
        OnlineDownloadError::Invalid("created download task could not be read".to_owned())
    })
}

pub fn list_tasks(connection: &Connection) -> Result<Vec<OnlineDownloadTask>, OnlineDownloadError> {
    let mut statement = connection.prepare(
        "SELECT task_id FROM online_download_tasks ORDER BY created_at DESC, task_id ASC",
    )?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|id| {
            task(connection, id)?.ok_or_else(|| {
                OnlineDownloadError::Invalid(format!("download task {id} disappeared"))
            })
        })
        .collect()
}

pub fn task(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<OnlineDownloadTask>, OnlineDownloadError> {
    let mut statement = connection.prepare(
        "SELECT kind, title, state, destination, total_items, completed_items,
                skipped_items, failed_items, created_at, updated_at, selected_audio_source_id
         FROM online_download_tasks WHERE task_id = ?1",
    )?;
    let mut rows = statement.query([task_id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let mut task = OnlineDownloadTask {
        task_id: task_id.to_owned(),
        kind: row.get(0)?,
        title: row.get(1)?,
        state: OnlineDownloadState::from_db(&row.get::<_, String>(2)?)?,
        destination: row.get(3)?,
        selected_audio_source_id: row.get(10)?,
        total_items: integer_u32(row.get(4)?),
        completed_items: integer_u32(row.get(5)?),
        skipped_items: integer_u32(row.get(6)?),
        failed_items: integer_u32(row.get(7)?),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        items: Vec::new(),
    };
    drop(rows);
    drop(statement);
    let mut item_statement = connection.prepare(
        "SELECT item_id, position, state, track_json, target_path, message,
                bytes_downloaded, total_bytes
         FROM online_download_items WHERE task_id = ?1 ORDER BY position ASC",
    )?;
    task.items = item_statement
        .query_map([task_id], |row| {
            let state = OnlineDownloadItemState::from_db(&row.get::<_, String>(2)?)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            let track = serde_json::from_str::<OnlineTrack>(&row.get::<_, String>(3)?)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            Ok(OnlineDownloadItem {
                item_id: row.get(0)?,
                position: integer_u32(row.get(1)?),
                state,
                track,
                target_path: row.get(4)?,
                message: row.get(5)?,
                bytes_downloaded: integer_u64(row.get(6)?),
                total_bytes: row.get::<_, Option<i64>>(7)?.map(integer_u64),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(task))
}

pub fn set_task_state(
    connection: &Connection,
    task_id: &str,
    state: OnlineDownloadState,
    updated_at: i64,
) -> Result<(), OnlineDownloadError> {
    let changed = connection.execute(
        "UPDATE online_download_tasks SET state = ?2, updated_at = ?3 WHERE task_id = ?1",
        params![task_id, state.as_db(), updated_at],
    )?;
    if changed == 0 {
        return Err(OnlineDownloadError::Invalid(format!(
            "download task {task_id} was not found"
        )));
    }
    Ok(())
}

pub fn set_item_state(
    connection: &Connection,
    item_id: &str,
    state: OnlineDownloadItemState,
    target_path: Option<&Path>,
    message: Option<&str>,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
) -> Result<(), OnlineDownloadError> {
    connection.execute(
        "UPDATE online_download_items SET state = ?2, target_path = ?3, message = ?4,
                bytes_downloaded = ?5, total_bytes = ?6, temporary_path = NULL
         WHERE item_id = ?1 AND state IN ('resolving', 'downloading')",
        params![
            item_id,
            state.as_db(),
            target_path.map(|path| path.to_string_lossy().into_owned()),
            message,
            i64::try_from(bytes_downloaded).unwrap_or(i64::MAX),
            total_bytes.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
        ],
    )?;
    Ok(())
}

pub fn remove_temporary_files(
    connection: &Connection,
    task_id: &str,
) -> Result<(), OnlineDownloadError> {
    let mut statement = connection.prepare(
        "SELECT temporary_path FROM online_download_items
         WHERE task_id = ?1 AND temporary_path IS NOT NULL",
    )?;
    let paths = statement
        .query_map([task_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for path in paths {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    connection.execute(
        "UPDATE online_download_items SET temporary_path = NULL WHERE task_id = ?1",
        [task_id],
    )?;
    Ok(())
}

pub fn reset_resumable_items(
    connection: &Connection,
    task_id: &str,
) -> Result<(), OnlineDownloadError> {
    connection.execute(
        "UPDATE online_download_items SET state = 'queued', message = NULL
         WHERE task_id = ?1 AND state = 'paused'",
        [task_id],
    )?;
    Ok(())
}

pub fn retry_item(
    connection: &Connection,
    task_id: &str,
    item_id: &str,
) -> Result<(), OnlineDownloadError> {
    let changed = connection.execute(
        "UPDATE online_download_items SET state = 'queued', message = NULL,
                bytes_downloaded = 0, total_bytes = NULL, temporary_path = NULL
         WHERE task_id = ?1 AND item_id = ?2 AND state IN ('failed', 'cancelled')",
        params![task_id, item_id],
    )?;
    if changed == 0 {
        return Err(OnlineDownloadError::Invalid(
            "only failed or cancelled download items can be retried".to_owned(),
        ));
    }
    Ok(())
}

pub fn replace_failed_item_track(
    connection: &Connection,
    task_id: &str,
    item_id: &str,
    track: &OnlineTrack,
) -> Result<(), OnlineDownloadError> {
    if track.candidates.is_empty() {
        return Err(OnlineDownloadError::Invalid(
            "refreshed download track has no candidates".to_owned(),
        ));
    }
    let mut snapshot = track.clone();
    for candidate in &mut snapshot.candidates {
        candidate.raw_info = serde_json::json!({});
    }
    let changed = connection.execute(
        "UPDATE online_download_items SET track_json = ?3, message = NULL,
                bytes_downloaded = 0, total_bytes = NULL, temporary_path = NULL
         WHERE task_id = ?1 AND item_id = ?2 AND state = 'failed'",
        params![task_id, item_id, serde_json::to_string(&snapshot)?],
    )?;
    if changed == 0 {
        return Err(OnlineDownloadError::Invalid(
            "only failed download items can refresh candidates".to_owned(),
        ));
    }
    Ok(())
}

pub fn claim_next_item(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<OnlineDownloadItem>, OnlineDownloadError> {
    let transaction = connection.unchecked_transaction()?;
    let item_id = transaction
        .query_row(
            "SELECT item_id FROM online_download_items
             WHERE task_id = ?1 AND state = 'queued' ORDER BY position ASC LIMIT 1",
            [task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(item_id) = item_id else {
        transaction.commit()?;
        return Ok(None);
    };
    transaction.execute(
        "UPDATE online_download_items SET state = 'resolving', message = NULL
         WHERE item_id = ?1 AND state = 'queued'",
        [&item_id],
    )?;
    let item = transaction.query_row(
        "SELECT position, track_json FROM online_download_items WHERE item_id = ?1",
        [&item_id],
        |row| {
            let track = serde_json::from_str::<OnlineTrack>(&row.get::<_, String>(1)?)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            Ok(OnlineDownloadItem {
                item_id: item_id.clone(),
                position: integer_u32(row.get(0)?),
                state: OnlineDownloadItemState::Resolving,
                track,
                target_path: None,
                message: None,
                bytes_downloaded: 0,
                total_bytes: None,
            })
        },
    )?;
    transaction.commit()?;
    Ok(Some(item))
}

pub fn set_item_downloading(
    connection: &Connection,
    item_id: &str,
    temporary_path: &Path,
    total_bytes: Option<u64>,
) -> Result<(), OnlineDownloadError> {
    connection.execute(
        "UPDATE online_download_items SET state = 'downloading', temporary_path = ?2,
                total_bytes = ?3, message = NULL WHERE item_id = ?1",
        params![
            item_id,
            temporary_path.to_string_lossy(),
            total_bytes.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
        ],
    )?;
    Ok(())
}

pub fn update_item_progress(
    connection: &Connection,
    item_id: &str,
    bytes_downloaded: u64,
) -> Result<(), OnlineDownloadError> {
    connection.execute(
        "UPDATE online_download_items SET bytes_downloaded = ?2 WHERE item_id = ?1",
        params![item_id, i64::try_from(bytes_downloaded).unwrap_or(i64::MAX)],
    )?;
    Ok(())
}

pub fn mark_pending_items(
    connection: &Connection,
    task_id: &str,
    state: OnlineDownloadItemState,
    message: &str,
) -> Result<(), OnlineDownloadError> {
    connection.execute(
        "UPDATE online_download_items SET state = ?2, message = ?3,
                temporary_path = NULL, bytes_downloaded = 0
         WHERE task_id = ?1 AND state IN ('queued', 'resolving', 'downloading', 'paused')",
        params![task_id, state.as_db(), message],
    )?;
    Ok(())
}

pub fn refresh_task_counts(
    connection: &Connection,
    task_id: &str,
    updated_at: i64,
) -> Result<OnlineDownloadState, OnlineDownloadError> {
    let (completed, skipped, failed, pending) = connection.query_row(
        "SELECT
            SUM(CASE WHEN state = 'completed' THEN 1 ELSE 0 END),
            SUM(CASE WHEN state = 'skipped' THEN 1 ELSE 0 END),
            SUM(CASE WHEN state = 'failed' THEN 1 ELSE 0 END),
            SUM(CASE WHEN state IN ('queued', 'resolving', 'downloading', 'paused') THEN 1 ELSE 0 END)
         FROM online_download_items WHERE task_id = ?1",
        [task_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?)),
    )?;
    let current = connection.query_row(
        "SELECT state FROM online_download_tasks WHERE task_id = ?1",
        [task_id],
        |row| row.get::<_, String>(0),
    )?;
    let current = OnlineDownloadState::from_db(&current)?;
    let state = if current == OnlineDownloadState::Cancelled || pending > 0 {
        current
    } else if failed > 0 {
        OnlineDownloadState::CompletedWithErrors
    } else {
        OnlineDownloadState::Completed
    };
    connection.execute(
        "UPDATE online_download_tasks SET completed_items = ?2, skipped_items = ?3,
                failed_items = ?4, state = ?5, updated_at = ?6 WHERE task_id = ?1",
        params![
            task_id,
            completed,
            skipped,
            failed,
            state.as_db(),
            updated_at
        ],
    )?;
    Ok(state)
}

pub fn render_filename(
    template: &str,
    track: &OnlineTrack,
    channel: &str,
) -> Result<String, OnlineDownloadError> {
    let mut output = String::new();
    let chars = template.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '[' && (index == 0 || chars[index - 1] != '\\') {
            let mut end = index + 1;
            while end < chars.len() && (chars[end] != ']' || chars[end.saturating_sub(1)] == '\\') {
                end += 1;
            }
            if end == chars.len() {
                return Err(OnlineDownloadError::Invalid(
                    "filename template has an unmatched [".to_owned(),
                ));
            }
            let group = chars[index + 1..end].iter().collect::<String>();
            let (rendered, complete) = render_segment(&group, track, channel)?;
            if complete {
                output.push_str(&rendered);
            }
            index = end + 1;
            continue;
        }
        let start = index;
        while index < chars.len()
            && (chars[index] != '[' || (index > 0 && chars[index - 1] == '\\'))
        {
            index += 1;
        }
        let segment = chars[start..index].iter().collect::<String>();
        output.push_str(&render_segment(&segment, track, channel)?.0);
    }
    let safe = safe_filename(&output);
    if safe.is_empty() {
        return Err(OnlineDownloadError::Invalid(
            "filename template produced an empty filename".to_owned(),
        ));
    }
    Ok(safe)
}

pub fn write_metadata(
    path: &Path,
    track: &OnlineTrack,
    cover: Option<&[u8]>,
) -> Result<(), OnlineDownloadError> {
    let mut tagged_file = lofty::read_from_path(path).map_err(|error| {
        OnlineDownloadError::Invalid(format!("downloaded audio metadata is unreadable: {error}"))
    })?;
    if tagged_file.primary_tag().is_none() {
        let tag_type = tagged_file.primary_tag_type();
        tagged_file.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged_file.primary_tag_mut().ok_or_else(|| {
        OnlineDownloadError::Invalid("downloaded audio format has no writable tag".to_owned())
    })?;
    tag.set_title(track.title.clone());
    tag.set_artist(track.artist.clone());
    if let Some(album) = track
        .album
        .as_ref()
        .filter(|album| !album.trim().is_empty())
    {
        tag.set_album(album.clone());
    }
    if let Some(number) = track.track_number {
        tag.set_track(number);
    }
    if let Some(number) = track.disc_number {
        tag.set_disk(number);
    }
    if let Some(cover) = cover {
        let mut picture = Picture::from_reader(&mut Cursor::new(cover)).map_err(|error| {
            OnlineDownloadError::Invalid(format!("download cover is invalid: {error}"))
        })?;
        picture.set_pic_type(PictureType::CoverFront);
        tag.push_picture(picture);
    }
    tagged_file
        .save_to_path(path, WriteOptions::default())
        .map_err(|error| {
            OnlineDownloadError::Invalid(format!("download metadata could not be written: {error}"))
        })?;
    Ok(())
}

fn render_segment(
    segment: &str,
    track: &OnlineTrack,
    channel: &str,
) -> Result<(String, bool), OnlineDownloadError> {
    let fields = [
        ("artist", track.artist.as_str()),
        ("title", track.title.as_str()),
        ("album", track.album.as_deref().unwrap_or("")),
        (
            "trackNumber",
            &track
                .track_number
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ),
        ("channel", channel),
    ];
    let mut output = segment.to_owned();
    let mut complete = true;
    for (name, value) in fields {
        let token = format!("{{{name}}}");
        if output.contains(&token) {
            if value.trim().is_empty() {
                complete = false;
            }
            output = output.replace(&token, value);
        }
    }
    if output.contains('{') || output.contains('}') {
        return Err(OnlineDownloadError::Invalid(
            "filename template contains an unsupported field".to_owned(),
        ));
    }
    Ok((
        output
            .replace("\\[", "[")
            .replace("\\]", "]")
            .replace("\\\\", "\\"),
        complete,
    ))
}

fn safe_filename(value: &str) -> String {
    let mut output = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['.', ' '])
        .to_owned();
    if output.chars().count() > 180 {
        output = output.chars().take(180).collect::<String>();
        output = output.trim_matches(['.', ' ']).to_owned();
    }
    output
}

fn integer_u32(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn integer_u64(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::online_music::OnlineTrackCandidate;
    use std::collections::BTreeMap;

    fn track(album: Option<&str>) -> OnlineTrack {
        OnlineTrack {
            key: "song".to_owned(),
            title: "A/B: Song".to_owned(),
            artist: "Artist".to_owned(),
            album: album.map(str::to_owned),
            duration_seconds: Some(180),
            cover_url: None,
            track_number: Some(2),
            disc_number: None,
            candidates: vec![OnlineTrackCandidate {
                channel_id: "plugin::wy".to_owned(),
                plugin_id: "plugin".to_owned(),
                source_id: "wy".to_owned(),
                channel_name: "NetEase".to_owned(),
                id: "1".to_owned(),
                title: "A/B: Song".to_owned(),
                artist: "Artist".to_owned(),
                album: album.map(str::to_owned),
                duration_seconds: Some(180),
                cover_url: None,
                track_number: Some(2),
                disc_number: None,
                platform_ids: BTreeMap::new(),
                raw_info: serde_json::json!({ "secret": true }),
                rank: 1,
            }],
        }
    }

    #[test]
    fn filename_template_omits_empty_optional_group_and_removes_path_characters() {
        assert_eq!(
            render_filename(
                "{artist} - {title}[ \\[{album}\\]]",
                &track(None),
                "NetEase"
            )
            .expect("filename should render"),
            "Artist - A B Song"
        );
        assert_eq!(
            render_filename(
                "{artist} - {title}[ \\[{album}\\]]",
                &track(Some("Album")),
                "NetEase"
            )
            .expect("filename should render"),
            "Artist - A B Song [Album]"
        );
    }

    #[test]
    fn persisted_task_strips_opaque_raw_info_and_recovers_paused() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let mut connection = Connection::open_in_memory().expect("database should open");
        database::initialize(&mut connection).expect("database should initialize");
        let created = create_task(
            &connection,
            "track",
            "A song",
            directory.path(),
            &[track(Some("Album"))],
            Some("source-1"),
            10,
        )
        .expect("task should create");
        assert_eq!(created.state, OnlineDownloadState::Queued);
        assert_eq!(
            created.items[0].track.candidates[0].raw_info,
            serde_json::json!({})
        );
        recover_interrupted_tasks(&connection, 20).expect("task should recover");
        let recovered = task(&connection, &created.task_id)
            .expect("task should load")
            .expect("task should exist");
        assert_eq!(recovered.state, OnlineDownloadState::Paused);
        assert_eq!(recovered.items[0].state, OnlineDownloadItemState::Paused);
    }

    #[test]
    fn late_worker_completion_cannot_overwrite_a_paused_item() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let mut connection = Connection::open_in_memory().expect("database should open");
        database::initialize(&mut connection).expect("database should initialize");
        let created = create_task(
            &connection,
            "track",
            "A song",
            directory.path(),
            &[track(None)],
            None,
            10,
        )
        .expect("task should create");
        let item = claim_next_item(&connection, &created.task_id)
            .expect("item should claim")
            .expect("item should exist");
        mark_pending_items(
            &connection,
            &created.task_id,
            OnlineDownloadItemState::Paused,
            "Paused by user",
        )
        .expect("item should pause");

        set_item_state(
            &connection,
            &item.item_id,
            OnlineDownloadItemState::Completed,
            Some(&directory.path().join("song.mp3")),
            None,
            100,
            Some(100),
        )
        .expect("late completion update should be ignored");

        let paused = task(&connection, &created.task_id)
            .expect("task should load")
            .expect("task should exist");
        assert_eq!(paused.items[0].state, OnlineDownloadItemState::Paused);
    }

    #[test]
    fn failed_item_candidate_refresh_should_replace_only_the_sanitized_snapshot() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let mut connection = Connection::open_in_memory().expect("database should open");
        database::initialize(&mut connection).expect("database should initialize");
        let created = create_task(
            &connection,
            "track",
            "A song",
            directory.path(),
            &[track(None)],
            None,
            10,
        )
        .expect("task should create");
        let item = claim_next_item(&connection, &created.task_id)
            .expect("item should claim")
            .expect("item should exist");
        set_item_state(
            &connection,
            &item.item_id,
            OnlineDownloadItemState::Failed,
            None,
            Some("failed"),
            0,
            None,
        )
        .expect("item should fail");
        let mut refreshed = track(Some("Album"));
        refreshed.title = "Refreshed Song".to_owned();

        replace_failed_item_track(&connection, &created.task_id, &item.item_id, &refreshed)
            .expect("failed item should refresh");

        let updated = task(&connection, &created.task_id)
            .expect("task should load")
            .expect("task should exist");
        assert_eq!(updated.items[0].state, OnlineDownloadItemState::Failed);
        assert_eq!(updated.items[0].track.title, "Refreshed Song");
        assert_eq!(
            updated.items[0].track.candidates[0].raw_info,
            serde_json::json!({})
        );
    }

    #[test]
    fn restart_recovery_removes_registered_partial_files() {
        let directory = tempfile::tempdir().expect("temporary directory should open");
        let partial = directory.path().join(".fika-download-test.mp3");
        std::fs::write(&partial, b"partial").expect("partial should write");
        let mut connection = Connection::open_in_memory().expect("database should open");
        database::initialize(&mut connection).expect("database should initialize");
        let created = create_task(
            &connection,
            "track",
            "A song",
            directory.path(),
            &[track(None)],
            None,
            10,
        )
        .expect("task should create");
        let item = claim_next_item(&connection, &created.task_id)
            .expect("item should claim")
            .expect("item should exist");
        set_item_downloading(&connection, &item.item_id, &partial, Some(100))
            .expect("item should begin downloading");

        recover_interrupted_tasks(&connection, 20).expect("task should recover");

        assert!(!partial.exists());
    }
}
