use std::collections::HashSet;

use regex::{Regex, RegexBuilder};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::online_music::OnlineTrack;
use crate::{library, local_track_from_row, now_timestamp, LocalTrack};

const MAX_COLLECTION_NAME_CHARS: usize = 80;
const MAX_SMART_RULE_VALUE_CHARS: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum CollectionError {
    #[error("collection name cannot be empty")]
    EmptyName,
    #[error("collection name cannot contain control characters")]
    InvalidName,
    #[error("collection name cannot exceed {MAX_COLLECTION_NAME_CHARS} characters")]
    NameTooLong,
    #[error("a collection named '{0}' already exists")]
    DuplicateName(String),
    #[error("collection was not found: {0}")]
    NotFound(String),
    #[error("collection item is invalid: {0}")]
    InvalidItem(String),
    #[error("smart collection rules are invalid: {0}")]
    InvalidSmartRules(String),
    #[error("smart collection members are managed by its rules")]
    SmartCollectionImmutable,
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SmartCollectionField {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Genre,
    Year,
    Codec,
    BitrateKbps,
    SampleRateHz,
    DurationSeconds,
    TrackNumber,
    DiscNumber,
    FileName,
    FilePath,
    FileSizeBytes,
    ModifiedAt,
    IndexedAt,
    PlayCount,
    Rating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum SmartCollectionOperator {
    Equals,
    NotEquals,
    Contains,
    DoesNotContain,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    MatchesRegex,
    DoesNotMatchRegex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SmartCollectionRule {
    pub field: SmartCollectionField,
    pub operator: SmartCollectionOperator,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct SmartCollectionRules {
    pub rules: Vec<SmartCollectionRule>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MusicCollectionSummary {
    pub id: String,
    pub name: String,
    pub item_count: i64,
    pub local_count: i64,
    pub online_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub smart_rules: Option<SmartCollectionRules>,
}

#[derive(Debug, Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub enum MusicCollectionItemKind {
    Local,
    Online,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MusicCollectionItem {
    pub id: String,
    pub position: i64,
    pub kind: MusicCollectionItemKind,
    pub local_track: Option<LocalTrack>,
    pub local_album_group_id: Option<String>,
    pub online_track: Option<OnlineTrack>,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MusicCollectionDetail {
    pub collection: MusicCollectionSummary,
    pub items: Vec<MusicCollectionItem>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "bindings.ts")]
pub struct MusicCollectionMutation {
    pub collection: MusicCollectionSummary,
    pub added: i64,
    pub skipped: i64,
    pub removed: i64,
}

#[derive(Debug)]
struct StoredCollectionItem {
    id: String,
    position: i64,
    kind: String,
    local_track_id: Option<i64>,
    online_track_json: Option<String>,
    added_at: i64,
}

#[derive(Debug)]
struct StoredCollectionSummary {
    id: String,
    name: String,
    item_count: i64,
    local_count: i64,
    online_count: i64,
    created_at: i64,
    updated_at: i64,
    smart_rules_json: Option<String>,
}

#[derive(Debug)]
struct ResolvedCollection {
    id: String,
    name: String,
    item_count: i64,
    local_count: i64,
    online_count: i64,
    created_at: i64,
    updated_at: i64,
    smart_rules: Option<SmartCollectionRules>,
}

#[derive(Debug)]
struct PreparedSmartRule {
    field: SmartCollectionField,
    matcher: SmartRuleMatcher,
}

#[derive(Debug)]
enum SmartRuleMatcher {
    EqualsText(String),
    NotEqualsText(String),
    ContainsText(String),
    DoesNotContainText(String),
    Number {
        operator: SmartCollectionOperator,
        value: i64,
    },
    MatchesRegex(Regex),
    DoesNotMatchRegex(Regex),
}

const COLLECTION_SUMMARY_SQL: &str = "
    SELECT
        collection.id,
        collection.name,
        COUNT(item.id) AS item_count,
        COALESCE(SUM(CASE WHEN item.item_kind = 'local' THEN 1 ELSE 0 END), 0) AS local_count,
        COALESCE(SUM(CASE WHEN item.item_kind = 'online' THEN 1 ELSE 0 END), 0) AS online_count,
        collection.created_at,
        collection.updated_at,
        collection.smart_rules_json
    FROM music_collections collection
    LEFT JOIN music_collection_items item ON item.collection_id = collection.id
";

const LOCAL_TRACK_BY_ID_SQL: &str = "
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
        play_count,
        rating
    FROM local_tracks
    WHERE id = ?1
";

pub fn list_collections(
    connection: &Connection,
) -> Result<Vec<MusicCollectionSummary>, CollectionError> {
    let sql = format!(
        "{COLLECTION_SUMMARY_SQL}
         GROUP BY collection.id
         ORDER BY collection.created_at, collection.rowid"
    );
    let mut statement = connection.prepare(&sql)?;
    let stored = statement
        .query_map([], stored_collection_summary_from_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(CollectionError::from)?;
    drop(statement);
    let collections = stored
        .into_iter()
        .map(ResolvedCollection::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let tracks = collections
        .iter()
        .any(|collection| collection.smart_rules.is_some())
        .then(|| crate::list_tracks(connection))
        .transpose()?;
    collections
        .into_iter()
        .map(|collection| collection.into_summary(tracks.as_deref()))
        .collect()
}

pub fn create_collection(
    connection: &mut Connection,
    name: &str,
    smart_rules: Option<SmartCollectionRules>,
) -> Result<MusicCollectionSummary, CollectionError> {
    let name = normalize_name(name)?;
    if let Some(rules) = smart_rules.as_ref() {
        prepare_smart_rules(rules)?;
    }
    let transaction = connection.transaction()?;
    ensure_name_available(&transaction, &name, None)?;
    let id = Uuid::new_v4().to_string();
    let timestamp = now_timestamp();
    let smart_rules_json = smart_rules
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    transaction.execute(
        "INSERT INTO music_collections (
            id, name, created_at, updated_at, smart_rules_json
         ) VALUES (?1, ?2, ?3, ?3, ?4)",
        params![id, name, timestamp, smart_rules_json],
    )?;
    let collection = collection_summary(&transaction, &id)?;
    transaction.commit()?;
    Ok(collection)
}

pub fn rename_collection(
    connection: &mut Connection,
    collection_id: &str,
    name: &str,
) -> Result<MusicCollectionSummary, CollectionError> {
    let name = normalize_name(name)?;
    let transaction = connection.transaction()?;
    ensure_collection_exists(&transaction, collection_id)?;
    ensure_name_available(&transaction, &name, Some(collection_id))?;
    transaction.execute(
        "UPDATE music_collections SET name = ?2, updated_at = ?3 WHERE id = ?1",
        params![collection_id, name, now_timestamp()],
    )?;
    let collection = collection_summary(&transaction, collection_id)?;
    transaction.commit()?;
    Ok(collection)
}

pub fn delete_collection(
    connection: &Connection,
    collection_id: &str,
) -> Result<(), CollectionError> {
    let removed = connection.execute(
        "DELETE FROM music_collections WHERE id = ?1",
        [collection_id],
    )?;
    if removed == 0 {
        return Err(CollectionError::NotFound(collection_id.to_owned()));
    }
    Ok(())
}

pub fn collection_detail(
    connection: &Connection,
    collection_id: &str,
) -> Result<MusicCollectionDetail, CollectionError> {
    let resolved = resolved_collection(connection, collection_id)?;
    if let Some(rules) = resolved.smart_rules.as_ref() {
        let prepared = prepare_smart_rules(rules)?;
        let collection_created_at = resolved.created_at;
        let tracks = crate::list_tracks(connection)?;
        let matching_tracks = tracks
            .into_iter()
            .filter(|track| smart_rules_match(&prepared, track))
            .collect::<Vec<_>>();
        let item_count = i64::try_from(matching_tracks.len()).unwrap_or(i64::MAX);
        let collection = resolved.into_summary_with_count(item_count);
        let items = matching_tracks
            .into_iter()
            .enumerate()
            .map(|(position, track)| MusicCollectionItem {
                id: format!("smart:{collection_id}:{}", track.id),
                position: i64::try_from(position).unwrap_or(i64::MAX),
                kind: MusicCollectionItemKind::Local,
                local_album_group_id: Some(library::album_group_id(&track)),
                local_track: Some(track),
                online_track: None,
                added_at: collection_created_at,
            })
            .collect();
        return Ok(MusicCollectionDetail { collection, items });
    }

    let collection = resolved.into_summary_with_count_from_storage();
    let mut item_statement = connection.prepare(
        "SELECT id, position, item_kind, local_track_id, online_track_json, added_at
         FROM music_collection_items
         WHERE collection_id = ?1
         ORDER BY position, added_at, id",
    )?;
    let stored_items = item_statement
        .query_map([collection_id], |row| {
            Ok(StoredCollectionItem {
                id: row.get(0)?,
                position: row.get(1)?,
                kind: row.get(2)?,
                local_track_id: row.get(3)?,
                online_track_json: row.get(4)?,
                added_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(item_statement);

    let mut local_track_statement = connection.prepare(LOCAL_TRACK_BY_ID_SQL)?;
    let mut items = Vec::with_capacity(stored_items.len());
    for stored in stored_items {
        let (kind, local_track, local_album_group_id, online_track) = match stored.kind.as_str() {
            "local" => {
                let track_id = stored.local_track_id.ok_or_else(|| {
                    CollectionError::InvalidItem(format!(
                        "local item {} has no track reference",
                        stored.id
                    ))
                })?;
                let track = local_track_statement
                    .query_row([track_id], local_track_from_row)
                    .optional()?
                    .ok_or_else(|| {
                        CollectionError::InvalidItem(format!(
                            "local item {} references a missing track",
                            stored.id
                        ))
                    })?;
                let group_id = library::album_group_id(&track);
                (
                    MusicCollectionItemKind::Local,
                    Some(track),
                    Some(group_id),
                    None,
                )
            }
            "online" => {
                let payload = stored.online_track_json.as_deref().ok_or_else(|| {
                    CollectionError::InvalidItem(format!(
                        "online item {} has no track snapshot",
                        stored.id
                    ))
                })?;
                let track = serde_json::from_str(payload)?;
                (MusicCollectionItemKind::Online, None, None, Some(track))
            }
            kind => {
                return Err(CollectionError::InvalidItem(format!(
                    "item {} has unsupported kind {kind}",
                    stored.id
                )));
            }
        };
        items.push(MusicCollectionItem {
            id: stored.id,
            position: stored.position,
            kind,
            local_track,
            local_album_group_id,
            online_track,
            added_at: stored.added_at,
        });
    }

    Ok(MusicCollectionDetail { collection, items })
}

pub fn add_local_tracks(
    connection: &mut Connection,
    collection_id: &str,
    track_ids: &[i64],
) -> Result<MusicCollectionMutation, CollectionError> {
    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, collection_id)?;
    let mut position = next_position(&transaction, collection_id)?;
    let mut added = 0_i64;
    for track_id in track_ids {
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO music_collection_items (
                id, collection_id, position, item_kind, entry_key,
                local_track_id, online_track_json, added_at
             ) VALUES (?1, ?2, ?3, 'local', ?4, ?5, NULL, ?6)",
            params![
                Uuid::new_v4().to_string(),
                collection_id,
                position,
                format!("local:{track_id}"),
                track_id,
                now_timestamp(),
            ],
        )?;
        if inserted > 0 {
            added += 1;
            position += 1;
        }
    }
    finish_mutation(
        transaction,
        collection_id,
        added,
        i64::try_from(track_ids.len()).unwrap_or(i64::MAX) - added,
        0,
    )
}

pub fn add_online_tracks(
    connection: &mut Connection,
    collection_id: &str,
    tracks: &[OnlineTrack],
) -> Result<MusicCollectionMutation, CollectionError> {
    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, collection_id)?;
    let mut position = next_position(&transaction, collection_id)?;
    let mut added = 0_i64;
    for track in tracks {
        if track.key.trim().is_empty() {
            return Err(CollectionError::InvalidItem(
                "online track key cannot be empty".to_owned(),
            ));
        }
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO music_collection_items (
                id, collection_id, position, item_kind, entry_key,
                local_track_id, online_track_json, added_at
             ) VALUES (?1, ?2, ?3, 'online', ?4, NULL, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                collection_id,
                position,
                format!("online:{}", track.key),
                serde_json::to_string(track)?,
                now_timestamp(),
            ],
        )?;
        if inserted > 0 {
            added += 1;
            position += 1;
        }
    }
    finish_mutation(
        transaction,
        collection_id,
        added,
        i64::try_from(tracks.len()).unwrap_or(i64::MAX) - added,
        0,
    )
}

pub fn copy_items(
    connection: &mut Connection,
    target_collection_id: &str,
    source_collection_id: &str,
    item_ids: &[String],
) -> Result<MusicCollectionMutation, CollectionError> {
    let source = collection_detail(connection, source_collection_id)?;
    let selected = item_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let source_items = source
        .items
        .into_iter()
        .filter(|item| selected.contains(item.id.as_str()))
        .collect::<Vec<_>>();
    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, target_collection_id)?;
    let mut position = next_position(&transaction, target_collection_id)?;
    let mut added = 0_i64;
    let timestamp = now_timestamp();

    for item in source_items {
        let (kind, entry_key, local_track_id, online_track_json) = match item.kind {
            MusicCollectionItemKind::Local => {
                let track_id = item.local_track.map(|track| track.id).ok_or_else(|| {
                    CollectionError::InvalidItem(format!(
                        "local item {} has no track snapshot",
                        item.id
                    ))
                })?;
                ("local", format!("local:{track_id}"), Some(track_id), None)
            }
            MusicCollectionItemKind::Online => {
                let track = item.online_track.ok_or_else(|| {
                    CollectionError::InvalidItem(format!(
                        "online item {} has no track snapshot",
                        item.id
                    ))
                })?;
                (
                    "online",
                    format!("online:{}", track.key),
                    None,
                    Some(serde_json::to_string(&track)?),
                )
            }
        };
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO music_collection_items (
                id, collection_id, position, item_kind, entry_key,
                local_track_id, online_track_json, added_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                target_collection_id,
                position,
                kind,
                entry_key,
                local_track_id,
                online_track_json,
                timestamp,
            ],
        )?;
        if inserted > 0 {
            added += 1;
            position += 1;
        }
    }

    finish_mutation(
        transaction,
        target_collection_id,
        added,
        i64::try_from(item_ids.len()).unwrap_or(i64::MAX) - added,
        0,
    )
}

pub fn local_tracks_for_items(
    connection: &Connection,
    collection_id: &str,
    item_ids: &[String],
) -> Result<Vec<LocalTrack>, CollectionError> {
    let selected = item_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    Ok(collection_detail(connection, collection_id)?
        .items
        .into_iter()
        .filter(|item| selected.contains(item.id.as_str()))
        .filter_map(|item| item.local_track)
        .collect())
}

pub fn remove_items(
    connection: &mut Connection,
    collection_id: &str,
    item_ids: &[String],
) -> Result<MusicCollectionMutation, CollectionError> {
    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, collection_id)?;
    let mut removed = 0_i64;
    for item_id in item_ids {
        removed += transaction.execute(
            "DELETE FROM music_collection_items WHERE collection_id = ?1 AND id = ?2",
            params![collection_id, item_id],
        )? as i64;
    }
    finish_mutation(transaction, collection_id, 0, 0, removed)
}

fn finish_mutation(
    transaction: Transaction<'_>,
    collection_id: &str,
    added: i64,
    skipped: i64,
    removed: i64,
) -> Result<MusicCollectionMutation, CollectionError> {
    if added > 0 || removed > 0 {
        transaction.execute(
            "UPDATE music_collections SET updated_at = ?2 WHERE id = ?1",
            params![collection_id, now_timestamp()],
        )?;
    }
    let collection = collection_summary(&transaction, collection_id)?;
    transaction.commit()?;
    Ok(MusicCollectionMutation {
        collection,
        added,
        skipped,
        removed,
    })
}

fn collection_summary(
    connection: &Connection,
    collection_id: &str,
) -> Result<MusicCollectionSummary, CollectionError> {
    let resolved = resolved_collection(connection, collection_id)?;
    let tracks = resolved
        .smart_rules
        .is_some()
        .then(|| crate::list_tracks(connection))
        .transpose()?;
    resolved.into_summary(tracks.as_deref())
}

fn resolved_collection(
    connection: &Connection,
    collection_id: &str,
) -> Result<ResolvedCollection, CollectionError> {
    let sql = format!(
        "{COLLECTION_SUMMARY_SQL}
         WHERE collection.id = ?1
         GROUP BY collection.id"
    );
    connection
        .query_row(&sql, [collection_id], stored_collection_summary_from_row)
        .optional()?
        .ok_or_else(|| CollectionError::NotFound(collection_id.to_owned()))
        .and_then(ResolvedCollection::try_from)
}

fn stored_collection_summary_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredCollectionSummary> {
    Ok(StoredCollectionSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        item_count: row.get(2)?,
        local_count: row.get(3)?,
        online_count: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        smart_rules_json: row.get(7)?,
    })
}

impl ResolvedCollection {
    fn try_from(stored: StoredCollectionSummary) -> Result<Self, CollectionError> {
        let smart_rules = stored
            .smart_rules_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        Ok(Self {
            id: stored.id,
            name: stored.name,
            item_count: stored.item_count,
            local_count: stored.local_count,
            online_count: stored.online_count,
            created_at: stored.created_at,
            updated_at: stored.updated_at,
            smart_rules,
        })
    }

    fn into_summary(
        self,
        tracks: Option<&[LocalTrack]>,
    ) -> Result<MusicCollectionSummary, CollectionError> {
        let item_count = match (self.smart_rules.as_ref(), tracks) {
            (Some(rules), Some(tracks)) => {
                let prepared = prepare_smart_rules(rules)?;
                i64::try_from(
                    tracks
                        .iter()
                        .filter(|track| smart_rules_match(&prepared, track))
                        .count(),
                )
                .unwrap_or(i64::MAX)
            }
            (Some(_), None) => {
                return Err(CollectionError::InvalidSmartRules(
                    "the local library could not be loaded".to_owned(),
                ));
            }
            (None, _) => self.item_count,
        };
        Ok(self.into_summary_with_count(item_count))
    }

    fn into_summary_with_count_from_storage(self) -> MusicCollectionSummary {
        let item_count = self.item_count;
        self.into_summary_with_count(item_count)
    }

    fn into_summary_with_count(self, item_count: i64) -> MusicCollectionSummary {
        let is_smart = self.smart_rules.is_some();
        MusicCollectionSummary {
            id: self.id,
            name: self.name,
            item_count,
            local_count: if is_smart {
                item_count
            } else {
                self.local_count
            },
            online_count: if is_smart { 0 } else { self.online_count },
            created_at: self.created_at,
            updated_at: self.updated_at,
            smart_rules: self.smart_rules,
        }
    }
}

fn ensure_collection_exists(
    connection: &Connection,
    collection_id: &str,
) -> Result<(), CollectionError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM music_collections WHERE id = ?1)",
        [collection_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(CollectionError::NotFound(collection_id.to_owned()));
    }
    Ok(())
}

