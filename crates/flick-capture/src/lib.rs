//! Capture workers and frame sources for `02-vision-pipeline.md` §1.
//!
//! The public entry point is [`spawn_capture`]: it runs a [`FrameSource`] on a
//! dedicated OS thread and overwrites a [`LatestFrameSlot`]. Consumers always
//! take the newest frame; there is intentionally no queue in this crate.

use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

use crossbeam_channel::{Receiver, Sender};
use flick_core::{CameraId, CaptureError, Frame, FrameSource, PixelFormat, SourceInfo, SourceKind};
use image::ImageReader;
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType},
};
use parking_lot::{Condvar, Mutex};
use tracing::{debug, error, warn};

const SUPERVISOR_RETRY: Duration = Duration::from_secs(2);

/// macOS camera authorization state as exposed to the engine status endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPermissionStatus {
    /// The platform reports that camera access is authorized.
    Authorized,
    /// The platform reports that camera access is not currently authorized.
    NotAuthorized,
    /// The current platform/backend cannot report a detailed authorization state.
    Unknown,
}

/// Returns the best available camera permission status without attempting a bypass.
#[must_use]
pub fn camera_permission_status() -> CameraPermissionStatus {
    #[cfg(target_os = "macos")]
    {
        if nokhwa::nokhwa_check() {
            CameraPermissionStatus::Authorized
        } else {
            CameraPermissionStatus::NotAuthorized
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        CameraPermissionStatus::Unknown
    }
}

#[derive(Debug, Default)]
struct SlotState {
    frame: Option<Frame>,
    dropped_since_take: u64,
    total_dropped: u64,
}

/// A single-frame overwrite buffer used between capture and inference.
#[derive(Debug, Clone, Default)]
pub struct LatestFrameSlot {
    inner: Arc<(Mutex<SlotState>, Condvar)>,
}

impl LatestFrameSlot {
    /// Creates an empty latest-frame slot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `frame`, dropping any unread older frame.
    pub fn store(&self, frame: Frame) {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock();
        if state.frame.is_some() {
            state.dropped_since_take = state.dropped_since_take.saturating_add(1);
            state.total_dropped = state.total_dropped.saturating_add(1);
        }
        state.frame = Some(frame);
        cvar.notify_one();
    }

    /// Takes the newest frame, if one is currently available.
    pub fn take_latest(&self) -> Option<Frame> {
        let (lock, _) = &*self.inner;
        let mut state = lock.lock();
        state.dropped_since_take = 0;
        state.frame.take()
    }

    /// Waits for and takes the newest frame until `timeout` elapses.
    pub fn wait_latest(&self, timeout: Duration) -> Option<Frame> {
        let (lock, cvar) = &*self.inner;
        let mut state = lock.lock();
        if state.frame.is_none() {
            let _ = cvar.wait_for(&mut state, timeout);
        }
        state.dropped_since_take = 0;
        state.frame.take()
    }

    /// Returns the total number of overwritten unread frames.
    #[must_use]
    pub fn total_dropped(&self) -> u64 {
        self.inner.0.lock().total_dropped
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureCommand {
    SetTargetFps(u32),
    Stop,
}

/// Snapshot of the capture worker state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    /// The worker is producing frames.
    Running,
    /// The worker is retrying after a disconnect.
    Disconnected,
    /// The worker was asked to stop.
    Stopped,
    /// The worker failed permanently.
    Failed(String),
}

/// Handle for a running capture worker.
#[derive(Debug)]
pub struct CaptureHandle {
    commands: Sender<CaptureCommand>,
    status: Arc<Mutex<CaptureStatus>>,
    join: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    /// Sets the source's target FPS. The worker applies it at the next frame boundary.
    pub fn set_target_fps(&self, fps: u32) -> Result<(), CaptureError> {
        self.commands
            .send(CaptureCommand::SetTargetFps(fps))
            .map_err(|err| CaptureError::Other(format!("capture worker stopped: {err}")))
    }

    /// Returns the last observed worker status.
    #[must_use]
    pub fn status(&self) -> CaptureStatus {
        self.status.lock().clone()
    }

    /// Requests a graceful stop and joins the worker thread.
    pub fn stop(mut self) -> Result<(), CaptureError> {
        let _ = self.commands.send(CaptureCommand::Stop);
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| {
                CaptureError::Other("capture worker panicked while stopping".to_owned())
            })?;
        }
        Ok(())
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(CaptureCommand::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Runs `source` on a dedicated capture thread and writes to `slot`.
#[must_use]
pub fn spawn_capture<S>(source: S, slot: LatestFrameSlot) -> CaptureHandle
where
    S: FrameSource,
{
    let (commands_tx, commands_rx) = crossbeam_channel::unbounded();
    let status = Arc::new(Mutex::new(CaptureStatus::Running));
    let worker_status = Arc::clone(&status);
    let join = thread::Builder::new()
        .name(format!("flick-capture-{}", source.info().id))
        .spawn(move || capture_loop(source, slot, commands_rx, worker_status))
        .ok();

    CaptureHandle {
        commands: commands_tx,
        status,
        join,
    }
}

fn capture_loop<S>(
    mut source: S,
    slot: LatestFrameSlot,
    commands: Receiver<CaptureCommand>,
    status: Arc<Mutex<CaptureStatus>>,
) where
    S: FrameSource,
{
    let running = AtomicBool::new(true);
    while running.load(Ordering::Relaxed) {
        for command in commands.try_iter() {
            match command {
                CaptureCommand::SetTargetFps(fps) => source.set_target_fps(fps),
                CaptureCommand::Stop => {
                    *status.lock() = CaptureStatus::Stopped;
                    running.store(false, Ordering::Relaxed);
                }
            }
        }
        if !running.load(Ordering::Relaxed) {
            break;
        }

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.next_frame())) {
            Ok(Ok(frame)) => {
                *status.lock() = CaptureStatus::Running;
                slot.store(frame);
            }
            Ok(Err(CaptureError::Disconnected)) => {
                *status.lock() = CaptureStatus::Disconnected;
                warn!(source = %source.info().id, "capture source disconnected; retrying");
                thread::sleep(SUPERVISOR_RETRY);
            }
            Ok(Err(err)) => {
                let message = err.to_string();
                *status.lock() = CaptureStatus::Failed(message.clone());
                error!(source = %source.info().id, error = %message, "capture source failed");
                thread::sleep(SUPERVISOR_RETRY);
            }
            Err(_) => {
                *status.lock() = CaptureStatus::Failed("capture worker panicked".to_owned());
                break;
            }
        }
    }
}

/// Options for [`LocalCameraSource`].
#[derive(Debug, Clone)]
pub struct LocalCameraOptions {
    /// Flick camera id.
    pub camera_id: CameraId,
    /// Device index from the native backend.
    pub index: u32,
    /// Requested width.
    pub width: u32,
    /// Requested height.
    pub height: u32,
    /// Requested FPS.
    pub fps: u32,
    /// Whether frames are mirrored in the user's view.
    pub mirror: bool,
}

impl Default for LocalCameraOptions {
    fn default() -> Self {
        Self {
            camera_id: CameraId::new(),
            index: 0,
            width: 1280,
            height: 720,
            fps: 30,
            mirror: true,
        }
    }
}

/// A local webcam backed by `nokhwa`/AVFoundation, Media Foundation or V4L2.
pub struct LocalCameraSource {
    info: SourceInfo,
    camera: Camera,
    seq: u64,
    rgb: Vec<u8>,
}

impl LocalCameraSource {
    /// Opens a local camera with the requested options.
    pub fn open(options: LocalCameraOptions) -> Result<Self, CaptureError> {
        #[cfg(target_os = "macos")]
        nokhwa::nokhwa_initialize(|granted| {
            debug!(granted, "nokhwa camera initialization completed");
        });

        let camera_format = CameraFormat::new_from(
            options.width,
            options.height,
            FrameFormat::MJPEG,
            options.fps,
        );
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(camera_format));
        let mut camera =
            Camera::new(CameraIndex::Index(options.index), requested).map_err(map_nokhwa_error)?;
        camera.open_stream().map_err(map_nokhwa_error)?;
        let format = camera.camera_format();
        let resolution = format.resolution();
        let info = SourceInfo {
            id: options.camera_id,
            kind: SourceKind::Local,
            width: resolution.width(),
            height: resolution.height(),
            fps: format.frame_rate(),
            mirror: options.mirror,
        };
        let len = rgb_len(info.width, info.height)?;
        Ok(Self {
            info,
            camera,
            seq: 0,
            rgb: vec![0; len],
        })
    }

    /// Lists local camera devices with stable backend ids where available.
    pub fn enumerate() -> Result<Vec<LocalCameraDevice>, CaptureError> {
        let backend = nokhwa::native_api_backend()
            .ok_or_else(|| CaptureError::Unavailable("no native camera backend".to_owned()))?;
        let devices = nokhwa::query(backend).map_err(map_nokhwa_error)?;
        Ok(devices
            .into_iter()
            .map(|device| LocalCameraDevice {
                stable_id: if device.misc().is_empty() {
                    device.index().to_string()
                } else {
                    device.misc()
                },
                name: device.human_name(),
            })
            .collect())
    }
}

