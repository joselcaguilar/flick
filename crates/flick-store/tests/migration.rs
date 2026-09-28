use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use flick_store::Store;
use rusqlite::{OptionalExtension, params};
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

fn unique_test_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(env::current_dir()?
        .join("target")
        .join("flick-store-tests")
        .join(format!("{}-{ts}", std::process::id())))
}