fn ensure_manual_collection(
    connection: &Connection,
    collection_id: &str,
) -> Result<(), CollectionError> {
    let smart_rules_json = connection
        .query_row(
            "SELECT smart_rules_json FROM music_collections WHERE id = ?1",
            [collection_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    match smart_rules_json {
        None => Err(CollectionError::NotFound(collection_id.to_owned())),
        Some(Some(_)) => Err(CollectionError::SmartCollectionImmutable),
        Some(None) => Ok(()),
    }
}

fn ensure_name_available(
    connection: &Connection,
    name: &str,
    excluded_collection_id: Option<&str>,
) -> Result<(), CollectionError> {
    let duplicate = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM music_collections
            WHERE name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id <> ?2)
         )",
        params![name, excluded_collection_id],
        |row| row.get::<_, bool>(0),
    )?;
    if duplicate {
        return Err(CollectionError::DuplicateName(name.to_owned()));
    }
    Ok(())
}

fn next_position(connection: &Connection, collection_id: &str) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0)
         FROM music_collection_items WHERE collection_id = ?1",
        [collection_id],
        |row| row.get(0),
    )
}

fn prepare_smart_rules(
    rules: &SmartCollectionRules,
) -> Result<Vec<PreparedSmartRule>, CollectionError> {
    if rules.rules.is_empty() {
        return Err(CollectionError::InvalidSmartRules(
            "at least one rule is required".to_owned(),
        ));
    }
    rules
        .rules
        .iter()
        .enumerate()
        .map(|(index, rule)| prepare_smart_rule(index, rule))
        .collect()
}

