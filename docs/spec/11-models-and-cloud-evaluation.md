# 11 — Models & cloud evaluation (Hugging Face, Azure Foundry, Cognitive Services)

This doc answers two questions:
1. Do small pre-trained Hugging Face models help?
2. Would Azure AI Foundry / Cognitive Services (available to the owner at almost no cost) give better results or performance?

Research date: 2026-09-28. Re-check model licenses and Azure service status before each release (`07-…` §6.1).

## 1. Requirements that decide the answer
| Requirement | Why it matters |
|---|---|
| **R1 Latency:** gesture → HA `sent` p95 ≤ 150 ms (static), per-frame inference ≤ 15 ms | Any per-frame network round trip (≥ 50–600 ms) breaks this |
| **R2 Privacy:** video never leaves the machine by default | Rules out per-frame cloud vision |
| **R3 Offline:** works when the internet is down (HA is local) | Rules out hard cloud dependencies |
| **R4 Learn from the UI:** new static, motion and two-hand gestures from 5–10 takes in seconds | Needs a landmark-based few-shot learner, not a fixed-label classifier |
| **R5 Cross-platform, small:** macOS/Windows/Linux, CPU-capable, ≤ ~30 MB of default models | Rules out heavy models in the default install |
| **R6 Commercially licensed:** Apache-2.0/MIT/BSD for bundled models; no NC/ShareAlike weights | Blocks many HF models and datasets |

## 2. Hugging Face: verdict per category

