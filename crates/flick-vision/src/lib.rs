//! Hand perception, ONNX Runtime sessions and vision parity code for `02-vision-pipeline.md` §§2–3.
//!
//! Public API highlights:
//! - [`ModelSet::load`] parses `models/manifest.toml` and verifies cached model SHA-256 values.
//! - [`OrtSessionFactory`] creates ONNX Runtime sessions with platform EP ordering and CPU fallback.
//! - [`HandPipelineImpl`] implements [`flick_core::HandPipeline`] and is hot-swap friendly: build a new
//!   instance off-thread, then swap it between frames.
//! - [`FaceKeypointRunner`] and [`SceneEmbedder`] are on-demand helpers for targeting and scene signatures.

use std::{
    collections::{BTreeMap, VecDeque},
    f32::consts::{FRAC_PI_2, PI},
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};

use flick_core::{
    CannedGestureScores, ExecutionProvider, FaceKeypoints, Frame, HandFrame, HandObservation,
    HandPipeline, Handedness, PixelFormat, RectF, StageTimings, VisionError,
};
use nalgebra::{Matrix2, Vector2};
use ndarray::Array4;
use ort::{
    ep,
    session::{Session, SessionInputValue, builder::GraphOptimizationLevel},
    value::TensorRef,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use smallvec::SmallVec;
use tracing::warn;

const PALM_INPUT_SIZE: u32 = 192;
const LANDMARK_INPUT_SIZE: u32 = 224;
const FACE_INPUT_SIZE: u32 = 128;
const SCENE_EMBEDDING_DIMS: usize = 384;

/// A loaded model manifest plus verified cache paths.
#[derive(Debug, Clone)]
pub struct ModelSet {
    manifest: ModelManifest,
    root: PathBuf,
    verified: BTreeMap<String, PathBuf>,
}

impl ModelSet {
    /// Loads `manifest.toml` from a model root or an explicit manifest path and verifies selected cached models.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, VisionError> {
        let path = path.as_ref();
        let manifest_path = if path.is_dir() {
            path.join("manifest.toml")
        } else {
            path.to_path_buf()
        };
        let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let root = if manifest_dir.file_name().and_then(|name| name.to_str()) == Some("models") {
            manifest_dir
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        } else {
            manifest_dir.to_path_buf()
        };
        let text = fs::read_to_string(&manifest_path).map_err(|err| {
            VisionError::ModelUnavailable(format!(
                "failed to read {}: {err}",
                manifest_path.display()
            ))
        })?;
        let manifest: ModelManifest = toml::from_str(&text).map_err(|err| {
            VisionError::ModelUnavailable(format!(
                "failed to parse {}: {err}",
                manifest_path.display()
            ))
        })?;
        let mut verified = BTreeMap::new();
        for model in manifest.models.iter().filter(|model| model.is_selected()) {
            let cache_path = root.join(&model.cache_path);
            verify_model_file(&cache_path, &model.sha256).map_err(|err| {
                VisionError::ModelUnavailable(format!(
                    "model {} failed verification at {}: {err}",
                    model.id,
                    cache_path.display()
                ))
            })?;
            verified.insert(model.id.clone(), cache_path);
        }
        Ok(Self {
            manifest,
            root,
            verified,
        })
    }

    /// Loads a manifest without requiring the cache files to exist. Useful for `fetch-models` planning.
    pub fn read_manifest(path: impl AsRef<Path>) -> Result<ModelManifest, VisionError> {
        let text = fs::read_to_string(path.as_ref()).map_err(|err| {
            VisionError::ModelUnavailable(format!(
                "failed to read {}: {err}",
                path.as_ref().display()
            ))
        })?;
        toml::from_str(&text).map_err(|err| VisionError::ModelUnavailable(err.to_string()))
    }

    /// Returns the manifest root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns a manifest entry by id.
    #[must_use]
    pub fn model(&self, id: &str) -> Option<&ModelEntry> {
        self.manifest.models.iter().find(|model| model.id == id)
    }

    /// Returns the verified cache path for a model id.
    #[must_use]
    pub fn path(&self, id: &str) -> Option<&Path> {
        self.verified.get(id).map(PathBuf::as_path)
    }

    /// Verifies and returns an optional cache path when the manifest entry and file are present.
    pub fn optional_path(&self, id: &str) -> Result<Option<PathBuf>, VisionError> {
        let Some(model) = self.model(id) else {
            return Ok(None);
        };
        let cache_path = self.root.join(&model.cache_path);
        if !cache_path.exists() {
            return Ok(None);
        }
        verify_model_file(&cache_path, &model.sha256).map_err(|err| {
            VisionError::ModelUnavailable(format!(
                "optional model {} failed verification at {}: {err}",
                model.id,
                cache_path.display()
            ))
        })?;
        Ok(Some(cache_path))
    }

    /// Returns all verified model ids and paths.
    pub fn verified_paths(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.verified
            .iter()
            .map(|(id, path)| (id.as_str(), path.as_path()))
    }
}

/// `models/manifest.toml` top-level document.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelManifest {
    /// Manifest schema version.
    pub schema_version: Option<u32>,
    /// Model entries.
    pub models: Vec<ModelEntry>,
}

/// One model entry from the manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelEntry {
    /// Stable model id.
    pub id: String,
    /// Version/revision label.
    pub version: String,
    /// File format, usually `onnx`.
    pub format: String,
    /// Spike selection/conversion state.
    pub conversion_status: Option<String>,
    /// Download URL.
    pub source_url: String,
    /// Expected SHA-256 of the cache file.
    pub sha256: String,
    /// Relative cache path.
    pub cache_path: PathBuf,
    /// Preferred EP names from the manifest.
    #[serde(default)]
    pub preferred_ep: Vec<String>,
    /// Input tensor descriptions.
    #[serde(default)]
    pub inputs: Vec<TensorInfo>,
    /// Output tensor descriptions.
    #[serde(default)]
    pub outputs: Vec<TensorInfo>,
}

impl ModelEntry {
    /// Whether this entry should be fetched and verified by default.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.conversion_status
            .as_deref()
            .is_none_or(|status| status.starts_with("selected"))
    }
}

/// Tensor metadata recorded by the model spike.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TensorInfo {
    /// Tensor name.
    pub name: String,
    /// Element type.
    pub dtype: String,
    /// Human-readable shape.
    pub shape: String,
    /// Optional description.
    pub description: Option<String>,
}

/// Verifies one cached model file against its manifest hash.
pub fn verify_model_file(path: &Path, expected_sha256: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|err| err.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_sha256) {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch: expected {expected_sha256}, got {actual}"
        ))
    }
}

/// Execution provider selected for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpKind {
    /// Auto-select by platform order/benchmark.
    Auto,
    /// CoreML execution provider.
    CoreMl,
    /// DirectML execution provider.
    DirectMl,
    /// CUDA execution provider.
    Cuda,
    /// OpenVINO execution provider.
    OpenVino,
    /// XNNPACK execution provider.
    Xnnpack,
    /// CPU execution provider.
    Cpu,
}

impl From<ExecutionProvider> for EpKind {
    fn from(value: ExecutionProvider) -> Self {
        match value {
            ExecutionProvider::Auto => Self::Auto,
            ExecutionProvider::Coreml => Self::CoreMl,
            ExecutionProvider::Directml => Self::DirectMl,
            ExecutionProvider::Cuda => Self::Cuda,
            ExecutionProvider::Openvino => Self::OpenVino,
            ExecutionProvider::Xnnpack => Self::Xnnpack,
            ExecutionProvider::Cpu => Self::Cpu,
        }
    }
}

/// Persistable EP choices keyed by model id (`settings.inference.ep_choice`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpChoice {
    /// Model id to EP mapping.
    pub per_model: BTreeMap<String, EpKind>,
}

impl EpChoice {
    /// Returns the requested EP for `model_id`, or [`EpKind::Auto`].
    #[must_use]
    pub fn get(&self, model_id: &str) -> EpKind {
        self.per_model
            .get(model_id)
            .copied()
            .unwrap_or(EpKind::Auto)
    }

    /// Records an EP choice.
    pub fn insert(&mut self, model_id: impl Into<String>, ep: EpKind) {
        self.per_model.insert(model_id.into(), ep);
    }
}

/// ORT session factory with Flick's threading and provider defaults.
#[derive(Debug, Clone)]
pub struct OrtSessionFactory {
    /// CoreML compiled-model cache directory.
    pub coreml_cache_dir: PathBuf,
    /// ORT intra-op thread count.
    pub intra_op: usize,
    /// ORT inter-op thread count.
    pub inter_op: usize,
}