fn prepare_smart_rule(
    index: usize,
    rule: &SmartCollectionRule,
) -> Result<PreparedSmartRule, CollectionError> {
    let value = rule.value.trim();
    if value.is_empty() {
        return Err(CollectionError::InvalidSmartRules(format!(
            "rule {} requires a value",
            index + 1
        )));
    }
    if value.chars().count() > MAX_SMART_RULE_VALUE_CHARS {
        return Err(CollectionError::InvalidSmartRules(format!(
            "rule {} value cannot exceed {MAX_SMART_RULE_VALUE_CHARS} characters",
            index + 1
        )));
    }

    let matcher = if rule.field.is_numeric() {
        if !rule.operator.supports_numbers() {
            return Err(CollectionError::InvalidSmartRules(format!(
                "rule {} uses a text operator on a numeric field",
                index + 1
            )));
        }
        let number = value.parse::<i64>().map_err(|_| {
            CollectionError::InvalidSmartRules(format!(
                "rule {} value must be a whole number",
                index + 1
            ))
        })?;
        SmartRuleMatcher::Number {
            operator: rule.operator,
            value: number,
        }
    } else {
        match rule.operator {
            SmartCollectionOperator::Equals => {
                SmartRuleMatcher::EqualsText(library::normalize_text(value))
            }
            SmartCollectionOperator::NotEquals => {
                SmartRuleMatcher::NotEqualsText(library::normalize_text(value))
            }
            SmartCollectionOperator::Contains => {
                SmartRuleMatcher::ContainsText(library::normalize_text(value))
            }
            SmartCollectionOperator::DoesNotContain => {
                SmartRuleMatcher::DoesNotContainText(library::normalize_text(value))
            }
            SmartCollectionOperator::MatchesRegex | SmartCollectionOperator::DoesNotMatchRegex => {
                let regex = RegexBuilder::new(value)
                    .case_insensitive(true)
                    .unicode(true)
                    .build()
                    .map_err(|error| {
                        CollectionError::InvalidSmartRules(format!(
                            "rule {} has an invalid regular expression: {error}",
                            index + 1
                        ))
                    })?;
                if rule.operator == SmartCollectionOperator::MatchesRegex {
                    SmartRuleMatcher::MatchesRegex(regex)
                } else {
                    SmartRuleMatcher::DoesNotMatchRegex(regex)
                }
            }
            _ => {
                return Err(CollectionError::InvalidSmartRules(format!(
                    "rule {} uses a numeric operator on a text field",
                    index + 1
                )));
            }
        }
    };
    Ok(PreparedSmartRule {
        field: rule.field,
        matcher,
    })
}

