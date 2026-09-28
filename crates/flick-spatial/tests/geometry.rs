use std::time::{Duration, Instant};

use flick_core::{
    AnchorId, AnchorStatus, CameraId, FaceKeypoints, HandFrame, HandObservation, Handedness, RectF,
    SelectionState, StageTimings,
};
use flick_spatial::{
    Anchor, AnchorGeometry, CameraIntrinsics, PointingRay, RayEstimator, RayEstimatorSettings,
    RayModel, RaySource, RealignPair, TargetSelectorImpl, TargetSelectorSettings, TeachObservation,
    TeachSession, TeachTarget, realign,
};
use nalgebra::{Matrix3, Unit, UnitQuaternion, Vector3};
use proptest::prelude::*;
use smallvec::smallvec;

proptest! {
    #[test]
    fn ray_recovery_known_ray_within_one_degree(
        yaw in -0.35_f32..0.35_f32,
        pitch in -0.20_f32..0.20_f32,
        distance in 1.0_f32..2.6_f32,
    ) {
        let intrinsics = CameraIntrinsics::sane_default(1280, 720);
        let eye = Vector3::new(0.0, 0.0, 0.65);
        let direction = UnitQuaternion::from_euler_angles(pitch, yaw, 0.0) * Vector3::new(0.0, 0.0, 1.0);
        let tip = eye + direction * distance;
        let hand = match synthetic_hand_with_tip(&intrinsics, tip, direction) {
            Ok(hand) => hand,
            Err(err) => return Err(TestCaseError::fail(err)),
        };
        let face = match synthetic_face(&intrinsics, eye) {
            Ok(face) => face,
            Err(err) => return Err(TestCaseError::fail(err)),
        };
        let mut estimator = RayEstimator::new(intrinsics, RayEstimatorSettings::default());
        let ray = match estimator.estimate(&hand, Some(&face), Instant::now()) {
            Ok(ray) => ray,
            Err(err) => return Err(TestCaseError::fail(err.to_string())),
        };
        let recovered = Vector3::new(ray.direction[0], ray.direction[1], ray.direction[2]);
        prop_assert!(angle_deg(recovered, direction) <= 1.0);
    }

    #[test]
    fn triangulation_recovers_point3d_anchor(
        x in -0.5_f32..0.5_f32,
        y in -0.3_f32..0.3_f32,
        z in 1.5_f32..3.0_f32,
    ) {
        let target = Vector3::new(x, y, z);
        let origins = [
            Vector3::new(-0.45, 0.0, 0.6),
            Vector3::new(0.45, 0.0, 0.6),
            Vector3::new(0.0, 0.35, 0.7),
        ];
        let anchor_id = AnchorId::new();
        let mut session = TeachSession::new(
            anchor_id,
            "Synthetic target",
            TeachTarget::Entity("light.synthetic".to_owned()),
            "light",
            "test.estimator",
        );
        for (idx, origin) in origins.iter().enumerate() {
            let direction = (target - origin).normalize();
            session.add_observation(TeachObservation {
                spot_index: idx as u32,
                ray: PointingRay::new(vec_to_array(*origin), vec_to_array(direction), RaySource::FingerOnly),
                frames: 30,
                ray_jitter_deg: 0.2,
            });
        }
        let outcome = match session.finish(&[]) {
            Ok(outcome) => outcome,
            Err(err) => return Err(TestCaseError::fail(err.to_string())),
        };
        match outcome.anchor.geometry {
            AnchorGeometry::Point3d { position, .. } => {
                let recovered = Vector3::new(position[0], position[1], position[2]);
                prop_assert!((recovered - target).norm() <= 0.03);
                prop_assert!(outcome.quality.residual_deg <= 1.0);
            }
            AnchorGeometry::Direction { .. } => return Err(TestCaseError::fail("unexpected direction fallback")),
        }
    }

    #[test]
    fn kabsch_realign_recovers_rotation(
        yaw in -0.45_f32..0.45_f32,
        pitch in -0.30_f32..0.30_f32,
        roll in -0.25_f32..0.25_f32,
    ) {
        let rotation = UnitQuaternion::from_euler_angles(pitch, yaw, roll).to_rotation_matrix().into_inner();
        let old = [
            Vector3::new(0.0, 0.0, 1.0).normalize(),
            Vector3::new(0.7, -0.1, 1.0).normalize(),
            Vector3::new(-0.2, 0.5, 1.0).normalize(),
        ];
        let pairs = old.map(|direction| RealignPair {
            anchor_id: AnchorId::new(),
            old_direction: vec_to_array(direction),
            new_direction: vec_to_array(rotation * direction),
        });
        let result = match realign(&pairs) {
            Ok(result) => result,
            Err(err) => return Err(TestCaseError::fail(err.to_string())),
        };
        prop_assert!(result.residual_deg <= 0.1);
        for pair in pairs {
            let transformed = result.transform.transform_direction(pair.old_direction);
            prop_assert!(angle_deg(Vector3::new(transformed[0], transformed[1], transformed[2]), Vector3::new(pair.new_direction[0], pair.new_direction[1], pair.new_direction[2])) <= 0.1);
        }
    }
}