impl OrtSessionFactory {
    /// Creates a session factory rooted at the model cache directory.
    #[must_use]
    pub fn new(cache_root: impl Into<PathBuf>) -> Self {
        Self {
            coreml_cache_dir: cache_root.into().join("coreml"),
            intra_op: 2,
            inter_op: 1,
        }
    }

    /// Returns Flick's platform EP order before CPU fallback.
    #[must_use]
    pub fn platform_order() -> Vec<EpKind> {
        #[cfg(target_os = "macos")]
        {
            vec![EpKind::CoreMl, EpKind::Xnnpack, EpKind::Cpu]
        }
        #[cfg(target_os = "windows")]
        {
            vec![EpKind::DirectMl, EpKind::Cpu]
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            vec![EpKind::Cuda, EpKind::OpenVino, EpKind::Xnnpack, EpKind::Cpu]
        }
    }

    /// Builds a session, falling back to CPU if the requested EP cannot be loaded.
    pub fn build_session(
        &self,
        model_path: &Path,
        requested: EpKind,
    ) -> Result<(Session, EpKind), VisionError> {
        let order = if requested == EpKind::Auto {
            Self::platform_order()
        } else if requested == EpKind::Cpu {
            vec![EpKind::Cpu]
        } else {
            vec![requested, EpKind::Cpu]
        };
        let mut last_error = None;
        for ep in order {
            match self.try_build_session(model_path, ep) {
                Ok(session) => return Ok((session, ep)),
                Err(err) => last_error = Some(err),
            }
        }
        Err(VisionError::Inference(last_error.unwrap_or_else(|| {
            "no execution provider could load the model".to_owned()
        })))
    }

    /// Performs a bounded first-run provider benchmark. The engine persists the returned [`EpChoice`].
    pub fn auto_benchmark(&self, models: &ModelSet) -> EpChoice {
        let started = Instant::now();
        let mut choice = EpChoice::default();
        for (id, path) in models.verified_paths() {
            if started.elapsed().as_secs() >= 10 {
                choice.insert(id, EpKind::Cpu);
                continue;
            }
            let Some(model) = models.model(id) else {
                choice.insert(id, EpKind::Cpu);
                continue;
            };
            let mut candidates = model
                .preferred_ep
                .iter()
                .filter_map(|name| parse_ep_name(name))
                .filter(|ep| *ep != EpKind::Auto)
                .collect::<Vec<_>>();
            candidates.push(default_ep_for_model(id));
            candidates.push(EpKind::Cpu);
            candidates.sort();
            candidates.dedup();
            let selected = self
                .benchmark_model(path, model, &candidates, started)
                .unwrap_or_else(|| default_ep_for_model(id));
            choice.insert(id, selected);
        }
        choice
    }

    fn benchmark_model(
        &self,
        model_path: &Path,
        model: &ModelEntry,
        candidates: &[EpKind],
        started: Instant,
    ) -> Option<EpKind> {
        let inputs = prepare_dummy_inputs(model).ok()?;
        let mut best: Option<(EpKind, f32)> = None;
        for ep in candidates {
            if started.elapsed().as_secs() >= 10 {
                break;
            }
            let Ok((mut session, selected)) = self.build_session(model_path, *ep) else {
                continue;
            };
            let Ok(ms) = benchmark_session(&mut session, &inputs, 5, 10) else {
                continue;
            };
            if best.is_none_or(|(_, best_ms)| ms < best_ms) {
                best = Some((selected, ms));
            }
        }
        best.map(|(ep, _)| ep)
    }

    fn try_build_session(&self, model_path: &Path, ep_kind: EpKind) -> Result<Session, String> {
        let _ = ort::init().commit();
        let mut builder = Session::builder()
            .map_err(|err| err.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|err| err.to_string())?
            .with_intra_threads(self.intra_op)
            .map_err(|err| err.to_string())?
            .with_inter_threads(self.inter_op)
            .map_err(|err| err.to_string())?;

        match ep_kind {
            EpKind::Cpu => {}
            EpKind::CoreMl => {
                fs::create_dir_all(&self.coreml_cache_dir).map_err(|err| err.to_string())?;
                let coreml = ep::CoreML::default()
                    .with_model_format(ep::coreml::ModelFormat::NeuralNetwork)
                    .with_compute_units(ep::coreml::ComputeUnits::CPUAndNeuralEngine)
                    .with_model_cache_dir(self.coreml_cache_dir.to_string_lossy())
                    .build();
                builder = builder
                    .with_execution_providers([coreml.error_on_failure()])
                    .map_err(|err| err.to_string())?;
            }
            EpKind::Auto => return Err("auto EP must be expanded before session build".to_owned()),
            EpKind::DirectMl | EpKind::Cuda | EpKind::OpenVino | EpKind::Xnnpack => {
                return Err(format!(
                    "{ep_kind:?} execution provider is not enabled in this build"
                ));
            }
        }

        builder
            .commit_from_file(model_path)
            .map_err(|err| err.to_string())
    }
}

fn parse_ep_name(name: &str) -> Option<EpKind> {
    match name.to_ascii_lowercase().as_str() {
        "auto" => Some(EpKind::Auto),
        "coreml" => Some(EpKind::CoreMl),
        "directml" => Some(EpKind::DirectMl),
        "cuda" => Some(EpKind::Cuda),
        "openvino" => Some(EpKind::OpenVino),
        "xnnpack" => Some(EpKind::Xnnpack),
        "cpu" => Some(EpKind::Cpu),
        _ => None,
    }
}

fn default_ep_for_model(model_id: &str) -> EpKind {
    match model_id {
        "hand_landmark_full" => EpKind::CoreMl,
        "palm_detection_full" | "face_detection_short" | "scene_embedder" => EpKind::Cpu,
        _ => EpKind::Cpu,
    }
}

#[derive(Debug, Clone)]
struct PreparedInput {
    name: String,
    shape: Vec<i64>,
    data: Vec<f32>,
}

fn prepare_dummy_inputs(model: &ModelEntry) -> Result<Vec<PreparedInput>, VisionError> {
    model
        .inputs
        .iter()
        .map(|input| {
            let shape = parse_tensor_shape(&input.shape)?;
            let len = shape.iter().try_fold(1_usize, |acc, dim| {
                usize::try_from(*dim)
                    .ok()
                    .and_then(|dim| acc.checked_mul(dim))
            });
            let Some(len) = len else {
                return Err(VisionError::InvalidModelOutput(format!(
                    "invalid tensor shape {} for {}",
                    input.shape, input.name
                )));
            };
            Ok(PreparedInput {
                name: input.name.clone(),
                shape,
                data: vec![0.0; len],
            })
        })
        .collect()
}

fn parse_tensor_shape(shape: &str) -> Result<Vec<i64>, VisionError> {
    shape
        .split(['x', 'X'])
        .map(|part| {
            part.trim().parse::<i64>().map_err(|err| {
                VisionError::InvalidModelOutput(format!("invalid tensor shape {shape}: {err}"))
            })
        })
        .collect()
}

fn benchmark_session(
    session: &mut Session,
    inputs: &[PreparedInput],
    warmup: usize,
    iterations: usize,
) -> Result<f32, VisionError> {
    for _ in 0..warmup {
        run_prepared(session, inputs)?;
    }
    let mut elapsed = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        run_prepared(session, inputs)?;
        elapsed.push(started.elapsed().as_secs_f32() * 1_000.0);
    }
    elapsed.sort_by(f32::total_cmp);
    let index = ((elapsed.len().saturating_sub(1)) as f32 * 0.95).round() as usize;
    elapsed
        .get(index)
        .copied()
        .ok_or_else(|| VisionError::Inference("benchmark produced no timings".to_owned()))
}

fn run_prepared(session: &mut Session, inputs: &[PreparedInput]) -> Result<(), VisionError> {
    let mut values: Vec<(String, SessionInputValue<'_>)> = Vec::with_capacity(inputs.len());
    for input in inputs {
        let tensor = TensorRef::from_array_view((input.shape.clone(), input.data.as_slice()))
            .map_err(|err| VisionError::Inference(err.to_string()))?;
        values.push((input.name.clone(), tensor.into()));
    }
    session
        .run(values)
        .map_err(|err| VisionError::Inference(err.to_string()))?;
    Ok(())
}

/// One SSD anchor in normalized input coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Center x.
    pub x_center: f32,
    /// Center y.
    pub y_center: f32,
    /// Anchor width.
    pub w: f32,
    /// Anchor height.
    pub h: f32,
}

