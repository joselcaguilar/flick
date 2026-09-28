use std::{error::Error, fs, path::PathBuf, str::FromStr};

use flick_core::{AnchorId, BuiltinGesture, GestureId, GesturePhase, SelectionState};
use flick_gestures::{
    ExpectedEvent, GestureEngineConfig, GestureMapping, ReplayRunner, TargetMode, TriggerMode,
    compare_expected, read_expected_str, read_jsonl_str,
};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn replay_synthetic_gesture_fixtures() -> TestResult {
    for case in fixture_cases()? {
        let records = read_jsonl_str(&fs::read_to_string(path(&case.jsonl))?)?;
        let expected = read_expected_str(&fs::read_to_string(path(&case.expected))?)?;
        let mut runner = ReplayRunner::new(case.config);
        let outcome = runner.run_records(&records, &case.selection);
        compare_expected(&expected, &outcome.events)?;
    }
    Ok(())
}

#[test]
fn pinch_dial_fixture_is_monotonic_and_brief_pinches_do_not_fire() -> TestResult {
    let records = read_jsonl_str(&fs::read_to_string(path(
        "tools/fixtures/positives/pinch_dial.jsonl",
    ))?)?;
    let expected = read_expected_str(&fs::read_to_string(path(
        "tools/fixtures/positives/pinch_dial.expected.json",
    ))?)?;
    let values: Vec<i32> = expected
        .iter()
        .filter(|event| matches!(event.phase, GesturePhase::Fired | GesturePhase::Update))
        .filter_map(|event| event.value_milli)
        .collect();
    assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));

    let brief: Vec<_> = records.into_iter().take(4).collect();
    let mut runner = ReplayRunner::new(pinch_config());
    let outcome = runner.run_records(&brief, &SelectionState::Idle);
    assert!(outcome.events.is_empty());
    Ok(())
}

struct FixtureCase {
    jsonl: String,
    expected: String,
    selection: SelectionState,
    config: GestureEngineConfig,
}

fn fixture_cases() -> Result<Vec<FixtureCase>, Box<dyn Error>> {
    let mut cases = vec![
        FixtureCase {
            jsonl: "tools/fixtures/landmarks/owner_fan_circle.jsonl".to_owned(),
            expected: "tools/fixtures/landmarks/owner_fan_circle.expected.json".to_owned(),
            selection: selected_fan()?,
            config: config_for(&[BuiltinGesture::CircleAny], TargetMode::Either),
        },
        FixtureCase {
            jsonl: "tools/fixtures/landmarks/owner_fan_stop.jsonl".to_owned(),
            expected: "tools/fixtures/landmarks/owner_fan_stop.expected.json".to_owned(),
            selection: selected_fan()?,
            config: config_for(&[BuiltinGesture::TwoHandSeparate], TargetMode::Either),
        },
    ];
    for (name, gesture) in [
        ("swipe_left", BuiltinGesture::SwipeLeft),
        ("swipe_right", BuiltinGesture::SwipeRight),
        ("swipe_up", BuiltinGesture::SwipeUp),
        ("swipe_down", BuiltinGesture::SwipeDown),
        ("circle_cw", BuiltinGesture::CircleCw),
        ("circle_ccw", BuiltinGesture::CircleCcw),
        (
            "two_hand_separate_horizontal",
            BuiltinGesture::TwoHandSeparate,
        ),
    ] {
        cases.push(FixtureCase {
            jsonl: format!("tools/fixtures/positives/{name}.jsonl"),
            expected: format!("tools/fixtures/positives/{name}.expected.json"),
            selection: SelectionState::Idle,
            config: config_for(&[gesture], TargetMode::Global),
        });
    }
    cases.push(FixtureCase {
        jsonl: "tools/fixtures/positives/pinch_dial.jsonl".to_owned(),
        expected: "tools/fixtures/positives/pinch_dial.expected.json".to_owned(),
        selection: SelectionState::Idle,
        config: pinch_config(),
    });
    cases.push(FixtureCase {
        jsonl: "tools/fixtures/negatives/relaxed_motion.jsonl".to_owned(),
        expected: "tools/fixtures/negatives/relaxed_motion.expected.json".to_owned(),
        selection: SelectionState::Idle,
        config: config_for(
            &[
                BuiltinGesture::SwipeLeft,
                BuiltinGesture::SwipeRight,
                BuiltinGesture::SwipeUp,
                BuiltinGesture::SwipeDown,
                BuiltinGesture::CircleAny,
                BuiltinGesture::TwoHandSeparate,
                BuiltinGesture::PinchDial,
            ],
            TargetMode::Global,
        ),
    });
    Ok(cases)
}

fn config_for(gestures: &[BuiltinGesture], target_mode: TargetMode) -> GestureEngineConfig {
    let mut config = GestureEngineConfig::default();
    config.trigger.mappings = gestures
        .iter()
        .map(|gesture| {
            let mut mapping = GestureMapping::tap(GestureId::Builtin(*gesture));
            mapping.target_mode = target_mode;
            mapping.allow_two_hands = matches!(gesture, BuiltinGesture::TwoHandSeparate);
            mapping.cooldown_ms = match gesture {
                BuiltinGesture::SwipeLeft
                | BuiltinGesture::SwipeRight
                | BuiltinGesture::SwipeUp
                | BuiltinGesture::SwipeDown => 600,
                BuiltinGesture::CircleCw
                | BuiltinGesture::CircleCcw
                | BuiltinGesture::CircleAny => 700,
                BuiltinGesture::TwoHandSeparate => 800,
                _ => 1_000,
            };
            mapping
        })
        .collect();
    config
}

fn pinch_config() -> GestureEngineConfig {
    let mut config = config_for(&[BuiltinGesture::PinchDial], TargetMode::Global);
    if let Some(mapping) = config.trigger.mappings.first_mut() {
        mapping.mode = TriggerMode::Dial;
        mapping.cooldown_ms = 0;
    }
    config
}

fn selected_fan() -> Result<SelectionState, Box<dyn Error>> {
    Ok(SelectionState::Selected {
        anchor_id: AnchorId::from_str("01J00000000000000000000002")?,
        domain: "fan".to_owned(),
        expires_at_ms: 4_000,
    })
}

fn path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[allow(dead_code)]
fn assert_event_shape(events: &[ExpectedEvent]) {
    assert!(!events.is_empty());
}
