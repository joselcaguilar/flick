# Flick — Product & engineering spec

> **Flick** turns any camera into a gesture remote for Home Assistant. Point at the ceiling fan and draw a circle: speed 1. Pull stacked hands apart: stop. A thumbs-up turns on the lights; a pinch dials the volume.
> Local-first, minimal training, OTA-updatable, open-core.

This folder is the **source of truth** for humans and coding agents building Flick. If code and spec disagree, fix the spec first (with a decision-log entry) or fix the code.

## Documents

| # | File | What it answers |
|---|---|---|
| 00 | [00-product-overview.md](00-product-overview.md) | Why, for whom, MVP stories, goals/non-goals, positioning, open-core model, metrics |
| 01 | [01-architecture.md](01-architecture.md) | ADRs, components, core traits/types, threading, latency budget, repo layout, versions |
| 02 | [02-vision-pipeline.md](02-vision-pipeline.md) | Capture (webcam → RTSP), models/EPs, hand perception, gesture tiers, trigger FSM, performance |
| 03 | [03-home-assistant-integration.md](03-home-assistant-integration.md) | WebSocket client, "do we need a response?", auth, action resolution, safety policy |
| 04 | [04-gesture-studio-and-ux.md](04-gesture-studio-and-ux.md) | Screens, onboarding, Gesture Studio, mappings, HUD, settings, a11y, UI acceptance |
| 05 | [05-desktop-app-and-distribution.md](05-desktop-app-and-distribution.md) | Tauri shell, sidecar, tray/HUD windows, macOS permissions & signing, Win/Linux, CI/CD |
| 06 | [06-data-model-and-api.md](06-data-model-and-api.md) | Files, SQLite DDL, settings keys, REST + WS contract, gesture pack format |
| 07 | [07-quality-security-licensing.md](07-quality-security-licensing.md) | Lean testing policy + the complete test suite, fixtures, dev tooling, CI gates, privacy, security, licenses, open-core boundary |
| 08 | [08-roadmap-and-work-breakdown.md](08-roadmap-and-work-breakdown.md) | Phases, epics, agent-sized tasks with dependencies + acceptance criteria, risks |
| 09 | [09-device-targeting.md](09-device-targeting.md) | Point-to-select: pointing ray, taught anchors, places/re-align, verbs, fan levels, Teach flow |
| 10 | [10-ota-updates.md](10-ota-updates.md) | Signed OTA for the app, model packs and gesture catalog; channels, staged rollout, rollback, hosting/CI |
| 11 | [11-models-and-cloud-evaluation.md](11-models-and-cloud-evaluation.md) | Hugging Face models and Azure Foundry / AI services: what helps, what doesn't, and why |
| — | [../../PRODUCT.md](../../PRODUCT.md) · `DESIGN.md` (created in P1-700) | Impeccable design context: users, purpose, principles (PRODUCT.md) and the visual system (DESIGN.md) |

### Reading order by role

| Role | Read first | Then |
|---|---|---|
| Everyone | 00, 01 §2 (ADRs), 08 (your task) | the sections your task links to |
| Vision / gestures agent | 02, 01 §3–5 | 06 §3 (samples/embeddings/motion takes), 07 §1, 11 §2 |
| Spatial / targeting agent | 09, 02 §4.2 (point pose) | 03 §6.3 (verbs), 06 §3 (places/anchors), 07 §1 |
| HA agent | 03 | 06 §4–6, 07 §5, 09 §5 |
| Backend / API agent | 06, 01 §3–4 | 03 §8, 07 §5.1, 10 §8 |
| OTA / release agent | 10, 05 §8 | 06 §8 (migrations), 07 §5.3–5.5, 11 §3 |
| UI agent | **PRODUCT.md, DESIGN.md, 04 §0 (Impeccable)**, 04 | 06 §5–6, 05 §1–3, 09 §7 |
| Desktop agent | 05 | 07 §3, §5.3, §6, 10 §4 |

## Stack at a glance