impl FrameSource for LocalCameraSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        let buffer = self.camera.frame().map_err(map_nokhwa_error)?;
        let resolution = buffer.resolution();
        if resolution.width() != self.info.width || resolution.height() != self.info.height {
            self.info.width = resolution.width();
            self.info.height = resolution.height();
            self.rgb
                .resize(rgb_len(self.info.width, self.info.height)?, 0);
        }
        buffer
            .decode_image_to_buffer::<RgbFormat>(&mut self.rgb)
            .map_err(|err| CaptureError::Decode(err.to_string()))?;
        let frame = Frame {
            camera_id: self.info.id,
            seq: self.seq,
            captured_at: Instant::now(),
            wall_ts: SystemTime::now(),
            width: self.info.width,
            height: self.info.height,
            format: PixelFormat::Rgb8,
            data: Arc::<[u8]>::from(self.rgb.clone()),
        };
        self.seq = self.seq.saturating_add(1);
        Ok(frame)
    }

    fn set_target_fps(&mut self, fps: u32) {
        if let Err(err) = self.camera.set_frame_rate(fps) {
            warn!(error = %err, fps, "camera rejected target FPS");
        } else {
            self.info.fps = fps;
        }
    }
}

/// A local camera reported by the native backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCameraDevice {
    /// Stable backend identifier when available.
    pub stable_id: String,
    /// Human-readable camera name.
    pub name: String,
}

