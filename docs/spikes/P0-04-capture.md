# P0-04 Capture spike

## Scope
Implemented the Phase 1 capture path in `flick-capture`:

- dedicated capture worker thread;
- latest-frame-wins single slot (`LatestFrameSlot`) with overwrite/drop accounting;
- `set_target_fps` command path for idle/active switching;
- `LocalCameraSource` through `nokhwa` native backends (AVFoundation on macOS);
- `FileSource` for `FLICK_FAKE_CAMERA`:
  - video files decoded by spawning the `ffmpeg` CLI and reading RGB24 frames from stdout;
  - deterministic sorted image-sequence directories;
  - real-time pacing, `--fast` support through `PlaybackMode::Fast`, and loop support;
- camera permission status via `nokhwa_check()` on macOS.

## Findings

### Latest-frame slot
The slot is an overwrite buffer, not a queue. A slow consumer observes the newest frame and older unread frames are counted as dropped. This keeps capture-to-inference latency bounded to one frame.

### Local camera / TCC
The implementation calls `nokhwa_initialize` on macOS and surfaces permission/open failures as `CaptureError::Unavailable` with the backend message. This terminal session may not have TCC camera permission; no bypass was attempted. If permission is denied during manual smoke, the code path should be left intact and tests should use `FileSource`.

### FFmpeg CLI decision
`FileSource` intentionally uses the `ffmpeg` command-line tool instead of `ffmpeg-next`. Homebrew has FFmpeg 9 here, while Rust bindings commonly lag major FFmpeg releases. This avoids linking/version risk for test fixtures and fake-camera replays.

**Proposed ADR/spec change:** for Phase 1 `FileSource`, prefer the `ffmpeg` CLI for video replay. Keep `ffmpeg-next` only for the Phase 2 RTSP source where low-latency decode flags and hardware decode need a library integration.

### Pixel formats and conversion
`LocalCameraSource` requests a format decodable by `nokhwa`'s `RgbFormat`, so MJPEG/YUYV/NV12/native buffers are converted to RGB in the capture worker. A dedicated NV12/YUYV SIMD path can replace this later if P95 conversion exceeds the 3 ms budget on real hardware.

## Manual status
No camera smoke was run that requires new TCC permission. Use:

```bash
cargo xtask record-landmarks --camera 0 --out target/manual-landmarks.jsonl --frames 30
```

If this fails with a permission error, record the denied status and use `FLICK_FAKE_CAMERA` for validation.

## Acceptance mapping
- P1-201: worker thread, latest-slot, `set_target_fps` implemented.
- P1-202: `nokhwa` local source and stable enumeration implemented; real hot-unplug recovery should be verified with hardware.
- P1-203: video and image-sequence file source implemented.
- P1-204: macOS permission status surfaced at library level; exact AVFoundation enum states require a future safe binding if UI needs `not_determined` vs `denied`.