| Category | Examples found on the Hub | Verdict | Reason |
|---|---|---|---|
| **MediaPipe ports** (palm detection, hand landmarks, pose, gesture) | `qualcomm/MediaPipe-Hand-Detection` (Apache-2.0, ~10K downloads, updated Sep 2026), `qualcomm/MediaPipe-Pose-Estimation` (~14K), `qualcomm/MediaPipe-Hand-Gesture-Recognition` | ✅ **Base of the pipeline** (already chosen in `02-…`) | Small (≈ 3–8 MB), fast, Apache-2.0, ONNX-exportable. Qualcomm's repos are good reference exports for validating our ONNX conversion |
| **Gesture image classifiers** (ViT/ResNet fine-tuned on HaGRID etc.) | many hobby repos, ~0 downloads, often with no license | ❌ | Static poses only; fixed labels (can't add "my fan circle" from the UI, R4); 20–90 MB each (R5); licenses unclear or ShareAlike (R6); worse than landmarks + few-shot under varied lighting and backgrounds |
| **Video action recognition** (VideoMAE, X-CLIP, TimeSformer) | general Kinetics / Something-Something models | ❌ | 100 MB–1 GB, 50–300 ms per clip, fixed classes, datasets often non-commercial |
| **Small VLMs** (Florence-2, SmolVLM, Moondream, Qwen-VL small) | `onnx-community/Florence-2-base-ft` (MIT) | ❌ as recognizer / ✅ for setup | 100–1000 ms per frame; fine for one-shot "what devices do I see?" suggestions (`09-…` §8) |
| **Open-vocabulary detection** | `google/owlv2-base-patch16-ensemble` (Apache-2.0, 155M params) | ✅ **setup-time, on demand** | Finds "ceiling fan", "lamp", "tv" in a single snapshot for the teach assistant |
| **Segmentation** | `onnx-community/sam2.1-hiera-tiny-ONNX` (Apache-2.0) | ✅ setup-time, on demand (Phase 2) | Tap-to-teach object outline |
| **Monocular depth** | `depth-anything/Depth-Anything-V2-Small-hf` (**Apache-2.0**, 24.8M) | ✅ setup-time, on demand (Phase 2) | Converts a 2D tap into a `point3d` anchor. ⚠️ **Base/Large are CC-BY-NC-4.0 → banned** |
| **Image embeddings** | `facebook/dinov2-small` (Apache-2.0, 22.1M) | ✅ **Core** (scene signature, every 60 s) | Detects "the camera moved" (`09-…` §6) |
| **Face keypoints** | MediaPipe BlazeFace short-range (Apache-2.0) | ✅ **Core, on demand while pointing** | Eye-rooted pointing ray (`09-…` §3) |
| **Datasets** | HaGRID / HaGRIDv2 (CC BY-SA 4.0 / "other"), Jester (non-commercial) | ⚠️ **Evaluation only** | Never ship weights trained on them in the commercial build unless the license is cleared |

**Conclusion:**
- Mini pre-trained HF gesture models do **not** help the core problem. Users need to create *their own* gestures in the UI and bind them to *their own* devices. That is a few-shot problem on top of a strong hand-landmark model, and MediaPipe is the best open option (`02-…` §2).
- HF **is** valuable for:
  1. reference exports of MediaPipe;
  2. small setup-time helpers (detection, segmentation, depth, embeddings), shipped as on-demand OTA packs (`10-…` §5.4) and never run per frame.

### 2.1 MediaPipe and UI-created gestures
MediaPipe's canned gesture classifier (7 labels) is used only as the Tier 0 built-in set. **New gestures do not go through MediaPipe Model Maker**:

| Tier | Learns from UI? | How | Training time |
|---|---|---|---|
| 0 built-ins (MediaPipe canned + geometric: point, pinch, circles, two-hand separate) | parameters only | thresholds in the catalog pack | none |
| 1 custom static poses | ✅ | 5–10 takes → landmark embedding → prototype / k-NN (`02-…` §4.3) | < 1 s |
| 2 custom motion + two-hand | ✅ | 3–5 takes → DTW templates over normalized trajectories (`02-…` §4.4) | < 1 s |
| Future: Flick Motion Embedder | ✅ (still few-shot) | tiny temporal encoder (≤ 1 MB) pre-trained on permissive or synthetic trajectories and never fine-tuned on device; users' takes become prototypes in its embedding space | < 1 s on device; model trained on Azure ML (§3.2) and shipped as an OTA pack |

## 3. Azure AI Foundry & Cognitive Services

### 3.1 In the recognition hot path: ❌ No
| Option | Status / fit | Blocking issue |
|---|---|---|
| Azure AI Vision **Image Analysis 4.0** | Deprecated; **retires 2028-09-25** | No hand-gesture or hand-keypoint output; network round trip per frame (R1, R2, R3) |
| **Spatial Analysis** (people/pose on video) | **Retired 2025-03-30** | Not available |
| Image Analysis 4.0 **custom model** preview | Retired 2025-03-31 → Custom Vision is the GA alternative | Classifier with fixed labels trained in the cloud: fails R4 (seconds-to-minutes cloud training, not in-UI instant), R1, R2 |
| **Custom Vision** (GA) | Can export compact models (ONNX/CoreML) | Static images, fixed labels, cloud training loop; our landmark few-shot approach is faster to train, smaller and more accurate for hands |
| Foundry multimodal models (GPT-4.1/4o-class, Phi-4-multimodal) on each frame | Available | 300–2000 ms, per-frame cost, video upload: fails R1, R2, R3 and cost |
| **Foundry Local** (on-device, ONNX Runtime; Win / macOS Apple silicon / Linux; Rust SDK; no Azure subscription) | Preview | Runs *LLMs/SLMs*, not a hand-landmark pipeline; we already run ORT directly with better control |

Budget for comparison (to be confirmed in P0 spikes): local hand pipeline ≈ **8–12 ms/frame on an M1** (`01-…` §5). A cloud round trip from a home network is typically 50–150 ms before any inference. **Local is 10–100× faster and free per frame**, so Azure cannot improve the core latency or accuracy.

### 3.2 Where Azure does help: ✅ Yes (and the owner's credits are well used here)
| Use | Service | Phase | Who pays at scale |
|---|---|---|---|
| **OTA hosting** (app + packs), CDN, custom domain | Azure Blob Storage + Azure Front Door (`10-…` §7) | 1 | Flick (small; Front Door has a base fee, alternatives listed there) |
| **Windows code signing** (no EV token) | **Azure Artifact Signing** (formerly Trusted Signing; ~US$9.99/month basic tier) | 1 | Flick |
| **CI secrets / signing keys** | Azure Key Vault + GitHub OIDC | 1 | Flick |
| **Model conversion, quantization, benchmarking; nightly evaluation** on recorded landmark datasets | Azure ML (GPU compute, jobs) | 1–2 | Flick (owner's credits) |
| **Train the Flick Motion Embedder** and future pack versions | Azure ML | 2 | Flick |
| **Setup assistant** (opt-in): "which devices do you see?" on ONE snapshot, name matching in any language | Foundry multimodal model | 2 | **User's own endpoint/key** or Flick Pro backend; local Florence-2/OWLv2 fallback |
| **Smart Context** (Phase 3 Pro): natural-language rules ("when I point at the fan after 11 pm, set speed 1 only") compiled to Flick rules | Foundry Local (on device, preview) or Foundry cloud model; alongside Laya / Jev (`00-…`) | 3 | Local = free; cloud = user key or Pro |
| **Opt-in crash reports** | Application Insights | 1 | Flick |

### 3.3 Cost & policy caveats
- The owner's near-free access covers **development and Flick's own infrastructure**. It does **not** cover end users. Any runtime cloud feature for users must be one of:
  - bring-your-own key/endpoint (stored in the OS keychain), or
  - a paid Pro backend with rate limits.
- **Core must work 100 % offline.** Cloud features are opt-in, per use, with a preview of the data sent (`07-…` §4).
- Keep the Azure dependency swappable: the setup assistant targets an OpenAI-compatible chat/vision API, so Foundry, a local Foundry Local endpoint or others can be used; hosting is plain HTTPS static files (Blob, R2 or GitHub Releases).
- Foundry Local is preview: verify its license/redistribution terms before bundling (open question, README).

## 4. Final model inventory

| Model | Size (approx.) | License | Runs | Shipped |
|---|---|---|---|---|
| Palm detector (MediaPipe) | ~2 MB | Apache-2.0 | every frame when no hand tracked | bundled + OTA |
| Hand landmarker full (MediaPipe) | ~6 MB | Apache-2.0 | every frame with hands | bundled + OTA |
| Gesture embedder/classifier (MediaPipe canned) | ~1 MB | Apache-2.0 | every frame with hands | bundled + OTA |
| BlazeFace short-range | ~0.5 MB | Apache-2.0 | only in point pose | bundled + OTA |
| DINOv2-small (scene signature) | ~45 MB fp16 / ~23 MB int8 | Apache-2.0 | every 60 s + on wake | OTA pack (downloaded on first anchor teach) |
| Pose lite | ~3 MB | Apache-2.0 | RTSP arm ray (Phase 2) | on demand |
| OWLv2 base / Florence-2 base | ~300–600 MB | Apache-2.0 / MIT | setup only | on demand |
| SAM 2.1 hiera tiny | ~40 MB | Apache-2.0 | tap-to-teach only | on demand |
| Depth Anything V2 **Small** | ~50 MB | Apache-2.0 | tap-to-teach only | on demand |
| Flick Motion Embedder | ≤ 1 MB | Apache-2.0 (ours) | motion frames | OTA (Phase 2 research) |

Default install stays ≈ **10 MB of models**. Everything else is on demand.