#[test]
fn target_selector_fsm_table_selects_and_expires() -> Result<(), String> {
    let intrinsics = CameraIntrinsics::sane_default(1280, 720);
    let camera_id = CameraId::new();
    let anchor_id = AnchorId::new();
    let origin = Vector3::new(0.0, 0.0, 0.8);
    let direction = Vector3::new(0.0, -0.12, 1.0).normalize();
    let anchor = test_anchor(
        anchor_id,
        "Fan",
        origin + direction * 2.0,
        "fan.ventilador_dormitorio",
        "fan",
    );
    let mut settings = TargetSelectorSettings::default();
    settings.ray.model = RayModel::Finger;
    let mut selector = TargetSelectorImpl::new(intrinsics.clone(), vec![anchor], settings);
    let start = Instant::now();
    let rows = [
        (0_u64, "idle"),
        (250, "hover"),
        (500, "hover"),
        (800, "selected"),
        (1000, "selected"),
        (5000, "idle"),
    ];
    for (ms, expected) in rows {
        let frame = if ms == 5000 {
            empty_frame(camera_id, start + Duration::from_millis(ms))
        } else {
            frame_for_direction(
                camera_id,
                &intrinsics,
                origin,
                direction,
                start + Duration::from_millis(ms),
            )?
        };
        let state = selector.update(&frame, None);
        match expected {
            "idle" => assert!(
                matches!(state, SelectionState::Idle),
                "state at {ms}ms was {state:?}"
            ),
            "hover" => assert!(
                matches!(state, SelectionState::Hover { anchor_id: id, .. } if id == anchor_id),
                "state at {ms}ms was {state:?}"
            ),
            "selected" => assert!(
                matches!(state, SelectionState::Selected { anchor_id: id, .. } if id == anchor_id),
                "state at {ms}ms was {state:?}"
            ),
            other => return Err(format!("unknown expected state {other}")),
        }
    }
    Ok(())
}

#[test]
fn lock_scenario_circling_after_selection_never_reselects() -> Result<(), String> {
    let intrinsics = CameraIntrinsics::sane_default(1280, 720);
    let camera_id = CameraId::new();
    let fan_id = AnchorId::new();
    let lamp_id = AnchorId::new();
    let origin = Vector3::new(0.0, 0.0, 0.8);
    let fan_direction = Vector3::new(0.0, -0.12, 1.0).normalize();
    let yaw25 = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 25.0_f32.to_radians());
    let lamp_direction = yaw25 * fan_direction;
    let anchors = vec![
        test_anchor(
            fan_id,
            "Ventilador Dormitorio",
            origin + fan_direction * 2.0,
            "fan.ventilador_dormitorio",
            "fan",
        ),
        test_anchor(
            lamp_id,
            "Lock test lamp",
            origin + lamp_direction * 2.0,
            "light.lock_test",
            "light",
        ),
    ];
    let mut settings = TargetSelectorSettings::default();
    settings.ray.model = RayModel::Finger;
    let mut selector = TargetSelectorImpl::new(intrinsics.clone(), anchors, settings);
    let start = Instant::now();
    for ms in [0_u64, 250, 500, 800] {
        let frame = frame_for_direction(
            camera_id,
            &intrinsics,
            origin,
            fan_direction,
            start + Duration::from_millis(ms),
        )?;
        let _state = selector.update(&frame, None);
    }
    assert_eq!(selector.selected(), Some(fan_id));

    for step in 0..36_u64 {
        let phase = step as f32 / 36.0 * std::f32::consts::TAU;
        let blended = (fan_direction
            + lamp_direction * (0.8 * phase.sin().max(0.0))
            + Vector3::new(0.10 * phase.cos(), 0.08 * phase.sin(), 0.0))
        .normalize();
        let frame = frame_for_direction(
            camera_id,
            &intrinsics,
            origin,
            blended,
            start + Duration::from_millis(850 + step * 33),
        )?;
        let _state = selector.update(&frame, None);
        assert_eq!(selector.selected(), Some(fan_id));
    }
    Ok(())
}