/// A decoded palm candidate in normalized frame coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct PalmDetection {
    /// Bounding box.
    pub bbox: RectF,
    /// Seven MediaPipe palm keypoints.
    pub keypoints: [[f32; 2]; 7],
    /// Sigmoid confidence.
    pub score: f32,
}

/// Letterbox transform used before palm inference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Letterbox {
    /// Input image width.
    pub src_width: u32,
    /// Input image height.
    pub src_height: u32,
    /// Model input width.
    pub dst_width: u32,
    /// Model input height.
    pub dst_height: u32,
    /// Resize scale from source pixels to destination pixels.
    pub scale: f32,
    /// Horizontal padding in destination pixels.
    pub pad_x: f32,
    /// Vertical padding in destination pixels.
    pub pad_y: f32,
}

impl Letterbox {
    /// Creates a letterbox transform preserving aspect ratio.
    #[must_use]
    pub fn new(src_width: u32, src_height: u32, dst_width: u32, dst_height: u32) -> Self {
        let scale =
            (dst_width as f32 / src_width as f32).min(dst_height as f32 / src_height as f32);
        let resized_w = src_width as f32 * scale;
        let resized_h = src_height as f32 * scale;
        Self {
            src_width,
            src_height,
            dst_width,
            dst_height,
            scale,
            pad_x: (dst_width as f32 - resized_w) * 0.5,
            pad_y: (dst_height as f32 - resized_h) * 0.5,
        }
    }

    fn unletterbox_point(&self, point: [f32; 2]) -> [f32; 2] {
        let x_px = point[0] * self.dst_width as f32 - self.pad_x;
        let y_px = point[1] * self.dst_height as f32 - self.pad_y;
        [
            (x_px / self.scale / self.src_width as f32).clamp(0.0, 1.0),
            (y_px / self.scale / self.src_height as f32).clamp(0.0, 1.0),
        ]
    }

    fn unletterbox_rect(&self, rect: RectF) -> RectF {
        let p0 = self.unletterbox_point([rect.x, rect.y]);
        let p1 = self.unletterbox_point([rect.x + rect.w, rect.y + rect.h]);
        RectF {
            x: p0[0].min(p1[0]),
            y: p0[1].min(p1[1]),
            w: (p1[0] - p0[0]).abs(),
            h: (p1[1] - p0[1]).abs(),
        }
    }
}

/// Generates the 2016 MediaPipe palm anchors from the normative SSD options.
#[must_use]
pub fn generate_palm_anchors() -> Vec<Anchor> {
    let strides = [8_u32, 16, 16, 16];
    let min_scale = 0.148_437_5_f32;
    let max_scale = 0.75_f32;
    let mut anchors = Vec::with_capacity(2016);
    let mut layer_id = 0_usize;
    while layer_id < strides.len() {
        let mut anchor_scales = Vec::new();
        let stride = strides[layer_id];
        let mut last_same_stride = layer_id;
        while last_same_stride < strides.len() && strides[last_same_stride] == stride {
            let scale = calculate_scale(min_scale, max_scale, last_same_stride, strides.len());
            let scale_next = if last_same_stride == strides.len() - 1 {
                1.0
            } else {
                calculate_scale(min_scale, max_scale, last_same_stride + 1, strides.len())
            };
            anchor_scales.push(scale);
            anchor_scales.push((scale * scale_next).sqrt());
            last_same_stride += 1;
        }
        let feature_w = PALM_INPUT_SIZE.div_ceil(stride);
        let feature_h = PALM_INPUT_SIZE.div_ceil(stride);
        for y in 0..feature_h {
            for x in 0..feature_w {
                for _scale in &anchor_scales {
                    anchors.push(Anchor {
                        x_center: (x as f32 + 0.5) / feature_w as f32,
                        y_center: (y as f32 + 0.5) / feature_h as f32,
                        w: 1.0,
                        h: 1.0,
                    });
                }
            }
        }
        layer_id = last_same_stride;
    }
    anchors
}

fn calculate_scale(min_scale: f32, max_scale: f32, layer: usize, layers: usize) -> f32 {
    if layers == 1 {
        (min_scale + max_scale) * 0.5
    } else {
        min_scale + (max_scale - min_scale) * layer as f32 / (layers - 1) as f32
    }
}

/// Decodes raw palm model outputs and applies weighted NMS.
pub fn decode_palms(
    regressors: &[[f32; 18]],
    logits: &[f32],
    anchors: &[Anchor],
    letterbox: Letterbox,
    min_score: f32,
    nms_iou: f32,
) -> Result<Vec<PalmDetection>, VisionError> {
    if regressors.len() != logits.len() || regressors.len() != anchors.len() {
        return Err(VisionError::InvalidModelOutput(format!(
            "palm output lengths differ: regressors={}, logits={}, anchors={}",
            regressors.len(),
            logits.len(),
            anchors.len()
        )));
    }
    let mut detections = Vec::new();
    for ((raw, logit), anchor) in regressors.iter().zip(logits).zip(anchors) {
        let score = sigmoid(*logit);
        if score < min_score {
            continue;
        }
        let cx = raw[0] / PALM_INPUT_SIZE as f32 * anchor.w + anchor.x_center;
        let cy = raw[1] / PALM_INPUT_SIZE as f32 * anchor.h + anchor.y_center;
        let w = raw[2] / PALM_INPUT_SIZE as f32 * anchor.w;
        let h = raw[3] / PALM_INPUT_SIZE as f32 * anchor.h;
        let mut keypoints = [[0.0_f32; 2]; 7];
        for (idx, point) in keypoints.iter_mut().enumerate() {
            let base = 4 + idx * 2;
            *point = letterbox.unletterbox_point([
                raw[base] / PALM_INPUT_SIZE as f32 * anchor.w + anchor.x_center,
                raw[base + 1] / PALM_INPUT_SIZE as f32 * anchor.h + anchor.y_center,
            ]);
        }
        let bbox = letterbox.unletterbox_rect(RectF {
            x: cx - w * 0.5,
            y: cy - h * 0.5,
            w,
            h,
        });
        detections.push(PalmDetection {
            bbox,
            keypoints,
            score,
        });
    }
    Ok(weighted_nms(detections, nms_iou))
}

/// Weighted non-maximum suppression for palm candidates.
#[must_use]
pub fn weighted_nms(mut detections: Vec<PalmDetection>, iou_threshold: f32) -> Vec<PalmDetection> {
    detections.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut remaining: VecDeque<PalmDetection> = detections.into();
    let mut kept = Vec::new();
    while let Some(seed) = remaining.pop_front() {
        let mut cluster = vec![seed];
        let mut index = 0;
        while index < remaining.len() {
            if iou(cluster[0].bbox, remaining[index].bbox) > iou_threshold {
                if let Some(detection) = remaining.remove(index) {
                    cluster.push(detection);
                }
            } else {
                index += 1;
            }
        }
        kept.push(weighted_average(&cluster));
    }
    kept
}

