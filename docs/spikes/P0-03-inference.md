# P0-03 — ONNX Runtime inference benchmark

Date: 2026-09-28
Host: Apple M5, macOS 27, aarch64
Crate: standalone `spikes/p0-03-ort/` with an empty `[workspace]` table
Command: `cargo run --release -- --models-dir ../../models/cache --coreml-cache-dir ../../models/cache/coreml/p0-03 -n 200 --warmup 20`

## Results

All timings are dummy-tensor inference in milliseconds. CoreML uses the NeuralNetwork format unless noted. MLProgram failed for the MediaPipe/OpenCV/BlazeFace convolution models with CoreML compile errors like `Required param 'pad' is missing`.

| Model | Budget | CPU p50 / p95 | CoreML NN p50 / p95 | First load / compile observed | Cache entries | Result |
|---|---:|---:|---:|---:|---:|---|
| `palm_detection_full` | ≤ 4 p95 | 6.63 / 8.40 | 10.11 / 12.70 | CPU 14.6 ms; CoreML first compile 1869 ms | 241 | **No-go** |
| `hand_landmark_full` | ≤ 4 p95 | 7.12 / 8.75 | 0.56 / 0.59 | CPU 5.8–9.2 ms; CoreML first compile 337 ms | 9 | **Go with CoreML NN** |
| `face_detection_short` | ≤ 2 p95 | 1.14 / 1.39 | 1.89 / 2.08 | CPU 2.6–3.6 ms; CoreML first compile 561 ms | 97 | **Go on CPU** |
| `scene_embedder` | every 60 s, OTA | 43.77 / 53.42 | 84.43 / 117.83 | CPU 53–59 ms; CoreML NN first compile 1799 ms; MLProgram first compile 15282 ms | 593 NN / 555 MLProgram | **Go as CPU OTA-only** |
| `canned_gesture_classifier_qaihub_candidate` | ≤ 1 p95 | 0.04 / 0.08 | 0.13 / 0.16 | CPU 9 ms; CoreML NN first compile 307 ms; MLProgram 92 ms | 9 NN / 10 MLProgram | Perf OK; contract mismatch |

The CoreML cache directory option is wired and verified: cache directories were populated under `models/cache/coreml/p0-03`.

## Go/no-go

- **P0-03 is red for the selected palm detector**: neither CPU nor CoreML NeuralNetwork meets the ≤ 4 ms p95 budget on this host with the OpenCV 192x192 model.
- **Hand landmarks are green with CoreML NeuralNetwork** and should use CoreML by default on macOS.
- **BlazeFace is green on CPU** and should not pay CoreML dispatch/compile overhead.
- **DINOv2-small is not suitable for per-frame use** but is acceptable for an OTA scene-signature job every 60 s or on wake.
- **Gesture classifier perf is trivial**, but the Qualcomm ONNX candidate does not satisfy ADR-007's embedder/classifier contract.

## Follow-ups

1. Try `ort` XNNPACK once the main workspace lands; tiny models may beat CoreML dispatch.
2. Benchmark OpenCV int8/block-quantized palm exports and Qualcomm's 256x256 palm detector, but only if their anchors/post-processing can be reconciled with Flick's Rust decoder.
3. Self-convert the official MediaPipe palm model with a CoreML-friendly graph and static pads.
4. Keep the CoreML cache directory in the session factory; first compile is expensive but subsequent loads are much lower.
