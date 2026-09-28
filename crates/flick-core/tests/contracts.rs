use flick_core::{Action, GestureId};
use serde_json::json;

#[test]
fn gesture_id_parse_display_contract() {
    let valid = [
        "builtin.closed_fist",
        "builtin.open_palm",
        "builtin.pointing_up",
        "builtin.thumb_up",
        "builtin.thumb_down",
        "builtin.victory",
        "builtin.i_love_you",
        "builtin.point",
        "builtin.swipe_left",
        "builtin.swipe_right",
        "builtin.swipe_up",
        "builtin.swipe_down",
        "builtin.pinch_dial",
        "builtin.circle_cw",
        "builtin.circle_ccw",
        "builtin.circle_any",
        "builtin.two_hand_separate",
        "custom.01K6AFF2Q4AGF2B3H2T4PP93Y5",
        "motion.01K6AFF2Q4AGF2B3H2T4PP93Y5",
        "system.none",
    ];

    for input in valid {
        let parsed = input.parse::<GestureId>().map(|id| id.to_string());
        assert_eq!(parsed.as_deref(), Ok(input));
    }

    for input in [
        "",
        "thumb_up",
        "builtin.unknown",
        "custom.not-a-ulid",
        "system.pause",
    ] {
        assert!(
            input.parse::<GestureId>().is_err(),
            "{input} should be rejected"
        );
    }
}

#[test]
fn action_json_round_trips_spec_examples() -> Result<(), serde_json::Error> {
    let cases = [
        json!({
            "kind": "call_service",
            "domain": "media_player",
            "service": "media_next_track",
            "target": { "entity_id": ["media_player.living_room"] },
            "data": {},
            "preset": "media.next"
        }),
        json!({
            "kind": "dial",
            "entity_id": "light.living_room",
            "property": "brightness_pct",
            "gain": 1.0,
            "min": 1,
            "max": 100
        }),
        json!({ "kind": "verb", "verb": "up" }),
        json!({ "kind": "verb", "verb": "level_set", "level": 1 }),
        json!({
            "kind": "dial",
            "entity_id": "$selected",
            "property": "percentage",
            "gain": 1.0
        }),
    ];

    for value in cases {
        let action: Action = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(action)?, value);
    }
    Ok(())
}
