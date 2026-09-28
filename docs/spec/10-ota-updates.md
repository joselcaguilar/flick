# 10 — Over-the-air (OTA) updates

Crate: `flick-update` (new, in the engine). The desktop shell also uses `tauri-plugin-updater`.
Status: **Core, Phase 1.** Every build is OTA-ready from the first public beta (P1-1101…1105).

## 1. What updates, and how

| # | Artifact | Updated by | Restart? | Signature | Rollback |
|---|---|---|---|---|---|
| 1 | **App bundle** (Tauri shell + sidecar engine) | `tauri-plugin-updater` | Yes (on quit, or "Restart to update") | minisign (Tauri) + OS code signing (Developer ID / Artifact Signing) | Assisted downgrade (§4.3) |
| 2 | **Model packs** (hand/face/pose/embedder ONNX + metadata) | `flick-update` | **No**, hot swap | ed25519 (Flick metadata) + sha256 | Automatic (keeps N−1) |
| 3 | **Catalog pack**: built-in gesture parameters, verb presets, FOV table, safety denylist additions, HA presets, feature flags | `flick-update` | No | ed25519 + sha256 | Automatic |
| 4 | **Headless engine** (`flickd`, Docker image, Linux packages) | brew / apt / `docker pull` for the binary; `flick-update` for 2 and 3 | Service restart | Package manager signatures + cosign for images | Package-manager pin |

Principle: **models and parameters ship more often than code.** Packs are data, validated on the user's machine before activation (§5). They never contain executable code.

## 2. Update metadata (TUF-inspired)

```
https://updates.flick.app/            (placeholder domain — open question)
  root.json                            # trusted keys + thresholds; public key embedded in the app
  channels/
    stable/index.json                  # signed, expires in 7 days
    beta/index.json
    nightly/index.json
  app/<version>/<target>/…             # Tauri artifacts + latest.json per channel
  packs/<pack_id>/<version>/pack.tar.zst
```

`channels/<ch>/index.json` (signed by the `targets` key):

```json
{
  "schema": 1,
  "channel": "stable",
  "version": 42,
  "expires": "2026-10-05T00:00:00Z",
  "app": {
    "latest": "1.3.0",
    "min_supported": "1.1.0",
    "rollout_pct": 25,
    "revoked": ["1.2.1"]
  },
  "packs": [
    {
      "id": "hand-landmarker",
      "version": "2026.10.1",
      "sha256": "…",
      "size": 7340032,
      "requires_engine": ">=1.2.0",
      "rollout_pct": 100,
      "on_demand": false
    },
    {
      "id": "catalog",
      "version": "2026.10.3",
      "sha256": "…",
      "size": 48213,
      "requires_engine": ">=1.0.0",
      "rollout_pct": 100
    },
    {
      "id": "owlv2-base",
      "version": "2026.09.1",
      "sha256": "…",
      "size": 620000000,
      "on_demand": true
    }
  ],
  "signatures": [{ "keyid": "…", "sig": "…" }]
}
```

| Protection | Mechanism |
|---|---|
| Tampering | ed25519 signatures over canonical JSON. Pack sha256 is checked before extraction |
| Rollback/freeze attacks | Monotonic `version` per channel (a lower version is refused). `expires` means a stale index is ignored, with a UI warning after 14 days offline (the current pack keeps working) |
| Key compromise | Offline `root` key (hardware token, 2 of 3 maintainers). The `targets` key lives in CI (GitHub OIDC → Azure Key Vault). Rotating `root.json` requires signatures from the old root key |
| Revocation | `app.revoked`: the updater refuses these versions and offers the latest version to installs running them. Packs can be revoked the same way |

Minisign keys for the Tauri updater are separate. They are stored in Key Vault and used only in `release.yml`.

## 3. Channels & staged rollout
- Channels: `stable` (default), `beta`, `nightly`. Setting `updates.channel`. Switching to a less stable channel asks for confirmation. Switching back waits for stable to catch up (no downgrade by default).
- **Staged rollout (client-side):**
  - `bucket = sha256(install_id ‖ version) mod 100`. `install_id` is a random ULID created at first run. It is local and never sent.
  - An update is offered when `bucket < rollout_pct`.
  - The same scheme applies to packs.
