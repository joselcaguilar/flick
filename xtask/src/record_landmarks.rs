//! `record-landmarks` turns a capture source into HandFrame JSONL fixtures.

use std::{fs::File, io::Write, path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use flick_capture::{FileSource, FileSourceOptions, LocalCameraOptions, LocalCameraSource};
use flick_core::{FaceKeypoints, FrameSource, HandObservation, HandPipeline, StageTimings};
use flick_vision::{EpChoice, FaceKeypointRunner, HandPipelineImpl, ModelSet};
use serde::Serialize;

/// Arguments for `cargo xtask record-landmarks`.
#[derive(Debug, Clone, ClapArgs)]
pub struct Args {
    /// Local camera index.
    #[arg(long, default_value_t = 0)]
    pub camera: u32,
    /// Output JSONL file.
    #[arg(long)]
    pub out: PathBuf,
    /// Include on-demand face keypoints.
    #[arg(long)]
    pub with_face: bool,
    /// Use a fake camera path instead of a local camera. Defaults to FLICK_FAKE_CAMERA when set.
    #[arg(long, env = "FLICK_FAKE_CAMERA")]
    pub fake_camera: Option<PathBuf>,
    /// Play fake camera as fast as possible.
    #[arg(long)]
    pub fast: bool,
    /// Maximum frames to record.
    #[arg(long, default_value_t = 300)]
    pub frames: u64,
}

#[derive(Serialize)]
struct JsonlHandFrame<'a> {
    t_ms: u64,
    camera_id: flick_core::CameraId,
    seq: u64,
    hands: &'a [HandObservation],
    timings: StageTimings,
    #[serde(skip_serializing_if = "Option::is_none")]
    face: Option<FaceKeypoints>,
}

/// Runs the landmark recorder.
pub fn run(args: Args) -> Result<()> {
    let models = ModelSet::load("models/manifest.toml").context(
        "loading verified models from models/cache; run `cargo xtask fetch-models` first",
    )?;
    let mut pipeline = HandPipelineImpl::new(models.clone(), EpChoice::default());
    let mut face_runner = args.with_face.then(|| FaceKeypointRunner::new(&models));
    let mut source: Box<dyn FrameSource> = if let Some(path) = args.fake_camera.clone() {
        let mut options = FileSourceOptions::fake_camera(path);
        if args.fast {
            options.playback = flick_capture::PlaybackMode::Fast;
        }
        Box::new(FileSource::open(options).context("opening fake camera")?)
    } else {
        Box::new(
            LocalCameraSource::open(LocalCameraOptions {
                index: args.camera,
                ..LocalCameraOptions::default()
            })
            .context("opening local camera")?,
        )
    };
    let mut out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let started = Instant::now();
    for _ in 0..args.frames {
        let frame = source.next_frame().context("capturing frame")?;
        let hands = pipeline
            .process(&frame)
            .context("running vision pipeline")?;
        let face = if let Some(runner) = face_runner.as_mut() {
            runner.detect(&frame).context("running face keypoints")?
        } else {
            None
        };
        let line = JsonlHandFrame {
            t_ms: started.elapsed().as_millis() as u64,
            camera_id: hands.camera_id,
            seq: hands.seq,
            hands: hands.hands.as_slice(),
            timings: hands.timings,
            face,
        };
        serde_json::to_writer(&mut out, &line).context("serializing HandFrame JSON")?;
        out.write_all(b"\n").context("writing JSONL newline")?;
    }
    Ok(())
}
