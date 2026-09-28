# 07 — Quality, security, privacy & licensing

## 1. Testing

### 1.1 Policy: only the tests that matter (normative)
Flick is mostly built by coding agents, and agents tend to generate large test suites for trivial or unused code. That slows CI, costs maintenance and hides the tests that matter. The rule: **write the tests a senior engineer would write, and no more.**

**Write a test only when at least one of these is true:**
1. **Hard logic:** math, geometry, parsing, state machines or resolution tables, where a bug is easy to make and hard to see (NMS, triangulation, TriggerFsm, verb resolution, the `a`/`c`/`r` parser).
2. **Costly or silent failure:** safety policy, secret redaction, auth, signature verification, migrations and rollback.
3. **A contract across a boundary:** the HA protocol, the pack/channel-index formats. The REST contract is covered by OpenAPI generation + the drift check, not by per-route tests.
4. **A fixed bug:** one regression test that reproduces it.
5. **An acceptance criterion in 08** that can't be shown by an existing test, a replay fixture, a nightly metric or a manual check.

**Never write tests for:**
- getters/setters, constructors, `Default`, derived traits, serde round-trips of plain structs, DTO mappers, constants, config wiring;
- generated code (OpenAPI types, `schemars` schemas) or third-party behavior (tokio, axum, SQLite, React, shadcn);
- code nothing calls yet. Don't write that code either;
- private helpers already exercised through the public function;
- presentational React components, markup snapshots or visual-regression suites. Visuals are reviewed through PR screenshots + Impeccable (`04-…` §0);
- implementation details: mock call counts, internal state, log wording;
- the same behavior at several layers. Test it once, at the cheapest layer that proves it;
- exhaustive permutations. Use **one table-driven test** with representative rows + edge cases, or one property test.

