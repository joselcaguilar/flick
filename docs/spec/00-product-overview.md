# 00 — Product overview: Flick

> **Flick turns any camera into a private, instant remote for Home Assistant.
> No wearables, no cloud, no training marathon.**

## 1. Problem

Controlling a smart home today means reaching for a phone, a wall panel, or a voice assistant. Each has a failure mode:

- **Phone/app:** too slow for "just turn the light off".
- **Voice:** unreliable with TV noise, awkward with guests, unusable for Deaf/hard-of-hearing or speech-impaired people, and often cloud-dependent.
- **Physical buttons:** need batteries and have to be within reach.
- **Existing gesture hobby projects:** Python scripts that need a lot of training data and tuning, have no safety features, and are painful to install.

## 2. Solution

A desktop app that runs on the user's **Mac or PC** (not on the usually low-power Home Assistant host). It:

1. Watches a camera: the built-in/Continuity webcam in the MVP, and **Unifi Protect / any RTSP camera** next.
2. Recognizes hand gestures locally in **real time (under 350 ms)**:
   - **Built-in gestures work with zero training**: the 7 MediaPipe poses plus point, pinch, swipes, circles and a two-hand "stop".
   - Custom **static, motion and two-hand** gestures are recorded in the UI from **3–10 takes** and learned in under a second.
3. Lets users **point at a real device to select it**, then act on it with a verb gesture. Example: point at the ceiling fan and draw a circle → speed 1; point at it and pull stacked hands apart → off. Users teach Flick where each HA device is by pointing at it (`09-device-targeting.md`).
4. Calls **Home Assistant services directly** over the WebSocket API, so users don't have to write automations.
5. Gives instant feedback (HUD overlay + sounds) so users know what happened, even when the device is out of sight.
6. **Updates over the air:** the app, models and gesture catalog update, signed, with staged rollout and automatic rollback (`10-ota-updates.md`).

## 3. Personas

| Persona | Description | Needs | Priority |
|---|---|---|---|
| **Alex — HA power user** | Runs HA on a Pi/NUC, owns Unifi cameras and a Mac mini/MacBook. Tinkers on weekends. | Fast setup, reliable triggers, advanced mappings, RTSP support, no cloud | **Primary** |
| **Maria — accessibility user** | Limited mobility or speech, or Deaf/HoH; voice assistants fail for her. | Very reliable gestures, custom gestures that fit her body, clear visual feedback, no false triggers | **Primary** (design driver) |
| **Sam — household member** | Non-technical; uses what Alex set up. | "It just works", obvious feedback, can't break anything | Secondary |
| **Riley — creator** | Shares setups on Reddit/YouTube/HA forums. | Exportable gesture packs, nice demos | Secondary (growth) |

## 4. Core user stories (MVP)

- As Alex, I install Flick, connect my HA instance, and control a light with a thumbs-up **in under 3 minutes**.
- As Alex, I map *swipe right* to "next track" on a media player and *pinch + move up/down* to brightness.
- As Maria, I record a custom gesture that fits my hand in about **30 seconds** and use it immediately.
- As Alex, I teach Flick where my bedroom ceiling fan (`fan.ventilador_dormitorio`) is by pointing at it from two spots in **≤ 90 s**. Then *point at the fan + draw a circle* sets speed 1, and *point at the fan + pull stacked hands apart* turns it off.
- As Alex, I create a **new motion gesture** (e.g. a "Z" in the air) or a **two-hand gesture** in the Studio by recording 3–5 takes, with no code or datasets.
- As Alex, I get app, model and gesture-catalog updates automatically. A bad update never breaks my setup, because it is rejected or rolled back on its own.
- As Maria, I trust Flick not to trigger while I talk with my hands or watch TV.
- As Sam, I see a small overlay confirming "Living room lights → off ✓", or a clear error if HA is offline.
- As anyone, I can pause Flick instantly from the menu bar, and the camera is off when Flick isn't watching.

## 5. Goals & non-goals

### Goals (MVP, macOS)
- Time from install to first successful gesture action: **≤ 3 min** (median, new user with HA running).
- Built-in gesture recall: **≥ 95 %** at 0.5–3 m, normal indoor light.
- False activations with default settings: **< 0.2 per hour** of "living-room activity" footage.
- Gesture onset → HA `call_service` sent: **p95 ≤ 300 ms**. Onset → HA ack: **p95 ≤ 350 ms** with fast local integrations.
- Custom static gesture from **≤ 10 samples** with **≥ 90 %** held-out accuracy. Custom motion or two-hand gesture from **≤ 5 takes** with **≥ 85 %**.
- **Pointing selection:** **≥ 95 %** correct device for anchors ≥ 20° apart, **≤ 1 %** wrong-device selections (MacBook camera, 1–3 m; verified in spike P0-08).
- Teaching a device by pointing: **≤ 90 s** per device.
- OTA: **0** bricked installs; bad model packs are rejected before activation or rolled back within 24 h.
- Resource use on an M1: **< 5 % CPU idle** (no hand visible), **< 20 % CPU active**, **< 300 MB RAM**.