fn map_nokhwa_error(err: nokhwa::NokhwaError) -> CaptureError {
    let message = err.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("permission") || lower.contains("authoriz") || lower.contains("denied") {
        CaptureError::Unavailable(format!(
            "camera permission denied or unavailable: {message}"
        ))
    } else if lower.contains("disconnect")
        || lower.contains("not found")
        || lower.contains("no device")
    {
        CaptureError::Disconnected
    } else {
        CaptureError::Other(message)
    }
}

/// Playback speed mode for [`FileSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    /// Pace frames according to the source FPS.
    RealTime,
    /// Emit frames as fast as the consumer asks for them.
    Fast,
}

/// Options for deterministic file playback.
#[derive(Debug, Clone)]
pub struct FileSourceOptions {
    /// Flick camera id.
    pub camera_id: CameraId,
    /// Source path: video file or image-sequence directory.
    pub path: PathBuf,
    /// Mirror flag to propagate in [`SourceInfo`].
    pub mirror: bool,
    /// Loop at EOF.
    pub loop_playback: bool,
    /// Real-time or fast replay.
    pub playback: PlaybackMode,
    /// Optional FPS override.
    pub fps: Option<u32>,
}

impl FileSourceOptions {
    /// Builds options for the `FLICK_FAKE_CAMERA` path.
    #[must_use]
    pub fn fake_camera(path: impl Into<PathBuf>) -> Self {
        Self {
            camera_id: CameraId::new(),
            path: path.into(),
            mirror: true,
            loop_playback: false,
            playback: PlaybackMode::RealTime,
            fps: None,
        }
    }
}

