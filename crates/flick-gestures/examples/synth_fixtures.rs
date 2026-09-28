//! Deterministic synthetic landmark fixture generator for the gesture crate.

use std::{error::Error, fs, io::Write, path::Path, str::FromStr};

use flick_core::{
    AnchorId, BuiltinGesture, CameraId, GestureId, HandObservation, Handedness, RectF,
    SelectionState, StageTimings,
};
use flick_gestures::{
    GestureEngineConfig, GestureMapping, HandFrameRecord, ReplayRunner, TargetMode, TriggerMode,
};

const CAMERA_ID: &str = "01J00000000000000000000001";
const FAN_ANCHOR_ID: &str = "01J00000000000000000000002";
const FRAME_MS: u64 = 33;
const SCALE: f32 = 0.04;

type DynResult<T> = Result<T, Box<dyn Error>>;

fn main() -> DynResult<()> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = workspace.join("tools/fixtures");
    for dir in ["landmarks", "positives", "negatives", "motion"] {
        fs::create_dir_all(fixtures.join(dir))?;
    }

    write_fixture(
        &fixtures.join("landmarks/owner_fan_circle.jsonl"),
        owner_circle_records(false)?,
        selected_fan()?,
        config_for(&[BuiltinGesture::CircleAny], TargetMode::Either),
    )?;
    write_fixture(
        &fixtures.join("landmarks/owner_fan_stop.jsonl"),
        two_hand_separate_records(false)?,
        selected_fan()?,
        config_for(&[BuiltinGesture::TwoHandSeparate], TargetMode::Either),
    )?;
    for (name, gesture, records) in [
        (
            "swipe_left",
            BuiltinGesture::SwipeLeft,
            swipe_records(BuiltinGesture::SwipeLeft)?,
        ),
        (
            "swipe_right",
            BuiltinGesture::SwipeRight,
            swipe_records(BuiltinGesture::SwipeRight)?,
        ),
        (
            "swipe_up",
            BuiltinGesture::SwipeUp,
            swipe_records(BuiltinGesture::SwipeUp)?,
        ),
        (
            "swipe_down",
            BuiltinGesture::SwipeDown,
            swipe_records(BuiltinGesture::SwipeDown)?,
        ),
        (
            "circle_cw",
            BuiltinGesture::CircleCw,
            owner_circle_records(false)?,
        ),
        (
            "circle_ccw",
            BuiltinGesture::CircleCcw,
            owner_circle_records(true)?,
        ),
    ] {
        write_fixture(
            &fixtures.join(format!("positives/{name}.jsonl")),
            records,
            SelectionState::Idle,
            config_for(&[gesture], TargetMode::Global),
        )?;
    }
    write_fixture(
        &fixtures.join("positives/pinch_dial.jsonl"),
        pinch_dial_records()?,
        SelectionState::Idle,
        pinch_config(),
    )?;
    write_fixture(
        &fixtures.join("positives/two_hand_separate_horizontal.jsonl"),
        two_hand_separate_records(true)?,
        SelectionState::Idle,
        config_for(&[BuiltinGesture::TwoHandSeparate], TargetMode::Global),
    )?;
    write_fixture(
        &fixtures.join("negatives/relaxed_motion.jsonl"),
        relaxed_records()?,
        SelectionState::Idle,
        config_for(
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
    )?;
    write_motion_takes(&fixtures.join("motion/custom_motion_takes.json"))?;
    Ok(())
}

fn write_fixture(
    path: &Path,
    records: Vec<HandFrameRecord>,
    selection: SelectionState,
    config: GestureEngineConfig,
) -> DynResult<()> {
    let mut file = fs::File::create(path)?;
    for record in &records {
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")?;
    }

    let mut runner = ReplayRunner::new(config);
    let outcome = runner.run_records(&records, &selection);
    let expected_path = path.with_extension("expected.json");
    fs::write(
        expected_path,
        format!("{}\n", serde_json::to_string_pretty(&outcome.events)?),
    )?;
    Ok(())
}

