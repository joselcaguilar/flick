//! Trigger state machine that debounces recognizer candidates into events.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use flick_core::{
    AnchorId, BuiltinGesture, CameraId, GestureCandidate, GestureEvent, GestureEventId, GestureId,
    GesturePhase, HandFrame, Handedness, SelectionState, SuppressionReason,
};
use smallvec::SmallVec;

/// Mapping trigger mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    /// Fire once as soon as the vote passes.
    Tap,
    /// Fire after the gesture remains held for `hold_ms`.
    Hold,
    /// Fire after `hold_ms`, then repeatedly every `repeat_ms`.
    Repeat,
    /// Continuous value gesture with `Fired`, rate-limited `Update` and `End`.
    Dial,
}

/// Hand constraint for a mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandConstraint {
    /// Either hand.
    Any,
    /// Left hand only.
    Left,
    /// Right hand only.
    Right,
}

/// Targeting mode for a mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    /// Mapping is global and does not need a selected anchor.
    Global,
    /// Mapping requires a selected anchor.
    Targeted,
    /// Mapping can operate globally or on a selected anchor.
    Either,
}

/// Gesture mapping metadata needed by the FSM.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureMapping {
    /// Gesture id this mapping consumes.
    pub gesture_id: GestureId,
    /// Trigger mode.
    pub mode: TriggerMode,
    /// Hand constraint.
    pub hand: HandConstraint,
    /// Allows `hand = Any` while two hands are visible.
    pub allow_two_hands: bool,
    /// Requires arm mode to be active.
    pub require_armed: bool,
    /// Per-mapping cooldown.
    pub cooldown_ms: u64,
    /// Whether this mapping is global or targeted.
    pub target_mode: TargetMode,
}

impl GestureMapping {
    /// Creates a default tap mapping for a gesture.
    #[must_use]
    pub const fn tap(gesture_id: GestureId) -> Self {
        Self {
            gesture_id,
            mode: TriggerMode::Tap,
            hand: HandConstraint::Any,
            allow_two_hands: false,
            require_armed: false,
            cooldown_ms: 1_000,
            target_mode: TargetMode::Global,
        }
    }
}

/// Optional arm-mode configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmConfig {
    /// Whether arm mode is enabled.
    pub enabled: bool,
    /// Gesture that opens the arm window.
    pub gesture: GestureId,
    /// Required hold time for the arm gesture.
    pub hold_ms: u64,
    /// Duration of the armed window.
    pub window_ms: u64,
}

impl Default for ArmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gesture: GestureId::Builtin(BuiltinGesture::OpenPalm),
            hold_ms: 600,
            window_ms: 4_000,
        }
    }
}

/// Optional pause gesture configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PauseGestureConfig {
    /// Whether pause toggling is enabled.
    pub enabled: bool,
    /// Gesture that toggles pause.
    pub gesture: GestureId,
    /// Required hold time before toggling.
    pub hold_ms: u64,
}

impl Default for PauseGestureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gesture: GestureId::Builtin(BuiltinGesture::ILoveYou),
            hold_ms: 1_500,
        }
    }
}

/// Trigger FSM settings.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerConfig {
    /// Required votes in the window.
    pub vote_n: usize,
    /// Vote window size.
    pub vote_m: usize,
    /// Votes required (within `vote_m`) when a device is selected; capped at `vote_n`.
    pub selected_vote_n: usize,
    /// Absence required before another fire.
    pub release_ms: u64,
    /// Hold mode threshold.
    pub hold_ms: u64,
    /// Repeat interval after the first repeat fire.
    pub repeat_ms: u64,
    /// Dial update interval.
    pub dial_update_ms: u64,
    /// Conflict margin required when recognizers disagree.
    pub conflict_margin: f32,
    /// Arm-mode settings.
    pub arm: ArmConfig,
    /// Pause gesture settings.
    pub pause: PauseGestureConfig,
    /// Mappings known to the FSM.
    pub mappings: Vec<GestureMapping>,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            vote_n: 6,
            vote_m: 8,
            selected_vote_n: 3,
            release_ms: 250,
            hold_ms: 800,
            repeat_ms: 400,
            dial_update_ms: 125,
            conflict_margin: 0.10,
            arm: ArmConfig::default(),
            pause: PauseGestureConfig::default(),
            mappings: Vec::new(),
        }
    }
}

