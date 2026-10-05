//! SQLite migrations, repositories and retention for `06-data-model-and-api.md` §§2–3 and §8.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use flick_core::StoreError;
use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};
use serde_json::Value;

const DB_FILE: &str = "flick.db";
const BACKUP_DIR: &str = "backups";
const BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);
const SNAPSHOT_KEEP: usize = 3;

/// Store result type using the core storage error boundary.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Embedded forward-only migrations.
#[must_use]
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/0001_init.sql")),
        M::up(include_str!("../migrations/0002_ha_urls.sql")),
        M::up(include_str!("../migrations/0003_anchor_ray_source.sql")),
        M::up(include_str!("../migrations/0004_anchor_finger_aim.sql")),
        M::up(include_str!("../migrations/0005_anchor_area_override.sql")),
        M::up(include_str!("../migrations/0006_camera_area_override.sql")),
    ])
}

/// SQLite-backed Flick store.
pub struct Store {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

impl Store {
    /// Opens `<data_dir>/flick.db`, creating a snapshot only when a migration is pending.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        fs::create_dir_all(data_dir).map_err(file_error)?;
        let db_path = data_dir.join(DB_FILE);
        if db_path.exists() && non_empty_file(&db_path)? {
            snapshot_if_migration_pending(&db_path, &data_dir.join(BACKUP_DIR))?;
        }
        Self::open_path(db_path)
    }

    /// Opens an explicit SQLite file path and runs migrations.
    pub fn open_path(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_owned();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(file_error)?;
        }
        let mut conn = Connection::open(&db_path).map_err(database_error)?;
        configure_connection(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
        })
    }

    /// Opens an in-memory database for deterministic tests.
    pub fn open_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory().map_err(database_error)?;
        configure_connection(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: PathBuf::from(":memory:"),
        })
    }

    /// Returns the SQLite path backing this store.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Locks the underlying connection for repository operations owned by integration crates.
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Returns a settings repository over the locked connection.
    pub fn settings(&self) -> SettingsRepository<'_> {
        SettingsRepository { store: self }
    }

    /// Returns a maintenance repository over the locked connection.
    pub fn maintenance(&self) -> MaintenanceRepository<'_> {
        MaintenanceRepository { store: self }
    }

    /// Returns an activity-log repository over the locked connection.
    pub fn activity(&self) -> ActivityRepository<'_> {
        ActivityRepository { store: self }
    }

    /// Deletes a place; SQLite cascades anchors and targeted mappings.
    pub fn delete_place(&self, place_id: &str) -> Result<usize> {
        self.maintenance().delete_place(place_id)
    }
}

/// Settings key/value repository.
pub struct SettingsRepository<'a> {
    store: &'a Store,
}

impl SettingsRepository<'_> {
    /// Reads a JSON setting.
    pub fn get(&self, key: &str) -> Result<Option<Value>> {
        let conn = self.store.connection();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(database_error)?
        .map(|raw| serde_json::from_str(&raw).map_err(json_error))
        .transpose()
    }

    /// Lists all settings as decoded JSON.
    pub fn all(&self) -> Result<Vec<(String, Value)>> {
        let conn = self.store.connection();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings ORDER BY key")
            .map_err(database_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(database_error)?;
        let mut settings = Vec::new();
        for row in rows {
            let (key, raw) = row.map_err(database_error)?;
            settings.push((key, serde_json::from_str(&raw).map_err(json_error)?));
        }
        Ok(settings)
    }

    /// Upserts a JSON setting with a millisecond timestamp.
    pub fn set(&self, key: &str, value: &Value, updated_at_ms: i64) -> Result<()> {
        let raw = serde_json::to_string(value).map_err(json_error)?;
        let conn = self.store.connection();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, raw, updated_at_ms],
        )
        .map_err(database_error)?;
        Ok(())
    }
}

/// Store maintenance operations.
pub struct MaintenanceRepository<'a> {
    store: &'a Store,
}