/// A deterministic fake camera backed by the `ffmpeg` CLI or an image directory.
pub enum FileSource {
    /// Video file decoded as raw RGB24 frames by `ffmpeg`.
    Video(VideoFileSource),
    /// Sorted image sequence directory.
    Images(ImageSequenceSource),
}

impl FileSource {
    /// Opens a video file or image-sequence directory.
    pub fn open(options: FileSourceOptions) -> Result<Self, CaptureError> {
        if options.path.is_dir() {
            ImageSequenceSource::open(options).map(Self::Images)
        } else {
            VideoFileSource::open(options).map(Self::Video)
        }
    }

    /// Opens the path from `FLICK_FAKE_CAMERA`, if set.
    pub fn from_env() -> Result<Option<Self>, CaptureError> {
        let Some(path) = std::env::var_os("FLICK_FAKE_CAMERA") else {
            return Ok(None);
        };
        Self::open(FileSourceOptions::fake_camera(path)).map(Some)
    }
}

impl FrameSource for FileSource {
    fn info(&self) -> &SourceInfo {
        match self {
            Self::Video(source) => source.info(),
            Self::Images(source) => source.info(),
        }
    }

    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        match self {
            Self::Video(source) => source.next_frame(),
            Self::Images(source) => source.next_frame(),
        }
    }

    fn set_target_fps(&mut self, fps: u32) {
        match self {
            Self::Video(source) => source.set_target_fps(fps),
            Self::Images(source) => source.set_target_fps(fps),
        }
    }
}

/// Video-backed file source using `ffmpeg -f rawvideo -pix_fmt rgb24 pipe:1`.
pub struct VideoFileSource {
    info: SourceInfo,
    options: FileSourceOptions,
    child: Child,
    stdout: ChildStdout,
    seq: u64,
    start: Instant,
    frame_len: usize,
    frame_period: Duration,
}

impl VideoFileSource {
    /// Opens a video source by probing it with `ffprobe` and spawning `ffmpeg`.
    pub fn open(mut options: FileSourceOptions) -> Result<Self, CaptureError> {
        let probe = probe_video(&options.path)?;
        let fps = options.fps.unwrap_or(probe.fps.max(1));
        options.fps = Some(fps);
        let info = SourceInfo {
            id: options.camera_id,
            kind: SourceKind::File,
            width: probe.width,
            height: probe.height,
            fps,
            mirror: options.mirror,
        };
        let frame_len = rgb_len(info.width, info.height)?;
        let (child, stdout) = spawn_ffmpeg(&options.path, fps)?;
        Ok(Self {
            info,
            options,
            child,
            stdout,
            seq: 0,
            start: Instant::now(),
            frame_len,
            frame_period: frame_period(fps),
        })
    }

    fn restart(&mut self) -> Result<(), CaptureError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let fps = self.info.fps.max(1);
        let (child, stdout) = spawn_ffmpeg(&self.options.path, fps)?;
        self.child = child;
        self.stdout = stdout;
        self.start = Instant::now();
        Ok(())
    }

    fn pace(&self, seq: u64) {
        if self.options.playback == PlaybackMode::Fast {
            return;
        }
        let deadline = self.start + mul_duration(self.frame_period, seq);
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        }
    }
}

