use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use std::sync::{Arc, Mutex};

#[cfg(test)]
const CURRENT_SCHEMA_VERSION: i64 = 11;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CredentialStoreError {
    #[error("credential database lock was poisoned")]
    LockPoisoned,
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

pub(crate) struct AppCredentialStore {
    connection: Arc<Mutex<Connection>>,
    provider_id: &'static str,
}

impl AppCredentialStore {
    pub(crate) fn new(connection: Arc<Mutex<Connection>>, provider_id: &'static str) -> Self {
        Self {
            connection,
            provider_id,
        }
    }

    pub(crate) fn save_secret(
        &self,
        account_ref: &str,
        secret: &str,
    ) -> Result<(), CredentialStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| CredentialStoreError::LockPoisoned)?;
        connection.execute(
            "INSERT INTO account_credentials (provider_id, account_ref, secret)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(provider_id, account_ref) DO UPDATE SET
                 secret = excluded.secret",
            params![self.provider_id, account_ref, secret],
        )?;
        Ok(())
    }

    pub(crate) fn load_secret(
        &self,
        account_ref: &str,
    ) -> Result<Option<String>, CredentialStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| CredentialStoreError::LockPoisoned)?;
        connection
            .query_row(
                "SELECT secret FROM account_credentials
                 WHERE provider_id = ?1 AND account_ref = ?2",
                params![self.provider_id, account_ref],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn delete_secret(&self, account_ref: &str) -> Result<(), CredentialStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| CredentialStoreError::LockPoisoned)?;
        connection.execute(
            "DELETE FROM account_credentials WHERE provider_id = ?1 AND account_ref = ?2",
            params![self.provider_id, account_ref],
        )?;
        Ok(())
    }
}

