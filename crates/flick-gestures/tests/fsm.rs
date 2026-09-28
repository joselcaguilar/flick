use std::time::{Duration, Instant};

use flick_core::{
    BuiltinGesture, CameraId, GestureCandidate, GestureId, HandFrame, HandObservation, Handedness,
    RectF, SelectionState, StageTimings, SuppressionReason,
};
use flick_gestures::{
    ArmConfig, GestureMapping, HandConstraint, TargetMode, TriggerConfig, TriggerFsmSet,
    TriggerMode,
};
use proptest::prelude::*;
use smallvec::SmallVec;

#[test]
fn trigger_fsm_hard_behaviors() {
    for case in CASES {
        (case.run)();
    }
}

struct Case {
    run: fn(),
}

const CASES: &[Case] = &[
    Case {
        run: tap_votes_once,
    },
    Case {
        run: hold_repeat_and_release,
    },
    Case {
        run: dial_updates_and_ends,
    },
    Case {
        run: arm_and_pause_gates,
    },
    Case {
        run: conflicts_and_two_hand_guard,
    },
    Case {
        run: targeting_precedence,
    },
];

fn tap_votes_once() {
    let mut fsm = fsm(vec![GestureMapping::tap(gesture(BuiltinGesture::ThumbUp))]);
    let mut events = Vec::new();
    for index in 0..6 {
        events.extend(
            fsm.update(
                &frame(index * 33, 1),
                &[candidate(BuiltinGesture::ThumbUp, 0.9)],
                &SelectionState::Idle,
            )
            .events,
        );
    }
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].gesture_id, gesture(BuiltinGesture::ThumbUp));
}