impl FrameSource for VideoFileSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        self.pace(self.seq);
        let mut data = vec![0_u8; self.frame_len];
        match self.stdout.read_exact(&mut data) {
            Ok(()) => {}
            Err(err)
                if err.kind() == std::io::ErrorKind::UnexpectedEof
                    && self.options.loop_playback =>
            {
                self.restart()?;
                self.stdout
                    .read_exact(&mut data)
                    .map_err(|read_err| CaptureError::Decode(read_err.to_string()))?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(CaptureError::Disconnected);
            }
            Err(err) => return Err(CaptureError::Decode(err.to_string())),
        }
        let frame = Frame {
            camera_id: self.info.id,
            seq: self.seq,
            captured_at: Instant::now(),
            wall_ts: SystemTime::now(),
            width: self.info.width,
            height: self.info.height,
            format: PixelFormat::Rgb8,
            data: Arc::<[u8]>::from(data),
        };
        self.seq = self.seq.saturating_add(1);
        Ok(frame)
    }

    fn set_target_fps(&mut self, fps: u32) {
        self.info.fps = fps.max(1);
        self.frame_period = frame_period(self.info.fps);
    }
}

impl Drop for VideoFileSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Image-sequence source. Files are sorted by path for deterministic replay.
pub struct ImageSequenceSource {
    info: SourceInfo,
    files: Vec<PathBuf>,
    index: usize,
    seq: u64,
    start: Instant,
    frame_period: Duration,
    options: FileSourceOptions,
}

impl ImageSequenceSource {
    /// Opens a directory of images.
    pub fn open(options: FileSourceOptions) -> Result<Self, CaptureError> {
        let mut files = fs::read_dir(&options.path)
            .map_err(|err| CaptureError::Unavailable(err.to_string()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_image_path(path))
            .collect::<Vec<_>>();
        files.sort();
        let first = files
            .first()
            .ok_or_else(|| CaptureError::Unavailable("image sequence is empty".to_owned()))?;
        let image = ImageReader::open(first)
            .map_err(|err| CaptureError::Decode(err.to_string()))?
            .with_guessed_format()
            .map_err(|err| CaptureError::Decode(err.to_string()))?
            .decode()
            .map_err(|err| CaptureError::Decode(err.to_string()))?
            .to_rgb8();
        let fps = options.fps.unwrap_or(30).max(1);
        let info = SourceInfo {
            id: options.camera_id,
            kind: SourceKind::File,
            width: image.width(),
            height: image.height(),
            fps,
            mirror: options.mirror,
        };
        Ok(Self {
            info,
            files,
            index: 0,
            seq: 0,
            start: Instant::now(),
            frame_period: frame_period(fps),
            options,
        })
    }

    fn pace(&self, seq: u64) {
        if self.options.playback == PlaybackMode::Fast {
            return;
        }
        let deadline = self.start + mul_duration(self.frame_period, seq);
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        }
    }
}

impl FrameSource for ImageSequenceSource {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn next_frame(&mut self) -> Result<Frame, CaptureError> {
        if self.index >= self.files.len() {
            if self.options.loop_playback {
                self.index = 0;
                self.start = Instant::now();
            } else {
                return Err(CaptureError::Disconnected);
            }
        }
        self.pace(self.seq);
        let path = &self.files[self.index];
        let image = ImageReader::open(path)
            .map_err(|err| CaptureError::Decode(err.to_string()))?
            .with_guessed_format()
            .map_err(|err| CaptureError::Decode(err.to_string()))?
            .decode()
            .map_err(|err| CaptureError::Decode(err.to_string()))?
            .to_rgb8();
        if image.width() != self.info.width || image.height() != self.info.height {
            return Err(CaptureError::UnsupportedFormat(format!(
                "image {} has {}x{}, expected {}x{}",
                path.display(),
                image.width(),
                image.height(),
                self.info.width,
                self.info.height
            )));
        }
        let frame = Frame {
            camera_id: self.info.id,
            seq: self.seq,
            captured_at: Instant::now(),
            wall_ts: SystemTime::now(),
            width: self.info.width,
            height: self.info.height,
            format: PixelFormat::Rgb8,
            data: Arc::<[u8]>::from(image.into_raw()),
        };
        self.index += 1;
        self.seq = self.seq.saturating_add(1);
        Ok(frame)
    }