- **Pausing a rollout:** CI lowers `rollout_pct`, or adds the version to `revoked`. This is just a metadata change, with no rebuild.

## 4. App updates (Tauri)

### 4.1 Check
- On launch plus every 6 h (jittered ±30 min), and on demand from Settings → Updates.
- Endpoint: `…/app/{{target}}/{{arch}}/{{current_version}}?channel=<ch>`. It is served as a static `latest.json` per channel.
- The rollout/revoked logic runs in the `version_comparator` hook, which uses the signed channel index.

### 4.2 Install
- Download in the background, then verify (minisign + OS signature).
- **Install on quit** by default, or through "Restart to update" in the tray.
- **Never during:** a pending confirmation, a Studio recording, a Teach session, or a pinch-dial interaction.
- `updates.auto_install_app` (default `true`). When `false`, Flick notifies only.

### 4.3 Post-update health & rollback
Before installing:
- Flick snapshots `flick.db` to `backups/flick-<old_version>-<ts>.db`.
- Flick records `update_state.pending_app = {from, to}`.

First launch after the update, health check within 60 s:
- engine up;
- HA connected (if it was configured);
- camera opens (if it was running);
- the migrations applied.

On failure:
- A banner says "Update had a problem" with **Roll back to <old>**.
- It re-downloads the old signed artifact through a `version_comparator` that accepts a downgrade when `rollback_to` is set.
- It restores the DB snapshot.

Also:
- Crash-loop detection: 3 engine crashes within 10 min → offer rollback automatically.
- Migrations must be forward-only and backward-readable for one minor version (`06-…` §8). Otherwise, the DB snapshot restore is required.

## 5. Model & catalog packs (`flick-update`)

### 5.1 Pack format
`pack.tar.zst` contains:
- `manifest.json`: id, version, kind (`model|catalog`), files + sha256, `requires_engine`, `provides` (e.g. `hand_landmarker@2`), license SPDX + source URL, input/output tensor specs, calibration data;
- the model or JSON files;
- `golden/`: fixed inputs + expected outputs within a tolerance.

### 5.2 Pipeline

```mermaid
flowchart LR
  A[index says new pack] --> B[resumable download\n(HTTP range)]
  B --> C[sha256 + signature]
  C --> D[self-test on golden inputs]
  D --> E[EP benchmark\nCoreML/DirectML/CUDA/CPU]
  E --> F{model pack?}
  F -- yes --> G[shadow eval on\nuser's own data]
  F -- no --> H[schema + policy check]
  G --> I{pass?}
  H --> I
  I -- yes --> J[activate when idle\natomic pointer swap]
  I -- no --> K[reject + keep current\nreport reason in UI]
  J --> L[24 h watchdog]
  L -- regression --> M[auto-rollback to N-1]
```

**Shadow evaluation** (runs locally, and nothing leaves the machine). The new model re-runs on the stored landmark samples and teaching observations and must meet all of these:
- leave-one-take-out accuracy of the user's custom gestures ≥ old − 2 points;
- no built-in gesture drops below its current precision on the golden set;
- anchors recomputed with the new estimator move ≤ 5° (`09-…` §11).

Hand-landmark model packs cannot re-run on stored landmarks. For them, golden **video** clips shipped in the pack + the user's quick "hold up your hand" check (optional) are used, and user templates are re-embedded from the stored `world` landmarks when possible (ADR-013).

**Activation:**
- Happens only when there has been no hand activity for ≥ 10 s, with `updates.install_when_idle = true` (default).
- The new ORT session is built in the background. The pointer is swapped between frames, so no frame is dropped.
- N−1 is kept on disk.

**Watchdog (24 h)** triggers auto-rollback when any of these regress versus the previous 24 h baseline:
- p95 latency > +30 %;
- detection rate −20 %;
- suppression `low_confidence` ×2;
- engine crash.

The event `model.rolled_back {pack, reason}` goes to the UI.

### 5.3 Catalog pack rules
- It is validated against a JSON Schema bundled in the engine version (`requires_engine`).
- It may **tighten safety only**: add sensitive domains/services, raise thresholds or cooldowns. It can never remove denylist entries or lower confirmation requirements (the engine enforces this).
- Built-in gesture parameters in the catalog are defaults only. User overrides (`gestures.params`) always win.
- Feature flags (`flags.<name>: bool`) are evaluated locally. They can hide or enable a UI surface or engine path. There is no remote kill of local data.