fn hold_repeat_and_release() {
    let mut hold_mapping = GestureMapping::tap(gesture(BuiltinGesture::OpenPalm));
    hold_mapping.mode = TriggerMode::Hold;
    let mut repeat_mapping = GestureMapping::tap(gesture(BuiltinGesture::SwipeRight));
    repeat_mapping.mode = TriggerMode::Repeat;
    let mut config = TriggerConfig {
        vote_n: 1,
        vote_m: 1,
        hold_ms: 100,
        repeat_ms: 100,
        release_ms: 50,
        mappings: vec![hold_mapping, repeat_mapping],
        ..TriggerConfig::default()
    };
    let mut fsm = TriggerFsmSet::new(config.clone());
    assert!(
        fsm.update(
            &frame(0, 1),
            &[candidate(BuiltinGesture::OpenPalm, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .is_empty()
    );
    assert_eq!(
        fsm.update(
            &frame(120, 1),
            &[candidate(BuiltinGesture::OpenPalm, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .len(),
        1
    );
    assert_eq!(
        fsm.update(&frame(200, 0), &[], &SelectionState::Idle)
            .events
            .len(),
        0
    );
    assert_eq!(
        fsm.update(&frame(260, 0), &[], &SelectionState::Idle)
            .events
            .len(),
        1
    );

    config.vote_n = 1;
    let mut repeat = TriggerFsmSet::new(config);
    assert!(
        repeat
            .update(
                &frame(0, 1),
                &[candidate(BuiltinGesture::SwipeRight, 0.9)],
                &SelectionState::Idle,
            )
            .events
            .is_empty()
    );
    assert_eq!(
        repeat
            .update(
                &frame(120, 1),
                &[candidate(BuiltinGesture::SwipeRight, 0.9)],
                &SelectionState::Idle,
            )
            .events
            .len(),
        1
    );
    assert_eq!(
        repeat
            .update(
                &frame(230, 1),
                &[candidate(BuiltinGesture::SwipeRight, 0.9)],
                &SelectionState::Idle,
            )
            .events
            .len(),
        1
    );
}

fn dial_updates_and_ends() {
    let mut mapping = GestureMapping::tap(gesture(BuiltinGesture::PinchDial));
    mapping.mode = TriggerMode::Dial;
    let config = TriggerConfig {
        vote_n: 1,
        vote_m: 1,
        dial_update_ms: 100,
        release_ms: 50,
        mappings: vec![mapping],
        ..TriggerConfig::default()
    };
    let mut fsm = TriggerFsmSet::new(config);
    assert_eq!(
        fsm.update(
            &frame(0, 1),
            &[candidate_with_value(BuiltinGesture::PinchDial, 0.1)],
            &SelectionState::Idle,
        )
        .events[0]
            .phase,
        flick_core::GesturePhase::Fired
    );
    assert!(
        fsm.update(
            &frame(50, 1),
            &[candidate_with_value(BuiltinGesture::PinchDial, 0.2)],
            &SelectionState::Idle,
        )
        .events
        .is_empty()
    );
    assert_eq!(
        fsm.update(
            &frame(120, 1),
            &[candidate_with_value(BuiltinGesture::PinchDial, 0.3)],
            &SelectionState::Idle,
        )
        .events[0]
            .phase,
        flick_core::GesturePhase::Update
    );
    assert_eq!(
        fsm.update(&frame(160, 0), &[], &SelectionState::Idle)
            .events
            .len(),
        0
    );
    assert_eq!(
        fsm.update(&frame(220, 0), &[], &SelectionState::Idle)
            .events[0]
            .phase,
        flick_core::GesturePhase::End
    );
}

fn arm_and_pause_gates() {
    let mut gated = GestureMapping::tap(gesture(BuiltinGesture::ThumbUp));
    gated.require_armed = true;
    let pause = flick_gestures::PauseGestureConfig {
        enabled: true,
        hold_ms: 50,
        ..Default::default()
    };
    let config = TriggerConfig {
        vote_n: 1,
        vote_m: 1,
        arm: ArmConfig {
            enabled: true,
            hold_ms: 50,
            window_ms: 500,
            ..Default::default()
        },
        pause,
        mappings: vec![gated],
        ..TriggerConfig::default()
    };
    let mut fsm = TriggerFsmSet::new(config);
    assert_eq!(
        fsm.update(
            &frame(0, 1),
            &[candidate(BuiltinGesture::ThumbUp, 0.9)],
            &SelectionState::Idle,
        )
        .suppressed[0]
            .reason,
        SuppressionReason::NotArmed
    );
    assert!(
        fsm.update(
            &frame(0, 1),
            &[candidate(BuiltinGesture::OpenPalm, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .is_empty()
    );
    assert!(
        fsm.update(
            &frame(60, 1),
            &[candidate(BuiltinGesture::OpenPalm, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .is_empty()
    );
    assert_eq!(
        fsm.update(
            &frame(90, 1),
            &[candidate(BuiltinGesture::ThumbUp, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .len(),
        1
    );
    assert!(
        fsm.update(
            &frame(200, 1),
            &[candidate(BuiltinGesture::ILoveYou, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .is_empty()
    );
    assert!(
        fsm.update(
            &frame(260, 1),
            &[candidate(BuiltinGesture::ILoveYou, 0.9)],
            &SelectionState::Idle,
        )
        .events
        .is_empty()
    );
    assert!(fsm.paused());
}

fn conflicts_and_two_hand_guard() {
    let mut ambiguous_fsm = fsm(vec![
        GestureMapping::tap(gesture(BuiltinGesture::ThumbUp)),
        GestureMapping::tap(gesture(BuiltinGesture::ThumbDown)),
    ]);
    let ambiguous = ambiguous_fsm.update(
        &frame(0, 1),
        &[
            candidate(BuiltinGesture::ThumbUp, 0.80),
            candidate(BuiltinGesture::ThumbDown, 0.75),
        ],
        &SelectionState::Idle,
    );
    assert_eq!(ambiguous.suppressed[0].reason, SuppressionReason::Ambiguous);

    let mut guarded = fsm(vec![GestureMapping::tap(gesture(BuiltinGesture::ThumbUp))]);
    let blocked = guarded.update(
        &frame(0, 2),
        &[candidate(BuiltinGesture::ThumbUp, 0.9)],
        &SelectionState::Idle,
    );
    assert_eq!(blocked.suppressed[0].reason, SuppressionReason::NoMapping);

    let mut mapping = GestureMapping::tap(gesture(BuiltinGesture::ThumbUp));
    mapping.allow_two_hands = true;
    let mut allowed = TriggerFsmSet::new(TriggerConfig {
        vote_n: 1,
        vote_m: 1,
        mappings: vec![mapping],
        ..TriggerConfig::default()
    });
    assert_eq!(
        allowed
            .update(
                &frame(0, 2),
                &[candidate(BuiltinGesture::ThumbUp, 0.9)],
                &SelectionState::Idle,
            )
            .events
            .len(),
        1
    );
}

fn targeting_precedence() {
    let anchor = flick_core::AnchorId::new();
    let selection = SelectionState::Selected {
        anchor_id: anchor,
        domain: "fan".to_owned(),
        expires_at_ms: 1_000,
    };
    let mut either = GestureMapping::tap(gesture(BuiltinGesture::CircleAny));
    either.target_mode = TargetMode::Either;
    let mut fsm = TriggerFsmSet::new(TriggerConfig {
        vote_n: 1,
        vote_m: 1,
        mappings: vec![either],
        ..TriggerConfig::default()
    });
    let event = &fsm
        .update(
            &frame(0, 1),
            &[candidate(BuiltinGesture::CircleCw, 0.9)],
            &selection,
        )
        .events[0];
    assert_eq!(event.target, Some(anchor));

    let mut targeted = GestureMapping::tap(gesture(BuiltinGesture::CircleCw));
    targeted.target_mode = TargetMode::Targeted;
    let mut no_target = TriggerFsmSet::new(TriggerConfig {
        vote_n: 1,
        vote_m: 1,
        mappings: vec![targeted],
        ..TriggerConfig::default()
    });
    assert_eq!(
        no_target
            .update(
                &frame(0, 1),
                &[candidate(BuiltinGesture::CircleCw, 0.9)],
                &SelectionState::Idle,
            )
            .suppressed[0]
            .reason,
        SuppressionReason::NoTarget
    );
}

proptest! {
    #[test]
    fn no_double_fire_inside_cooldown(gap_ms in 50_u64..1_000) {
        let mut mapping = GestureMapping::tap(gesture(BuiltinGesture::ThumbUp));
        mapping.cooldown_ms = 1_000;
        let mut fsm = TriggerFsmSet::new(TriggerConfig {
            vote_n: 1,
            vote_m: 1,
            release_ms: 10,
            mappings: vec![mapping],
            ..TriggerConfig::default()
        });
        prop_assert_eq!(fsm.update(
            &frame(0, 1),
            &[candidate(BuiltinGesture::ThumbUp, 0.9)],
            &SelectionState::Idle,
        ).events.len(), 1);
        let _ = fsm.update(&frame(20, 0), &[], &SelectionState::Idle);
        let _ = fsm.update(&frame(40, 0), &[], &SelectionState::Idle);
        prop_assert!(fsm.update(
            &frame(gap_ms, 1),
            &[candidate(BuiltinGesture::ThumbUp, 0.9)],
            &SelectionState::Idle,
        ).events.is_empty());
    }
}

fn fsm(mappings: Vec<GestureMapping>) -> TriggerFsmSet {
    TriggerFsmSet::new(TriggerConfig {
        mappings,
        ..TriggerConfig::default()
    })
}

fn frame(t_ms: u64, hands: usize) -> HandFrame {
    let observations: Vec<HandObservation> = (0..hands)
        .map(|index| dummy_hand(index as u32 + 1))
        .collect();
    HandFrame {
        camera_id: CameraId::new(),
        seq: t_ms,
        captured_at: Instant::now() + Duration::from_millis(t_ms),
        hands: SmallVec::from_vec(observations),
        timings: StageTimings::default(),
    }
}

fn dummy_hand(track_id: u32) -> HandObservation {
    HandObservation {
        track_id,
        hand: Handedness::Right,
        handedness_score: 1.0,
        presence: 1.0,
        image: [[0.0; 3]; 21],
        world: [[0.0; 3]; 21],
        bbox: RectF {
            x: 0.0,
            y: 0.0,
            w: 0.1,
            h: 0.1,
        },
        embedding: None,
        canned_scores: None,
    }
}

fn candidate(gesture_id: BuiltinGesture, confidence: f32) -> GestureCandidate {
    GestureCandidate {
        gesture_id: gesture(gesture_id),
        track_id: 1,
        hand: Handedness::Right,
        confidence,
        progress: Some(confidence),
        value: None,
    }
}

fn candidate_with_value(gesture_id: BuiltinGesture, value: f32) -> GestureCandidate {
    GestureCandidate {
        value: Some(value),
        ..candidate(gesture_id, 0.9)
    }
}

fn gesture(gesture: BuiltinGesture) -> GestureId {
    GestureId::Builtin(gesture)
}

#[allow(dead_code)]
fn left_hand_mapping(gesture: BuiltinGesture) -> GestureMapping {
    let mut mapping = GestureMapping::tap(GestureId::Builtin(gesture));
    mapping.hand = HandConstraint::Left;
    mapping
}
