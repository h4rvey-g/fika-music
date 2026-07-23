use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

#[cfg(test)]
const CURRENT_SCHEMA_VERSION: i64 = 6;

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
    connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
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
}