fn weighted_average(cluster: &[PalmDetection]) -> PalmDetection {
    let weight_sum = cluster
        .iter()
        .map(|detection| detection.score)
        .sum::<f32>()
        .max(f32::EPSILON);
    let mut bbox = RectF {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    let mut keypoints = [[0.0_f32; 2]; 7];
    let mut score = 0.0_f32;
    for detection in cluster {
        let weight = detection.score / weight_sum;
        bbox.x += detection.bbox.x * weight;
        bbox.y += detection.bbox.y * weight;
        bbox.w += detection.bbox.w * weight;
        bbox.h += detection.bbox.h * weight;
        score = score.max(detection.score);
        for (dst, src) in keypoints.iter_mut().zip(detection.keypoints) {
            dst[0] += src[0] * weight;
            dst[1] += src[1] * weight;
        }
    }
    PalmDetection {
        bbox,
        keypoints,
        score,
    }
}

/// Computes intersection-over-union for normalized rectangles.
#[must_use]
pub fn iou(a: RectF, b: RectF) -> f32 {
    let ax1 = a.x + a.w;
    let ay1 = a.y + a.h;
    let bx1 = b.x + b.w;
    let by1 = b.y + b.h;
    let ix0 = a.x.max(b.x);
    let iy0 = a.y.max(b.y);
    let ix1 = ax1.min(bx1);
    let iy1 = ay1.min(by1);
    let iw = (ix1 - ix0).max(0.0);
    let ih = (iy1 - iy0).max(0.0);
    let intersection = iw * ih;
    let union = a.w * a.h + b.w * b.h - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

/// A rotated square ROI in normalized frame coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotatedRoi {
    /// Center x.
    pub cx: f32,
    /// Center y.
    pub cy: f32,
    /// Square side length in normalized frame units.
    pub size: f32,
    /// Clockwise rotation in radians in image coordinates.
    pub rotation: f32,
}

/// Creates a hand ROI from a decoded palm using the spec's wrist→middle-MCP rotation.
#[must_use]
pub fn roi_from_palm(palm: &PalmDetection) -> RotatedRoi {
    let wrist = palm.keypoints[0];
    let middle_mcp = palm.keypoints[2];
    let rotation = normalize_radians(
        FRAC_PI_2 - (-(middle_mcp[1] - wrist[1])).atan2(middle_mcp[0] - wrist[0]),
    );
    let center = Vector2::new(
        palm.bbox.x + palm.bbox.w * 0.5,
        palm.bbox.y + palm.bbox.h * 0.5,
    );
    let shift = rotate(Vector2::new(0.0, -0.5 * palm.bbox.h), rotation);
    let shifted = center + shift;
    RotatedRoi {
        cx: shifted.x,
        cy: shifted.y,
        size: palm.bbox.w.max(palm.bbox.h) * 2.6,
        rotation,
    }
}

/// Creates the next-frame tracking ROI from landmarks.
#[must_use]
pub fn roi_from_landmarks(landmarks: &[[f32; 3]; 21]) -> RotatedRoi {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in landmarks {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    let wrist = landmarks[0];
    let middle = landmarks[9];
    let rotation =
        normalize_radians(FRAC_PI_2 - (-(middle[1] - wrist[1])).atan2(middle[0] - wrist[0]));
    let size = (max_x - min_x).max(max_y - min_y) * 2.0;
    let center = Vector2::new((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    let shifted = center + rotate(Vector2::new(0.0, -0.1 * size), rotation);
    RotatedRoi {
        cx: shifted.x,
        cy: shifted.y,
        size,
        rotation,
    }
}

/// ROI affine projection pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiAffine {
    roi: RotatedRoi,
    inverse_rotation: Matrix2<f32>,
}

impl RoiAffine {
    /// Creates projection transforms for a ROI.
    #[must_use]
    pub fn new(roi: RotatedRoi) -> Self {
        let (sin, cos) = roi.rotation.sin_cos();
        let inverse_rotation = Matrix2::new(cos, sin, -sin, cos);
        Self {
            roi,
            inverse_rotation,
        }
    }

    /// Projects normalized crop coordinates `[0,1]` back to normalized image coordinates.
    #[must_use]
    pub fn roi_to_image(&self, point: [f32; 2]) -> [f32; 2] {
        let local = Vector2::new(
            (point[0] - 0.5) * self.roi.size,
            (point[1] - 0.5) * self.roi.size,
        );
        let image = Vector2::new(self.roi.cx, self.roi.cy) + rotate(local, self.roi.rotation);
        [image.x, image.y]
    }

    /// Projects normalized image coordinates into normalized crop coordinates.
    #[must_use]
    pub fn image_to_roi(&self, point: [f32; 2]) -> [f32; 2] {
        let image = Vector2::new(point[0] - self.roi.cx, point[1] - self.roi.cy);
        let local = self.inverse_rotation * image;
        [local.x / self.roi.size + 0.5, local.y / self.roi.size + 0.5]
    }
}

/// Warps an RGB frame ROI to a square RGB crop with bilinear sampling.
pub fn warp_rgb_roi(
    frame: &Frame,
    roi: RotatedRoi,
    output_size: u32,
) -> Result<Vec<u8>, VisionError> {
    if frame.format != PixelFormat::Rgb8 {
        return Err(VisionError::UnsupportedFrame(
            "ROI warp requires RGB8".to_owned(),
        ));
    }
    let affine = RoiAffine::new(roi);
    let mut output = vec![0_u8; (output_size * output_size * 3) as usize];
    for y in 0..output_size {
        for x in 0..output_size {
            let image_point = affine.roi_to_image([
                (x as f32 + 0.5) / output_size as f32,
                (y as f32 + 0.5) / output_size as f32,
            ]);
            let rgb = bilinear_rgb(frame, image_point[0], image_point[1]);
            let offset = ((y * output_size + x) * 3) as usize;
            output[offset..offset + 3].copy_from_slice(&rgb);
        }
    }
    Ok(output)
}

fn bilinear_rgb(frame: &Frame, nx: f32, ny: f32) -> [u8; 3] {
    if !(0.0..=1.0).contains(&nx) || !(0.0..=1.0).contains(&ny) {
        return [0, 0, 0];
    }
    let x = nx * (frame.width.saturating_sub(1)) as f32;
    let y = ny * (frame.height.saturating_sub(1)) as f32;
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(frame.width - 1);
    let y1 = (y0 + 1).min(frame.height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let c00 = pixel_rgb(frame, x0, y0);
    let c10 = pixel_rgb(frame, x1, y0);
    let c01 = pixel_rgb(frame, x0, y1);
    let c11 = pixel_rgb(frame, x1, y1);
    let mut out = [0_u8; 3];
    for channel in 0..3 {
        let top = c00[channel] as f32 * (1.0 - tx) + c10[channel] as f32 * tx;
        let bottom = c01[channel] as f32 * (1.0 - tx) + c11[channel] as f32 * tx;
        out[channel] = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn pixel_rgb(frame: &Frame, x: u32, y: u32) -> [u8; 3] {
    let offset = ((y * frame.width + x) * 3) as usize;
    [
        frame.data[offset],
        frame.data[offset + 1],
        frame.data[offset + 2],
    ]
}

fn rotate(vector: Vector2<f32>, radians: f32) -> Vector2<f32> {
    let (sin, cos) = radians.sin_cos();
    Vector2::new(
        vector.x * cos - vector.y * sin,
        vector.x * sin + vector.y * cos,
    )
}

fn normalize_radians(mut radians: f32) -> f32 {
    while radians > PI {
        radians -= 2.0 * PI;
    }
    while radians < -PI {
        radians += 2.0 * PI;
    }
    radians
}

/// Converts MediaPipe handedness into the user's physical hand.
#[must_use]
pub fn mirror_aware_handedness(mediapipe: Handedness, mirrored_input: bool) -> Handedness {
    if mirrored_input {
        mediapipe
    } else {
        match mediapipe {
            Handedness::Left => Handedness::Right,
            Handedness::Right => Handedness::Left,
        }
    }
}

/// One-Euro smoothing filter for landmark coordinates.
#[derive(Debug, Clone)]
pub struct OneEuroFilter {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,
    last_time: Option<f32>,
    last_value: f32,
    last_derivative: f32,
}

impl OneEuroFilter {
    /// Creates a filter with the spec's initial tuning.
    #[must_use]
    pub fn new() -> Self {
        Self {
            min_cutoff: 1.0,
            beta: 0.02,
            d_cutoff: 1.0,
            last_time: None,
            last_value: 0.0,
            last_derivative: 0.0,
        }
    }

    /// Filters `value` at timestamp seconds.
    pub fn filter(&mut self, time_s: f32, value: f32) -> f32 {
        let Some(last_time) = self.last_time else {
            self.last_time = Some(time_s);
            self.last_value = value;
            return value;
        };
        let dt = (time_s - last_time).max(1.0 / 120.0);
        let derivative = (value - self.last_value) / dt;
        let derivative_alpha = smoothing_alpha(dt, self.d_cutoff);
        let filtered_derivative = exp_smooth(derivative_alpha, derivative, self.last_derivative);
        let cutoff = self.min_cutoff + self.beta * filtered_derivative.abs();
        let alpha = smoothing_alpha(dt, cutoff);
        let filtered = exp_smooth(alpha, value, self.last_value);
        self.last_time = Some(time_s);
        self.last_value = filtered;
        self.last_derivative = filtered_derivative;
        filtered
    }
}

impl Default for OneEuroFilter {
    fn default() -> Self {
        Self::new()
    }
}

fn smoothing_alpha(dt: f32, cutoff: f32) -> f32 {
    let tau = 1.0 / (2.0 * PI * cutoff);
    1.0 / (1.0 + tau / dt)
}

fn exp_smooth(alpha: f32, value: f32, previous: f32) -> f32 {
    alpha * value + (1.0 - alpha) * previous
}

/// Adaptive FPS state machine (idle 5 fps ↔ active 30 fps).
#[derive(Debug, Clone)]
pub struct AdaptiveFrameRate {
    idle_fps: u32,
    active_fps: u32,
    active_until_ms: u64,
    current_fps: u32,
}

impl AdaptiveFrameRate {
    /// Creates an adaptive FPS controller.
    #[must_use]
    pub fn new(idle_fps: u32, active_fps: u32) -> Self {
        Self {
            idle_fps,
            active_fps,
            active_until_ms: 0,
            current_fps: idle_fps,
        }
    }

    /// Updates hand activity and returns the target FPS.
    pub fn update(&mut self, now_ms: u64, hands_present: bool) -> u32 {
        if hands_present {
            self.active_until_ms = now_ms.saturating_add(2_000);
            self.current_fps = self.active_fps;
        } else if now_ms >= self.active_until_ms {
            self.current_fps = self.idle_fps;
        }
        self.current_fps
    }
}

/// Main hand pipeline implementation.
pub struct HandPipelineImpl {
    runtime: Option<HandRuntime>,
    ep_choice: EpChoice,
    anchors: Vec<Anchor>,
    tracks: Vec<TrackState>,
    next_track_id: u32,
    frame_count: u64,
    adaptive_fps: AdaptiveFrameRate,
    started: Instant,
    mirrored_input: bool,
    max_hands: usize,
}

impl HandPipelineImpl {
    /// Builds a hand pipeline from a verified model set and persisted EP choices.
    pub fn new(models: ModelSet, ep_choice: EpChoice) -> Result<Self, VisionError> {
        let runtime = HandRuntime::new(&models, &ep_choice)?;
        Ok(Self {
            runtime: Some(runtime),
            ep_choice,
            anchors: generate_palm_anchors(),
            tracks: Vec::new(),
            next_track_id: 1,
            frame_count: 0,
            adaptive_fps: AdaptiveFrameRate::new(5, 30),
            started: Instant::now(),
            mirrored_input: true,
            max_hands: 2,
        })
    }

    /// Builds a model-free pipeline for tests and fake-landmark flows.
    #[must_use]
    pub fn without_models() -> Self {
        Self {
            runtime: None,
            ep_choice: EpChoice::default(),
            anchors: generate_palm_anchors(),
            tracks: Vec::new(),
            next_track_id: 1,
            frame_count: 0,
            adaptive_fps: AdaptiveFrameRate::new(5, 30),
            started: Instant::now(),
            mirrored_input: true,
            max_hands: 2,
        }
    }

    /// Sets whether MediaPipe should interpret the input as mirrored/selfie video.
    #[must_use]
    pub fn with_mirrored_input(mut self, mirrored_input: bool) -> Self {
        self.mirrored_input = mirrored_input;
        self
    }

    /// Sets the maximum number of hands to process per frame.
    #[must_use]
    pub fn with_max_hands(mut self, max_hands: usize) -> Self {
        self.max_hands = max_hands.clamp(1, 2);
        self
    }

    /// Returns the generated palm anchors.
    #[must_use]
    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    /// Returns the desired capture FPS after processing the last frame.
    #[must_use]
    pub fn target_fps(&self) -> u32 {
        self.adaptive_fps.current_fps
    }

    /// Returns the EP choice map used to build this pipeline.
    #[must_use]
    pub fn ep_choice(&self) -> &EpChoice {
        &self.ep_choice
    }

    fn process_with_runtime(
        &mut self,
        runtime: &mut HandRuntime,
        frame: &Frame,
        timings: &mut StageTimings,
    ) -> Result<SmallVec<[HandObservation; 2]>, VisionError> {
        let mut candidates = self
            .tracks
            .iter()
            .take(self.max_hands)
            .map(|track| RoiCandidate {
                roi: track.roi,
                source: RoiSource::Tracking,
            })
            .collect::<Vec<_>>();

        let should_run_palm = self.tracks.is_empty() || self.frame_count.is_multiple_of(10);
        if should_run_palm {
            let palm_started = Instant::now();
            let palms = runtime.run_palm(frame, &self.anchors)?;
            timings.palm_ms = palm_started.elapsed().as_secs_f32() * 1_000.0;
            candidates.extend(palms.iter().take(self.max_hands).map(|palm| RoiCandidate {
                roi: roi_from_palm(palm),
                source: RoiSource::Palm,
            }));
        }

        let landmark_started = Instant::now();
        let mut raw = Vec::new();
        for candidate in candidates.into_iter().take(self.max_hands * 2) {
            if let Some(observation) =
                runtime.run_landmarks(frame, candidate.roi, self.mirrored_input)?
            {
                raw.push((candidate.source, observation));
            }
        }
        timings.landmarks_ms = landmark_started.elapsed().as_secs_f32() * 1_000.0;
        dedupe_raw_observations(&mut raw);

        let tracking_started = Instant::now();
        let time_s = self.started.elapsed().as_secs_f32();
        let mut assigned_tracks = vec![false; self.tracks.len()];
        let mut hands = SmallVec::<[HandObservation; 2]>::new();
        for (_, mut observation) in raw.into_iter().take(self.max_hands) {
            let track_index = self.match_track(&observation, &assigned_tracks);
            let index = if let Some(index) = track_index {
                assigned_tracks[index] = true;
                index
            } else {
                let id = self.next_track_id;
                self.next_track_id = self.next_track_id.saturating_add(1);
                self.tracks.push(TrackState::new(id, &observation));
                assigned_tracks.push(true);
                self.tracks.len() - 1
            };
            let track = &mut self.tracks[index];
            let smoothed = track.update(time_s, &observation);
            observation.image = smoothed;
            observation.bbox = bbox_from_landmarks(&observation.image);
            let (embedding, canned_scores) = runtime.run_gesture_models(&observation)?;
            hands.push(HandObservation {
                track_id: track.id,
                hand: observation.hand,
                handedness_score: observation.handedness_score,
                presence: observation.presence,
                image: observation.image,
                world: observation.world,
                bbox: observation.bbox,
                embedding,
                canned_scores,
            });
        }
        for (index, track) in self.tracks.iter_mut().enumerate() {
            if !assigned_tracks.get(index).copied().unwrap_or(false) {
                track.missed = track.missed.saturating_add(1);
            }
        }
        self.tracks.retain(|track| track.missed <= 5);
        timings.tracking_ms = tracking_started.elapsed().as_secs_f32() * 1_000.0;
        Ok(hands)
    }

    fn match_track(&self, observation: &RawHandObservation, assigned: &[bool]) -> Option<usize> {
        let mut best = None;
        let mut best_iou = 0.3_f32;
        for (index, track) in self.tracks.iter().enumerate() {
            if assigned.get(index).copied().unwrap_or(false) || track.hand != observation.hand {
                continue;
            }
            let overlap = iou(track.bbox, observation.bbox);
            if overlap >= best_iou {
                best = Some(index);
                best_iou = overlap;
            }
        }
        best
    }
}

impl HandPipeline for HandPipelineImpl {
    fn process(&mut self, frame: &Frame) -> Result<HandFrame, VisionError> {
        let started = Instant::now();
        if frame.format != PixelFormat::Rgb8 {
            return Err(VisionError::UnsupportedFrame(
                "HandPipelineImpl expects RGB8 frames".to_owned(),
            ));
        }
        self.frame_count = self.frame_count.saturating_add(1);
        let mut timings = StageTimings::default();
        let mut runtime = self.runtime.take();
        let hands = if let Some(runtime) = runtime.as_mut() {
            self.process_with_runtime(runtime, frame, &mut timings)?
        } else {
            SmallVec::new()
        };
        self.runtime = runtime;
        let now_ms = self.started.elapsed().as_millis() as u64;
        self.adaptive_fps.update(now_ms, !hands.is_empty());
        timings.total_ms = started.elapsed().as_secs_f32() * 1_000.0;
        Ok(HandFrame {
            camera_id: frame.camera_id,
            seq: frame.seq,
            captured_at: frame.captured_at,
            hands,
            timings,
        })
    }
}

struct HandRuntime {
    palm: ModelRunner,
    landmark: ModelRunner,
    embedder: Option<ModelRunner>,
    classifier: Option<ModelRunner>,
}

type GestureModelOutputs = (Option<[f32; 128]>, Option<CannedGestureScores>);

impl HandRuntime {
    fn new(models: &ModelSet, ep_choice: &EpChoice) -> Result<Self, VisionError> {
        let factory = OrtSessionFactory::new(models.root().join("models/cache"));
        Ok(Self {
            palm: build_required_runner(models, &factory, ep_choice, "palm_detection_full")?,
            landmark: build_required_runner(models, &factory, ep_choice, "hand_landmark_full")?,
            embedder: build_optional_runner(models, &factory, ep_choice, "gesture_embedder")?,
            classifier: build_optional_runner(
                models,
                &factory,
                ep_choice,
                "canned_gesture_classifier",
            )?,
        })
    }

    fn run_palm(
        &mut self,
        frame: &Frame,
        anchors: &[Anchor],
    ) -> Result<Vec<PalmDetection>, VisionError> {
        let (letterbox, input) = letterbox_rgb_to_nhwc(frame, PALM_INPUT_SIZE)?;
        let outputs = run_single_input(
            &mut self.palm.session,
            "input_1",
            vec![1, PALM_INPUT_SIZE as i64, PALM_INPUT_SIZE as i64, 3],
            &input,
        )?;
        let regressors = output_tensor(&outputs, "Identity")?;
        let logits = output_tensor(&outputs, "Identity_1")?;
        let (chunks, remainder) = regressors.as_chunks::<18>();
        if !remainder.is_empty() {
            return Err(VisionError::InvalidModelOutput(
                "palm regressors length is not divisible by 18".to_owned(),
            ));
        }
        let mut decoded = Vec::with_capacity(chunks.len());
        decoded.extend_from_slice(chunks);
        decode_palms(&decoded, logits, anchors, letterbox, 0.5, 0.3)
    }

    fn run_landmarks(
        &mut self,
        frame: &Frame,
        roi: RotatedRoi,
        mirrored_input: bool,
    ) -> Result<Option<RawHandObservation>, VisionError> {
        let crop = warp_rgb_roi(frame, roi, LANDMARK_INPUT_SIZE)?;
        let input = rgb_u8_to_nhwc_f32(&crop);
        let outputs = run_single_input(
            &mut self.landmark.session,
            "input_1",
            vec![1, LANDMARK_INPUT_SIZE as i64, LANDMARK_INPUT_SIZE as i64, 3],
            &input,
        )?;
        let image = output_tensor(&outputs, "Identity")?;
        let presence = output_tensor(&outputs, "Identity_1")?;
        let handedness = output_tensor(&outputs, "Identity_2")?;
        let world = output_tensor(&outputs, "Identity_3")?;
        if image.len() < 63 || world.len() < 63 || presence.is_empty() || handedness.is_empty() {
            return Err(VisionError::InvalidModelOutput(
                "hand landmark outputs have unexpected lengths".to_owned(),
            ));
        }
        let presence = normalize_score(presence[0]);
        if presence < 0.5 {
            return Ok(None);
        }
        let handedness_score = normalize_score(handedness[0]);
        let mediapipe_hand = if handedness_score >= 0.5 {
            Handedness::Right
        } else {
            Handedness::Left
        };
        let hand = mirror_aware_handedness(mediapipe_hand, mirrored_input);
        let affine = RoiAffine::new(roi);
        let mut projected = [[0.0_f32; 3]; 21];
        let mut world_landmarks = [[0.0_f32; 3]; 21];
        for index in 0..21 {
            let base = index * 3;
            let crop_x = normalize_model_coord(image[base], LANDMARK_INPUT_SIZE);
            let crop_y = normalize_model_coord(image[base + 1], LANDMARK_INPUT_SIZE);
            let projected_xy = affine.roi_to_image([crop_x, crop_y]);
            projected[index] = [
                projected_xy[0],
                projected_xy[1],
                normalize_model_coord(image[base + 2], LANDMARK_INPUT_SIZE),
            ];
            world_landmarks[index] = [world[base], world[base + 1], world[base + 2]];
        }
        let bbox = bbox_from_landmarks(&projected);
        if bbox.h < 0.06 {
            return Ok(None);
        }
        Ok(Some(RawHandObservation {
            hand,
            handedness_score,
            presence,
            image: projected,
            world: world_landmarks,
            bbox,
        }))
    }

    fn run_gesture_models(
        &mut self,
        hand: &RawHandObservation,
    ) -> Result<GestureModelOutputs, VisionError> {
        let Some(embedder) = self.embedder.as_mut() else {
            return Ok((None, None));
        };
        let mut hand_input = Vec::with_capacity(63);
        for point in hand.image {
            hand_input.extend_from_slice(&point);
        }
        let mut world_input = Vec::with_capacity(63);
        for point in hand.world {
            world_input.extend_from_slice(&point);
        }
        let handedness_input = [hand.handedness_score];
        let outputs = run_multi_input(
            &mut embedder.session,
            &[
                ("hand", vec![1, 21, 3], hand_input.as_slice()),
                ("handedness", vec![1, 1], handedness_input.as_slice()),
                ("world_hand", vec![1, 21, 3], world_input.as_slice()),
            ],
        )?;
        let embedding_values = output_tensor(&outputs, "Identity")?;
        if embedding_values.len() < 128 {
            return Err(VisionError::InvalidModelOutput(
                "gesture embedder returned fewer than 128 values".to_owned(),
            ));
        }
        let mut embedding = [0.0_f32; 128];
        embedding.copy_from_slice(&embedding_values[..128]);
        drop(outputs);

        let Some(classifier) = self.classifier.as_mut() else {
            return Ok((Some(embedding), None));
        };
        let outputs = run_single_input(
            &mut classifier.session,
            "hand_embedding",
            vec![1, 128],
            &embedding,
        )?;
        let scores = output_tensor(&outputs, "Identity")?;
        if scores.len() < 8 {
            return Err(VisionError::InvalidModelOutput(
                "canned classifier returned fewer than 8 values".to_owned(),
            ));
        }
        let mut canned_scores = [0.0_f32; 8];
        canned_scores.copy_from_slice(&scores[..8]);
        normalize_scores8(&mut canned_scores);
        Ok((
            Some(embedding),
            Some(CannedGestureScores {
                scores: canned_scores,
            }),
        ))
    }
}

struct ModelRunner {
    session: Session,
    #[allow(dead_code)]
    ep: EpKind,
}

fn build_required_runner(
    models: &ModelSet,
    factory: &OrtSessionFactory,
    ep_choice: &EpChoice,
    id: &str,
) -> Result<ModelRunner, VisionError> {
    let path = models
        .path(id)
        .ok_or_else(|| VisionError::ModelUnavailable(format!("{id} is not verified")))?;
    let requested = requested_ep(id, ep_choice);
    let (session, ep) = factory.build_session(path, requested)?;
    Ok(ModelRunner { session, ep })
}

fn build_optional_runner(
    models: &ModelSet,
    factory: &OrtSessionFactory,
    ep_choice: &EpChoice,
    id: &str,
) -> Result<Option<ModelRunner>, VisionError> {
    let Some(model) = models.model(id) else {
        return Ok(None);
    };
    if model.format != "onnx" {
        return Ok(None);
    }
    let Some(path) = models.optional_path(id)? else {
        return Ok(None);
    };
    let requested = requested_ep(id, ep_choice);
    match factory.build_session(&path, requested) {
        Ok((session, ep)) => Ok(Some(ModelRunner { session, ep })),
        Err(err) => {
            warn!(model = id, error = %err, "optional gesture model disabled");
            Ok(None)
        }
    }
}

fn requested_ep(model_id: &str, ep_choice: &EpChoice) -> EpKind {
    let requested = ep_choice.get(model_id);
    if requested == EpKind::Auto {
        default_ep_for_model(model_id)
    } else {
        requested
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RoiSource {
    Tracking,
    Palm,
}

#[derive(Debug, Clone, Copy)]
struct RoiCandidate {
    roi: RotatedRoi,
    source: RoiSource,
}

#[derive(Debug, Clone)]
struct RawHandObservation {
    hand: Handedness,
    handedness_score: f32,
    presence: f32,
    image: [[f32; 3]; 21],
    world: [[f32; 3]; 21],
    bbox: RectF,
}

#[derive(Debug, Clone)]
struct TrackState {
    id: u32,
    hand: Handedness,
    bbox: RectF,
    roi: RotatedRoi,
    missed: u8,
    filters: [[OneEuroFilter; 3]; 21],
}

impl TrackState {
    fn new(id: u32, observation: &RawHandObservation) -> Self {
        Self {
            id,
            hand: observation.hand,
            bbox: observation.bbox,
            roi: roi_from_landmarks(&observation.image),
            missed: 0,
            filters: std::array::from_fn(|_| std::array::from_fn(|_| OneEuroFilter::new())),
        }
    }

    fn update(&mut self, time_s: f32, observation: &RawHandObservation) -> [[f32; 3]; 21] {
        self.hand = observation.hand;
        self.bbox = observation.bbox;
        self.roi = roi_from_landmarks(&observation.image);
        self.missed = 0;
        let mut smoothed = observation.image;
        for (point_index, point) in smoothed.iter_mut().enumerate() {
            for (axis, value) in point.iter_mut().enumerate() {
                *value = self.filters[point_index][axis].filter(time_s, *value);
            }
        }
        smoothed
    }
}

fn dedupe_raw_observations(observations: &mut Vec<(RoiSource, RawHandObservation)>) {
    observations.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.1.presence.total_cmp(&a.1.presence))
    });
    let mut kept: Vec<(RoiSource, RawHandObservation)> = Vec::new();
    'outer: for observation in observations.drain(..) {
        for (_, kept_observation) in &kept {
            if iou(observation.1.bbox, kept_observation.bbox) > 0.5 {
                continue 'outer;
            }
        }
        kept.push(observation);
    }
    *observations = kept;
}

fn run_single_input<'s>(
    session: &'s mut Session,
    name: &str,
    shape: Vec<i64>,
    data: &[f32],
) -> Result<ort::session::SessionOutputs<'s>, VisionError> {
    run_multi_input(session, &[(name, shape, data)])
}

fn run_multi_input<'s>(
    session: &'s mut Session,
    inputs: &[(&str, Vec<i64>, &[f32])],
) -> Result<ort::session::SessionOutputs<'s>, VisionError> {
    let mut values: Vec<(String, SessionInputValue<'_>)> = Vec::with_capacity(inputs.len());
    for (name, shape, data) in inputs {
        let tensor = TensorRef::from_array_view((shape.clone(), *data))
            .map_err(|err| VisionError::Inference(err.to_string()))?;
        values.push(((*name).to_owned(), tensor.into()));
    }
    session
        .run(values)
        .map_err(|err| VisionError::Inference(err.to_string()))
}

fn output_tensor<'a>(
    outputs: &'a ort::session::SessionOutputs<'_>,
    name: &str,
) -> Result<&'a [f32], VisionError> {
    let Some(value) = outputs.get(name) else {
        let names = outputs.keys().collect::<Vec<_>>().join(", ");
        return Err(VisionError::InvalidModelOutput(format!(
            "missing output {name}; available outputs: {names}"
        )));
    };
    let (_, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|err| VisionError::InvalidModelOutput(err.to_string()))?;
    Ok(data)
}