#[test]
fn ambiguity_suppresses_selection_when_two_anchors_share_margin() -> Result<(), String> {
    let intrinsics = CameraIntrinsics::sane_default(1280, 720);
    let camera_id = CameraId::new();
    let fan_id = AnchorId::new();
    let lamp_id = AnchorId::new();
    let origin = Vector3::new(0.0, 0.0, 0.8);
    let center = Vector3::new(0.0, -0.08, 1.0).normalize();
    let yaw_left = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -2.0_f32.to_radians());
    let yaw_right = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 2.0_f32.to_radians());
    let anchors = vec![
        test_anchor(
            fan_id,
            "Fan",
            origin + (yaw_left * center) * 2.0,
            "fan.ventilador_dormitorio",
            "fan",
        ),
        test_anchor(
            lamp_id,
            "Lamp",
            origin + (yaw_right * center) * 2.0,
            "light.lamp",
            "light",
        ),
    ];
    let mut settings = TargetSelectorSettings::default();
    settings.ray.model = RayModel::Finger;
    let mut selector = TargetSelectorImpl::new(intrinsics.clone(), anchors, settings);
    let start = Instant::now();

    for ms in [0_u64, 250, 550, 900] {
        let frame = frame_for_direction(
            camera_id,
            &intrinsics,
            origin,
            center,
            start + Duration::from_millis(ms),
        )?;
        let state = selector.update(&frame, None);
        if ms >= 250 {
            assert!(
                matches!(state, SelectionState::Aiming),
                "state at {ms}ms was {state:?}"
            );
        }
    }

    assert_eq!(selector.selected(), None);
    assert!(
        selector
            .take_events()
            .iter()
            .any(|event| matches!(event, flick_spatial::TargetEvent::Ambiguous { anchor_ids, .. } if anchor_ids.contains(&fan_id) && anchor_ids.contains(&lamp_id))),
        "expected ambiguous target event",
    );
    Ok(())
}

fn test_anchor(
    id: AnchorId,
    name: &str,
    position: Vector3<f32>,
    entity_id: &str,
    domain: &str,
) -> Anchor {
    Anchor {
        id,
        name: name.to_owned(),
        target: TeachTarget::Entity(entity_id.to_owned()),
        domain: domain.to_owned(),
        geometry: AnchorGeometry::Point3d {
            position: vec_to_array(position),
            covariance: [[0.0; 3]; 3],
        },
        uncertainty_deg: 0.5,
        verb_params: serde_json::json!({}),
        status: AnchorStatus::Ok,
        estimator_version: "test.estimator".to_owned(),
    }
}

fn frame_for_direction(
    camera_id: CameraId,
    intrinsics: &CameraIntrinsics,
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    at: Instant,
) -> Result<HandFrame, String> {
    let hand = synthetic_hand(intrinsics, origin, direction)?;
    Ok(HandFrame {
        camera_id,
        seq: 0,
        captured_at: at,
        hands: smallvec![hand],
        timings: StageTimings::default(),
    })
}

fn empty_frame(camera_id: CameraId, at: Instant) -> HandFrame {
    HandFrame {
        camera_id,
        seq: 0,
        captured_at: at,
        hands: smallvec![],
        timings: StageTimings::default(),
    }
}

fn synthetic_hand_with_tip(
    intrinsics: &CameraIntrinsics,
    tip: Vector3<f32>,
    direction: Vector3<f32>,
) -> Result<HandObservation, String> {
    let world = hand_world();
    let finger_axis = Vector3::new(
        world[8][0] - world[5][0],
        world[8][1] - world[5][1],
        world[8][2] - world[5][2],
    );
    let rotation = rotation_between(finger_axis, direction);
    let translation = tip - rotation * Vector3::new(world[8][0], world[8][1], world[8][2]);
    projected_hand(intrinsics, world, rotation, translation)
}