**Shape:**
- One table-driven test per function beats many near-identical tests. For gesture and targeting behavior, the replay harness (real recorded landmarks) beats mocks.
- Name tests after the behavior they protect (`stale_action_dropped_after_disconnect`), not after the function.
- Deterministic and fast: no sleeps (use tokio's paused time), no network except mock HA / `serve-updates`, no camera. `cargo test --workspace` stays under ~2 min in CI.
- Reuse the shared fixtures and the single mock HA. Add a fixture only for a new behavior or a reproduced bug.
- **No coverage targets.** Coverage is not measured in CI and is never a reason to add a test.
- Deleting redundant, flaky or obsolete tests is welcome.

**Review:**
- Every PR lists each new test with one line: "protects against …".
- If the test code is larger than the code under test, the PR explains why. Geometry and parsers may justify it; CRUD never does.
- Reviewers (human or agent) reject tests that break these rules.

### 1.2 The test suite
This is the complete list. Adding a new category needs a decision-log entry (`01-…` §10).

| Layer | What (only this) | Where | When |
|---|---|---|---|
| Unit (Rust) | Hard logic only: palm decode + weighted NMS, ROI math, TriggerFsm (one table test + one property test: no double fire within the cooldown), DTW matching, verb resolution + fan levels (one table test), safety validator (one table test), HA error mapping + compressed-state parser, dial coalescing, log redaction, `GestureId` parsing | next to the code | every PR |
| Geometry (`flick-spatial`) | One property test each for ray recovery, triangulation and Kabsch re-align (synthetic poses + noise); one table test for the TargetSelector FSM | `crates/flick-spatial/tests/` | every PR |
| Replay (one harness) | Recorded landmark (+ face-keypoint) JSONL → the exact `GestureEvent` / `target.*` sequence and the HA calls sent to mock HA. It covers gestures and targeting, incl. the fan scenario and the selection-lock fixture (`09-…` §11) | `crates/flick-engine/tests/replay.rs` over `tools/fixtures/{landmarks,targeting}/` | every PR (short fixtures); long negatives nightly |
| HA protocol | Mock HA: handshake, reconnect + resubscribe with the stale-action drop, the `bedroom_fan` scenario (`03-…` §9) | `crates/flick-ha/tests/` | every PR |
| Security-critical | API auth (token, Host, Origin); OTA verification: tamper, lower version, expired, catalog loosening the denylist (`10-…` §10) | `crates/flick-api/tests/`, `crates/flick-update/tests/` | every PR |
| Store | One migration test: a previous-release DB snapshot → migrate → the N−1 queries still work (`06-…` §8); cascade delete place → anchors → targeted mappings | `crates/flick-store/tests/` | every PR |
| Vision parity | The Rust ONNX pipeline vs reference outputs that `tools/training/parity.py` generates once and stores as JSON (thresholds in `02-…` §8) | `crates/flick-vision/tests/parity.rs` | nightly |
| Quality metrics | Recall, false triggers (incl. **0 targeted actions**), few-shot/motion accuracy, point pose, selection accuracy (`02-…` §8, `09-…` §11). Evaluation runs over the shared fixtures, not extra test code | `nightly.yml` | nightly |
| UI unit | Vitest only for non-trivial logic: the WS event reducer, sentence-builder validation, dial value math | `ui/src/**/*.test.ts` | every PR |
| UI E2E | **The 5 journeys below, nothing else** | `ui/e2e/` against `flick-engine --dev` + `FLICK_FAKE_LANDMARKS` + mock HA + `serve-updates` | every PR (macOS) |
| HA E2E | One script, two checks: 👍 toggles `light.bed_light`; point + circle sets a demo fan to level 1, then stop turns it off (`03-…` §9) | `tools/ha-e2e/` (docker HA `demo`) | nightly |
| App update E2E | Signed N−1 → N with a migration, then a simulated failure → rollback + DB restore | `release-dryrun.yml` | before each release tag |
| Performance | `flick-engine bench`: end-to-end onset → sent + per-stage p50/p95 vs `bench/baseline-m1.json`. No per-function micro-benchmarks unless a hot-path regression is being investigated | `nightly.yml` (macos-14) | nightly |
| Soak | 8 h `--fake-camera loop.mp4`: memory growth < 5 %, no fd leaks, reconnect storms survive | manual | before beta/stable releases |

**The 5 UI journeys (Playwright):**
1. Onboarding → connect mock HA → the first 👍 fires. "Skip HA" leads to demo mode.
2. Record a custom gesture (static, then motion via landmark replay) → map it → it fires.
3. Teach a device (landmark replay) → point + ↻ → `fan.turn_on {percentage: 1}`; point + two hands apart → `fan.turn_off`.
4. A sensitive mapping stays blocked until Safety is enabled and a confirm gesture is set.
5. Update ready → banner → "Restart to update" (against `serve-updates`).

Explicitly **not** in the suite: per-route API happy-path tests, retention/WAL/pragma tests, request-id counters, UI component or snapshot tests, HUD visual-regression tests, rollout-bucketing statistics, separate targeting or OTA "matrix" suites, coverage reports.

### 1.3 Fixtures
- **Only self-recorded, consented footage** (team members), licensed CC0 inside the repo. Keep videos short (≤ 15 s, 720p, H.264) and landmark JSONL for long runs.
- Layout:
  ```
  tools/fixtures/
    videos/        near_720p.mp4, thumb_up.mp4, swipe_right.mp4, pinch_dial.mp4, two_hands.mp4, dark_room.mp4 …
    landmarks/     *.jsonl (one HandFrame per line) + *.expected.json
    positives/     built-in gestures × hands × distances × light (landmark JSONL); circles, two_hand_separate
    motion/        custom motion + two-hand takes (Studio format) for DTW accuracy tests
    negatives/     ≥ 2 h "living room" activity as landmark JSONL (talking, TV, eating, phone use)
    targeting/     point_fan_circle.jsonl, point_fan_stop.jsonl, two_anchors_25deg.jsonl
                   (hand landmarks + BlazeFace keypoints + intrinsics) + seeded anchors *.anchors.json
    pointing-p0-08/ accuracy dataset: ≥ 3 people × 5 targets × 3 positions (ground-truth target ids)
  ```
- **Keep the set small.** Add a fixture only for a new behavior or a reproduced bug; prefer extending an existing `*.expected.json`.
- Public datasets (e.g. HaGRID) may be used **for evaluation only** after their license is checked and recorded in `tools/fixtures/LICENSES.md`. Never redistribute them in the repo.

## 2. Developer & agent tooling (makes everything testable without a camera or HA)
- `flick-engine --dev` does the following:
  - binds `127.0.0.1:7871`
  - fixed token `dev-token` (printed)
  - allows CORS from `http://localhost:5173` (Vite)
  - enables `/api/v1/debug/*` routes (inject a gesture event, dump the FSM state)
- `--fake-camera <video|dir>`: uses `FileSource` (also `FLICK_FAKE_CAMERA`). `--loop` repeats.
- `cargo run -p flick-ha --example mock_ha -- --port 8123 --scenario tools/ha-e2e/scenarios/demo.json` runs a scripted mock HA.
- `cargo xtask fetch-models` · `cargo xtask gen-api` · `cargo xtask bench` · `cargo xtask record-landmarks --camera 0 --out file.jsonl [--with-face]` (turns a live session into a fixture; `--with-face` also records BlazeFace keypoints for targeting fixtures).
- `FLICK_FAKE_LANDMARKS=<file.jsonl>` skips vision and replays landmarks (fast, deterministic E2E for targeting and motion).
- `cargo xtask serve-updates --dir ./target/updates` serves a local signed update tree on `http://localhost:7880` with **test keys** (`10-…` §7). Dev builds honor `FLICK_UPDATE_URL`; release builds ignore it.

## 3. Quality gates (CI must pass)
- `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace` on macOS, Windows, Linux.
- `cargo deny check` (licenses allowlist, advisories, bans, sources).
- UI: `pnpm lint`, `pnpm typecheck`, `pnpm test`.
- API drift: generated `ui/src/api/schema.d.ts` is up to date.
- No coverage gate or target (§1.1).
- Nightly: parity, false-trigger replay (< 0.2 fires/h, and **0 targeted actions**), recall (≥ 95 %), motion accuracy (≥ 85 %), selection accuracy (`09-…` §11), bench (no > 20 % regression), HA E2E.
- UI PRs: the Impeccable definition of done (`04-…` §0).

## 4. Privacy

| Principle | Implementation |
|---|---|
| Video never leaves the device | No network sink for frames exists in core. The MJPEG preview is served only on loopback, with a one-time ticket |
| No images stored | Samples = landmarks only (ADR-013). Diagnostic frame capture is off and only available in `--dev` |
| Taught devices are geometry | Anchors = 3D points/directions + teaching landmarks and eye keypoints. Places = a 384-float scene embedding with people masked out, **never an image**. "Delete all taught devices" in Settings. Packs never include anchors (`06-…` §7) |
| Cloud is opt-in, per use | Nothing in the recognition path uses the network. The Phase 2 setup assistant (Foundry) sends **one snapshot** only after a consent screen that previews the exact image. Each use needs its own consent, the key is BYO/Pro, and requests are not logged by Flick (`11-…` §3) |
| Update checks are anonymous | Requests carry only the channel path, app version and target triple. `install_id` never leaves the device, and rollout bucketing is local (`10-…` §9) |
| Minimal data | Activity log retention 7 days / 10 000 rows; "Delete all gesture data" in Settings |
| Camera only when needed | Tray shows state; auto-pause when HA is offline for > 60 s; quiet hours; pause shortcut |
| No telemetry by default | Zero analytics in core. Optional opt-in crash reports (`privacy.crash_reports`, Phase 2, Azure Application Insights or Sentry), which never include frames, landmarks, anchors, tokens or HA entity names |
| Transparency | Settings → Privacy → "What Flick stores" lists tables and retention; the diagnostics bundle is human-readable and redacted |
| Smart Context (Phase 4) | Laya runs locally. Jev requires explicit opt-in, the user's own API key, and a preview of the exact text sent (entity names/states/time — never images) |

## 5. Security

### 5.1 Local API threat model
- **Threats:**
  - other local processes or users calling the API
  - web pages in the user's browser attacking `127.0.0.1` (CSRF / DNS rebinding)
  - the LAN in headless mode
- **Controls:**
  - Bind `127.0.0.1` only (desktop).
  - 256-bit per-launch token passed via stdin; bearer auth on every route except `/health`.
  - **Host header allowlist** (`127.0.0.1:<port>`, `localhost:<port>`) → blocks DNS rebinding.
  - **Origin allowlist** for CORS and WS (`tauri://localhost`, `http://tauri.localhost`, engine origin).
  - WS auth via subprotocol; MJPEG via a one-time 60 s ticket.
  - Request body limit of 5 MB (packs); rate limit on `/ha/connect` and `/license`.
  - Headless LAN mode (Phase 3): TLS (self-signed, fingerprint shown), pairing code → device tokens (revocable), no default credentials.

### 5.2 Secrets
- HA tokens, OAuth refresh tokens, RTSP credentials and license keys live in the OS keychain (`keyring` 4.x).
- Never logged: a `tracing` redaction layer masks `access_token`, `Authorization`, `refresh_token`, `rtsp(s)://user:pass@`, license keys.
- The recommended HA setup is a **dedicated non-admin user**. The docs explain the admin-only command degradation.

### 5.3 Supply chain
- `cargo-deny` + `pnpm audit` in CI; Dependabot/Renovate weekly.
- SBOM per release (CycloneDX for Rust and npm).
- Models are pinned by sha256 in `models/manifest.toml` (bundled) or the signed pack manifest (OTA), and verified on load.
- Release signing:
  - Apple Developer ID + notarization
  - Azure Artifact Signing (Windows, from the first preview build)
  - minisign for updater artifacts
  - Signing keys only in protected GitHub environments, via GitHub OIDC → Azure Key Vault (no long-lived cloud secrets)

### 5.5 OTA key hierarchy & update security (`10-…` §2)

| Key | Holds | Stored | Rotation |
|---|---|---|---|
| `root` (ed25519) | signs `root.json` (trusted keys, thresholds) | **offline** hardware tokens, 2 of 3 maintainers | yearly, or on compromise; new root signed by old |
| `targets` (ed25519) | signs channel `index.json` | Azure Key Vault (HSM-backed), used by CI through OIDC | quarterly; revocable via root |
| Tauri minisign | signs app update archives | Key Vault | on compromise only (the key is embedded in shipped apps; rotation needs a transitional release) |
| Apple / Artifact Signing | OS code signing | Apple / Azure | per vendor policy |

- **Pack safety:** packs are data only. They contain no native code, scripts or ONNX custom ops (the loader refuses models with custom-op domains). Each pack passes a golden self-test and a benchmark on the user's machine before activation.
- **Catalog can only tighten safety:** a catalog pack that removes a denied or sensitive domain/service is rejected (`03-…` §7).
- **Freshness:** monotonic index versions and `expires` defend against rollback and freeze attacks.
- **Incident response:**
  1. Revoke the pack/app version in the index.
  2. Lower `rollout_pct` to 0.
  3. If the `targets` key leaked, rotate it with the offline root.
  4. Publish an advisory.
  5. Clients pick up the change within 6 h.

### 5.4 Home Assistant safety
- Denied/sensitive service policy (`03-…` §7) is enforced **server-side in the engine**, not only in the UI.
- The stale-action guard (2 s), cooldowns and N-of-M voting prevent runaway or delayed actions.

## 6. Licensing & compliance

### 6.1 Third-party components

| Component | License | Obligation / note |
|---|---|---|
| MediaPipe models (palm, landmarks, gesture embedder/classifier) | Apache-2.0 | Include NOTICE; state that models were converted to ONNX |
| ONNX ports (PINTO0309 / Qualcomm AI Hub) | Check each model card (PINTO: MIT/Apache per model; Qualcomm: model-specific) | Prefer self-converted models if a port's terms are unclear |
| ONNX Runtime | MIT | Attribution |
| `ort` | MIT OR Apache-2.0 | — |
| Tauri + plugins | MIT OR Apache-2.0 | — |
| FFmpeg | **LGPL-2.1+** (build without `--enable-gpl` / `--enable-nonfree`) | **Dynamic linking**, ship the exact source or offer + build script, allow relinking. Decode only, so no x264/x265 |
| `nokhwa` | Apache-2.0 | — |
| React, Tailwind, shadcn/ui, Lucide | MIT / ISC | — |
| UI fonts (chosen in DESIGN.md) | OFL-1.1 / Apache-2.0 only | Bundle the font files + license text; no runtime CDN |
| Impeccable (design skill, dev-time only) | Apache-2.0 | Not shipped in the app; attribution in `CONTRIBUTING.md` |
| BlazeFace short-range (MediaPipe face detection) | Apache-2.0 | NOTICE; ONNX conversion noted |
| MediaPipe Pose lite (Phase 2, arm-rooted ray) | Apache-2.0 | NOTICE |
| DINOv2-small (scene signature) | Apache-2.0 | NOTICE; model card link |
| OWLv2 base (setup assistant, on-demand, Phase 2) | Apache-2.0 | Downloaded on demand as an OTA pack |
| Florence-2-base(-ft) (setup assistant alternative, Phase 2) | MIT | On-demand pack |
| SAM 2.1 tiny (tap-to-teach outline, Phase 2) | Apache-2.0 | On-demand pack |
| Depth Anything V2 **Small** (tap-to-teach depth, Phase 2) | Apache-2.0 | **Base/Large are CC-BY-NC-4.0 → banned**. Pin the Small checkpoint by sha256 |
| Foundry Local (Pro Smart Context, Phase 4) | Microsoft terms (preview) | **Verify redistribution/bundling terms before shipping.** Until then, integrate only as a user-installed runtime |
| Azure AI Foundry models (opt-in setup assistant) | Per-model terms via Azure | User's own subscription or Flick Pro backend; the image is never used for training (verify per model/region at integration) |
| Laya (Phase 4) | Apache-2.0 | Model weights license verified at integration |
| RTMDet / RTMW (MMPose/MMDetection) (Phase 4) | Apache-2.0 | Verify specific checkpoint cards |
| V-JEPA 2 (Phase 4, experimental) | MIT (code) | Verify checkpoint license before shipping |
| Jev (Phase 4, opt-in) | TypeSafe commercial API terms | User brings their own key; Flick does not resell |

- **Banned** (enforced by `cargo-deny`/review): AGPL (e.g. Ultralytics), GPL in shipped binaries, non-commercial model/dataset licenses (CC-BY-NC) in anything shipped.
- `THIRD_PARTY_NOTICES.md` is generated at release (`cargo about` + `license-checker` for npm) and bundled in the app (Settings → About → Licenses).

### 6.2 Open-core boundary

| Area | Core (Apache-2.0, public `flick`) | Pro (closed, private `flick-pro`) |
|---|---|---|
| Cameras | 1 local + 1 RTSP | Unlimited cameras, zones (ROI per mapping), multi-camera dedupe |
| Gestures | Tier 0 built-ins (incl. point, swipes, circles, pinch-dial, two-hand "stop"), Tier 1 custom static, **custom motion and two-hand templates** | Far-field body gestures (ceiling/wall cameras) |
| Device targeting | Point-to-select on a single camera: anchors, places, re-align, verbs, fan levels | Multi-camera anchor fusion; far-field targeting (body pose from RTSP) |
| Intelligence | — | Smart Context (Laya local / Jev opt-in / Foundry Local) |
| Deployment | Desktop app; `--dev` engine; OTA updates | Headless engine with LAN pairing & remote management UI, Docker (CUDA) |
| Packs | Import/export static, motion and two-hand gestures | — |
| Cloud assists | BYO-key setup assistant (Phase 2) | Setup assistant included via the Flick Pro backend (no key needed) |

- **Mechanism:**
  - Pro crates implement core traits (`Entitlements`, `GestureRecognizer`, `FrameSource`, `ActionPlanner`) and register through `EngineBuilder::with_plugin(...)` in `flick-engine` behind the `pro` cargo feature.
  - Core contains **no DRM**, just the default `CoreEntitlements`.
- **License keys** (Phase 3):
  - Format: `FLICK1-<base64url(payload)>.<base64url(ed25519 signature)>`.
  - Payload: `{"lid": "...", "plan": "pro", "seats": 3, "features": ["cameras","fusion","farfield","context","headless","assist"], "issued_at": "...", "expires_at": null | "...", "email_sha256": "..."}`.
  - Verified **offline** with the public key embedded in `flick-pro-license`.
  - Grace: expired subscriptions keep working for 14 days, then Pro features disable. Data is kept and there is no lockout of Core features.
  - Seat counting is honor-based (no phone-home) in v1.
- Payments via a merchant of record (Paddle or Lemon Squeezy), which handles VAT/sales tax. The license key is delivered by email and via a `flick://license?key=` deep link.
- **Contributions:** DCO sign-off (`Signed-off-by`) on the public repo. No CLA, because Pro code is separate and not relicensed from contributions.

### 6.3 Trademark & naming
- Before public launch:
  - trademark search for **"Flick"** in classes 9 (software) and 42 (SaaS) in the target markets (US, EU)
  - note the close mark **"Flic"** (flic.io, smart-home buttons with a Home Assistant integration)
- Secure the domain + social handles; update the bundle id (`app.flick.desktop`), OAuth client_id URL and updater URLs once the domain is final.
- Keep "Home Assistant" usage compliant with the Open Home Foundation brand guidelines ("works with Home Assistant", no logo misuse).
