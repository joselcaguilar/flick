use std::{
    env, fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use flick_store::{Store, configure_connection, snapshot_database};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

#[test]
fn initial_migration_supports_previous_queries_and_cascades_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_test_dir()?;
    fs::create_dir_all(&root)?;
    let db_path = root.join("flick.db");
    let store = Store::open_path(&db_path)?;

    {
        let conn = store.connection();
        let thumb_up_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM gestures WHERE id = 'builtin.thumb_up'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(thumb_up_count, 1);

        conn.execute(
            "INSERT INTO cameras (id, name, kind, created_at, updated_at) VALUES (?1, ?2, 'local', 1, 1)",
            params!["camera-1", "Camera"],
        )?;
        conn.execute(
            "INSERT INTO places (id, camera_id, name, scene_signature, embedder_version, intrinsics, active, created_at, updated_at) \
             VALUES (?1, ?2, ?3, zeroblob(1536), ?4, ?5, 1, 1, 1)",
            params!["place-1", "camera-1", "Desk", "dinov2-small@1", json!({"hfov_deg": 70}).to_string()],
        )?;
        conn.execute(
            "INSERT INTO anchors (id, place_id, name, target, domain, kind, uncertainty_deg, verb_params, estimator_version, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 'fan', 'direction', 4.0, ?5, ?6, 1, 1)",
            params!["anchor-1", "place-1", "Fan", json!({"entity_id": "fan.desk"}).to_string(), json!({"levels": [1]}).to_string(), "ray.v1"],
        )?;
        conn.execute(
            "INSERT INTO mappings (id, name, gesture_id, target_mode, anchor_id, action, created_at, updated_at) \
             VALUES (?1, ?2, 'builtin.circle_cw', 'anchor', ?3, ?4, 1, 1)",
            params!["mapping-1", "Fan level", "anchor-1", json!({"kind": "verb", "verb": "level_set", "level": 1}).to_string()],
        )?;

        conn.query_row(
            "SELECT key, value FROM settings WHERE key = ?1",
            params!["missing"],
            |_| Ok(()),
        )
        .optional()?;
        let _: String = conn.query_row(
            "SELECT id FROM mappings WHERE id = ?1",
            params!["mapping-1"],
            |row| row.get(0),
        )?;
    }

    assert_eq!(store.delete_place("place-1")?, 1);
    let conn = store.connection();
    let anchor_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM anchors WHERE place_id = ?1",
        params!["place-1"],
        |row| row.get(0),
    )?;
    let mapping_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mappings WHERE id = ?1",
        params!["mapping-1"],
        |row| row.get(0),
    )?;
    assert_eq!(anchor_count, 0);
    assert_eq!(mapping_count, 0);

    drop(conn);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn snapshot_from_live_wal_contains_committed_rows() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_test_dir()?;
    fs::create_dir_all(&root)?;
    let db_path = root.join("flick.db");
    let store = Store::open_path(&db_path)?;
    store
        .settings()
        .set("committed", &json!({"visible": true}), 1)?;

    let writer = Connection::open(&db_path)?;
    configure_connection(&writer)?;
    writer.execute("BEGIN IMMEDIATE", [])?;
    writer.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
        params!["uncommitted", json!({"visible": false}).to_string(), 2],
    )?;

    let snapshot = snapshot_database(&db_path, &root.join("backups"))?;
    writer.execute("ROLLBACK", [])?;

    let snapshot_conn = Connection::open(snapshot)?;
    let committed: String = snapshot_conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params!["committed"],
        |row| row.get(0),
    )?;
    let uncommitted: Option<String> = snapshot_conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params!["uncommitted"],
            |row| row.get(0),
        )
        .optional()?;
    assert_eq!(committed, json!({"visible": true}).to_string());
    assert!(uncommitted.is_none());

    drop(snapshot_conn);
    drop(writer);
    drop(store);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn additive_future_schema_opens_without_rollback() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_test_dir()?;
    fs::create_dir_all(&root)?;
    let db_path = root.join("flick.db");
    let store = Store::open_path(&db_path)?;
    drop(store);
    {
        let conn = Connection::open(&db_path)?;
        configure_connection(&conn)?;
        conn.execute("CREATE TABLE additive_future (id INTEGER PRIMARY KEY)", [])?;
        // Far past any shipped migration, so adding one never makes this version pending.
        conn.pragma_update(None, "user_version", 100i64)?;
    }

    let reopened = Store::open_path(&db_path)?;
    let count: i64 = reopened.connection().query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'additive_future'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);

    drop(reopened);
    fs::remove_dir_all(root)?;
    Ok(())
}

fn unique_test_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Parallel tests share a PID and macOS clocks tick in microseconds, so add a counter.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(env::current_dir()?
        .join("target")
        .join("flick-store-tests")
        .join(format!("{}-{ts}-{seq}", std::process::id())))
}