impl MaintenanceRepository<'_> {
    /// Deletes a place and relies on foreign-key cascades for anchors and targeted mappings.
    pub fn delete_place(&self, place_id: &str) -> Result<usize> {
        let conn = self.store.connection();
        conn.execute("DELETE FROM places WHERE id = ?1", params![place_id])
            .map_err(database_error)
    }

    /// Prunes activity rows older than `max_age_ms` and caps total rows to `max_rows`.
    pub fn prune_activity_log(
        &self,
        now_ms: i64,
        max_age_ms: i64,
        max_rows: usize,
    ) -> Result<usize> {
        let mut conn = self.store.connection();
        let tx = conn.transaction().map_err(database_error)?;
        let cutoff = now_ms.saturating_sub(max_age_ms);
        let old = tx
            .execute("DELETE FROM activity_log WHERE ts < ?1", params![cutoff])
            .map_err(database_error)?;
        let extra = tx
            .execute(
                "DELETE FROM activity_log WHERE id IN ( \
                 SELECT id FROM activity_log ORDER BY ts DESC LIMIT -1 OFFSET ?1 \
                 )",
                params![
                    i64::try_from(max_rows).map_err(|err| StoreError::Database(err.to_string()))?
                ],
            )
            .map_err(database_error)?;
        tx.commit().map_err(database_error)?;
        Ok(old + extra)
    }
}

/// One persisted `activity_log` row (`06-data-model-and-api.md` §2).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityRow {
    /// Activity ULID.
    pub id: String,
    /// Unix epoch milliseconds.
    pub ts_ms: i64,
    /// Camera that produced the gesture.
    pub camera_id: Option<String>,
    /// Gesture ID.
    pub gesture_id: Option<String>,
    /// Recognizer confidence in `0..=1`.
    pub confidence: Option<f64>,
    /// Mapping that matched, when any.
    pub mapping_id: Option<String>,
    /// Targeted anchor, when any.
    pub anchor_id: Option<String>,
    /// Human-readable action summary.
    pub action_summary: Option<String>,
    /// Outcome status.
    pub status: String,
    /// Machine-readable reason or error code.
    pub reason: Option<String>,
    /// Human-readable message.
    pub message: Option<String>,
    /// Latency breakdown JSON.
    pub latency: Option<Value>,
}

/// Read access to the activity log.
pub struct ActivityRepository<'a> {
    store: &'a Store,
}

impl ActivityRepository<'_> {
    /// Lists activity newest first, strictly older than `before_id` when given.
    ///
    /// Suppressed rows are hidden unless `include_suppressed` is set or `status` asks for them.
    pub fn list(
        &self,
        limit: usize,
        before_id: Option<&str>,
        status: Option<&str>,
        include_suppressed: bool,
    ) -> Result<Vec<ActivityRow>> {
        let limit = i64::try_from(limit).map_err(|err| StoreError::Database(err.to_string()))?;
        let conn = self.store.connection();
        let mut stmt = conn
            .prepare(
                "SELECT id, ts, camera_id, gesture_id, confidence, mapping_id, anchor_id, \
                 action_summary, status, reason, message, latency FROM activity_log \
                 WHERE (?1 IS NULL OR (ts, id) < (SELECT ts, id FROM activity_log WHERE id = ?1)) \
                 AND (?2 IS NULL OR status = ?2) \
                 AND (?3 OR IFNULL(?2, '') = 'suppressed' OR status <> 'suppressed') \
                 ORDER BY ts DESC, id DESC LIMIT ?4",
            )
            .map_err(database_error)?;
        let rows = stmt
            .query_map(
                params![before_id, status, include_suppressed, limit],
                |row| {
                    Ok((
                        ActivityRow {
                            id: row.get(0)?,
                            ts_ms: row.get(1)?,
                            camera_id: row.get(2)?,
                            gesture_id: row.get(3)?,
                            confidence: row.get(4)?,
                            mapping_id: row.get(5)?,
                            anchor_id: row.get(6)?,
                            action_summary: row.get(7)?,
                            status: row.get(8)?,
                            reason: row.get(9)?,
                            message: row.get(10)?,
                            latency: None,
                        },
                        row.get::<_, Option<String>>(11)?,
                    ))
                },
            )
            .map_err(database_error)?;
        rows.map(|row| {
            let (mut activity, latency) = row.map_err(database_error)?;
            activity.latency = latency
                .map(|raw| serde_json::from_str(&raw))
                .transpose()
                .map_err(json_error)?;
            Ok(activity)
        })
        .collect()
    }
}