fn config_for(gestures: &[BuiltinGesture], target_mode: TargetMode) -> GestureEngineConfig {
    let mut config = GestureEngineConfig::default();
    config.trigger.mappings = gestures
        .iter()
        .map(|gesture| {
            let mut mapping = GestureMapping::tap(GestureId::Builtin(*gesture));
            mapping.target_mode = target_mode;
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
            mapping.allow_two_hands = matches!(gesture, BuiltinGesture::TwoHandSeparate);
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

fn selected_fan() -> DynResult<SelectionState> {
    Ok(SelectionState::Selected {
        anchor_id: AnchorId::from_str(FAN_ANCHOR_ID)?,
        domain: "fan".to_owned(),
        expires_at_ms: 4_000,
    })
}

fn owner_circle_records(ccw: bool) -> DynResult<Vec<HandFrameRecord>> {
    let mut records = Vec::new();
    let radius = 0.085;
    let center = (0.52, 0.48);
    for seq in 0..48 {
        let turn = seq as f32 / 40.0;
        let angle =
            -std::f32::consts::FRAC_PI_2 + if ccw { -turn } else { turn } * std::f32::consts::TAU;
        let index = (
            center.0 + radius * angle.cos(),
            center.1 + radius * angle.sin(),
        );
        records.push(record(
            seq,
            seq * FRAME_MS,
            vec![hand_with_index(Pose::Point, 7, Handedness::Right, index)?],
        )?);
    }
    Ok(records)
}

fn swipe_records(gesture: BuiltinGesture) -> DynResult<Vec<HandFrameRecord>> {
    let (dx, dy) = match gesture {
        BuiltinGesture::SwipeLeft => (-0.22, 0.0),
        BuiltinGesture::SwipeRight => (0.22, 0.0),
        BuiltinGesture::SwipeUp => (0.0, -0.22),
        BuiltinGesture::SwipeDown => (0.0, 0.22),
        _ => (0.0, 0.0),
    };
    let mut records = Vec::new();
    for seq in 0..24 {
        let t = seq as f32 / 23.0;
        let center = (0.50 + dx * t, 0.56 + dy * t);
        records.push(record(
            seq,
            seq * FRAME_MS,
            vec![hand(Pose::OpenPalm, 3, Handedness::Right, center)?],
        )?);
    }
    Ok(records)
}

fn pinch_dial_records() -> DynResult<Vec<HandFrameRecord>> {
    let mut records = Vec::new();
    for seq in 0..42 {
        let pose = if seq < 27 { Pose::Pinch } else { Pose::Relaxed };
        let center_y = 0.66 - (seq.min(26) as f32 * 0.010);
        records.push(record(
            seq,
            seq * FRAME_MS,
            vec![hand(pose, 4, Handedness::Right, (0.52, center_y))?],
        )?);
    }
    Ok(records)
}

fn two_hand_separate_records(horizontal: bool) -> DynResult<Vec<HandFrameRecord>> {
    let mut records = Vec::new();
    for seq in 0_u64..28 {
        let progress = seq.saturating_sub(5) as f32 / 22.0;
        let spread = 0.01 + progress.clamp(0.0, 1.0) * 0.18;
        let (left_center, right_center) = if horizontal {
            ((0.50 - spread, 0.50 - 0.025), (0.50 + spread, 0.50 + 0.025))
        } else {
            ((0.50, 0.50 - spread), (0.50, 0.50 + spread))
        };
        records.push(record(
            seq,
            seq * FRAME_MS,
            vec![
                hand(Pose::OpenPalm, 11, Handedness::Left, left_center)?,
                hand(Pose::OpenPalm, 12, Handedness::Right, right_center)?,
            ],
        )?);
    }
    Ok(records)
}

fn relaxed_records() -> DynResult<Vec<HandFrameRecord>> {
    let mut records = Vec::new();
    for seq in 0..60 {
        let x = 0.48 + ((seq % 10) as f32 - 5.0) * 0.002;
        let y = 0.58 + ((seq % 7) as f32 - 3.0) * 0.002;
        records.push(record(
            seq,
            seq * FRAME_MS,
            vec![hand(Pose::Relaxed, 21, Handedness::Right, (x, y))?],
        )?);
    }
    Ok(records)
}

fn write_motion_takes(path: &Path) -> DynResult<()> {
    let take = serde_json::json!({
        "feature_version": "motion.synthetic.v1",
        "takes": [
            {"gesture_id": "motion.01J00000000000000000000003", "hands": 1,
             "points": [[0.0,0.0],[0.3,-0.2],[0.6,0.2],[1.0,0.0]]},
            {"gesture_id": "motion.01J00000000000000000000003", "hands": 1,
             "points": [[0.0,0.0],[0.28,-0.18],[0.62,0.22],[1.0,0.02]]},
            {"gesture_id": "motion.01J00000000000000000000004", "hands": 2,
             "points": [[0.0,0.0,0.0,0.1],[0.0,-0.4,0.0,0.5],[0.0,-0.8,0.0,0.9]]}
        ]
    });
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&take)?))?;
    Ok(())
}