    fn set_target_fps(&mut self, fps: u32) {
        self.info.fps = fps.max(1);
        self.frame_period = frame_period(self.info.fps);
    }
}

#[derive(Debug, Clone, Copy)]
struct ProbeResult {
    width: u32,
    height: u32,
    fps: u32,
}

fn probe_video(path: &Path) -> Result<ProbeResult, CaptureError> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height,r_frame_rate")
        .arg("-of")
        .arg("default=noprint_wrappers=1")
        .arg(path)
        .output()
        .map_err(|err| CaptureError::Unavailable(format!("ffprobe failed to start: {err}")))?;
    if !output.status.success() {
        return Err(CaptureError::Unavailable(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("width=") {
            width = value.parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("height=") {
            height = value.parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("r_frame_rate=") {
            fps = parse_rate(value);
        }
    }
    Ok(ProbeResult {
        width: width
            .ok_or_else(|| CaptureError::Unavailable("ffprobe did not return width".to_owned()))?,
        height: height
            .ok_or_else(|| CaptureError::Unavailable("ffprobe did not return height".to_owned()))?,
        fps: fps.unwrap_or(30).max(1),
    })
}

fn spawn_ffmpeg(path: &Path, fps: u32) -> Result<(Child, ChildStdout), CaptureError> {
    let mut child = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(path)
        .arg("-vf")
        .arg(format!("fps={fps}"))
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| CaptureError::Unavailable(format!("ffmpeg failed to start: {err}")))?;
    let stderr = child.stderr.take();
    if let Some(stderr) = stderr {
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                warn!(target: "flick_capture::ffmpeg", %line);
            }
        });
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CaptureError::Unavailable("ffmpeg stdout pipe unavailable".to_owned()))?;
    Ok((child, stdout))
}

fn parse_rate(value: &str) -> Option<u32> {
    let (num, den) = value.split_once('/')?;
    let num = num.parse::<u32>().ok()?;
    let den = den.parse::<u32>().ok()?.max(1);
    Some(((num as f32) / (den as f32)).round().max(1.0) as u32)
}

fn rgb_len(width: u32, height: u32) -> Result<usize, CaptureError> {
    let pixels = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(3))
        .ok_or_else(|| CaptureError::UnsupportedFormat("frame dimensions overflow".to_owned()))?;
    Ok(pixels as usize)
}

fn frame_period(fps: u32) -> Duration {
    Duration::from_nanos(1_000_000_000_u64 / u64::from(fps.max(1)))
}

fn mul_duration(duration: Duration, count: u64) -> Duration {
    let nanos = duration.as_nanos().saturating_mul(u128::from(count));
    let capped = nanos.min(u128::from(u64::MAX));
    Duration::from_nanos(capped as u64)
}

fn is_image_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "bmp" | "webp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u64) -> Frame {
        Frame {
            camera_id: CameraId::new(),
            seq,
            captured_at: Instant::now(),
            wall_ts: SystemTime::now(),
            width: 1,
            height: 1,
            format: PixelFormat::Rgb8,
            data: Arc::<[u8]>::from(vec![0, 0, 0]),
        }
    }

    #[test]
    fn latest_frame_slot_overwrites_unread_frames() {
        let slot = LatestFrameSlot::new();
        for seq in 0..5 {
            slot.store(frame(seq));
        }
        let latest = slot.take_latest().expect("slot has latest frame");
        assert_eq!(latest.seq, 4);
        assert!(slot.take_latest().is_none());
        assert_eq!(slot.total_dropped(), 4);
    }
}