fn smart_rules_match(rules: &[PreparedSmartRule], track: &LocalTrack) -> bool {
    rules.iter().all(|rule| rule.matches(track))
}

impl PreparedSmartRule {
    fn matches(&self, track: &LocalTrack) -> bool {
        match &self.matcher {
            SmartRuleMatcher::EqualsText(expected) => self
                .field
                .text_value(track)
                .is_some_and(|value| library::normalize_text(value) == *expected),
            SmartRuleMatcher::NotEqualsText(expected) => self
                .field
                .text_value(track)
                .is_some_and(|value| library::normalize_text(value) != *expected),
            SmartRuleMatcher::ContainsText(expected) => self
                .field
                .text_value(track)
                .is_some_and(|value| library::normalize_text(value).contains(expected)),
            SmartRuleMatcher::DoesNotContainText(expected) => self
                .field
                .text_value(track)
                .is_some_and(|value| !library::normalize_text(value).contains(expected)),
            SmartRuleMatcher::Number { operator, value } => self
                .field
                .number_value(track)
                .is_some_and(|actual| operator.compare_numbers(actual, *value)),
            SmartRuleMatcher::MatchesRegex(regex) => self
                .field
                .text_value(track)
                .is_some_and(|value| regex.is_match(value)),
            SmartRuleMatcher::DoesNotMatchRegex(regex) => self
                .field
                .text_value(track)
                .is_some_and(|value| !regex.is_match(value)),
        }
    }
}

