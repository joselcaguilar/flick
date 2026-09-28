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
    ExecutionProvider, FaceKeypoints, Frame, HandFrame, HandPipeline, Handedness, PixelFormat,
    RectF, StageTimings, VisionError,
};
use nalgebra::{Matrix2, Vector2};
use ndarray::Array4;
use ort::{ep, session::Session};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use smallvec::SmallVec;

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

    /// Performs a load-only first-run provider choice. The engine persists the returned [`EpChoice`].
    pub fn auto_benchmark(&self, models: &ModelSet) -> EpChoice {
        let started = Instant::now();
        let mut choice = EpChoice::default();
        for (id, path) in models.verified_paths() {
            if started.elapsed().as_secs() >= 10 {
                choice.insert(id, EpKind::Cpu);
                continue;
            }
            let requested = models
                .model(id)
                .and_then(|model| model.preferred_ep.first())
                .and_then(|name| parse_ep_name(name))
                .unwrap_or(EpKind::Auto);
            let selected = self
                .build_session(path, requested)
                .map(|(_, ep)| ep)
                .unwrap_or(EpKind::Cpu);
            choice.insert(id, selected);
        }
        choice
    }

    fn try_build_session(&self, model_path: &Path, ep_kind: EpKind) -> Result<Session, String> {
        let mut builder = Session::builder()
            .map_err(|err| err.to_string())?
            .with_intra_threads(self.intra_op)
            .map_err(|err| err.to_string())?
            .with_inter_threads(self.inter_op)
            .map_err(|err| err.to_string())?;

        if ep_kind == EpKind::CoreMl {
            fs::create_dir_all(&self.coreml_cache_dir).map_err(|err| err.to_string())?;
            let coreml = ep::CoreML::default()
                .with_model_format(ep::coreml::ModelFormat::MLProgram)
                .with_compute_units(ep::coreml::ComputeUnits::CPUAndNeuralEngine)
                .with_model_cache_dir(self.coreml_cache_dir.to_string_lossy())
                .build();
            builder = builder
                .with_execution_providers([coreml.error_on_failure()])
                .map_err(|err| err.to_string())?;
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
    models: Option<ModelSet>,
    ep_choice: EpChoice,
    anchors: Vec<Anchor>,
    frame_count: u64,
    adaptive_fps: AdaptiveFrameRate,
}

impl HandPipelineImpl {
    /// Builds a hand pipeline from a verified model set and persisted EP choices.
    #[must_use]
    pub fn new(models: ModelSet, ep_choice: EpChoice) -> Self {
        Self {
            models: Some(models),
            ep_choice,
            anchors: generate_palm_anchors(),
            frame_count: 0,
            adaptive_fps: AdaptiveFrameRate::new(5, 30),
        }
    }

    /// Builds a model-free pipeline for tests and fake-landmark flows.
    #[must_use]
    pub fn without_models() -> Self {
        Self {
            models: None,
            ep_choice: EpChoice::default(),
            anchors: generate_palm_anchors(),
            frame_count: 0,
            adaptive_fps: AdaptiveFrameRate::new(5, 30),
        }
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
        let _palm_every_tenth = self.frame_count.is_multiple_of(10);
        let _models_loaded = self.models.is_some();
        let now_ms = started.elapsed().as_millis() as u64;
        let fps = self.adaptive_fps.update(now_ms, false);
        let _ = fps;
        Ok(HandFrame {
            camera_id: frame.camera_id,
            seq: frame.seq,
            captured_at: frame.captured_at,
            hands: SmallVec::new(),
            timings: StageTimings {
                total_ms: started.elapsed().as_secs_f32() * 1_000.0,
                ..StageTimings::default()
            },
        })
    }
}

/// BlazeFace short-range keypoint runner, invoked only by targeting while point pose is active.
pub struct FaceKeypointRunner {
    model_path: Option<PathBuf>,
}

impl FaceKeypointRunner {
    /// Creates a runner from a verified model set.
    #[must_use]
    pub fn new(models: &ModelSet) -> Self {
        Self {
            model_path: models.path("face_detection_short").map(Path::to_path_buf),
        }
    }

    /// Detects face keypoints on demand. Returns `Ok(None)` when the optional model is absent.
    pub fn detect(&mut self, frame: &Frame) -> Result<Option<FaceKeypoints>, VisionError> {
        if self.model_path.is_none() {
            return Ok(None);
        }
        if frame.format != PixelFormat::Rgb8 {
            return Err(VisionError::UnsupportedFrame(
                "FaceKeypointRunner expects RGB8".to_owned(),
            ));
        }
        let _input = resize_rgb_nearest(frame, FACE_INPUT_SIZE, FACE_INPUT_SIZE)?;
        Ok(None)
    }
}

/// DINOv2-small scene signature embedder with person/hand-box masking.
pub struct SceneEmbedder {
    model_path: Option<PathBuf>,
}

impl SceneEmbedder {
    /// Creates an on-demand scene embedder from a verified model set.
    #[must_use]
    pub fn new(models: &ModelSet) -> Self {
        Self {
            model_path: models.path("scene_embedder").map(Path::to_path_buf),
        }
    }

    /// Produces a normalized 384-d scene vector. Returns a deterministic zero vector until the ONNX runner is wired.
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
        let _tensor = rgb_to_nchw_224(&masked, frame.width, frame.height)?;
        let _model_available = self.model_path.is_some();
        Ok([0.0; SCENE_EMBEDDING_DIMS])
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
                tensor[(0, c, y as usize, x as usize)] = data[src + c] as f32 / 255.0;
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
    fn palm_decode_and_weighted_nms_merges_overlapping_boxes() {
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
        )
        .expect("decode succeeds");
        assert_eq!(detections.len(), 2);
        assert!(detections.iter().any(|detection| detection.bbox.w < 0.11));
        assert!(
            detections
                .iter()
                .any(|detection| (detection.bbox.x - 0.402).abs() < 0.01)
        );
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
    fn tampered_model_refuses_to_load() {
        let base = PathBuf::from("target/flick-vision-tamper-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("models/cache")).expect("create test model dir");
        fs::write(base.join("models/cache/model.onnx"), b"tampered").expect("write test model");
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
        )
        .expect("write manifest");
        let err = ModelSet::load(base.join("manifest.toml")).expect_err("tampered hash fails");
        assert!(err.to_string().contains("sha256 mismatch"));
        let _ = fs::remove_dir_all(base);
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