/// A candidate suppressed by the FSM and its stable reason code.
#[derive(Debug, Clone, PartialEq)]
pub struct SuppressedCandidate {
    /// Candidate gesture id.
    pub gesture_id: GestureId,
    /// Track id that produced the candidate.
    pub track_id: u32,
    /// Suppression reason.
    pub reason: SuppressionReason,
}

/// Output from an FSM update.
#[derive(Debug, Clone, Default)]
pub struct FsmUpdate {
    /// Debounced events.
    pub events: Vec<GestureEvent>,
    /// Suppressed candidates for debug UI.
    pub suppressed: Vec<SuppressedCandidate>,
}

/// A set of per-track FSMs for one camera.
#[derive(Debug, Clone)]
pub struct TriggerFsmSet {
    config: TriggerConfig,
    tracks: HashMap<u32, TrackState>,
    armed_until: Option<Instant>,
    paused: bool,
    pause_since: Option<Instant>,
    arm_since: Option<Instant>,
}

impl TriggerFsmSet {
    /// Creates a new FSM set.
    #[must_use]
    pub fn new(config: TriggerConfig) -> Self {
        Self {
            config,
            tracks: HashMap::new(),
            armed_until: None,
            paused: false,
            pause_since: None,
            arm_since: None,
        }
    }

    /// Returns whether pause mode is active.
    #[must_use]
    pub const fn paused(&self) -> bool {
        self.paused
    }

    /// Clears pause mode.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Updates all track FSMs for one frame.
    #[must_use]
    pub fn update(
        &mut self,
        frame: &HandFrame,
        candidates: &[GestureCandidate],
        selection: &SelectionState,
    ) -> FsmUpdate {
        let mut result = FsmUpdate::default();
        let visible_hands = frame.hands.len();
        let visible_track_ids: Vec<u32> = frame.hands.iter().map(|hand| hand.track_id).collect();
        let mut by_track: HashMap<u32, SmallVec<[GestureCandidate; 4]>> = HashMap::new();
        for candidate in candidates {
            by_track
                .entry(candidate.track_id)
                .or_default()
                .push(candidate.clone());
        }
        for track_id in &visible_track_ids {
            self.tracks
                .entry(*track_id)
                .or_default()
                .begin_frame(self.config.vote_m);
        }

        for (track_id, candidates) in &by_track {
            if let Some(control_candidate) =
                choose_candidate(candidates, self.config.conflict_margin)
            {
                if self.update_pause(frame.captured_at, &control_candidate) {
                    continue;
                }
                if self.update_arm(frame.captured_at, &control_candidate) {
                    continue;
                }
            }
            if self.paused {
                if let Some(candidate) = choose_candidate(candidates, self.config.conflict_margin) {
                    result
                        .suppressed
                        .push(suppressed(&candidate, SuppressionReason::Paused));
                }
                continue;
            }

            let mut resolved = Vec::new();
            let mut suppressed_candidates = Vec::new();
            for candidate in candidates {
                let (mapping, target) =
                    match self.resolve_mapping(candidate, visible_hands, selection) {
                        Ok((mapping, target)) => (mapping.clone(), target),
                        Err(reason) => {
                            suppressed_candidates.push(suppressed(candidate, reason));
                            continue;
                        }
                    };
                if mapping.require_armed && !self.is_armed(frame.captured_at) {
                    suppressed_candidates.push(suppressed(candidate, SuppressionReason::NotArmed));
                    continue;
                }
                resolved.push(ResolvedCandidate {
                    candidate: candidate.clone(),
                    mapping,
                    target,
                });
            }

            let Some(resolved_candidate) = choose_resolved(&resolved, self.config.conflict_margin)
            else {
                if resolved.is_empty() {
                    if let Some(suppression) = most_relevant_suppression(&suppressed_candidates) {
                        result.suppressed.push(suppression.clone());
                    }
                } else if let Some(first) = resolved.first() {
                    result
                        .suppressed
                        .push(suppressed(&first.candidate, SuppressionReason::Ambiguous));
                }
                if let Some(track) = self.tracks.get_mut(track_id) {
                    result.events.extend(track.update_absent(
                        frame.camera_id,
                        frame.captured_at,
                        &self.config,
                    ));
                }
                continue;
            };

            let track = self.tracks.entry(*track_id).or_default();
            if track
                .active_gesture_id()
                .is_some_and(|gesture_id| gesture_id != resolved_candidate.candidate.gesture_id)
                && !track.active_is_tap()
            {
                result.events.extend(track.update_absent(
                    frame.camera_id,
                    frame.captured_at,
                    &self.config,
                ));
                if track.has_active() {
                    continue;
                }
            }
            let produced = track.update_candidate(
                frame.camera_id,
                frame.captured_at,
                &resolved_candidate.candidate,
                &resolved_candidate.mapping,
                resolved_candidate.target,
                &self.config,
            );
            match produced {
                TrackProduced::Events(events) => result.events.extend(events),
                TrackProduced::Suppressed(reason) => {
                    result
                        .suppressed
                        .push(suppressed(&resolved_candidate.candidate, reason));
                }
                TrackProduced::None => {}
            }
        }

        let active_track_ids: Vec<u32> = self.tracks.keys().copied().collect();
        for track_id in active_track_ids {
            if by_track.contains_key(&track_id) {
                continue;
            }
            if let Some(track) = self.tracks.get_mut(&track_id) {
                if !visible_track_ids.contains(&track_id) {
                    track.begin_frame(self.config.vote_m);
                }
                result.events.extend(track.update_absent(
                    frame.camera_id,
                    frame.captured_at,
                    &self.config,
                ));
            }
        }
        result
    }