fn synthetic_hand(
    intrinsics: &CameraIntrinsics,
    mcp_origin: Vector3<f32>,
    direction: Vector3<f32>,
) -> Result<HandObservation, String> {
    let world = hand_world();
    let finger_axis = Vector3::new(
        world[8][0] - world[5][0],
        world[8][1] - world[5][1],
        world[8][2] - world[5][2],
    );
    let rotation = rotation_between(finger_axis, direction);
    let mcp_world = Vector3::new(world[5][0], world[5][1], world[5][2]);
    let translation = mcp_origin - rotation * mcp_world;
    projected_hand(intrinsics, world, rotation, translation)
}

fn projected_hand(
    intrinsics: &CameraIntrinsics,
    world: [[f32; 3]; 21],
    rotation: Matrix3<f32>,
    translation: Vector3<f32>,
) -> Result<HandObservation, String> {
    let mut image = [[0.0_f32; 3]; 21];
    for (idx, point) in world.iter().enumerate() {
        let camera = rotation * Vector3::new(point[0], point[1], point[2]) + translation;
        let Some(projected) = intrinsics.project(vec_to_array(camera)) else {
            return Err("hand point projected behind camera".to_owned());
        };
        image[idx] = [projected[0], projected[1], camera.z];
    }
    Ok(HandObservation {
        track_id: 1,
        hand: Handedness::Right,
        handedness_score: 0.99,
        presence: 0.99,
        image,
        world,
        bbox: RectF {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        embedding: None,
    })
}

fn synthetic_face(
    intrinsics: &CameraIntrinsics,
    eye_center: Vector3<f32>,
) -> Result<FaceKeypoints, String> {
    let ipd = 0.063;
    let left = eye_center + Vector3::new(-ipd * 0.5, 0.0, 0.0);
    let right = eye_center + Vector3::new(ipd * 0.5, 0.0, 0.0);
    let Some(left_image) = intrinsics.project(vec_to_array(left)) else {
        return Err("left eye projected behind camera".to_owned());
    };
    let Some(right_image) = intrinsics.project(vec_to_array(right)) else {
        return Err("right eye projected behind camera".to_owned());
    };
    let mut points = [[0.0_f32; 2]; 6];
    points[0] = left_image;
    points[1] = right_image;
    Ok(FaceKeypoints {
        points,
        confidence: Some(0.99),
    })
}

fn hand_world() -> [[f32; 3]; 21] {
    let mut p = [[0.0_f32; 3]; 21];
    p[0] = [0.0, 0.0, 0.0];
    p[1] = [-0.035, -0.015, 0.015];
    p[2] = [-0.055, -0.045, 0.020];
    p[3] = [-0.070, -0.070, 0.015];
    p[4] = [-0.085, -0.090, 0.010];
    p[5] = [0.020, -0.025, 0.000];
    p[6] = [0.020, -0.080, 0.010];
    p[7] = [0.020, -0.125, 0.020];
    p[8] = [0.020, -0.175, 0.030];
    p[9] = [0.000, -0.030, 0.005];
    p[10] = [0.000, -0.070, 0.015];
    p[11] = [0.005, -0.045, 0.030];
    p[12] = [0.005, -0.020, 0.035];
    p[13] = [-0.020, -0.025, 0.005];
    p[14] = [-0.025, -0.060, 0.015];
    p[15] = [-0.020, -0.040, 0.030];
    p[16] = [-0.015, -0.018, 0.035];
    p[17] = [-0.040, -0.020, 0.000];
    p[18] = [-0.050, -0.050, 0.010];
    p[19] = [-0.045, -0.032, 0.025];
    p[20] = [-0.038, -0.015, 0.030];
    p
}

fn rotation_between(from: Vector3<f32>, to: Vector3<f32>) -> Matrix3<f32> {
    let from_unit = Unit::new_normalize(from);
    let to_unit = Unit::new_normalize(to);
    match UnitQuaternion::rotation_between(&from_unit, &to_unit) {
        Some(rotation) => rotation.to_rotation_matrix().into_inner(),
        None => Matrix3::identity(),
    }
}

fn angle_deg(a: Vector3<f32>, b: Vector3<f32>) -> f32 {
    let an = a.normalize();
    let bn = b.normalize();
    an.dot(&bn).clamp(-1.0, 1.0).acos().to_degrees()
}

fn vec_to_array(value: Vector3<f32>) -> [f32; 3] {
    [value.x, value.y, value.z]
}
