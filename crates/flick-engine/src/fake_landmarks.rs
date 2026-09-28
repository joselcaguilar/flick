//! Fake landmark replay for camera-free dev journeys.

use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, bail};
use flick_core::{
    AnchorId, CameraId, FaceKeypoints, GestureEvent, GestureEventId, GestureId, GesturePhase,
    HandFrame, HandObservation, Handedness, SelectionState, StageTimings,
};
use flick_gestures::{GestureEngine, GestureEngineConfig};
use flick_spatial::{
    Anchor, CameraIntrinsics, IntrinsicsSource, TargetSelectorImpl, TargetSelectorSettings,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::dispatcher::{Dispatcher, owner_fan_anchor};

/// One fixture available to dev-mode callers.
#[derive(Debug, Clone, Serialize)]
pub struct ReplayFixture {
    /// Stable fixture name for the dev replay endpoint.
    pub fixture: String,
    /// Absolute source path.
    pub path: PathBuf,
}

/// Completed replay counters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReplayStats {
    /// Fixture name or path that was replayed.
    pub fixture: String,
    /// Frames read from the fixture.
    pub frames: usize,
    /// Gesture events emitted by recognizers or fixture overrides.
    pub events: usize,
    /// Gesture events handed to the dispatcher.
    pub dispatched: usize,
}

/// Landmark fixture catalog rooted at `FLICK_FAKE_LANDMARKS`.
#[derive(Debug, Clone)]
pub struct ReplayCatalog {
    root: PathBuf,
}

impl ReplayCatalog {
    /// Creates a catalog from a file or directory.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns available `.jsonl` fixtures.
    pub fn fixtures(&self) -> anyhow::Result<Vec<ReplayFixture>> {
        if self.root.is_file() {
            return Ok(vec![ReplayFixture {
                fixture: fixture_name_for_file(&self.root, self.root.parent()),
                path: self.root.clone(),
            }]);
        }

        let mut files = Vec::new();
        collect_jsonl(&self.root, &mut files)?;
        files.sort();
        Ok(files
            .into_iter()
            .map(|path| ReplayFixture {
                fixture: fixture_name_for_file(&path, Some(&self.root)),
                path,
            })
            .collect())
    }

    /// Resolves a request fixture to a JSONL file.
    pub fn resolve(&self, fixture: &str) -> anyhow::Result<(String, PathBuf)> {
        if self.root.is_file() {
            let name = fixture_name_for_file(&self.root, self.root.parent());
            if fixture.is_empty() || fixture == name || fixture == self.root.to_string_lossy() {
                return Ok((name, self.root.clone()));
            }
        }

        let relative = fixture.trim().trim_start_matches('/');
        anyhow::ensure!(!relative.is_empty(), "fixture is required");
        let requested = Path::new(relative);
        let mut candidates = Vec::new();
        candidates.push(self.root.join(requested));
        if requested.extension().is_none() {
            candidates.push(self.root.join(format!("{relative}.jsonl")));
        }
        if self
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name != "fixtures")
        {
            let prefixed = self.root.parent().unwrap_or(&self.root).join(requested);
            candidates.push(prefixed.clone());
            if requested.extension().is_none() {
                candidates.push(prefixed.with_extension("jsonl"));
            }
        }
        for path in candidates {
            if path.is_file() {
                let name = fixture_name_for_file(&path, Some(&self.root));
                return Ok((name, path));
            }
        }
        bail!("fixture not found: {fixture}")
    }
}

/// Replays one fixture once through gesture recognition and dispatch.
pub async fn replay_once(
    fixture_name: String,
    path: PathBuf,
    dispatcher: Dispatcher,
) -> anyhow::Result<ReplayStats> {
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let records = read_records(&text)?;
    let mut stats = ReplayStats {
        fixture: fixture_name.clone(),
        ..ReplayStats::default()
    };
    let mut engine = GestureEngine::new(GestureEngineConfig::default());
    let anchors = load_spatial_anchors(&path);
    let mut selector = None::<TargetSelectorImpl>;
    let start_std = std::time::Instant::now();
    let start_tokio = tokio::time::Instant::now();

    for record in records {
        let deadline = start_tokio + Duration::from_millis(record.t_ms);
        tokio::time::sleep_until(deadline).await;
        let frame = record.to_frame(start_std)?;
        stats.frames += 1;

        let spatial_selection = if let Some(intrinsics) = &record.intrinsics {
            if selector.is_none() && !anchors.is_empty() {
                selector = Some(TargetSelectorImpl::new(
                    intrinsics.to_intrinsics(),
                    anchors.clone(),
                    TargetSelectorSettings::default(),
                ));
            }
            selector
                .as_mut()
                .map(|selector| selector.update(&frame, record.face.as_ref()))
        } else {
            None
        };
        let selection = record.selection_hint(&fixture_name, spatial_selection.as_ref());
        let update = engine.update(&frame, &selection);
        stats.events += update.events.len();
        for event in update.events {
            dispatcher.dispatch(&event).await;
            stats.dispatched += 1;
        }

        if let Some(gesture) = record.gesture {
            let event = gesture.to_event(frame.camera_id, &selection, frame.captured_at)?;
            stats.events += 1;
            dispatcher.dispatch(&event).await;
            stats.dispatched += 1;
        }
    }
    Ok(stats)
}