    fn update_pause(&mut self, now: Instant, candidate: &GestureCandidate) -> bool {
        if !self.config.pause.enabled || candidate.gesture_id != self.config.pause.gesture {
            self.pause_since = None;
            return false;
        }
        let since = self.pause_since.get_or_insert(now);
        if now.duration_since(*since) >= Duration::from_millis(self.config.pause.hold_ms) {
            self.paused = !self.paused;
            self.pause_since = None;
        }
        true
    }

    fn update_arm(&mut self, now: Instant, candidate: &GestureCandidate) -> bool {
        if !self.config.arm.enabled || candidate.gesture_id != self.config.arm.gesture {
            self.arm_since = None;
            return false;
        }
        let since = self.arm_since.get_or_insert(now);
        if now.duration_since(*since) >= Duration::from_millis(self.config.arm.hold_ms) {
            self.armed_until = Some(now + Duration::from_millis(self.config.arm.window_ms));
        }
        true
    }

    fn is_armed(&self, now: Instant) -> bool {
        !self.config.arm.enabled || self.armed_until.is_some_and(|until| now <= until)
    }

    fn resolve_mapping(
        &self,
        candidate: &GestureCandidate,
        visible_hands: usize,
        selection: &SelectionState,
    ) -> Result<(&GestureMapping, Option<AnchorId>), SuppressionReason> {
        let selected = selected_anchor(selection);
        let mut matching: Vec<&GestureMapping> = self
            .config
            .mappings
            .iter()
            .filter(|mapping| gesture_matches(mapping.gesture_id, candidate.gesture_id))
            .filter(|mapping| hand_matches(mapping.hand, candidate.hand))
            .collect();

        // With a device selected, the other hand is usually still pointing at it,
        // so verbs skip the two-hand guard.
        if selected.is_some()
            && let Some(targeted) = matching.iter().copied().find(|mapping| {
                matches!(
                    mapping.target_mode,
                    TargetMode::Targeted | TargetMode::Either
                )
            })
        {
            return Ok((targeted, selected));
        }

        matching.retain(|mapping| two_hand_allowed(mapping, candidate.gesture_id, visible_hands));
        if matching.is_empty() {
            return Err(SuppressionReason::NoMapping);
        }

        if selected.is_some() {
            if matching
                .iter()
                .any(|mapping| matches!(mapping.target_mode, TargetMode::Targeted))
            {
                return Err(SuppressionReason::TargetSelected);
            }
        } else if matching
            .iter()
            .all(|mapping| matches!(mapping.target_mode, TargetMode::Targeted))
        {
            return Err(SuppressionReason::NoTarget);
        }

        matching.sort_by_key(|mapping| match mapping.target_mode {
            TargetMode::Global => 0,
            TargetMode::Either => 1,
            TargetMode::Targeted => 2,
        });
        matching
            .into_iter()
            .find(|mapping| !matches!(mapping.target_mode, TargetMode::Targeted))
            .map(|mapping| (mapping, None))
            .ok_or(SuppressionReason::NoMapping)
    }
}