### 5.4 On-demand packs
Large or optional models (OWLv2, Florence-2, SAM 2.1 tiny, Depth Anything V2 Small, DINOv2-small, pose lite, Motion Embedder):
- downloaded only when a feature needs them (e.g. the first time tap-to-teach is used);
- with size and license shown before download;
- removable in Settings → Updates → Storage.

## 6. Offline & air-gapped
- Flick works indefinitely with no network. Updates are simply not checked, and a warning appears after 14 days only when the app is outdated **and** the channel index is stale.
- **Manual import:** Settings → Updates → *Import update file* accepts `.flickupdate`, a signed tarball with the index + packs.
  - It goes through the same verification pipeline.
  - For headless boxes: `flickd update import <file>`.

## 7. Hosting & CI
- **Primary:** Azure Blob Storage (static, versioned, immutable blobs for `app/` and `packs/`) behind **Azure Front Door** (CDN, TLS, custom domain). IaC: `infra/updates.bicep`.
  - Cost note: Front Door Standard has a monthly base fee.
  - Cheaper alternatives: plain Blob static website + Azure CDN, or Cloudflare R2 (zero egress). Decide in P0-09.
- **Mirror:** GitHub Releases for app artifacts (the Tauri updater's fallback endpoint list).
- **CI (GitHub Actions):**
  - `release.yml`:
    - build/sign/notarize (macOS) and Artifact Signing (Windows);
    - generate `latest.json`;
    - upload to Blob + GitHub Releases;
    - sign and update `channels/nightly` (then beta/stable through promotion).
  - `models.yml`:
    - convert/quantize (Azure ML job or runner);
    - run golden + benchmark tests on macOS/Windows/Linux runners;
    - build and sign the pack;
    - publish to nightly.
  - **Promotion = a metadata change** (bump `version`, add the entry to the `beta`/`stable` index, set `rollout_pct`). A human approves it through a GitHub environment.
- **Local testing:** `cargo xtask serve-updates --dir ./target/updates` runs a static server with test keys, so the app is pointed at `http://localhost:7880` in dev builds (`FLICK_UPDATE_URL`).

## 8. Settings & API
- Settings keys:

  | Key | Default |
  |---|---|
  | `updates.channel` | `"stable"` |
  | `updates.auto_install_app` | `true` |
  | `updates.auto_models` | `true` |
  | `updates.install_when_idle` | `true` |
  | `updates.install_id` | ULID, generated, read-only |

- REST: `GET /updates` (state), `POST /updates/check`, `POST /updates/install`, `POST /updates/rollback`, `POST /updates/import`; `GET /models`, `POST /models/{pack_id}/install` and `DELETE /models/{pack_id}` (both on-demand only). See `06-…` §5.
- WS: `update.available`, `update.progress`, `update.ready`, `model.activated`, `model.rolled_back`. See `06-…` §6.

## 9. Privacy
- The update check sends only: the requested channel file path, and the app version + target in the URL (needed by the Tauri endpoint). No `install_id`, no identifiers, no telemetry.
- Rollout bucketing happens on the device.
- Opt-in crash reports are a separate setting (`07-…` §4).

## 10. Acceptance tests
Only the security-critical and rollback paths are automated (`07-…` §1.1):
- **One table test over signed metadata/packs** (`crates/flick-update/tests/`): tampered pack (1 byte changed), index with a lower `version`, index past `expires`, and a catalog trying to remove `lock` from the denylist → each is rejected, the current pack stays active and the UI reason is set.
- **Self-test:** a pack with a planted regression (golden fixture) → rejected.
- **Watchdog:** a simulated latency regression after activation → auto-rollback.
- **Hot swap:** under a replayed 30 fps stream → 0 dropped frames, and no action fires twice.
- **App update** (`release-dryrun.yml`, before each release tag): N−1 → N with a DB migration; a simulated health-check failure → rollback restores the old version and the DB.

Manual checks only (no dedicated tests): rollout bucketing (a pure hash of `install_id`; reviewed once), and the air-gapped `.flickupdate` import (it uses the same verify path as online packs).
