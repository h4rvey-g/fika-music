use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

#[cfg(test)]
const CURRENT_SCHEMA_VERSION: i64 = 2;

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
    ])
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

    #[test]
    fn initialize_should_create_the_latest_schema_for_a_new_database() {
        let mut connection = Connection::open_in_memory().expect("database should open");

        initialize(&mut connection).expect("migrations should run");

        assert_eq!(user_version(&connection), CURRENT_SCHEMA_VERSION);
        assert!(has_manifest_fingerprint(&connection));
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
    }
}