impl SmartCollectionField {
    fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::Year
                | Self::BitrateKbps
                | Self::SampleRateHz
                | Self::DurationSeconds
                | Self::TrackNumber
                | Self::DiscNumber
                | Self::FileSizeBytes
                | Self::ModifiedAt
                | Self::IndexedAt
                | Self::PlayCount
                | Self::Rating
        )
    }

    fn text_value(self, track: &LocalTrack) -> Option<&str> {
        match self {
            Self::Title => Some(&track.title),
            Self::Artist => track.artist.as_deref(),
            Self::Album => track.album.as_deref(),
            Self::AlbumArtist => track.album_artist.as_deref(),
            Self::Genre => track.genre.as_deref(),
            Self::Codec => track.codec.as_deref(),
            Self::FileName => Some(&track.file_name),
            Self::FilePath => Some(&track.file_path),
            _ => None,
        }
    }

    fn number_value(self, track: &LocalTrack) -> Option<i64> {
        match self {
            Self::Year => track.year,
            Self::BitrateKbps => track.bitrate_kbps,
            Self::SampleRateHz => track.sample_rate_hz,
            Self::DurationSeconds => track.duration_seconds,
            Self::TrackNumber => track.track_number,
            Self::DiscNumber => track.disc_number,
            Self::FileSizeBytes => Some(track.file_size_bytes),
            Self::ModifiedAt => track.modified_at,
            Self::IndexedAt => Some(track.indexed_at),
            Self::PlayCount => Some(track.play_count),
            Self::Rating => Some(track.rating),
            _ => None,
        }
    }
}