### Non-goals (MVP)
- Voice control, cloud processing in the recognition path, or sending video anywhere. Azure/Foundry is used only for Flick's own infrastructure and opt-in setup helpers (`11-models-and-cloud-evaluation.md`).
- Face recognition or person identification.
- A Home Assistant add-on (HA hosts are too weak; the product decision is to run on the user's Mac/PC).
- HA events/MQTT output (product decision: **direct service calls only**).
- Mobile apps, sign-language interpretation, recording video.

## 6. Positioning

- **Category:** touchless control for Home Assistant.
- **One-liner:** "Flick your wrist, control your home."
- **Taglines to test:** "Flick it on." · "Your home, at a flick." · "Wave hello to your home."
- **Differentiators:**
  1. Works out of the box (zero-training built-ins).
  2. **Point at a device, then gesture:** select real things by pointing, taught in seconds. There are no lists and nothing to memorize.
  3. Few-shot custom static, motion and two-hand gestures, created in a friendly Studio.
  4. Local-first and private: frames never leave the machine, and only hand landmarks are stored.
  5. Built for low latency (Rust + hardware-accelerated ONNX).
  6. Safety by default (arm mode, frame voting, sensitive domains blocked).
  7. Uses cameras people already own (webcam now, Unifi next).
  8. Always up to date: signed OTA for the app, models and gesture catalog.
- **Alternatives users compare against:** HA Assist/voice assistants, Flic/Zigbee buttons, presence sensors, DIY MediaPipe scripts.

> ⚠️ **Brand risk:** "Flic" (flic.io) sells smart-home buttons with a Home Assistant integration. Run a trademark
> search and pick a domain before public launch (see `07-quality-security-licensing.md`).

## 7. Business model (open-core)

- **Flick Core (Apache-2.0, free):**
  - desktop app;
  - 1 local camera + 1 RTSP camera;
  - built-in gestures, few-shot custom static, **motion and two-hand** gestures, swipes, circles, pinch-dial;
  - **point-to-select device targeting** (single camera);
  - direct HA service calls;
  - gesture-pack import/export;
  - OTA updates.
- **Flick Pro (paid, closed source):**
  - unlimited cameras + zones;
  - multi-camera anchor fusion (point at a device seen by several cameras);
  - far-field body gestures and targeting for ceiling/wall cameras;
  - Smart Context (Laya local / Jev opt-in / Foundry Local);
  - headless engine with remote management.
- Licensing is offline-verified (no account needed). Details in `07-quality-security-licensing.md`.

## 8. Success metrics

| Metric | Target (first 6 months after public beta) |
|---|---|
| Activation: HA connected + ≥ 1 mapping fired in first session | ≥ 60 % of installs |
| D7 / D30 retention (≥ 1 gesture action that day) | ≥ 40 % / ≥ 25 % |
| Median gesture actions per active user per day | ≥ 8 |
| "False trigger" reports per 1 000 active users per week | < 5 |
| Pro conversion of weekly active users | 3–5 % |
| Community: GitHub stars / shared gesture packs | 5 000 / 200 |

## 9. Release phases (summary)

| Phase | Scope |
|---|---|
| **0 — Spikes** | Model parity, inference benchmarks, capture, HA client, Tauri↔engine, RTSP latency, **pointing accuracy**, **OTA pipeline** |
| **1 — MVP** | macOS Apple Silicon, webcam, built-ins + custom static/motion/two-hand + swipes + circles + pinch-dial, **point-to-select targeting**, HA direct calls, Studio + Teach, HUD, **OTA (app + model/catalog packs)**, Impeccable-driven design, signed/notarized DMG; Win/Linux CI builds |
| **2 — Reach** | RTSP/Unifi (arm-rooted pointing), polished Windows + Linux, HA OAuth login, gesture packs, i18n, tap-to-teach, opt-in setup assistant, Motion Embedder research |
| **3 — Pro** | Licensing, multi-camera + zones + anchor fusion, headless engine + LAN pairing, Docker (NVIDIA) |
| **4 — Intelligence** | Far-field body gestures, Smart Context (Laya / Foundry Local on-device, Jev opt-in), V-JEPA 2 research backend, mobile camera nodes |

Full breakdown: `08-roadmap-and-work-breakdown.md`.