fn letterbox_rgb_to_nhwc(frame: &Frame, size: u32) -> Result<(Letterbox, Vec<f32>), VisionError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(VisionError::UnsupportedFrame("empty frame".to_owned()));
    }
    let letterbox = Letterbox::new(frame.width, frame.height, size, size);
    let mut out = vec![0.0_f32; (size * size * 3) as usize];
    for y in 0..size {
        for x in 0..size {
            let dst_x = x as f32 + 0.5;
            let dst_y = y as f32 + 0.5;
            let src_x = (dst_x - letterbox.pad_x) / letterbox.scale;
            let src_y = (dst_y - letterbox.pad_y) / letterbox.scale;
            if src_x < 0.0
                || src_y < 0.0
                || src_x >= frame.width as f32
                || src_y >= frame.height as f32
            {
                continue;
            }
            let nx = if frame.width > 1 {
                src_x / (frame.width - 1) as f32
            } else {
                0.0
            };
            let ny = if frame.height > 1 {
                src_y / (frame.height - 1) as f32
            } else {
                0.0
            };
            let rgb = bilinear_rgb(frame, nx, ny);
            let offset = ((y * size + x) * 3) as usize;
            out[offset] = rgb[0] as f32 / 255.0;
            out[offset + 1] = rgb[1] as f32 / 255.0;
            out[offset + 2] = rgb[2] as f32 / 255.0;
        }
    }
    Ok((letterbox, out))
}

