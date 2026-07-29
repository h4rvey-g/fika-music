use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use uuid::Uuid;

use crate::online_music::OnlineTrack;
use crate::{library, local_track_from_row, now_timestamp, LocalTrack};

const MAX_COLLECTION_NAME_CHARS: usize = 80;

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
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),
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

const COLLECTION_SUMMARY_SQL: &str = "
    SELECT
        collection.id,
        collection.name,
        COUNT(item.id) AS item_count,
        COALESCE(SUM(CASE WHEN item.item_kind = 'local' THEN 1 ELSE 0 END), 0) AS local_count,
        COALESCE(SUM(CASE WHEN item.item_kind = 'online' THEN 1 ELSE 0 END), 0) AS online_count,
        collection.created_at,
        collection.updated_at
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
        play_count
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
    let collections = statement
        .query_map([], collection_summary_from_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(CollectionError::from)?;
    Ok(collections)
}

pub fn create_collection(
    connection: &mut Connection,
    name: &str,
) -> Result<MusicCollectionSummary, CollectionError> {
    let name = normalize_name(name)?;
    let transaction = connection.transaction()?;
    ensure_name_available(&transaction, &name, None)?;
    let id = Uuid::new_v4().to_string();
    let timestamp = now_timestamp();
    transaction.execute(
        "INSERT INTO music_collections (id, name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![id, name, timestamp],
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
    let collection = collection_summary(connection, collection_id)?;
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
    ensure_collection_exists(&transaction, collection_id)?;
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
    ensure_collection_exists(&transaction, collection_id)?;
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
    let transaction = connection.transaction()?;
    ensure_collection_exists(&transaction, target_collection_id)?;
    ensure_collection_exists(&transaction, source_collection_id)?;
    let mut position = next_position(&transaction, target_collection_id)?;
    let mut added = 0_i64;
    let timestamp = now_timestamp();

    for item_id in item_ids {
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO music_collection_items (
                id, collection_id, position, item_kind, entry_key,
                local_track_id, online_track_json, added_at
             )
             SELECT ?1, ?2, ?3, item_kind, entry_key,
                    local_track_id, online_track_json, ?4
             FROM music_collection_items
             WHERE collection_id = ?5 AND id = ?6",
            params![
                Uuid::new_v4().to_string(),
                target_collection_id,
                position,
                timestamp,
                source_collection_id,
                item_id,
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
    ensure_collection_exists(connection, collection_id)?;
    let selected = item_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut statement = connection.prepare(
        "SELECT
            track.id,
            track.file_path,
            track.file_name,
            track.title,
            track.artist,
            track.album,
            track.album_artist,
            track.genre,
            track.year,
            track.codec,
            track.bitrate_kbps,
            track.sample_rate_hz,
            track.duration_seconds,
            track.track_number,
            track.disc_number,
            track.file_size_bytes,
            track.modified_at,
            track.indexed_at,
            track.play_count,
            item.id
         FROM music_collection_items item
         JOIN local_tracks track ON track.id = item.local_track_id
         WHERE item.collection_id = ?1
           AND item.item_kind = 'local'
         ORDER BY item.position, item.added_at, item.id",
    )?;
    let tracks = statement
        .query_map([collection_id], |row| {
            let item_id = row.get::<_, String>(19)?;
            selected
                .contains(item_id.as_str())
                .then(|| local_track_from_row(row))
                .transpose()
        })?
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tracks)
}

pub fn remove_items(
    connection: &mut Connection,
    collection_id: &str,
    item_ids: &[String],
) -> Result<MusicCollectionMutation, CollectionError> {
    let transaction = connection.transaction()?;
    ensure_collection_exists(&transaction, collection_id)?;
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
    let sql = format!(
        "{COLLECTION_SUMMARY_SQL}
         WHERE collection.id = ?1
         GROUP BY collection.id"
    );
    connection
        .query_row(&sql, [collection_id], collection_summary_from_row)
        .optional()?
        .ok_or_else(|| CollectionError::NotFound(collection_id.to_owned()))
}

fn collection_summary_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MusicCollectionSummary> {
    Ok(MusicCollectionSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        item_count: row.get(2)?,
        local_count: row.get(3)?,
        online_count: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
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

        let collection = create_collection(&mut connection, "  Road Trip  ")
            .expect("collection should be created");
        let error =
            create_collection(&mut connection, "road trip").expect_err("duplicate should fail");

        assert_eq!(
            (collection.name.as_str(), error.to_string().as_str()),
            ("Road Trip", "a collection named 'road trip' already exists"),
        );
    }

    #[test]
    fn add_tracks_should_persist_local_and_online_items_without_duplicates() {
        let mut connection = test_connection();
        let track_id = insert_local_track(&connection);
        let collection =
            create_collection(&mut connection, "Mixed").expect("collection should be created");

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
        let collection =
            create_collection(&mut connection, "Temporary").expect("collection should be created");
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
            create_collection(&mut connection, "Source").expect("source should be created");
        let target =
            create_collection(&mut connection, "Target").expect("target should be created");
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
        let collection =
            create_collection(&mut connection, "Lookup").expect("collection should be created");
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
}