| Layer | Choice |
|---|---|
| Engine (backend) | Rust 2024 + tokio, sidecar process `flick-engine` |
| Inference | ONNX Runtime (`ort` 2.x): CoreML / DirectML / CUDA / OpenVINO / XNNPACK / CPU, auto-benchmarked |
| Models | MediaPipe hands (palm, landmarks, embedder, canned classifier) → ONNX; few-shot custom static gestures + DTW motion/two-hand templates recorded in the UI; BlazeFace (pointing ray) + DINOv2-small (places) |
| Device targeting | Point → eye-rooted ray → taught 3D anchors → 4 s selection → verb gesture (`09-…`) |
| Frontend | React 19 + TS + Vite + Tailwind + shadcn/ui (re-skinned to DESIGN.md) in a Tauri 2 shell (macOS first, Win/Linux in CI); visual design through Impeccable |
| Engine ↔ UI | HTTP + WebSocket on `127.0.0.1` (axum), per-launch token, OpenAPI-generated TS types |
| Home Assistant | WebSocket API, direct `call_service` + `result` ack (no webhooks, no HA events) |
| Storage | SQLite (landmarks and geometry, never images) + OS keychain for secrets |
| OTA | Tauri updater (app) + `flick-update` (signed model/catalog packs, hot swap, auto-rollback); Azure Blob + CDN, GitHub mirror |
| Cloud (Azure) | **Never in the recognition path.** Used for update hosting, Artifact Signing, Key Vault, Azure ML training; opt-in setup assistant (`11-…`) |
| LLM | Not in the hot path. Phase 4 "Smart Context": Laya or Foundry Local on-device, Jev API opt-in |

## Agent working agreement

1. **Pick work from 08 only.**
   - Take a task only when all its `deps` are done.
   - One task per branch/PR. Branch name: `task/<ID>-<short-name>` (e.g. `task/P1-402-trigger-fsm`).
   - If a task grows beyond **L**, stop and propose a split in the PR description.
2. **The spec is normative.**
   - Implement what the linked sections say. Where the spec is silent, choose the simplest option and note it in the PR.
   - To **change a decision**, update the ADR table and add a row to the decision log in `01-architecture.md` §10 **in the same PR**, plus any affected spec files.
   - Items marked *verify in spike* / *assumption* are not facts. Confirm them, then update the spec.
