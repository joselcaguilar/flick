use flick_store::Store;
use rusqlite::params;
use serde_json::json;

#[test]
fn activity_lists_newest_first_and_hides_suppressed() -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open_memory()?;
    {
        let conn = store.connection();
        for (id, ts, status) in [
            ("01A", 1_000, "ok"),
            ("01B", 2_000, "suppressed"),
            ("01C", 3_000, "error"),
            ("01D", 3_000, "ok"),
        ] {
            conn.execute(
                "INSERT INTO activity_log (id, ts, gesture_id, confidence, status, latency) VALUES (?1, ?2, 'builtin.thumb_up', 0.91, ?3, ?4)",
                params![id, ts, status, json!({"ha_ms": 13.0}).to_string()],
            )?;
        }
    }
    let activity = store.activity();

    let ids = |rows: Vec<flick_store::ActivityRow>| {
        rows.into_iter().map(|row| row.id).collect::<Vec<_>>()
    };
    assert_eq!(
        ids(activity.list(10, None, None, false)?),
        ["01D", "01C", "01A"]
    );
    assert_eq!(
        ids(activity.list(10, None, None, true)?),
        ["01D", "01C", "01B", "01A"]
    );
    assert_eq!(
        ids(activity.list(10, None, Some("suppressed"), false)?),
        ["01B"]
    );
    assert_eq!(ids(activity.list(1, Some("01D"), None, false)?), ["01C"]);
    assert_eq!(ids(activity.list(10, Some("01C"), None, false)?), ["01A"]);

    let newest = activity.list(1, None, None, false)?.remove(0);
    assert_eq!(newest.ts_ms, 3_000);
    assert_eq!(newest.confidence, Some(0.91));
    assert_eq!(newest.latency, Some(json!({"ha_ms": 13.0})));
    Ok(())
}