fn record(seq: u64, t_ms: u64, hands: Vec<HandObservation>) -> DynResult<HandFrameRecord> {
    Ok(HandFrameRecord {
        camera_id: CameraId::from_str(CAMERA_ID)?,
        seq,
        t_ms,
        hands,
        timings: StageTimings::default(),
    })
}

#[derive(Debug, Clone, Copy)]
enum Pose {
    OpenPalm,
    Point,
    Pinch,
    Relaxed,
}

fn hand_with_index(
    pose: Pose,
    track_id: u32,
    handedness: Handedness,
    index_tip: (f32, f32),
) -> DynResult<HandObservation> {
    let local = local_landmarks(pose);
    let offset = local[8];
    let center = (
        index_tip.0 - offset[0] * SCALE,
        index_tip.1 - offset[1] * SCALE,
    );
    hand(pose, track_id, handedness, center)
}

fn hand(
    pose: Pose,
    track_id: u32,
    handedness: Handedness,
    center: (f32, f32),
) -> DynResult<HandObservation> {
    let local = local_landmarks(pose);
    let mut image = [[0.0; 3]; 21];
    let mut world = [[0.0; 3]; 21];
    for (index, point) in local.iter().enumerate() {
        image[index] = [
            center.0 + point[0] * SCALE,
            center.1 + point[1] * SCALE,
            point[2],
        ];
        world[index] = [point[0] * SCALE, point[1] * SCALE, point[2] * SCALE];
    }
    let bbox = bbox_for(&image);
    Ok(HandObservation {
        track_id,
        hand: handedness,
        handedness_score: 0.99,
        presence: 0.99,
        image,
        world,
        bbox,
        embedding: None,
        canned_scores: None,
    })
}

fn local_landmarks(pose: Pose) -> [[f32; 3]; 21] {
    let mut p = [[0.0; 3]; 21];
    p[0] = [0.0, 0.0, 0.0];
    p[1] = [0.62, -0.30, 0.0];
    p[2] = [0.95, -0.45, 0.0];
    p[3] = [1.20, -0.55, 0.0];
    p[4] = match pose {
        Pose::Point => [1.45, -0.20, 0.0],
        Pose::Pinch => [-0.45, -3.38, 0.0],
        _ => [1.50, -0.85, 0.0],
    };
    set_finger(&mut p, 5, -0.48, finger_state(pose, 5));
    set_finger(&mut p, 9, 0.00, finger_state(pose, 9));
    set_finger(&mut p, 13, 0.42, finger_state(pose, 13));
    set_finger(&mut p, 17, 0.78, finger_state(pose, 17));
    if matches!(pose, Pose::Relaxed) {
        p[4] = [1.55, -0.35, 0.0];
    }
    p
}

#[derive(Debug, Clone, Copy)]
enum FingerState {
    Extended,
    Curled,
    Half,
}

fn finger_state(pose: Pose, mcp_index: usize) -> FingerState {
    match pose {
        Pose::OpenPalm | Pose::Pinch => FingerState::Extended,
        Pose::Point if mcp_index == 5 => FingerState::Extended,
        Pose::Point => FingerState::Curled,
        Pose::Relaxed if matches!(mcp_index, 13 | 17) => FingerState::Extended,
        Pose::Relaxed => FingerState::Half,
    }
}

fn set_finger(points: &mut [[f32; 3]; 21], mcp: usize, x: f32, state: FingerState) {
    points[mcp] = [x, -1.0, 0.0];
    match state {
        FingerState::Extended => {
            points[mcp + 1] = [x, -1.85, 0.0];
            points[mcp + 2] = [x, -2.65, 0.0];
            points[mcp + 3] = [x, -3.45, 0.0];
        }
        FingerState::Curled => {
            points[mcp + 1] = [x, -1.75, 0.0];
            points[mcp + 2] = [x, -1.20, 0.0];
            points[mcp + 3] = [x, -0.85, 0.0];
        }
        FingerState::Half => {
            points[mcp + 1] = [x, -1.55, 0.0];
            points[mcp + 2] = [x + 0.20, -1.70, 0.0];
            points[mcp + 3] = [x + 0.35, -1.35, 0.0];
        }
    }
}

fn bbox_for(image: &[[f32; 3]; 21]) -> RectF {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in image {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    RectF {
        x: min_x,
        y: min_y,
        w: max_x - min_x,
        h: max_y - min_y,
    }
}