#[derive(Debug, Clone)]
struct ResolvedCandidate {
    candidate: GestureCandidate,
    mapping: GestureMapping,
    target: Option<AnchorId>,
}

#[derive(Debug, Clone, Default)]
struct TrackState {
    votes: VecDeque<Vote>,
    active: Option<ActiveGesture>,
    release_since: Option<Instant>,
    last_fire: HashMap<GestureId, Instant>,
    hold_since: HashMap<GestureId, Instant>,
}

impl TrackState {
    fn active_gesture_id(&self) -> Option<GestureId> {
        self.active.as_ref().map(|active| active.gesture_id)
    }

    fn has_active(&self) -> bool {
        self.active.is_some()
    }

    /// A tap has nothing to release, so a different gesture may fire straight away;
    /// the old tap stays latched until then, so a jitter frame can't re-arm it.
    fn active_is_tap(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.mode == TriggerMode::Tap)
    }

    fn update_candidate(
        &mut self,
        camera_id: CameraId,
        now: Instant,
        candidate: &GestureCandidate,
        mapping: &GestureMapping,
        target: Option<AnchorId>,
        config: &TriggerConfig,
    ) -> TrackProduced {
        self.release_since = None;
        self.record_candidate_vote(candidate.gesture_id, config.vote_m);
        // A selected device already confirms intent, so its verbs need fewer votes.
        let needed = if target.is_some() {
            config.selected_vote_n.min(config.vote_n)
        } else {
            config.vote_n
        };
        if self.vote_count(candidate.gesture_id) < needed {
            return TrackProduced::Suppressed(SuppressionReason::VoteFailed);
        }

        if self
            .last_fire
            .get(&candidate.gesture_id)
            .is_some_and(|last| {
                now.duration_since(*last) < Duration::from_millis(mapping.cooldown_ms)
            })
            && !matches!(mapping.mode, TriggerMode::Repeat | TriggerMode::Dial)
        {
            return TrackProduced::Suppressed(SuppressionReason::Cooldown);
        }

        let hold_since = *self.hold_since.entry(candidate.gesture_id).or_insert(now);
        match mapping.mode {
            TriggerMode::Tap => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.gesture_id == candidate.gesture_id)
                {
                    return TrackProduced::None;
                }
                let event = make_event(
                    camera_id,
                    candidate,
                    GesturePhase::Fired,
                    target,
                    hold_since,
                    now,
                );
                self.last_fire.insert(candidate.gesture_id, now);
                self.active = Some(ActiveGesture::new(
                    candidate,
                    mapping.mode,
                    target,
                    hold_since,
                    now,
                ));
                TrackProduced::Events(vec![event])
            }
            TriggerMode::Hold => {
                if now.duration_since(hold_since) < Duration::from_millis(config.hold_ms) {
                    return TrackProduced::None;
                }
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.fired && active.gesture_id == candidate.gesture_id)
                {
                    return TrackProduced::None;
                }
                let event = make_event(
                    camera_id,
                    candidate,
                    GesturePhase::Fired,
                    target,
                    hold_since,
                    now,
                );
                self.last_fire.insert(candidate.gesture_id, now);
                self.active = Some(ActiveGesture::new(
                    candidate,
                    mapping.mode,
                    target,
                    hold_since,
                    now,
                ));
                TrackProduced::Events(vec![event])
            }
            TriggerMode::Repeat => {
                if now.duration_since(hold_since) < Duration::from_millis(config.hold_ms) {
                    return TrackProduced::None;
                }
                let should_fire = self
                    .active
                    .as_ref()
                    .is_none_or(|active| !active.fired || now >= active.next_at);
                if !should_fire {
                    return TrackProduced::None;
                }
                let event = make_event(
                    camera_id,
                    candidate,
                    GesturePhase::Fired,
                    target,
                    hold_since,
                    now,
                );
                self.last_fire.insert(candidate.gesture_id, now);
                let mut active =
                    ActiveGesture::new(candidate, mapping.mode, target, hold_since, now);
                active.next_at = now + Duration::from_millis(config.repeat_ms);
                self.active = Some(active);
                TrackProduced::Events(vec![event])
            }
            TriggerMode::Dial => {
                let phase = if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.gesture_id == candidate.gesture_id)
                {
                    let can_update = self
                        .active
                        .as_ref()
                        .is_some_and(|active| now >= active.next_at);
                    if !can_update {
                        return TrackProduced::None;
                    }
                    GesturePhase::Update
                } else {
                    GesturePhase::Fired
                };
                let event = make_event(camera_id, candidate, phase, target, hold_since, now);
                let mut active =
                    ActiveGesture::new(candidate, mapping.mode, target, hold_since, now);
                active.next_at = now + Duration::from_millis(config.dial_update_ms);
                self.active = Some(active);
                TrackProduced::Events(vec![event])
            }
        }
    }

    fn update_absent(
        &mut self,
        camera_id: CameraId,
        now: Instant,
        config: &TriggerConfig,
    ) -> Vec<GestureEvent> {
        if self.active.is_none() {
            self.hold_since.clear();
            return Vec::new();
        }
        let since = self.release_since.get_or_insert(now);
        if now.duration_since(*since) < Duration::from_millis(config.release_ms) {
            return Vec::new();
        }
        let active = self.active.take();
        self.release_since = None;
        self.hold_since.clear();
        self.votes.clear();
        let Some(active) = active else {
            return Vec::new();
        };
        if matches!(
            active.mode,
            TriggerMode::Hold | TriggerMode::Repeat | TriggerMode::Dial
        ) && active.fired
        {
            vec![GestureEvent {
                id: GestureEventId::new(),
                camera_id,
                gesture_id: active.gesture_id,
                hand: active.hand.unwrap_or(Handedness::Right),
                confidence: active.confidence,
                phase: GesturePhase::End,
                value: active.value,
                target: active.target,
                onset_at: active.onset_at,
                fired_at: now,
            }]
        } else {
            Vec::new()
        }
    }

    fn begin_frame(&mut self, vote_m: usize) {
        self.push_vote(None, vote_m);
    }

    fn record_candidate_vote(&mut self, gesture_id: GestureId, vote_m: usize) {
        if let Some(vote) = self.votes.back_mut()
            && vote.gesture_id.is_none()
        {
            vote.gesture_id = Some(gesture_id);
            return;
        }
        self.push_vote(Some(gesture_id), vote_m);
    }

    fn push_vote(&mut self, gesture_id: Option<GestureId>, vote_m: usize) {
        self.votes.push_back(Vote { gesture_id });
        while self.votes.len() > vote_m {
            self.votes.pop_front();
        }
    }

    fn vote_count(&self, gesture_id: GestureId) -> usize {
        self.votes
            .iter()
            .filter(|vote| vote.gesture_id == Some(gesture_id))
            .count()
    }
}