3. **Commits:**
   - [Conventional Commits](https://www.conventionalcommits.org/) (`feat(gestures): …`, `fix(ha): …`).
   - DCO sign-off (`git commit -s`) is required.
4. **Never commit:**
   - secrets, HA tokens, RTSP URLs with credentials
   - model binaries (use `cargo xtask fetch-models`)
   - third-party footage
5. **Keep the core/Pro boundary** (`07-…` §6.2):
   - No Pro-only logic in the public repo; use the extension traits.
   - No DRM in core.
6. **Test without hardware.** Every feature must be testable with `--fake-camera`, `FLICK_FAKE_LANDMARKS`, the landmark replay harness, `mock_ha` and `serve-updates` (`07-…` §2).
7. **UI work uses Impeccable** (`04-…` §0).
   - Read `PRODUCT.md` and `DESIGN.md` first. Never ship the stock component theme.
   - Run critique + audit + the detector before opening the PR.
8. **Lean tests** (`07-…` §1.1). Write the tests a senior engineer would write, and no more. Prefer one table-driven test or one replay fixture over many small tests. Never add tests for getters, serde derives, generated code, presentational components or code nothing calls.
9. **Everything that ships can be updated OTA.** Model, threshold or catalog changes go through a signed pack (`10-…`), not an app release, unless code changes are needed.

## Coding conventions

**Rust**
- Edition 2024; the toolchain is pinned in `rust-toolchain.toml`.
- `cargo fmt`; `clippy -D warnings` with workspace lints.
- Errors: `thiserror` in library crates; `anyhow` only in binaries/xtask.
- Logging: `tracing` with structured fields. Never log secrets; rely on the redaction layer.
- No `unwrap()`/`expect()` outside tests and provably-infallible cases (comment why).
- `unsafe` only in dedicated FFI modules, each with a `// SAFETY:` comment.
- Hot path (capture → vision → gestures): no allocations per frame after warm-up, no blocking I/O, no locks held across `.await`.
- Public items are documented. Tests live next to the code, plus crate `tests/` for integration.

**TypeScript / UI**
- `strict: true`, pnpm.
- Biome (lint + format).
- Vitest + Testing Library; Playwright for E2E.
- API types are **generated** (`cargo xtask gen-api`). Never hand-write DTOs.
- State: server state in TanStack Query, live WS state in Zustand.
- Components: shadcn/ui primitives re-skinned with Tailwind tokens generated from `DESIGN.md`. All strings go through i18n keys.
- Fonts and icons are bundled (the app works offline); no runtime CDNs.

**Python (tools only)**
- `uv` project in `tools/training/`; `ruff`.
- Never shipped in the app.

## Definition of done (every task)

- [ ] Acceptance criteria in 08 are demonstrated (test output; screenshots/GIF for UI changes).
- [ ] CI green on macOS, Windows, Linux (fmt, clippy, tests, `cargo deny`, UI lint/typecheck/test, API drift).
- [ ] Tests follow the lean policy (`07-…` §1.1): only tests for hard logic, costly failures, contracts or a fixed bug. The PR lists each new test and what it protects against. No tests for trivial or unused code, no coverage-driven tests.
- [ ] No performance regression > 20 % on `flick-engine bench` for hot-path changes.
- [ ] Spec, OpenAPI and user docs are updated if behavior or contracts changed.
- [ ] Privacy/security rules hold: no frames stored, no secrets logged, safety policy enforced server-side, nothing sent to the cloud without opt-in.
- [ ] **UI changes:** the Impeccable critique + audit summaries and the detector JSON (zero unresolved findings) are in the PR, with light/dark screenshots at 1280/800 px and the HUD on light/dark wallpapers (`04-…` §0).
- [ ] **Schema changes** follow the N−1 read rule (`06-…` §8). **Model/threshold changes** ship as a signed pack with golden fixtures (`10-…` §5).

## Glossary

| Term | Meaning |
|---|---|
| **Engine** | `flick-engine`: capture, vision, gestures, HA client, store, local API. It runs as a sidecar (desktop) or standalone (headless) |
| **Shell** | The Tauri app (`flick-desktop`): windows, tray, HUD, sidecar supervisor |
| **Tier 0 / 1 / 2** | Built-in static gestures / few-shot custom static gestures / motion gestures (swipes, circles, pinch-dial, two-hand "stop", custom DTW motion/two-hand templates), all Core |
| **Mapping** | **Global:** gesture (+ optional hand/camera filter) → HA action. **Targeted:** taught device (or "any device of domain X") + gesture → verb. Both have a trigger mode: tap / hold / repeat / dial |
| **Point pose / ray** | `builtin.point` (index extended, others curled) and the 3D ray from the eyes through the fingertip used to select a device |
| **Anchor** ("taught device") | A device's estimated position (or direction) in a camera's space, taught by pointing from 1–3 spots, bound to an HA entity/device/area |
| **Place** | A camera + scene signature (e.g. "Bedroom desk"). Anchors belong to a place; if the camera moves, the place is flagged `needs_realign` |
| **Selection** | The 4 s window after pointing + dwell at an anchor, in which verb gestures act on that device |
| **Verb** | A device-independent intent (`up`, `down`, `on`, `off`, `stop`, `toggle`, `level_set`) resolved to a concrete service per domain (`03-…` §6.3) |
| **Level** | A taught or derived setpoint (e.g. fan speed 1 = `percentage: 1` for `fan.ventilador_dormitorio`) |
| **Pack** | A signed OTA data bundle: a **model pack** (ONNX + metadata) or the **catalog pack** (gesture parameters, presets, FOV table, safety additions). Unrelated to user **gesture packs** (`.flickpack.json`) |
| **Channel** | Update track: `stable`, `beta`, `nightly`, with staged `rollout_pct` |
| **Impeccable** | The design skill/process used for all UI work; `PRODUCT.md` + `DESIGN.md` are its inputs |
| **TriggerFsm** | Per-track state machine that turns noisy per-frame scores into single, debounced events |
| **Arm mode** | Optional: hold open palm to open a short window in which gestures fire |
| **Sensitive action** | Locks, alarms, garage/door covers, etc. Need explicit opt-in + a confirm gesture |
| **EP** | ONNX Runtime execution provider (CoreML, DirectML, CUDA, …) |
| **LLAT** | Home Assistant long-lived access token |
| **HUD** | Transparent always-on-top overlay showing gesture/action feedback |
| **Smart Context** | Phase 4: a small model (Laya / Foundry Local / Jev) picks between user-defined candidate actions using time/state context |
| **Core / Pro** | Apache-2.0 open-source features / paid closed-source extensions |

## Open questions (track and resolve; update the spec when answered)

1. Final domain → bundle id, OAuth client_id URL, updater URL (currently the placeholder `app.flick.desktop`). Trademark check vs "Flic".
2. Phase 0 spike outcomes (P0-02 … P0-09). Each can change ADR-002/003/005/007/015/016 details. P0-08 decides the pointing defaults (tolerance, margin, place threshold 0.90, ray model); P0-09 decides update hosting (Front Door vs Blob+CDN vs R2) and confirms Windows Artifact Signing eligibility.
3. Merchant of record for Pro (Paddle vs Lemon Squeezy, see `07-…` §6.2) and pricing.
4. Is Intel Mac support needed (P2-09)?
5. License terms of the Laya and V-JEPA 2 weights for commercial Pro use (verify before Phase 4).
6. Flathub vs AppImage-only for Linux distribution.
7. Foundry Local redistribution/bundling terms (Pro Smart Context). Until verified, it is a user-installed runtime only.
8. Who pays for the opt-in cloud setup assistant for Core users (BYO key only?) and which Foundry model/region (image-retention terms).
9. Update domain and CDN (tied to question 1). The `root` key ceremony (hardware tokens, the 3 key holders).
10. Should the owner's `fan.ventilador_dormitorio` get an HA area? Recommended: assign it (e.g. "Dormitorio") so the Teach picker ranks it first.
