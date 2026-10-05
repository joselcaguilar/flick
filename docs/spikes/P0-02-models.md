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
| `gesture_embedder` | official MediaPipe `gesture_recognizer.task` `float16/1` → tf2onnx | Apache-2.0 | `c9136cb7fed9` | `hand` f32 `1x21x3`, `handedness` f32 `1x1`, `world_hand` f32 `1x21x3` → `Identity` f32 `1x128` |
| `canned_gesture_classifier` | official MediaPipe `gesture_recognizer.task` `float16/1` → tf2onnx | Apache-2.0 | `9d4770039ab2` | `hand_embedding` f32 `?x128` → `Identity` f32 `?x8` |
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
| Gesture embedder | selected ONNX | Extracted `gesture_embedder.tflite` from the official `.task`; converted with `tf2onnx` 1.17.0 / TensorFlow 2.21.0 / opset 17 on Homebrew Python 3.12. |
| Canned classifier | selected ONNX | Extracted `canned_gesture_classifier.tflite`; converted with the same `tf2onnx` path. Qualcomm ONNX remains a reference candidate because its input contract differs. |
| BlazeFace short range | selected ONNX | No conversion needed. |
| DINOv2-small | selected ONNX uint8 | OTA-only due size and latency. |

The local `uv` project is in `tools/training/` and is pinned to Homebrew Python 3.12:

```bash
cd tools/training
UV_PYTHON_DOWNLOADS=never uv run --extra tensorflow --extra inspect python convert.py fetch
UV_PYTHON_DOWNLOADS=never uv run --extra tensorflow --extra inspect python convert.py convert-gesture
```

Conversion output:

- `gesture_recognizer.task` downloaded and extracted successfully.
- `models/cache/gesture_embedder.onnx` — SHA-256 `c9136cb7fed90600ecfcab23eebecb1a42842158466d9a717345e2ec326af00a`, 546,171 bytes.
- `models/cache/canned_gesture_classifier.onnx` — SHA-256 `9d4770039ab2d4bece3e77bbacadfec1be3300e729e11686cb0a4276f66f3b8d`, 6,496 bytes.
- The ONNX files are derived artifacts: not committed; generated from the pinned `.task` by `tools/training/convert.py convert-gesture`; hosted later in the bundled baseline/model OTA pack.
- Superseded hashes: tf2onnx named generated constants nondeterministically and embedded the absolute `.tflite` path, so these bytes could not be rebuilt. `convert-gesture` now canonicalizes both (same graph and weights, identical outputs); the reproducible hashes are pinned in `models/manifest.toml`.

## Parity

Synthetic parity against the TFLite submodels passed on 64 deterministic landmark cases plus classifier embeddings. `tools/training/parity.py` writes the JSON report to `models/cache/parity-report.json`.

| Comparison | Max abs diff | Mean abs diff | Top-1 agreement |
|---|---:|---:|---:|
| Embedder TFLite vs ONNX | `5.96e-7` | `5.58e-9` | 100% argmax agreement over 128-d outputs |
| Classifier TFLite vs ONNX | `2.98e-7` | `1.37e-8` | 100% |
| End-to-end embedder+classifier | `4.17e-7` | `2.69e-8` | 100% |

Remaining fixture parity once videos exist:

1. Run MediaPipe Python Tasks on 3 short CC0/self-recorded videos.
2. Save palm boxes, landmarks, handedness, presence, world landmarks, and Tier 0 labels as JSON.
3. Compare Rust/ONNX output: landmark mean error ≤ 0.01 and Tier 0 agreement ≥ 97%.

## ADR-007 detail

The official gesture embedder/classifier now converts cleanly, so no ADR-007 fallback is needed for P1-305. Keep the geometric Tier 0 `builtin.point` and `ProtoKnn` path as specified. The Qualcomm classifier remains a reference/OTA experiment only because it bypasses the reusable 128-d embedder.
