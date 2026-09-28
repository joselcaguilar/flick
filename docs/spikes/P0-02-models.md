# P0-02 — Model sourcing, conversion, and parity

Date: 2026-09-28
Worktree: `task/P0-02-models`
Policy: spike only; no test suite added.

## Summary

The baseline model manifest is now in `models/manifest.toml`. Model binaries are downloaded into `models/cache/` and remain gitignored. The selected redistributable ONNX sources are:

| Spec id | Selected source | License | SHA-256 short | I/O contract |
|---|---|---:|---|---|
| `palm_detection_full` | `opencv/palm_detection_mediapipe` @ `233e619` | Apache-2.0 | `78ff51c38496` | `input_1` f32 `1x192x192x3` → `Identity` f32 `1x2016x18`, `Identity_1` f32 `1x2016x1` |
| `hand_landmark_full` | `opencv/handpose_estimation_mediapipe` @ `4b2a0b4` | Apache-2.0 | `db0898ae717b` | `input_1` f32 `1x224x224x3` → `Identity` `1x63`, `Identity_1` `1x1`, `Identity_2` `1x1`, `Identity_3` `1x63` |
| `gesture_embedder` | official MediaPipe `gesture_recognizer.task` `float16/1` | Apache-2.0 | `54abe78de1d1` (extracted TFLite) | intended: `hand`, `handedness`, `world_hand` → `Identity` f32 `1x128`; ONNX conversion blocked |
| `canned_gesture_classifier` | official MediaPipe `gesture_recognizer.task` `float16/1` | Apache-2.0 | `62a87ded76da` (extracted TFLite) | intended: `hand_embedding` f32 `1x128` → `Identity` f32 `1x8`; ONNX conversion blocked |
| `canned_gesture_classifier_qaihub_candidate` | Qualcomm AI Hub `mediapipe_hand_gesture` v0.63.0 | Apache-2.0 | `ef912edf59a2` (+ data `178f59f8502`) | `hand` f32 `1x64`, `mirrored_hand` f32 `1x64` → `Identity` f32 `1x8`; contract mismatch |
| `face_detection_short` | `unity/inference-engine-blaze-face` @ `6b3dcfd` | Apache-2.0 | `587fa34c93de` | `input` f32 `1x128x128x3` → `regressors` f32 `1x896x16`, `classificators` f32 `1x896x1` |
| `scene_embedder` | `onnx-community/dinov2-small` `model_uint8.onnx` @ `8b1f705` (base `facebook/dinov2-small`) | Apache-2.0 | `c179f8f7f592` | `pixel_values` f32 `1x3x224x224` → `last_hidden_state` f32 `1x257x384`; use CLS token |

## Candidate review

- **OpenCV Zoo Hugging Face ports** were selected for palm and hand landmarks because they match the spec dimensions, are Apache-2.0, have simple NHWC inputs, and expose the expected MediaPipe outputs.
- **PINTO0309** ports were checked as fallback candidates. They are useful because some include `*_inf_post_*` post-processing graphs, but that built-in post-processing would hide the Rust anchor/NMS logic that `02-vision-pipeline.md` wants to own and test.
- **Qualcomm AI Hub** ports were checked. The gesture package includes palm, landmark, and canned classifier ONNX files. The classifier is a good label/reference candidate but does not preserve ADR-007's reusable 128-d embedder contract.
- **BlazeFace** uses Unity's Apache-2.0 ONNX export of MediaPipe BlazeFace short range.
- **DINOv2-small** uses the quantized Transformers.js ONNX export and inherits Apache-2.0 from `facebook/dinov2-small`.

## Conversion status

| Model | Status | Notes |
|---|---|---|
| Palm detector | selected ONNX | No conversion needed. |
| Hand landmarker | selected ONNX | No conversion needed. |
| Gesture embedder | blocked | Extracted `gesture_embedder.tflite` from the official `.task`; conversion not completed on Python 3.14. |
| Canned classifier | blocked | Extracted `canned_gesture_classifier.tflite`; Qualcomm ONNX candidate has a different input contract. |
| BlazeFace short range | selected ONNX | No conversion needed. |
| DINOv2-small | selected ONNX uint8 | OTA-only due size and latency. |

The local `uv` project is in `tools/training/`. Its default scripts use only stdlib so they run under Homebrew Python 3.14:

```bash
UV_PYTHON_DOWNLOADS=never uv run --python /opt/homebrew/bin/python3.14 --no-sync python tools/training/convert.py fetch
UV_PYTHON_DOWNLOADS=never uv run --python /opt/homebrew/bin/python3.14 --no-sync python tools/training/convert.py inspect
```

Attempted conversion path:

- `gesture_recognizer.task` downloaded and extracted successfully.
- `tflite2onnx`/`onnx`/`numpy` install was attempted in a project-local `uv` run, but this environment could not fetch wheels from `files.pythonhosted.org`.
- PyPI metadata shows `tensorflow` 2.21 has no Python 3.14 macOS arm64 wheel, so the `tf2onnx` path is blocked on Python 3.14 even if downloads work.

Owner action: install `python@3.12` via Homebrew and rerun the gesture conversion step in the project-local `uv` environment. Do not install anything globally beyond Homebrew Python.

## Parity

Parity was not run. There are no consented/CC0 video fixtures in this worktree, and MediaPipe reference packages are not installed. `tools/training/parity.py` writes an explicit skipped report to `models/cache/parity-report.json`.

Planned parity once fixtures exist:

1. Run MediaPipe Python Tasks on 3 short CC0/self-recorded videos.
2. Save palm boxes, landmarks, handedness, presence, world landmarks, and Tier 0 labels as JSON.
3. Compare Rust/ONNX output: landmark mean error ≤ 0.01 and Tier 0 agreement ≥ 97%.

## Proposed ADR-007 detail change

If the official gesture embedder/classifier cannot be converted cleanly, keep palm + landmarks as the hard dependency and implement Tier 0 with geometric normalized-landmark rules plus the existing `ProtoKnn` feature path. The Qualcomm classifier can remain a reference/OTA experiment, but the MVP should not depend on a classifier that bypasses the reusable 128-d embedder.
