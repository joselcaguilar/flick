# P0-03 — ONNX Runtime inference benchmark

Date: 2026-09-28
Host: Apple M5, macOS 27, aarch64
Crate: standalone `spikes/p0-03-ort/` with an empty `[workspace]` table
Command: `CARGO_BUILD_JOBS=4 cargo run --release -- --models-dir ../../models/cache --coreml-cache-dir ../../models/cache/coreml/p0-03 -n 200 --warmup 20`

## Results

All timings are dummy-tensor inference in milliseconds. CoreML uses the NeuralNetwork format unless noted. MLProgram failed for the MediaPipe/OpenCV/BlazeFace convolution models with CoreML compile errors like `Required param 'pad' is missing`.

| Model | Budget | CPU p50 / p95 | CoreML NN p50 / p95 | First load / compile observed | Cache entries | Result |
|---|---:|---:|---:|---:|---:|---|
| `palm_detection_full` | ≤ 4 p95 | 13.23 / 17.10 | 9.43 / 12.31 | CPU 32.4 ms; CoreML 778 ms (cache warm from prior compile) | 241 | **No-go** |
| `hand_landmark_full` | ≤ 4 p95 | 7.72 / 10.00 | 0.63 / 0.81 | CPU 68.8 ms; CoreML 78.7 ms (cache warm) | 9 | **Go with CoreML NN** |
| `gesture_embedder` | ≤ 1 p95 with classifier | 0.030 / 0.033 | 0.249 / 0.393 (NN); 0.069 / 0.162 (MLProgram) | CPU 6.0 ms; CoreML NN 619 ms; MLProgram 642 ms | 25 NN / 10 MLProgram | **Go on CPU** |
| `canned_gesture_classifier` | ≤ 1 p95 with embedder | 0.005 / 0.006 | 0.026 / 0.053 (NN); 0.021 / 0.032 (MLProgram) | CPU 2.1 ms; CoreML NN 59.9 ms; MLProgram 178 ms | 9 NN / 10 MLProgram | **Go on CPU** |
| `face_detection_short` | ≤ 2 p95 | 1.48 / 1.83 | 2.81 / 4.42 | CPU 19.9 ms; CoreML 419 ms (cache warm) | 97 | **Go on CPU** |
| `scene_embedder` | every 60 s, OTA | 79.32 / 117.23 | 193.43 / 253.53 | CPU 140.7 ms; CoreML NN 2845 ms; MLProgram 8022 ms | 593 NN / 555 MLProgram | **Go as CPU OTA-only** |
| `canned_gesture_classifier_qaihub_candidate` | reference only | 0.046 / 0.063 | 0.178 / 0.275 (NN); 0.088 / 0.220 (MLProgram) | CPU 35.5 ms; CoreML NN 150 ms; MLProgram 109 ms | 9 NN / 10 MLProgram | Perf OK; contract mismatch |

The CoreML cache directory option is wired and verified: cache directories were populated under `models/cache/coreml/p0-03`.

## Go/no-go

- **P0-03 is red for the selected palm detector**: neither CPU nor CoreML NeuralNetwork meets the ≤ 4 ms p95 budget on this host with the OpenCV 192x192 model.
- **Hand landmarks are green with CoreML NeuralNetwork** and should use CoreML by default on macOS.
- **Gesture embedder + canned classifier are green on CPU** (combined p95 ≈ 0.04 ms on dummy inputs). CoreML works but dispatch/compile overhead makes it the wrong default.
- **BlazeFace is green on CPU** and should not pay CoreML dispatch/compile overhead.
- **DINOv2-small is not suitable for per-frame use** but is acceptable for an OTA scene-signature job every 60 s or on wake.
- **Qualcomm's classifier remains reference-only**, because the official converted classifier now satisfies ADR-007's 128-d embedder contract.

## Follow-ups

1. Try `ort` XNNPACK once the main workspace lands; tiny models may beat CoreML dispatch.
2. Benchmark OpenCV int8/block-quantized palm exports and Qualcomm's 256x256 palm detector, but only if their anchors/post-processing can be reconciled with Flick's Rust decoder.
3. Self-convert the official MediaPipe palm model with a CoreML-friendly graph and static pads.
4. Keep the CoreML cache directory in the session factory; first compile is expensive but subsequent loads are much lower.