#[derive(Debug, Clone, Copy)]
struct Vote {
    gesture_id: Option<GestureId>,
}

#[derive(Debug, Clone)]
struct ActiveGesture {
    gesture_id: GestureId,
    mode: TriggerMode,
    onset_at: Instant,
    next_at: Instant,
    fired: bool,
    hand: Option<Handedness>,
    confidence: f32,
    value: Option<f32>,
    target: Option<AnchorId>,
}

impl ActiveGesture {
    fn new(
        candidate: &GestureCandidate,
        mode: TriggerMode,
        target: Option<AnchorId>,
        onset_at: Instant,
        now: Instant,
    ) -> Self {
        Self {
            gesture_id: candidate.gesture_id,
            mode,
            onset_at,
            next_at: now,
            fired: true,
            hand: Some(candidate.hand),
            confidence: candidate.confidence,
            value: candidate.value,
            target,
        }
    }
}

enum TrackProduced {
    Events(Vec<GestureEvent>),
    Suppressed(SuppressionReason),
    None,
}

fn choose_candidate(
    candidates: &[GestureCandidate],
    conflict_margin: f32,
) -> Option<GestureCandidate> {
    let mut sorted = candidates.to_vec();
    sorted.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
    let best = sorted.first()?.clone();
    if let Some(second) = sorted.get(1)
        && second.gesture_id != best.gesture_id
        && best.confidence - second.confidence < conflict_margin
    {
        return None;
    }
    Some(best)
}