fn load_spatial_anchors(path: &Path) -> Vec<Anchor> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let candidates = [
        parent.join("bedroom_fan.anchors.json"),
        parent.join("targeting/bedroom_fan.anchors.json"),
        parent
            .parent()
            .unwrap_or(parent)
            .join("targeting/bedroom_fan.anchors.json"),
    ];
    for candidate in candidates {
        let Ok(text) = fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(seed) = serde_json::from_str::<AnchorSeed>(&text) else {
            continue;
        };
        return seed.anchors;
    }
    Vec::new()
}

fn read_records(text: &str) -> anyhow::Result<Vec<ReplayRecord>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn collect_jsonl(root: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("reading {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

fn fixture_name_for_file(path: &Path, root: Option<&Path>) -> String {
    let relative = root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    relative
        .with_extension("")
        .to_string_lossy()
        .trim_start_matches('/')
        .to_owned()
}

#[derive(Debug, Clone, Deserialize)]
struct ReplayRecord {
    camera_id: CameraId,
    seq: u64,
    t_ms: u64,
    #[serde(default)]
    intrinsics: Option<FixtureIntrinsics>,
    #[serde(default)]
    hands: Vec<HandObservation>,
    #[serde(default)]
    timings: StageTimings,
    #[serde(default)]
    face: Option<FaceKeypoints>,
    #[serde(default)]
    gesture: Option<FixtureGesture>,
}

impl ReplayRecord {
    fn to_frame(&self, base: std::time::Instant) -> anyhow::Result<HandFrame> {
        let mut timings = self.timings;
        timings.targeting_ms = if self.face.is_some() {
            timings.targeting_ms.max(0.1)
        } else {
            timings.targeting_ms
        };
        Ok(HandFrame {
            camera_id: self.camera_id,
            seq: self.seq,
            captured_at: base + Duration::from_millis(self.t_ms),
            hands: SmallVec::from_vec(self.hands.clone()),
            timings,
        })
    }

    fn selection_hint(
        &self,
        fixture_name: &str,
        spatial_selection: Option<&SelectionState>,
    ) -> SelectionState {
        if matches!(
            spatial_selection,
            Some(SelectionState::Selected { .. } | SelectionState::Hover { .. })
        ) {
            return selected_owner_fan();
        }
        if self
            .gesture
            .as_ref()
            .and_then(|gesture| gesture.target())
            .is_some()
            || fixture_name.contains("owner_fan_")
            || fixture_name.contains("point_fan_")
        {
            selected_owner_fan()
        } else {
            SelectionState::Idle
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AnchorSeed {
    anchors: Vec<Anchor>,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureIntrinsics {
    width: u32,
    height: u32,
    hfov_deg: f32,
    source: IntrinsicsSource,
    version: String,
}

impl FixtureIntrinsics {
    fn to_intrinsics(&self) -> CameraIntrinsics {
        CameraIntrinsics::from_horizontal_fov(
            self.width,
            self.height,
            self.hfov_deg,
            self.source.clone(),
            self.version.clone(),
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureGesture {
    id: String,
    phase: GesturePhase,
    target_anchor_id: Option<String>,
}

impl FixtureGesture {
    fn target(&self) -> Option<AnchorId> {
        self.target_anchor_id
            .as_deref()
            .and_then(|id| AnchorId::from_str(id).ok())
    }

    fn to_event(
        &self,
        camera_id: CameraId,
        selection: &SelectionState,
        at: std::time::Instant,
    ) -> anyhow::Result<GestureEvent> {
        let target = match selection {
            SelectionState::Selected { anchor_id, .. }
            | SelectionState::Hover { anchor_id, .. } => Some(*anchor_id),
            SelectionState::Idle | SelectionState::Aiming => self.target(),
        };
        Ok(GestureEvent {
            id: GestureEventId::new(),
            camera_id,
            gesture_id: GestureId::from_str(&self.id)?,
            hand: Handedness::Right,
            confidence: 1.0,
            phase: self.phase,
            value: None,
            target,
            onset_at: at,
            fired_at: at,
        })
    }
}

fn selected_owner_fan() -> SelectionState {
    SelectionState::Selected {
        anchor_id: owner_fan_anchor().id,
        domain: "fan".to_owned(),
        expires_at_ms: i64::MAX,
    }
}