/// Applies connection pragmas required by the spec.
pub fn configure_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(BUSY_TIMEOUT).map_err(database_error)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(database_error)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(database_error)?;
    Ok(())
}

/// Creates a consistent SQLite snapshot into the backup directory and prunes older snapshots.
pub fn snapshot_database(db_path: &Path, backup_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(backup_dir).map_err(file_error)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| StoreError::Database(err.to_string()))?
        .as_nanos();
    let snapshot = backup_dir.join(format!(
        "flick-{}-{ts}-{}.db",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    ));
    let conn = Connection::open(db_path).map_err(database_error)?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(database_error)?;
    let snapshot_arg = snapshot
        .to_str()
        .ok_or_else(|| StoreError::Database("snapshot path is not valid UTF-8".to_owned()))?;
    conn.execute("VACUUM main INTO ?1", params![snapshot_arg])
        .map_err(database_error)?;
    prune_snapshots(backup_dir, SNAPSHOT_KEEP)?;
    Ok(snapshot)
}

fn snapshot_if_migration_pending(db_path: &Path, backup_dir: &Path) -> Result<Option<PathBuf>> {
    prune_snapshots(backup_dir, SNAPSHOT_KEEP)?;
    let conn = Connection::open(db_path).map_err(database_error)?;
    configure_connection(&conn)?;
    let pending = migrations()
        .pending_migrations(&conn)
        .map_err(|err| StoreError::Migration(err.to_string()))?;
    if pending > 0 {
        snapshot_database(db_path, backup_dir).map(Some)
    } else {
        Ok(None)
    }
}

fn apply_migrations(conn: &mut Connection) -> Result<()> {
    let migrations = migrations();
    match migrations
        .pending_migrations(conn)
        .map_err(|err| StoreError::Migration(err.to_string()))?
    {
        pending if pending > 0 => migrations
            .to_latest(conn)
            .map_err(|err| StoreError::Migration(err.to_string())),
        pending if pending < 0 => {
            if schema_marked_breaking(conn)? {
                Err(StoreError::Migration(
                    "database was migrated by a breaking future schema".to_owned(),
                ))
            } else {
                tracing::warn!(
                    pending_migrations = pending,
                    "opening database migrated by an additive future schema"
                );
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn schema_marked_breaking(conn: &Connection) -> Result<bool> {
    let has_settings = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'settings'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(database_error)?
        .is_some();
    if !has_settings {
        return Ok(false);
    }
    let Some(raw) = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'schema.breaking_db'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(database_error)?
    else {
        return Ok(false);
    };
    serde_json::from_str::<bool>(&raw).map_err(json_error)
}

fn prune_snapshots(backup_dir: &Path, keep: usize) -> Result<()> {
    if !backup_dir.exists() {
        return Ok(());
    }
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(backup_dir).map_err(file_error)? {
        let entry = entry.map_err(file_error)?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with("flick-") && name.ends_with(".db") {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            snapshots.push((modified, path));
        }
    }
    snapshots.sort_by_key(|(modified, path)| (*modified, path.clone()));
    let remove_count = snapshots.len().saturating_sub(keep);
    for (_, path) in snapshots.into_iter().take(remove_count) {
        fs::remove_file(path).map_err(file_error)?;
    }
    Ok(())
}

fn non_empty_file(path: &Path) -> Result<bool> {
    fs::metadata(path)
        .map(|metadata| metadata.len() > 0)
        .map_err(file_error)
}

fn database_error(err: rusqlite::Error) -> StoreError {
    StoreError::Database(err.to_string())
}

fn json_error(err: serde_json::Error) -> StoreError {
    StoreError::InvalidJson(err.to_string())
}

fn file_error(err: std::io::Error) -> StoreError {
    StoreError::Database(err.to_string())
}