fn rgb_u8_to_nhwc_f32(data: &[u8]) -> Vec<f32> {
    data.iter().map(|value| f32::from(*value) / 255.0).collect()
}

fn normalize_model_coord(value: f32, input_size: u32) -> f32 {
    if value.abs() > 2.0 {
        value / input_size as f32
    } else {
        value
    }
}

fn normalize_score(value: f32) -> f32 {
    if (0.0..=1.0).contains(&value) {
        value
    } else {
        sigmoid(value)
    }
}

fn normalize_scores8(scores: &mut [f32; 8]) {
    let sum = scores.iter().sum::<f32>();
    let already_probabilities =
        scores.iter().all(|score| (0.0..=1.0).contains(score)) && (sum - 1.0).abs() < 0.05;
    if already_probabilities {
        return;
    }
    let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut exp_sum = 0.0_f32;
    for score in scores.iter_mut() {
        *score = (*score - max).exp();
        exp_sum += *score;
    }
    if exp_sum > f32::EPSILON {
        for score in scores.iter_mut() {
            *score /= exp_sum;
        }
    }
}

fn bbox_from_landmarks(landmarks: &[[f32; 3]; 21]) -> RectF {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in landmarks {
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

/// BlazeFace short-range keypoint runner, invoked only by targeting while point pose is active.
pub struct FaceKeypointRunner {
    runner: Option<ModelRunner>,
    anchors: Vec<[f32; 2]>,
}

impl FaceKeypointRunner {
    /// Creates a runner from a verified model set.
    #[must_use]
    pub fn new(models: &ModelSet) -> Self {
        let factory = OrtSessionFactory::new(models.root().join("models/cache"));
        let runner = build_required_runner(
            models,
            &factory,
            &EpChoice::default(),
            "face_detection_short",
        )
        .map_err(|err| {
            warn!(error = %err, "face keypoint runner disabled");
            err
        })
        .ok();
        Self {
            runner,
            anchors: generate_face_anchors(),
        }
    }

    /// Detects face keypoints on demand. Returns `Ok(None)` when the optional model is absent.
    pub fn detect(&mut self, frame: &Frame) -> Result<Option<FaceKeypoints>, VisionError> {
        if frame.format != PixelFormat::Rgb8 {
            return Err(VisionError::UnsupportedFrame(
                "FaceKeypointRunner expects RGB8".to_owned(),
            ));
        }
        let Some(runner) = self.runner.as_mut() else {
            return Ok(None);
        };
        let input = resize_rgb_nearest(frame, FACE_INPUT_SIZE, FACE_INPUT_SIZE)?;
        let input = rgb_u8_to_nhwc_f32(&input);
        let outputs = run_single_input(
            &mut runner.session,
            "input",
            vec![1, FACE_INPUT_SIZE as i64, FACE_INPUT_SIZE as i64, 3],
            &input,
        )?;
        let regressors = output_tensor(&outputs, "regressors")?;
        let logits = output_tensor(&outputs, "classificators")?;
        decode_face_keypoints(regressors, logits, &self.anchors)
    }
}

/// DINOv2-small scene signature embedder with person/hand-box masking.
pub struct SceneEmbedder {
    runner: Option<ModelRunner>,
}

impl SceneEmbedder {
    /// Creates an on-demand scene embedder from a verified model set.
    #[must_use]
    pub fn new(models: &ModelSet) -> Self {
        let factory = OrtSessionFactory::new(models.root().join("models/cache"));
        let runner =
            build_required_runner(models, &factory, &EpChoice::default(), "scene_embedder")
                .map_err(|err| {
                    warn!(error = %err, "scene embedder disabled");
                    err
                })
                .ok();
        Self { runner }
    }

    /// Produces a normalized 384-d scene vector.
    pub fn embed(
        &mut self,
        frame: &Frame,
        mask_boxes: &[RectF],
    ) -> Result<[f32; SCENE_EMBEDDING_DIMS], VisionError> {
        if frame.format != PixelFormat::Rgb8 {
            return Err(VisionError::UnsupportedFrame(
                "SceneEmbedder expects RGB8".to_owned(),
            ));
        }
        let mut masked = frame.data.to_vec();
        apply_masks(&mut masked, frame.width, frame.height, mask_boxes);
        let tensor = rgb_to_nchw_224(&masked, frame.width, frame.height)?;
        let Some(runner) = self.runner.as_mut() else {
            return Ok([0.0; SCENE_EMBEDDING_DIMS]);
        };
        let input = tensor.iter().copied().collect::<Vec<_>>();
        let outputs = run_single_input(
            &mut runner.session,
            "pixel_values",
            vec![1, 3, LANDMARK_INPUT_SIZE as i64, LANDMARK_INPUT_SIZE as i64],
            &input,
        )?;
        let values = output_tensor(&outputs, "last_hidden_state")?;
        if values.len() < SCENE_EMBEDDING_DIMS {
            return Err(VisionError::InvalidModelOutput(
                "scene embedder returned too few values".to_owned(),
            ));
        }
        let mut embedding = [0.0_f32; SCENE_EMBEDDING_DIMS];
        embedding.copy_from_slice(&values[..SCENE_EMBEDDING_DIMS]);
        normalize_l2(&mut embedding);
        Ok(embedding)
    }
}

fn generate_face_anchors() -> Vec<[f32; 2]> {
    let strides = [8_u32, 16, 16, 16];
    let mut anchors = Vec::with_capacity(896);
    for stride in strides {
        let feature = FACE_INPUT_SIZE.div_ceil(stride);
        for y in 0..feature {
            for x in 0..feature {
                let center = [
                    (x as f32 + 0.5) / feature as f32,
                    (y as f32 + 0.5) / feature as f32,
                ];
                anchors.push(center);
                anchors.push(center);
            }
        }
    }
    anchors
}

fn decode_face_keypoints(
    regressors: &[f32],
    logits: &[f32],
    anchors: &[[f32; 2]],
) -> Result<Option<FaceKeypoints>, VisionError> {
    let count = logits.len().min(anchors.len()).min(regressors.len() / 16);
    if count == 0 {
        return Err(VisionError::InvalidModelOutput(
            "empty BlazeFace outputs".to_owned(),
        ));
    }
    let mut best_index = None;
    let mut best_score = 0.5_f32;
    for (index, logit) in logits.iter().take(count).enumerate() {
        let score = sigmoid(*logit);
        if score > best_score {
            best_score = score;
            best_index = Some(index);
        }
    }
    let Some(index) = best_index else {
        return Ok(None);
    };
    let anchor = anchors[index];
    let raw = &regressors[index * 16..index * 16 + 16];
    let mut points = [[0.0_f32; 2]; 6];
    for (point_index, point) in points.iter_mut().enumerate() {
        let base = 4 + point_index * 2;
        point[0] = (raw[base] / FACE_INPUT_SIZE as f32 + anchor[0]).clamp(0.0, 1.0);
        point[1] = (raw[base + 1] / FACE_INPUT_SIZE as f32 + anchor[1]).clamp(0.0, 1.0);
    }
    Ok(Some(FaceKeypoints {
        points,
        confidence: Some(best_score),
    }))
}

fn normalize_l2<const N: usize>(values: &mut [f32; N]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in values {
            *value /= norm;
        }
    }
}

fn resize_rgb_nearest(frame: &Frame, width: u32, height: u32) -> Result<Vec<u8>, VisionError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(VisionError::UnsupportedFrame("empty frame".to_owned()));
    }
    let mut out = vec![0_u8; (width * height * 3) as usize];
    for y in 0..height {
        let src_y = y * frame.height / height;
        for x in 0..width {
            let src_x = x * frame.width / width;
            let src = ((src_y * frame.width + src_x) * 3) as usize;
            let dst = ((y * width + x) * 3) as usize;
            out[dst..dst + 3].copy_from_slice(&frame.data[src..src + 3]);
        }
    }
    Ok(out)
}