const INITIAL_SCHEMA: &str = "
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

    CREATE TABLE IF NOT EXISTS plugin_states (
        plugin_id TEXT PRIMARY KEY,
        package_path TEXT NOT NULL,
        origin TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 0,
        permissions_reviewed INTEGER NOT NULL DEFAULT 0,
        granted_capabilities TEXT NOT NULL DEFAULT '[]',
        installed_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS plugin_diagnostics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_id TEXT NOT NULL,
        code TEXT NOT NULL,
        level TEXT NOT NULL,
        source_id TEXT,
        message TEXT NOT NULL,
        timestamp INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_plugin_diagnostics_plugin
        ON plugin_diagnostics(plugin_id, id);

    CREATE TABLE IF NOT EXISTS netease_accounts (
        account_ref TEXT PRIMARY KEY,
        provider_id TEXT NOT NULL,
        user_id TEXT NOT NULL UNIQUE,
        display_name TEXT NOT NULL,
        avatar_url TEXT,
        status TEXT NOT NULL DEFAULT 'active',
        connected_at INTEGER NOT NULL,
        last_verified_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS netease_mutation_audit (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        account_ref TEXT NOT NULL,
        operation TEXT NOT NULL,
        playlist_id TEXT NOT NULL,
        track_id TEXT NOT NULL,
        outcome TEXT NOT NULL,
        message TEXT,
        occurred_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_netease_audit_account_time
        ON netease_mutation_audit(account_ref, occurred_at DESC);
";

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(INITIAL_SCHEMA),
        M::up_with_hook("", |transaction| {
            let has_manifest_fingerprint = transaction.query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('plugin_states')
                 WHERE name = 'manifest_fingerprint'",
                [],
                |row| row.get::<_, bool>(0),
            )?;
            if !has_manifest_fingerprint {
                transaction.execute(
                    "ALTER TABLE plugin_states
                     ADD COLUMN manifest_fingerprint TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            Ok(())
        }),
        M::up_with_hook("", |transaction| {
            add_column_if_missing(transaction, "local_tracks", "album_artist", "TEXT")?;
            add_column_if_missing(transaction, "local_tracks", "genre", "TEXT")?;
            add_column_if_missing(transaction, "local_tracks", "year", "INTEGER")?;
            add_column_if_missing(transaction, "local_tracks", "codec", "TEXT")?;
            add_column_if_missing(transaction, "local_tracks", "bitrate_kbps", "INTEGER")?;
            add_column_if_missing(transaction, "local_tracks", "sample_rate_hz", "INTEGER")?;
            add_column_if_missing(
                transaction,
                "local_tracks",
                "play_count",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            add_column_if_missing(
                transaction,
                "local_tracks",
                "metadata_version",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            Ok(())
        }),
        M::up(
            "
            CREATE TABLE IF NOT EXISTS app_settings (
                setting_key TEXT PRIMARY KEY,
                setting_value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS album_art_lookups (
                group_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                release_group_id TEXT,
                candidates_json TEXT,
                message TEXT,
                checked_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_album_art_lookups_status
                ON album_art_lookups(status, checked_at);
            ",
        ),
        M::up_with_hook("", |transaction| {
            add_column_if_missing(
                transaction,
                "album_art_lookups",
                "written_tracks",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            add_column_if_missing(
                transaction,
                "album_art_lookups",
                "failed_tracks",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            Ok(())
        }),
        M::up(
            "
            CREATE TABLE IF NOT EXISTS audio_source_states (
                audio_source_id TEXT PRIMARY KEY,
                package_path TEXT NOT NULL,
                manifest_fingerprint TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 0,
                permissions_reviewed INTEGER NOT NULL DEFAULT 0,
                granted_capabilities TEXT NOT NULL DEFAULT '[]',
                installed_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audio_source_diagnostics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                audio_source_id TEXT NOT NULL,
                code TEXT NOT NULL,
                level TEXT NOT NULL,
                source_id TEXT,
                message TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_audio_source_diagnostics_source
                ON audio_source_diagnostics(audio_source_id, id);
            ",
        ),
        M::up(
            "
            CREATE TABLE IF NOT EXISTS kugou_accounts (
                account_ref TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                user_id TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                avatar_url TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                connected_at INTEGER NOT NULL,
                last_verified_at INTEGER NOT NULL
            );
            ",
        ),
        M::up(
            "
            CREATE TABLE IF NOT EXISTS online_search_history (
                normalized_query TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                searched_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_online_search_history_time
                ON online_search_history(searched_at DESC);

            CREATE TABLE IF NOT EXISTS online_download_tasks (
                task_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                state TEXT NOT NULL,
                destination TEXT NOT NULL,
                total_items INTEGER NOT NULL,
                completed_items INTEGER NOT NULL DEFAULT 0,
                skipped_items INTEGER NOT NULL DEFAULT 0,
                failed_items INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS online_download_items (
                item_id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                state TEXT NOT NULL,
                track_json TEXT NOT NULL,
                target_path TEXT,
                message TEXT,
                bytes_downloaded INTEGER NOT NULL DEFAULT 0,
                total_bytes INTEGER,
                etag TEXT,
                last_modified TEXT,
                temporary_path TEXT,
                FOREIGN KEY(task_id) REFERENCES online_download_tasks(task_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_online_download_items_task_position
                ON online_download_items(task_id, position);
            ",
        ),
        M::up(
            "ALTER TABLE online_download_tasks
             ADD COLUMN selected_audio_source_id TEXT;",
        ),
        M::up(
            "
            CREATE TABLE IF NOT EXISTS account_credentials (
                provider_id TEXT NOT NULL,
                account_ref TEXT NOT NULL,
                secret TEXT NOT NULL,
                PRIMARY KEY(provider_id, account_ref)
            );

            UPDATE netease_accounts SET status = 'expired' WHERE status = 'active';
            UPDATE kugou_accounts SET status = 'expired' WHERE status = 'active';
            ",
        ),
        M::up(
            "
            CREATE TABLE IF NOT EXISTS music_collections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS music_collection_items (
                id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                item_kind TEXT NOT NULL CHECK(item_kind IN ('local', 'online')),
                entry_key TEXT NOT NULL,
                local_track_id INTEGER,
                online_track_json TEXT,
                added_at INTEGER NOT NULL,
                FOREIGN KEY(collection_id) REFERENCES music_collections(id) ON DELETE CASCADE,
                FOREIGN KEY(local_track_id) REFERENCES local_tracks(id) ON DELETE CASCADE,
                UNIQUE(collection_id, entry_key),
                UNIQUE(collection_id, position),
                CHECK(
                    (item_kind = 'local' AND local_track_id IS NOT NULL AND online_track_json IS NULL)
                    OR
                    (item_kind = 'online' AND local_track_id IS NULL AND online_track_json IS NOT NULL)
                )
            );

            CREATE INDEX IF NOT EXISTS idx_music_collection_items_collection_position
                ON music_collection_items(collection_id, position);
            ",
        ),
    ])
}

fn add_column_if_missing(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    let exists = transaction.query_row(
        &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"),
        [column],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        transaction.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

pub fn initialize(connection: &mut Connection) -> Result<(), rusqlite_migration::Error> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON;",
    )?;
    migrations().to_latest(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(connection: &Connection) -> i64 {
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("schema version should be readable")
    }

    fn has_manifest_fingerprint(connection: &Connection) -> bool {
        connection
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('plugin_states')
                 WHERE name = 'manifest_fingerprint'",
                [],
                |row| row.get(0),
            )
            .expect("plugin schema should be readable")
    }

    fn has_library_column(connection: &Connection, column: &str) -> bool {
        has_column(connection, "local_tracks", column)
    }

    fn has_column(connection: &Connection, table: &str, column: &str) -> bool {
        connection
            .query_row(
                &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"),
                [column],
                |row| row.get(0),
            )
            .expect("library schema should be readable")
    }

    fn has_table(connection: &Connection, table: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .expect("table existence should be readable")
    }

    #[test]
    fn initialize_should_create_the_latest_schema_for_a_new_database() {
        let mut connection = Connection::open_in_memory().expect("database should open");

        initialize(&mut connection).expect("migrations should run");

        assert_eq!(user_version(&connection), CURRENT_SCHEMA_VERSION);
        assert!(has_manifest_fingerprint(&connection));
        assert!(has_library_column(&connection, "play_count"));
        assert!(has_library_column(&connection, "metadata_version"));
        assert!(has_table(&connection, "app_settings"));
        assert!(has_table(&connection, "album_art_lookups"));
        assert!(has_table(&connection, "audio_source_states"));
        assert!(has_table(&connection, "audio_source_diagnostics"));
        assert!(has_table(&connection, "kugou_accounts"));
        assert!(has_table(&connection, "online_search_history"));
        assert!(has_table(&connection, "online_download_tasks"));
        assert!(has_table(&connection, "online_download_items"));
        assert!(has_table(&connection, "account_credentials"));
        assert!(has_table(&connection, "music_collections"));
        assert!(has_table(&connection, "music_collection_items"));
        assert!(has_column(
            &connection,
            "album_art_lookups",
            "failed_tracks"
        ));
    }

    #[test]
    fn initialize_should_upgrade_a_legacy_plugin_schema() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        connection
            .execute_batch(INITIAL_SCHEMA)
            .expect("legacy schema should initialize");

        initialize(&mut connection).expect("legacy schema should migrate");

        assert_eq!(user_version(&connection), CURRENT_SCHEMA_VERSION);
        assert!(has_manifest_fingerprint(&connection));
        assert!(has_library_column(&connection, "album_artist"));
    }

    #[test]
    fn initialize_should_adopt_an_unversioned_current_schema() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        connection
            .execute_batch(INITIAL_SCHEMA)
            .expect("legacy schema should initialize");
        connection
            .execute(
                "ALTER TABLE plugin_states
                 ADD COLUMN manifest_fingerprint TEXT NOT NULL DEFAULT ''",
                [],
            )
            .expect("current column should be added");

        initialize(&mut connection).expect("current schema should be adopted");

        assert_eq!(user_version(&connection), CURRENT_SCHEMA_VERSION);
        assert!(has_manifest_fingerprint(&connection));
        assert!(has_library_column(&connection, "sample_rate_hz"));
    }

    #[test]
    fn initialize_should_upgrade_version_three_with_album_art_tables() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        migrations()
            .to_version(&mut connection, 3)
            .expect("version three should initialize");

        initialize(&mut connection).expect("latest migration should run");

        assert_eq!(
            (
                user_version(&connection),
                has_table(&connection, "app_settings"),
                has_table(&connection, "album_art_lookups"),
                has_column(&connection, "album_art_lookups", "written_tracks"),
            ),
            (CURRENT_SCHEMA_VERSION, true, true, true),
        );
    }

    #[test]
    fn initialize_should_upgrade_version_four_without_losing_album_art_lookups() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        migrations()
            .to_version(&mut connection, 4)
            .expect("version four should initialize");
        connection
            .execute(
                "INSERT INTO album_art_lookups (
                    group_id, status, release_group_id, candidates_json, message, checked_at
                 ) VALUES ('album-1', 'partial', 'release-1', NULL, 'one failed', 1)",
                [],
            )
            .expect("album lookup should insert");

        initialize(&mut connection).expect("latest migration should run");

        let lookup = connection
            .query_row(
                "SELECT status, message, written_tracks, failed_tracks
                 FROM album_art_lookups WHERE group_id = 'album-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .expect("album lookup should remain readable");

        assert_eq!(
            (user_version(&connection), lookup),
            (
                CURRENT_SCHEMA_VERSION,
                ("partial".to_owned(), "one failed".to_owned(), 0, 0),
            ),
        );
    }

    #[test]
    fn initialize_should_upgrade_version_nine_with_account_credentials() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        migrations()
            .to_version(&mut connection, 9)
            .expect("version nine should initialize");
        connection
            .execute(
                "INSERT INTO netease_accounts (
                    account_ref, provider_id, user_id, display_name, status,
                    connected_at, last_verified_at
                 ) VALUES ('netease-account:1', 'fika-netease', '1', 'NetEase', 'active', 1, 1)",
                [],
            )
            .expect("legacy NetEase account should insert");
        connection
            .execute(
                "INSERT INTO kugou_accounts (
                    account_ref, provider_id, user_id, display_name, status,
                    connected_at, last_verified_at
                 ) VALUES ('kugou-account:1', 'fika-kugou', '1', 'KuGou', 'active', 1, 1)",
                [],
            )
            .expect("legacy KuGou account should insert");

        initialize(&mut connection).expect("latest migration should run");
        let active_accounts = connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM netease_accounts WHERE status = 'active') +
                    (SELECT COUNT(*) FROM kugou_accounts WHERE status = 'active')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("migrated account statuses should load");

        assert_eq!(
            (
                user_version(&connection),
                has_table(&connection, "account_credentials"),
                active_accounts,
            ),
            (CURRENT_SCHEMA_VERSION, true, 0)
        );
    }

    #[test]
    fn initialize_should_upgrade_version_ten_with_music_collections() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        migrations()
            .to_version(&mut connection, 10)
            .expect("version ten should initialize");

        initialize(&mut connection).expect("latest migration should run");

        assert_eq!(
            (
                user_version(&connection),
                has_table(&connection, "music_collections"),
                has_table(&connection, "music_collection_items"),
            ),
            (CURRENT_SCHEMA_VERSION, true, true),
        );
    }

    #[test]
    fn app_credential_store_should_persist_and_isolate_provider_secrets() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        initialize(&mut connection).expect("migrations should run");
        let connection = Arc::new(Mutex::new(connection));
        let netease = AppCredentialStore::new(Arc::clone(&connection), "fika-netease");
        let kugou = AppCredentialStore::new(connection, "fika-kugou");

        netease
            .save_secret("account-1", "netease-secret")
            .expect("NetEase secret should persist");
        kugou
            .save_secret("account-1", "kugou-secret")
            .expect("KuGou secret should persist");

        assert_eq!(
            (
                netease.load_secret("account-1").unwrap(),
                kugou.load_secret("account-1").unwrap(),
            ),
            (
                Some("netease-secret".to_owned()),
                Some("kugou-secret".to_owned()),
            )
        );
    }

    #[test]
    fn app_credential_store_should_delete_only_the_requested_secret() {
        let mut connection = Connection::open_in_memory().expect("database should open");
        initialize(&mut connection).expect("migrations should run");
        let store = AppCredentialStore::new(Arc::new(Mutex::new(connection)), "fika-netease");
        store
            .save_secret("account-1", "secret-1")
            .expect("first secret should persist");
        store
            .save_secret("account-2", "secret-2")
            .expect("second secret should persist");

        store
            .delete_secret("account-1")
            .expect("first secret should delete");

        assert_eq!(
            (
                store.load_secret("account-1").unwrap(),
                store.load_secret("account-2").unwrap(),
            ),
            (None, Some("secret-2".to_owned()))
        );
    }
}