fn choose_resolved(
    candidates: &[ResolvedCandidate],
    conflict_margin: f32,
) -> Option<ResolvedCandidate> {
    let mut sorted = candidates.to_vec();
    sorted.sort_by(|left, right| {
        candidate_priority(right.candidate.gesture_id)
            .cmp(&candidate_priority(left.candidate.gesture_id))
            .then_with(|| {
                right
                    .candidate
                    .confidence
                    .total_cmp(&left.candidate.confidence)
            })
    });
    let best = sorted.first()?.clone();
    if let Some(second) = sorted.get(1)
        && candidate_priority(second.candidate.gesture_id)
            == candidate_priority(best.candidate.gesture_id)
        && second.candidate.gesture_id != best.candidate.gesture_id
        && best.candidate.confidence - second.candidate.confidence < conflict_margin
    {
        return None;
    }
    Some(best)
}

fn candidate_priority(gesture_id: GestureId) -> u8 {
    match gesture_id {
        GestureId::Builtin(
            BuiltinGesture::SwipeLeft
            | BuiltinGesture::SwipeRight
            | BuiltinGesture::SwipeUp
            | BuiltinGesture::SwipeDown
            | BuiltinGesture::PinchDial
            | BuiltinGesture::CircleCw
            | BuiltinGesture::CircleCcw
            | BuiltinGesture::CircleAny
            | BuiltinGesture::TwoHandSeparate,
        )
        | GestureId::Motion(_) => 2,
        GestureId::Builtin(_) | GestureId::Custom(_) | GestureId::SystemNone => 1,
    }
}

fn most_relevant_suppression(candidates: &[SuppressedCandidate]) -> Option<&SuppressedCandidate> {
    candidates
        .iter()
        .find(|candidate| {
            !matches!(
                candidate.reason,
                SuppressionReason::NoMapping | SuppressionReason::NoTarget
            )
        })
        .or_else(|| candidates.first())
}

fn make_event(
    camera_id: CameraId,
    candidate: &GestureCandidate,
    phase: GesturePhase,
    target: Option<AnchorId>,
    onset_at: Instant,
    fired_at: Instant,
) -> GestureEvent {
    GestureEvent {
        id: GestureEventId::new(),
        camera_id,
        gesture_id: candidate.gesture_id,
        hand: candidate.hand,
        confidence: candidate.confidence,
        phase,
        value: candidate.value,
        target,
        onset_at,
        fired_at,
    }
}

fn selected_anchor(selection: &SelectionState) -> Option<AnchorId> {
    match selection {
        SelectionState::Selected { anchor_id, .. } => Some(*anchor_id),
        SelectionState::Idle | SelectionState::Aiming | SelectionState::Hover { .. } => None,
    }
}

fn hand_matches(constraint: HandConstraint, hand: Handedness) -> bool {
    match constraint {
        HandConstraint::Any => true,
        HandConstraint::Left => matches!(hand, Handedness::Left),
        HandConstraint::Right => matches!(hand, Handedness::Right),
    }
}

fn two_hand_allowed(mapping: &GestureMapping, gesture_id: GestureId, visible_hands: usize) -> bool {
    if visible_hands < 2 || is_two_hand_exempt(gesture_id) {
        return true;
    }
    !matches!(mapping.hand, HandConstraint::Any) || mapping.allow_two_hands
}

fn is_two_hand_exempt(gesture_id: GestureId) -> bool {
    matches!(
        gesture_id,
        GestureId::Builtin(BuiltinGesture::TwoHandSeparate) | GestureId::Motion(_)
    )
}

fn gesture_matches(mapping_id: GestureId, candidate_id: GestureId) -> bool {
    mapping_id == candidate_id
        || matches!(
            (mapping_id, candidate_id),
            (
                GestureId::Builtin(BuiltinGesture::CircleAny),
                GestureId::Builtin(BuiltinGesture::CircleCw | BuiltinGesture::CircleCcw)
            )
        )
}

fn suppressed(candidate: &GestureCandidate, reason: SuppressionReason) -> SuppressedCandidate {
    SuppressedCandidate {
        gesture_id: candidate.gesture_id,
        track_id: candidate.track_id,
        reason,
    }
}