fn apply_masks(data: &mut [u8], width: u32, height: u32, boxes: &[RectF]) {
    for rect in boxes {
        let x0 = (rect.x.clamp(0.0, 1.0) * width as f32).floor() as u32;
        let y0 = (rect.y.clamp(0.0, 1.0) * height as f32).floor() as u32;
        let x1 = ((rect.x + rect.w).clamp(0.0, 1.0) * width as f32).ceil() as u32;
        let y1 = ((rect.y + rect.h).clamp(0.0, 1.0) * height as f32).ceil() as u32;
        for y in y0.min(height)..y1.min(height) {
            for x in x0.min(width)..x1.min(width) {
                let offset = ((y * width + x) * 3) as usize;
                data[offset..offset + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
    }
}

fn rgb_to_nchw_224(data: &[u8], width: u32, height: u32) -> Result<Array4<f32>, VisionError> {
    if width == 0 || height == 0 {
        return Err(VisionError::UnsupportedFrame(
            "empty scene frame".to_owned(),
        ));
    }
    let mean = [0.485_f32, 0.456, 0.406];
    let std = [0.229_f32, 0.224, 0.225];
    let mut tensor = Array4::<f32>::zeros((
        1,
        3,
        LANDMARK_INPUT_SIZE as usize,
        LANDMARK_INPUT_SIZE as usize,
    ));
    for y in 0..LANDMARK_INPUT_SIZE {
        let src_y = y * height / LANDMARK_INPUT_SIZE;
        for x in 0..LANDMARK_INPUT_SIZE {
            let src_x = x * width / LANDMARK_INPUT_SIZE;
            let src = ((src_y * width + src_x) * 3) as usize;
            for c in 0..3 {
                let value = data[src + c] as f32 / 255.0;
                tensor[(0, c, y as usize, x as usize)] = (value - mean[c]) / std[c];
            }
        }
    }
    Ok(tensor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::Arc, time::SystemTime};

    #[test]
    fn palm_decode_and_weighted_nms_merges_overlapping_boxes() -> Result<(), VisionError> {
        let anchors = vec![
            Anchor {
                x_center: 0.5,
                y_center: 0.5,
                w: 1.0,
                h: 1.0,
            },
            Anchor {
                x_center: 0.505,
                y_center: 0.5,
                w: 1.0,
                h: 1.0,
            },
            Anchor {
                x_center: 0.8,
                y_center: 0.8,
                w: 1.0,
                h: 1.0,
            },
        ];
        let mut raw = [[0.0_f32; 18]; 3];
        raw[0][2] = 38.4;
        raw[0][3] = 38.4;
        raw[1][2] = 38.4;
        raw[1][3] = 38.4;
        raw[2][2] = 19.2;
        raw[2][3] = 19.2;
        let logits = [4.0, 3.0, 5.0];
        let detections = decode_palms(
            &raw,
            &logits,
            &anchors,
            Letterbox::new(192, 192, 192, 192),
            0.5,
            0.3,
        )?;
        assert_eq!(detections.len(), 2);
        assert!(detections.iter().any(|detection| detection.bbox.w < 0.11));
        assert!(
            detections
                .iter()
                .any(|detection| (detection.bbox.x - 0.402).abs() < 0.01)
        );
        Ok(())
    }

    #[test]
    fn roi_affine_round_trip_preserves_points() {
        let roi = RotatedRoi {
            cx: 0.45,
            cy: 0.55,
            size: 0.4,
            rotation: 0.7,
        };
        let affine = RoiAffine::new(roi);
        for point in [[0.5, 0.5], [0.1, 0.9], [0.8, 0.2]] {
            let image = affine.roi_to_image(point);
            let round_trip = affine.image_to_roi(image);
            assert!((round_trip[0] - point[0]).abs() < 1.0e-6);
            assert!((round_trip[1] - point[1]).abs() < 1.0e-6);
        }
    }

    #[test]
    fn handedness_is_swapped_for_non_mirrored_input() {
        assert_eq!(
            mirror_aware_handedness(Handedness::Left, true),
            Handedness::Left
        );
        assert_eq!(
            mirror_aware_handedness(Handedness::Right, true),
            Handedness::Right
        );
        assert_eq!(
            mirror_aware_handedness(Handedness::Left, false),
            Handedness::Right
        );
        assert_eq!(
            mirror_aware_handedness(Handedness::Right, false),
            Handedness::Left
        );
    }

    #[test]
    fn tampered_model_refuses_to_load() -> Result<(), Box<dyn std::error::Error>> {
        let base = PathBuf::from("target/flick-vision-tamper-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("models/cache"))?;
        fs::write(base.join("models/cache/model.onnx"), b"tampered")?;
        fs::write(
            base.join("manifest.toml"),
            r#"
schema_version = 1

[[models]]
id = "palm_detection_full"
version = "test"
format = "onnx"
conversion_status = "selected"
source_url = "file://model.onnx"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
cache_path = "models/cache/model.onnx"
"#,
        )?;
        let err = match ModelSet::load(base.join("manifest.toml")) {
            Ok(_) => panic!("tampered hash fails"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("sha256 mismatch"));
        let _ = fs::remove_dir_all(base);
        Ok(())
    }

    #[test]
    #[ignore = "requires model cache and MediaPipe reference outputs from P0-02"]
    fn parity_against_reference_outputs() {}

    #[allow(dead_code)]
    fn rgb_frame(width: u32, height: u32, data: Vec<u8>) -> Frame {
        Frame {
            camera_id: flick_core::CameraId::new(),
            seq: 0,
            captured_at: Instant::now(),
            wall_ts: SystemTime::now(),
            width,
            height,
            format: PixelFormat::Rgb8,
            data: Arc::<[u8]>::from(data),
        }
    }
}