impl SmartCollectionOperator {
    fn supports_numbers(self) -> bool {
        matches!(
            self,
            Self::Equals
                | Self::NotEquals
                | Self::GreaterThan
                | Self::GreaterThanOrEqual
                | Self::LessThan
                | Self::LessThanOrEqual
        )
    }

    fn compare_numbers(self, actual: i64, expected: i64) -> bool {
        match self {
            Self::Equals => actual == expected,
            Self::NotEquals => actual != expected,
            Self::GreaterThan => actual > expected,
            Self::GreaterThanOrEqual => actual >= expected,
            Self::LessThan => actual < expected,
            Self::LessThanOrEqual => actual <= expected,
            _ => false,
        }
    }
}

fn normalize_name(name: &str) -> Result<String, CollectionError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CollectionError::EmptyName);
    }
    if name.chars().any(char::is_control) {
        return Err(CollectionError::InvalidName);
    }
    if name.chars().count() > MAX_COLLECTION_NAME_CHARS {
        return Err(CollectionError::NameTooLong);
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;

    fn test_connection() -> Connection {
        let mut connection = Connection::open_in_memory().expect("database should open");
        database::initialize(&mut connection).expect("database should initialize");
        connection
    }

    fn insert_local_track(connection: &Connection) -> i64 {
        connection
            .execute(
                "INSERT INTO local_tracks (
                    file_path, file_name, title, file_size_bytes, indexed_at
                 ) VALUES ('/music/one.mp3', 'one.mp3', 'One', 1024, 1)",
                [],
            )
            .expect("track should insert");
        connection.last_insert_rowid()
    }

    fn insert_tagged_track(
        connection: &Connection,
        file_name: &str,
        title: &str,
        artist: &str,
        year: i64,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO local_tracks (
                    file_path, file_name, title, artist, year, file_size_bytes, indexed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 1024, 1)",
                params![
                    format!("/music/{file_name}"),
                    file_name,
                    title,
                    artist,
                    year
                ],
            )
            .expect("tagged track should insert");
        connection.last_insert_rowid()
    }

    fn smart_rules(rules: Vec<SmartCollectionRule>) -> SmartCollectionRules {
        SmartCollectionRules { rules }
    }

    fn online_track() -> OnlineTrack {
        OnlineTrack {
            key: "online-one".to_owned(),
            title: "Online One".to_owned(),
            artist: "Artist".to_owned(),
            album: Some("Album".to_owned()),
            duration_seconds: Some(180),
            cover_url: None,
            track_number: Some(1),
            disc_number: Some(1),
            candidates: Vec::new(),
        }
    }

    #[test]
    fn create_collection_should_trim_name_and_reject_case_insensitive_duplicate() {
        let mut connection = test_connection();

        let collection = create_collection(&mut connection, "  Road Trip  ", None)
            .expect("collection should be created");
        let error = create_collection(&mut connection, "road trip", None)
            .expect_err("duplicate should fail");

        assert_eq!(
            (collection.name.as_str(), error.to_string().as_str()),
            ("Road Trip", "a collection named 'road trip' already exists"),
        );
    }

    #[test]
    fn add_tracks_should_persist_local_and_online_items_without_duplicates() {
        let mut connection = test_connection();
        let track_id = insert_local_track(&connection);
        let collection = create_collection(&mut connection, "Mixed", None)
            .expect("collection should be created");

        add_local_tracks(&mut connection, &collection.id, &[track_id, track_id])
            .expect("local tracks should be added");
        add_online_tracks(
            &mut connection,
            &collection.id,
            &[online_track(), online_track()],
        )
        .expect("online tracks should be added");
        let detail =
            collection_detail(&connection, &collection.id).expect("collection should load");

        assert_eq!(
            (
                detail.collection.item_count,
                detail.collection.local_count,
                detail.collection.online_count,
                detail.items.len(),
            ),
            (2, 1, 1, 2),
        );
    }

    #[test]
    fn remove_items_should_update_summary_and_delete_collection_cascade() {
        let mut connection = test_connection();
        let collection = create_collection(&mut connection, "Temporary", None)
            .expect("collection should be created");
        add_online_tracks(&mut connection, &collection.id, &[online_track()])
            .expect("online track should be added");
        let item_id = collection_detail(&connection, &collection.id)
            .expect("collection should load")
            .items[0]
            .id
            .clone();

        let mutation = remove_items(&mut connection, &collection.id, &[item_id])
            .expect("item should be removed");
        delete_collection(&connection, &collection.id).expect("collection should delete");

        assert_eq!((mutation.removed, mutation.collection.item_count), (1, 0));
    }

    #[test]
    fn copy_items_should_preserve_mixed_items_and_skip_duplicates() {
        let mut connection = test_connection();
        let track_id = insert_local_track(&connection);
        let source =
            create_collection(&mut connection, "Source", None).expect("source should be created");
        let target =
            create_collection(&mut connection, "Target", None).expect("target should be created");
        add_local_tracks(&mut connection, &source.id, &[track_id])
            .expect("local track should be added");
        add_online_tracks(&mut connection, &source.id, &[online_track()])
            .expect("online track should be added");
        let item_ids = collection_detail(&connection, &source.id)
            .expect("source should load")
            .items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();

        let first = copy_items(&mut connection, &target.id, &source.id, &item_ids)
            .expect("items should copy");
        let second = copy_items(&mut connection, &target.id, &source.id, &item_ids)
            .expect("duplicate copy should be accepted");

        assert_eq!(
            (
                first.added,
                first.collection.local_count,
                first.collection.online_count,
                second.added,
                second.skipped,
            ),
            (2, 1, 1, 0, 2),
        );
    }

    #[test]
    fn local_tracks_for_items_should_ignore_online_items() {
        let mut connection = test_connection();
        let track_id = insert_local_track(&connection);
        let collection = create_collection(&mut connection, "Lookup", None)
            .expect("collection should be created");
        add_local_tracks(&mut connection, &collection.id, &[track_id])
            .expect("local track should be added");
        add_online_tracks(&mut connection, &collection.id, &[online_track()])
            .expect("online track should be added");
        let item_ids = collection_detail(&connection, &collection.id)
            .expect("collection should load")
            .items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();

        let tracks = local_tracks_for_items(&connection, &collection.id, &item_ids)
            .expect("local tracks should resolve");

        assert_eq!(
            tracks.iter().map(|track| track.id).collect::<Vec<_>>(),
            [track_id]
        );
    }

    #[test]
    fn smart_collection_should_match_all_text_and_numeric_rules() {
        let mut connection = test_connection();
        let matching =
            insert_tagged_track(&connection, "sun-2003.flac", "The Moment", "孙燕姿", 2003);
        insert_tagged_track(&connection, "sun-2000.flac", "Cloudy Day", "孙燕姿", 2000);
        let collection = create_collection(
            &mut connection,
            "孙燕姿 2000+",
            Some(smart_rules(vec![
                SmartCollectionRule {
                    field: SmartCollectionField::Artist,
                    operator: SmartCollectionOperator::Equals,
                    value: "孙燕姿".to_owned(),
                },
                SmartCollectionRule {
                    field: SmartCollectionField::Year,
                    operator: SmartCollectionOperator::GreaterThan,
                    value: "2000".to_owned(),
                },
            ])),
        )
        .expect("smart collection should be created");

        let detail = collection_detail(&connection, &collection.id)
            .expect("smart collection should resolve");

        assert_eq!(
            (
                detail.collection.item_count,
                detail.collection.local_count,
                detail.items[0].local_track.as_ref().map(|track| track.id),
            ),
            (1, 1, Some(matching)),
        );
    }

    #[test]
    fn smart_collection_should_reflect_library_changes_without_member_mutations() {
        let mut connection = test_connection();
        let collection = create_collection(
            &mut connection,
            "Live regex",
            Some(smart_rules(vec![SmartCollectionRule {
                field: SmartCollectionField::Title,
                operator: SmartCollectionOperator::MatchesRegex,
                value: "^live\\s+.+".to_owned(),
            }])),
        )
        .expect("smart collection should be created");
        let before = collection_detail(&connection, &collection.id)
            .expect("empty smart collection should resolve");

        insert_tagged_track(&connection, "live.flac", "LIVE FOREVER", "Artist", 2024);
        let after = collection_detail(&connection, &collection.id)
            .expect("updated smart collection should resolve");
        let listed =
            list_collections(&connection).expect("updated smart collection summary should resolve");

        assert_eq!(
            (
                before.collection.item_count,
                after.collection.item_count,
                listed[0].item_count,
            ),
            (0, 1, 1),
        );
    }

    #[test]
    fn smart_collection_should_filter_tracks_by_rating() {
        let mut connection = test_connection();
        let favorite =
            insert_tagged_track(&connection, "favorite.flac", "Favorite", "Artist", 2024);
        let other = insert_tagged_track(&connection, "other.flac", "Other", "Artist", 2024);
        connection
            .execute(
                "UPDATE local_tracks SET rating = 5 WHERE id = ?1",
                [favorite],
            )
            .expect("rating should update");
        connection
            .execute("UPDATE local_tracks SET rating = 3 WHERE id = ?1", [other])
            .expect("rating should update");
        let collection = create_collection(
            &mut connection,
            "Highly rated",
            Some(smart_rules(vec![SmartCollectionRule {
                field: SmartCollectionField::Rating,
                operator: SmartCollectionOperator::GreaterThanOrEqual,
                value: "4".to_owned(),
            }])),
        )
        .expect("smart collection should be created");

        let detail = collection_detail(&connection, &collection.id)
            .expect("smart collection should resolve");

        assert_eq!(
            detail.items[0].local_track.as_ref().map(|track| track.id),
            Some(favorite),
        );
    }

    #[test]
    fn copy_items_should_snapshot_matches_from_a_smart_collection() {
        let mut connection = test_connection();
        let track_id =
            insert_tagged_track(&connection, "smart-source.flac", "Matched", "Artist", 2024);
        let source = create_collection(
            &mut connection,
            "Smart source",
            Some(smart_rules(vec![SmartCollectionRule {
                field: SmartCollectionField::Title,
                operator: SmartCollectionOperator::Equals,
                value: "Matched".to_owned(),
            }])),
        )
        .expect("smart collection should be created");
        let target = create_collection(&mut connection, "Manual target", None)
            .expect("manual collection should be created");
        let source_item_id = collection_detail(&connection, &source.id)
            .expect("smart collection should resolve")
            .items[0]
            .id
            .clone();

        let mutation = copy_items(&mut connection, &target.id, &source.id, &[source_item_id])
            .expect("smart collection match should copy");
        let target_detail =
            collection_detail(&connection, &target.id).expect("manual collection should resolve");

        assert_eq!(
            (
                mutation.added,
                target_detail.items[0]
                    .local_track
                    .as_ref()
                    .map(|track| track.id),
            ),
            (1, Some(track_id)),
        );
    }

    #[test]
    fn create_smart_collection_should_reject_invalid_regular_expression() {
        let mut connection = test_connection();

        let error = create_collection(
            &mut connection,
            "Broken regex",
            Some(smart_rules(vec![SmartCollectionRule {
                field: SmartCollectionField::Artist,
                operator: SmartCollectionOperator::MatchesRegex,
                value: "[".to_owned(),
            }])),
        )
        .expect_err("invalid regex should fail");

        assert!(error.to_string().contains("invalid regular expression"));
    }

    #[test]
    fn create_smart_collection_should_not_limit_the_number_of_rules() {
        let mut connection = test_connection();
        let rules = (0..33)
            .map(|index| SmartCollectionRule {
                field: SmartCollectionField::Title,
                operator: SmartCollectionOperator::DoesNotContain,
                value: format!("excluded-{index}"),
            })
            .collect();

        let collection =
            create_collection(&mut connection, "Unlimited rules", Some(smart_rules(rules)))
                .expect("more than 32 rules should be accepted");

        assert_eq!(
            collection
                .smart_rules
                .expect("rules should persist")
                .rules
                .len(),
            33,
        );
    }

    #[test]
    fn smart_collection_should_reject_manual_member_mutations() {
        let mut connection = test_connection();
        let track_id = insert_local_track(&connection);
        let collection = create_collection(
            &mut connection,
            "Automatic",
            Some(smart_rules(vec![SmartCollectionRule {
                field: SmartCollectionField::Title,
                operator: SmartCollectionOperator::Contains,
                value: "One".to_owned(),
            }])),
        )
        .expect("smart collection should be created");

        let error = add_local_tracks(&mut connection, &collection.id, &[track_id])
            .expect_err("manual add should fail");

        assert_eq!(
            error.to_string(),
            "smart collection members are managed by its rules"
        );
    }
}
