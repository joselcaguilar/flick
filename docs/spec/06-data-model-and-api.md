# 06 — Data model & engine API

Crates: `flick-store` (SQLite), `flick-api` (axum + utoipa).
The OpenAPI document is generated at build time; `cargo xtask gen-api` writes `ui/src/api/schema.d.ts`.
CI fails if the generated file is out of date.

## 1. Conventions
- **IDs:** plain ULID strings for user entities, no type prefixes.
  Fixed string IDs for built-ins (`builtin.thumb_up`, `system.none`). Custom gestures: `custom.<ulid>`; custom motion: `motion.<ulid>`.
- **Time:** SQLite stores `INTEGER` unix epoch **milliseconds** (UTC). The API uses RFC 3339 strings.
- **JSON columns:** `TEXT` containing JSON, validated by serde on write.
- **Float vectors:** `BLOB`, little-endian `f32`, fixed length:
  - hand embedding: 128;
  - landmarks: 21×3 = 63 per set;
  - scene signature (DINOv2-small): 384;
  - 3D vectors: 3;
  - covariance: 9, row-major.
- **Geometry:** camera frame, meters (x right, y down, z forward), as in `09-…` §3.
- **Errors:** RFC 9457 `application/problem+json`: `{ "type", "title", "status", "detail", "code" }`, where `code` is a stable snake_case string (e.g. `mapping_sensitive_not_allowed`).

## 2. Files & locations

| Item | Path (desktop; headless overridable via `--data-dir` / `FLICK_DATA_DIR`) |
|---|---|
| Data dir | Tauri app data dir for identifier `app.flick.desktop`. macOS: `~/Library/Application Support/app.flick.desktop/`; Windows: `%APPDATA%\app.flick.desktop\`; Linux: `~/.local/share/app.flick.desktop/` |
| Database | `<data>/flick.db` (WAL mode, `foreign_keys=ON`, `busy_timeout=5000`) |
| Bootstrap config | `<data>/config.toml` |
| Logs | `<data>/logs/flick-engine.YYYY-MM-DD.log` (JSON lines, 7-day rotation) |
| Model packs | `<data>/packs/<pack_id>/<version>/` (active + previous kept; CoreML compiled cache under `coreml/`). Bundled packs are read from app resources until an OTA pack replaces them (`10-…` §5) |
| Update staging | `<data>/updates/` (downloads being verified; cleared after activation/rejection) |
| DB snapshots | `<data>/backups/flick-<version>-<ts>.db` (last 3 kept; `10-…` §4.3) |
| Secrets | OS keychain (service `app.flick.desktop`), never in files |

### 2.1 `config.toml` (bootstrap-only; user-facing settings live in SQLite `settings`)
```toml
[engine]
bind = "127.0.0.1"         # headless LAN mode requires explicit "0.0.0.0" + pairing (Phase 3)
port = 0                    # 0 = random (desktop sidecar). Headless default: 7870
log_level = "info"          # trace|debug|info|warn|error; env FLICK_LOG overrides

[inference]
execution_provider = "auto" # auto|coreml|directml|cuda|openvino|xnnpack|cpu
intra_threads = 2

[paths]
models_dir = ""             # empty = bundled resources
```
Environment overrides: `FLICK_DATA_DIR`, `FLICK_LOG`, `FLICK_PORT`, `FLICK_FAKE_CAMERA`, `FLICK_FAKE_LANDMARKS` (replay a `.jsonl` landmark fixture instead of running vision), `FLICK_EP`, `FLICK_UPDATE_URL` (dev/test builds only).

## 3. SQLite schema (migration `0001_init.sql`)

```sql
CREATE TABLE settings (
  key         TEXT PRIMARY KEY,           -- e.g. 'detection.sensitivity'
  value       TEXT NOT NULL,              -- JSON
  updated_at  INTEGER NOT NULL
);

CREATE TABLE ha_instances (
  id            TEXT PRIMARY KEY,         -- ULID
  name          TEXT NOT NULL,            -- location_name
  base_url      TEXT NOT NULL,            -- http(s)://host:port
  ha_uuid       TEXT UNIQUE,              -- from mDNS/get_config
  auth_kind     TEXT NOT NULL CHECK (auth_kind IN ('llat','oauth')),
  keychain_ref  TEXT NOT NULL,            -- 'ha:<uuid or id>'
  cert_sha256   TEXT,                     -- pinned self-signed cert (optional)
  ha_version    TEXT,
  is_default    INTEGER NOT NULL DEFAULT 1,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE ha_cache (
  ha_id       TEXT NOT NULL REFERENCES ha_instances(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,              -- 'states'|'services'|'areas'|'floors'|'labels'|'devices'|'entities'
  payload     TEXT NOT NULL,              -- JSON snapshot
  fetched_at  INTEGER NOT NULL,
  PRIMARY KEY (ha_id, kind)
);

CREATE TABLE cameras (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('local','rtsp','file')),
  device_ref    TEXT,                     -- local: stable unique id from OS; file: path
  url_redacted  TEXT,                     -- rtsp only, e.g. rtsps://10.0.0.1:7441/****
  keychain_ref  TEXT,                     -- rtsp: full URL/credentials in keychain
  trust_self_signed INTEGER NOT NULL DEFAULT 0,
  enabled       INTEGER NOT NULL DEFAULT 1,
  mirror        INTEGER NOT NULL DEFAULT 1,
  rotation      INTEGER NOT NULL DEFAULT 0 CHECK (rotation IN (0,90,180,270)),
  active_fps    INTEGER NOT NULL DEFAULT 30,
  idle_fps      INTEGER NOT NULL DEFAULT 5,
  max_hands     INTEGER NOT NULL DEFAULT 2 CHECK (max_hands IN (1,2)),
  roi           TEXT,                     -- JSON {x,y,w,h} normalized, optional
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE gestures (
  id              TEXT PRIMARY KEY,       -- builtin.* | custom.<ulid> | motion.<ulid> | system.none
  source          TEXT NOT NULL CHECK (source IN ('builtin','custom','pack','system')),
  kind            TEXT NOT NULL CHECK (kind IN ('static','motion','dial','negative')),
  hands_required  INTEGER NOT NULL DEFAULT 1 CHECK (hands_required IN (1,2)),  -- 2 = two-hand gesture
  name            TEXT NOT NULL,
  icon            TEXT,                   -- icon key or emoji
  hand_constraint TEXT NOT NULL DEFAULT 'any' CHECK (hand_constraint IN ('any','left','right')),
  threshold       REAL,                   -- NULL = default for tier
  enabled         INTEGER NOT NULL DEFAULT 1,
  pro             INTEGER NOT NULL DEFAULT 0,   -- reserved for Pro far-field body gestures (Phase 4)
  pack_id         TEXT,                   -- origin pack, if imported
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
);
-- Built-ins are seeded by migration and cannot be deleted (only disabled).

CREATE TABLE capture_sessions (
  id          TEXT PRIMARY KEY,
  gesture_id  TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  camera_id   TEXT REFERENCES cameras(id) ON DELETE SET NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('positive','negative')),
  target_takes INTEGER NOT NULL,
  status      TEXT NOT NULL CHECK (status IN ('running','completed','cancelled','failed')),
  created_at  INTEGER NOT NULL
);

CREATE TABLE gesture_samples (
  id               TEXT PRIMARY KEY,
  gesture_id       TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  session_id       TEXT REFERENCES capture_sessions(id) ON DELETE SET NULL,
  take_index       INTEGER NOT NULL,
  hand             TEXT NOT NULL CHECK (hand IN ('left','right')),
  handedness_score REAL NOT NULL,
  landmarks_image  BLOB NOT NULL,         -- 63 × f32
  landmarks_world  BLOB NOT NULL,         -- 63 × f32
  quality          REAL NOT NULL,         -- 0..1
  created_at       INTEGER NOT NULL
);
CREATE INDEX idx_samples_gesture ON gesture_samples(gesture_id);

CREATE TABLE sample_embeddings (         -- derived cache (ADR-013); safe to delete
  sample_id        TEXT NOT NULL REFERENCES gesture_samples(id) ON DELETE CASCADE,
  embedder_version TEXT NOT NULL,
  augmentation     INTEGER NOT NULL,      -- 0 = original, 1..n = augmented variant
  embedding        BLOB NOT NULL,         -- 128 × f32
  PRIMARY KEY (sample_id, embedder_version, augmentation)
);

CREATE TABLE motion_takes (              -- raw source of truth for motion / two-hand gestures (ADR-013)
  id            TEXT PRIMARY KEY,
  gesture_id    TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  session_id    TEXT REFERENCES capture_sessions(id) ON DELETE SET NULL,
  take_index    INTEGER NOT NULL,
  hands         INTEGER NOT NULL CHECK (hands IN (1,2)),
  frame_count   INTEGER NOT NULL,
  t_ms          BLOB NOT NULL,            -- frame_count × u32, ms since take start
  landmarks_image BLOB NOT NULL,          -- frame_count × hands × 63 × f32
  landmarks_world BLOB NOT NULL,          -- frame_count × hands × 63 × f32
  quality       REAL NOT NULL,
  created_at    INTEGER NOT NULL
);
CREATE INDEX idx_motion_takes_gesture ON motion_takes(gesture_id);

CREATE TABLE motion_templates (          -- derived from motion_takes (Core); safe to rebuild
  id               TEXT PRIMARY KEY,
  gesture_id       TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  take_id          TEXT NOT NULL REFERENCES motion_takes(id) ON DELETE CASCADE,
  channels         TEXT NOT NULL CHECK (channels IN ('single','two_hand')),
  features_version TEXT NOT NULL,         -- e.g. 'dtw32.v1' or 'motion_embedder@1'
  trajectory       BLOB NOT NULL,         -- 32 × channel dims × f32 (02-… §4.4)
  threshold        REAL NOT NULL,         -- per-template DTW threshold
  created_at       INTEGER NOT NULL
);

CREATE TABLE classifier_models (
  id               TEXT PRIMARY KEY,
  algorithm        TEXT NOT NULL CHECK (algorithm IN ('proto_knn','logreg')),
  embedder_version TEXT NOT NULL,
  params           BLOB NOT NULL,         -- bincode-serialized model
  metrics          TEXT NOT NULL,         -- JSON {loto_accuracy, per_class:{…}, confusions:[…]}
  active           INTEGER NOT NULL DEFAULT 0,
  trained_at       INTEGER NOT NULL
);

CREATE TABLE mappings (
  id                 TEXT PRIMARY KEY,
  name               TEXT NOT NULL,
  enabled            INTEGER NOT NULL DEFAULT 1,
  gesture_id         TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  hand               TEXT NOT NULL DEFAULT 'any' CHECK (hand IN ('any','left','right')),
  allow_two_hands    INTEGER NOT NULL DEFAULT 0,
  camera_ids         TEXT NOT NULL DEFAULT '[]',  -- JSON array; [] = all cameras
  zone_id            TEXT,                         -- Pro
  target_mode        TEXT NOT NULL DEFAULT 'global' CHECK (target_mode IN ('global','anchor','domain')),
  anchor_id          TEXT REFERENCES anchors(id) ON DELETE CASCADE,  -- target_mode = 'anchor'
  target_domain      TEXT,                         -- target_mode = 'domain' ("any selected fan")
  mode               TEXT NOT NULL DEFAULT 'tap' CHECK (mode IN ('tap','hold','repeat','dial')),
  hold_ms            INTEGER NOT NULL DEFAULT 800,
  repeat_ms          INTEGER NOT NULL DEFAULT 400,
  cooldown_ms        INTEGER NOT NULL DEFAULT 1000,
  require_armed      INTEGER NOT NULL DEFAULT 0,
  active_hours       TEXT,                         -- JSON {"from":"07:00","to":"23:30","days":[1..7]} or NULL
  action             TEXT NOT NULL,                -- JSON Action (§4)
  sensitive          INTEGER NOT NULL DEFAULT 0,   -- computed by validator
  sensitive_ack      INTEGER NOT NULL DEFAULT 0,
  confirm_gesture_id TEXT REFERENCES gestures(id),
  feedback           TEXT NOT NULL DEFAULT '{"hud":true,"sound":true}',
  sort_order         INTEGER NOT NULL DEFAULT 0,
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL,
  CHECK ((target_mode = 'global' AND anchor_id IS NULL AND target_domain IS NULL)
      OR (target_mode = 'anchor' AND anchor_id IS NOT NULL)
      OR (target_mode = 'domain' AND target_domain IS NOT NULL))
);
CREATE INDEX idx_mappings_gesture ON mappings(gesture_id) WHERE enabled = 1;
CREATE INDEX idx_mappings_anchor ON mappings(anchor_id) WHERE anchor_id IS NOT NULL;
-- Targeted mappings (anchor/domain) must use action.kind 'verb' or a 'dial' on "$selected" (§4).

CREATE TABLE activity_log (
  id             TEXT PRIMARY KEY,         -- ULID (time-sortable)
  ts             INTEGER NOT NULL,
  camera_id      TEXT,
  gesture_id     TEXT,
  hand           TEXT,
  confidence     REAL,
  mapping_id     TEXT,
  anchor_id      TEXT,                     -- selected device, if any
  action_summary TEXT,                     -- "light.toggle → Living room"
  status         TEXT NOT NULL CHECK (status IN ('fired','sent','ok','error','timeout','stale','suppressed')),
  reason         TEXT,                     -- suppression reason (02-… §5, incl. target_selected|no_target|ambiguous_target|needs_realign) or error code
  message        TEXT,
  ha_context_id  TEXT,
  latency        TEXT                      -- JSON {"detect_ms":..,"dispatch_ms":..,"ha_ms":..}
);
CREATE INDEX idx_activity_ts ON activity_log(ts DESC);
-- Retention: keep 7 days or 10 000 rows (whichever is smaller); pruned hourly.
-- 'suppressed' rows are written only while "Log ignored gestures" is on (settings.debug.log_suppressed, default off); when HA is not configured, actions are always logged as 'suppressed'.

CREATE TABLE gesture_packs (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  author      TEXT,
  version     TEXT,
  imported_at INTEGER NOT NULL,
  manifest    TEXT NOT NULL                -- original pack header JSON
);

-- ── Device targeting (09-…) ─────────────────────────────────────────────
CREATE TABLE places (
  id               TEXT PRIMARY KEY,
  camera_id        TEXT NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
  name             TEXT NOT NULL,          -- "Bedroom desk"
  scene_signature  BLOB NOT NULL,          -- 384 × f32, person/hand regions masked; never an image
  embedder_version TEXT NOT NULL,          -- e.g. 'dinov2-small@1'
  intrinsics       TEXT NOT NULL,          -- JSON {hfov_deg, cx, cy, source:'fov_table'|'default', version}
  status           TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','needs_realign')),
  active           INTEGER NOT NULL DEFAULT 0,  -- one active place per camera
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_places_active ON places(camera_id) WHERE active = 1;

CREATE TABLE anchors (
  id                TEXT PRIMARY KEY,
  place_id          TEXT NOT NULL REFERENCES places(id) ON DELETE CASCADE,
  name              TEXT NOT NULL,         -- defaults to the HA friendly name
  target            TEXT NOT NULL,         -- JSON {entity_id?|device_id?|area_id?}; exactly one
  domain            TEXT NOT NULL,         -- resolution domain for verbs (fan, light, …)
  kind              TEXT NOT NULL CHECK (kind IN ('point3d','direction','region2d')),
  position          BLOB,                  -- 3 × f32 (point3d / region2d)
  direction         BLOB,                  -- 3 × f32 unit (direction)
  teach_origin      BLOB,                  -- 3 × f32 (direction kind validity check)
  covariance        BLOB,                  -- 9 × f32
  uncertainty_deg   REAL NOT NULL,         -- widens hover tolerance (09-… §2)
  verb_params       TEXT NOT NULL DEFAULT '{}',  -- JSON e.g. {"levels":[1]} (03-… §6.3)
  sensitive         INTEGER NOT NULL DEFAULT 0,  -- computed by validator
  sensitive_ack     INTEGER NOT NULL DEFAULT 0,
  estimator_version TEXT NOT NULL,         -- ray + triangulation version; recompute when it changes
  status            TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','needs_realign','needs_reteach')),
  last_used_at      INTEGER,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE INDEX idx_anchors_place ON anchors(place_id);

CREATE TABLE anchor_observations (       -- raw teaching data; anchors are derived from these
  id                TEXT PRIMARY KEY,
  anchor_id         TEXT NOT NULL REFERENCES anchors(id) ON DELETE CASCADE,
  spot_index        INTEGER NOT NULL,
  frames            INTEGER NOT NULL,      -- frames in the 1 s capture
  hand              TEXT NOT NULL CHECK (hand IN ('left','right')),
  landmarks_image   BLOB NOT NULL,         -- frames × 63 × f32
  landmarks_world   BLOB NOT NULL,         -- frames × 63 × f32
  face_keypoints    BLOB,                  -- frames × 6 × 2 × f32 (BlazeFace), NULL if no face
  intrinsics_version TEXT NOT NULL,
  created_at        INTEGER NOT NULL
);

-- ── OTA (10-…) ──────────────────────────────────────────────────────────
CREATE TABLE model_packs (
  id            TEXT NOT NULL,             -- 'hand-landmarker', 'catalog', 'owlv2-base', …
  version       TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('model','catalog')),
  state         TEXT NOT NULL CHECK (state IN ('staged','shadow','active','previous','rejected','removed')),
  provides      TEXT NOT NULL,             -- JSON ["hand_landmarker@2"]
  sha256        TEXT NOT NULL,
  source        TEXT NOT NULL CHECK (source IN ('bundled','ota','import')),
  on_demand     INTEGER NOT NULL DEFAULT 0,
  reject_reason TEXT,                      -- e.g. 'golden_regression', 'latency_regression', 'signature'
  installed_at  INTEGER NOT NULL,
  activated_at  INTEGER,
  PRIMARY KEY (id, version)
);
CREATE UNIQUE INDEX idx_packs_active ON model_packs(id) WHERE state = 'active';

CREATE TABLE update_state (              -- small key/value table owned by flick-update
  key         TEXT PRIMARY KEY,            -- 'index_version:<channel>' | 'pending_app' | 'rollback_to' | 'last_check_at' | 'watchdog:<pack_id>'
  value       TEXT NOT NULL,               -- JSON
  updated_at  INTEGER NOT NULL
);
```

### 3.1 Settings keys (JSON values, defaults)

| Key | Default |
|---|---|
| `detection.sensitivity` | `"normal"` (`low` \| `normal` \| `high`) |
| `detection.vote` | `{"n":6,"m":8,"selected_n":3}` (`selected_n` applies to targeted/either verbs while a device is selected) |
| `detection.min_hand_size` | `0.06` |
| `detection.battery_saver` | `false` |
| `detection.arm` | `{"enabled":false,"gesture":"builtin.open_palm","hold_ms":600,"window_ms":4000}` |
| `detection.pause_gesture` | `{"enabled":false,"gesture":"builtin.i_love_you","hold_ms":1500}` |
| `safety.allow_sensitive` | `false` |
| `safety.confirm_gesture` | `"builtin.thumb_up"` |
| `privacy.pause_when_ha_offline` | `true` (camera stops when HA is unreachable > 60 s, e.g. laptop left home) |
| `privacy.pause_on_screen_lock` | `false` |
| `privacy.keep_awake` | `false` (power assertion while watching, for stationary Macs) |
| `quiet_hours` | `null` or `{"from":"23:00","to":"07:00"}` |
| `feedback.hud` | `{"enabled":true,"position":"top-center","duration_ms":1500}` |
| `feedback.sounds` | `{"enabled":true,"volume":0.6}` |
| `ui.theme` | `"system"` |
| `inference.ep_choice` | set by auto-benchmark |
| `debug.log_suppressed` | `false` (Activity → "Log ignored gestures") |
| `onboarding.completed` | `false` |
| `gestures.params` | `{}` (user overrides of built-in parameters; always win over catalog defaults, `10-…` §5.3) |
| `gestures.two_hand_separate.axis` | `"any"` (`any` \| `vertical` \| `horizontal`) |
| `targeting.enabled` | `true` (effective only when ≥ 1 anchor exists on the camera's active place) |
| `targeting.tolerance_deg` | `10` (5–15) |
| `targeting.dwell_ms` | `300` (the runtime currently uses the built-in selector default) |
| `targeting.window_ms` | `4000` |
| `targeting.ray_model` | `"auto"` (`auto` \| `eye` \| `finger`; `arm` in Phase 2) |
| `targeting.dominant_eye` | `"center"` (`center` \| `left` \| `right`) |
| `updates.channel` | `"stable"` (`stable` \| `beta` \| `nightly`) |
| `updates.auto_install_app` | `true` |
| `updates.auto_models` | `true` |
| `updates.install_when_idle` | `true` |
| `updates.install_id` | ULID generated at first run; read-only; never sent (`10-…` §3) |
| `privacy.crash_reports` | `false` (opt-in; `07-…` §4) |
| `cloud.setup_assistant` | `{"enabled":false,"provider":null}` (opt-in, Phase 2; the key lives in the keychain, `11-…` §3) |

## 4. Action JSON (in `mappings.action`)

```jsonc
// kind = call_service
{ "kind": "call_service", "domain": "media_player", "service": "media_next_track",
  "target": { "entity_id": ["media_player.living_room"] }, "data": {} , "preset": "media.next" }

// kind = dial
{ "kind": "dial", "entity_id": "light.living_room", "property": "brightness_pct",   // brightness_pct|volume_level|position|percentage|temperature
  "gain": 1.0, "min": 1, "max": 100 }

// kind = verb (targeted mappings only; resolved against the selected anchor, 03-… §6.3)
{ "kind": "verb", "verb": "up" }                      // up|down|on|off|stop|toggle|level_set
{ "kind": "verb", "verb": "level_set", "level": 1 }   // level is 1-based into anchors.verb_params.levels

// kind = dial on the selected device (targeted mappings only)
{ "kind": "dial", "entity_id": "$selected", "property": "percentage", "gain": 1.0 }
```
- `preset` is optional UI metadata (see `03-…` §6.1). The dispatcher ignores it.
- **Validation:**
  - `verb` and `$selected` are rejected on global mappings (`422 verb_requires_target`).
  - `level_set` needs `level ≤ len(levels)` for anchor mappings (`422 level_not_taught`).
  - Verbs are checked against the anchor's `supported_features` (`03-…` §6.3).

## 5. REST API (`/api/v1`)

**Auth:**
- Every request needs `Authorization: Bearer <token>`.
- `Host` must be `127.0.0.1:<port>` or `localhost:<port>` (DNS-rebinding protection).
- CORS allows only `tauri://localhost`, `http://tauri.localhost`, and the engine's own origin.

| Method & path | Body → Response | Notes |
|---|---|---|
| `GET /health` | → `{status, version, uptime_s}` | No auth (used by the supervisor); returns minimal info |
| `GET /status` | → `EngineStatus` | cameras, fps, stage p50/p95, HA state, paused |
| `POST /engine/pause` | `{duration_s?: number}` → `EngineStatus` | null = until resumed |
| `POST /engine/resume` | → `EngineStatus` | |
| `GET /settings` / `PATCH /settings` | partial JSON map → full map | validated per key |
| `GET /ha/discover` | → `[{name, base_url, uuid, version}]` | mDNS browse, 3 s |
| `POST /ha/connect` | `{base_url, token, trust_cert_sha256?}` → `HaInstance` | verifies before saving |
| `GET /ha/status` | → `{state, ha_version, instance}` | |
| `DELETE /ha` | → 204 | removes token from keychain |
| `GET /ha/areas` | → `[{area_id, name, floor_id}]` | |
| `GET /ha/entities?domain=&area_id=&q=` | → `[{entity_id, name, domain, area_id, device_id, state, device_class}]` | from cache |
| `GET /ha/services?domain=` | → service schema subset | |
| `POST /ha/call` | `Action` → `ActionOutcome` | "Test" buttons; same safety rules |
| `GET /cameras/available` | → `[{device_ref, name, kind:"local", formats}]` | |
| `GET /cameras` / `POST /cameras` | `CameraCreate` → `Camera` | enforces `Entitlements.max_cameras` (`402 camera_limit` in Core) |
| `PATCH /cameras/{id}` / `DELETE /cameras/{id}` | | |
| `POST /cameras/{id}/start` / `stop` | → `CameraStatus` | |
| `POST /cameras/test-rtsp` | `{url, trust_self_signed}` → `{ok, width, height, codec, latency_ms, error?}` | Phase 2 |
| `POST /cameras/{id}/preview-ticket` | → `{url: "/stream/{id}.mjpg?ticket=…", expires_at}` | one-time ticket, 60 s. The stream is `multipart/x-mixed-replace`; append `&framing=raw` to get the same bytes as `application/octet-stream` (needed by WKWebView `fetch`) |
| `GET /gestures` | → `Gesture[]` with `{sample_count, accuracy?, distinctiveness?}` | |
| `POST /gestures` | `{name, kind?:"static"|"motion", hands_required?, hand_constraint, icon?}` → `Gesture` | creates `custom.<ulid>` (static) or `motion.<ulid>`. If `kind` is omitted, the type is auto-detected from the first capture (`02-…` §4.4) and the id is assigned then |
| `PATCH /gestures/{id}` / `DELETE /gestures/{id}` | | built-ins: only `enabled`, `threshold` |
| `POST /gestures/{id}/capture` | `{camera_id, kind:"positive"|"negative", takes: 1..20, take_ms: 1500}` → `CaptureSession` | progress via WS. Motion: `take_ms` ≤ 2000 |
| `GET /gestures/{id}/motion-takes` | → `MotionTake[]` (normalized trajectories only, for glyphs) | |
| `PATCH /gestures/{id}/type` | `{kind, hands_required}` → `Gesture` | user override of the auto-detected type; rebuilds templates |
| `POST /capture/{session_id}/cancel` | → 204 | |
| `GET /gestures/{id}/samples` | → `Sample[]` (landmarks only, for thumbnails) | |
| `DELETE /gestures/{id}/samples/{sample_id}` | → 204 | |
| `POST /classifier/train` | `{algorithm?: "proto_knn"|"logreg"}` → `ClassifierReport` | < 1 s; activates new model. Also rebuilds motion templates of changed gestures |
| `GET /classifier` | → `ClassifierReport` | |
| `GET /mappings` / `POST /mappings` | `MappingCreate` → `Mapping` | `422` on safety violations |
| `PATCH /mappings/{id}` / `DELETE /mappings/{id}` | | |
| `POST /mappings/{id}/test` | → `ActionOutcome` | fires the action now |
| `PUT /mappings/order` | `[id…]` → 204 | |
| `GET /activity?limit=50&before=<ulid>&status=&include_suppressed=` | → `{items, next_before}` | `limit` 1–200 (default 50, else 422 `invalid_limit`); `before` is the cursor id (empty = none); `status` ∈ fired\|sent\|ok\|error\|timeout\|stale\|suppressed (empty = all, else 422 `invalid_status`); `include_suppressed` = `true`\|`1` (implied by `status=suppressed`), otherwise suppressed rows are hidden. Items are newest first (`ts DESC, id DESC`) and include `camera_id`, `confidence`; `next_before` is the last id when the page is full. 500 `activity_store_failed` |
| `GET /places?camera_id=` | → `Place[]` (no signature bytes) | |
| `PATCH /places/{id}` / `DELETE /places/{id}` | `{name}` | delete cascades anchors + targeted mappings (UI confirms) |
| `POST /places/{id}/realign` | → `RealignSession {id, prompts:[anchor_id…]}` | `09-…` §6 |
| `POST /realign/{session_id}/point` | `{anchor_id}` → `{captured, residual_deg?}` | 1 s capture |
| `POST /realign/{session_id}/commit` | → `{applied, residual_deg, needs_reteach:[anchor_id]}` | |
| `GET /anchors?place_id=` | → `Anchor[]` with `{status, verbs, last_used_at}` | |
| `PATCH /anchors/{id}` / `DELETE /anchors/{id}` | `{name?, verb_params?}` | delete cascades its mappings |
| `POST /anchors/{id}/test` | → `{selected, angular_error_deg, runner_up?}` | waits ≤ 5 s for the next point |
| `POST /teach` | `{camera_id, target:{entity_id|device_id|area_id}, anchor_id?}` → `TeachSession` | `anchor_id` = re-teach. Creates the place on first use |
| `POST /teach/{session_id}/spot` | → `{spot_index, ray_jitter_deg, confidence, kind:"direction"|"point3d", residual_deg?}` | 1 s capture; progress via `teach.progress` |
| `POST /teach/{session_id}/levels/use-current` | `{level}` → `{levels, current_percentage}` | `03-…` §6.3.1 |
| `POST /teach/{session_id}/levels/test` | `{level}` → `ActionOutcome` | real call |
| `POST /teach/{session_id}/commit` | `{name?, verbs:[{gesture_id, action}]}` → `{anchor, mapping_ids, distinctiveness_warnings[]}` | creates the anchor + targeted mappings |
| `POST /teach/{session_id}/cancel` | → 204 | discards observations |
| `POST /setup-assistant/suggest` | `{camera_id, provider:"local"|"foundry"}` → `[{label, bbox, candidates:[entity_id]}]` | Phase 2; `foundry` requires consent (`11-…` §3) |
| `GET /updates` | → `{channel, app:{current, available?, state, rollback_to?}, packs:[…], index_version, last_check_at, busy_reason?}` | `busy_reason`: `confirm_pending`/`recording`/`teaching`/`dial` |
| `POST /updates/check` | → `UpdateState` | |
| `POST /updates/install` | `{kind:"app"|"pack", id?}` → 202 | app installs are executed by the shell (`05-…` §1.1) |
| `POST /updates/rollback` | `{kind:"app"|"pack", id?}` → 202 | |
| `POST /updates/import` | `multipart .flickupdate` → `{accepted:[…], rejected:[{id, reason}]}` | `10-…` §6 |
| `GET /models` | → `ModelPack[]` | |
| `POST /models/{pack_id}/install` | → 202 | on-demand packs only (`10-…` §5.4) |
| `DELETE /models/{pack_id}` | → 204 | on-demand packs only |
| `POST /packs/export` | `{gesture_ids[], include_mapping_templates}` → `GesturePack` JSON (download) | |
| `POST /packs/import/preview` | `GesturePack` → `{gestures:[{id, name, conflict?}], mapping_templates:[{slots}]}` | |
| `POST /packs/import/commit` | `{pack, choices}` → `{imported_gesture_ids}` | re-embeds + retrains |
| `GET /license` / `POST /license` | `{key}` → `License` | Phase 3 |
| `GET /diagnostics/bundle` | → `application/zip` | redacted logs + config + perf stats |
| `GET /openapi.json` | → OpenAPI 3.1 | |

## 6. WebSocket events (`GET /api/v1/events`)

- **Auth:** `new WebSocket(url, ["flick.v1", "bearer." + token])`. The server echoes `flick.v1`. Origin check as for CORS.
- **Client → server:**
  ```json
  {"type":"subscribe","topics":["status","gestures","actions","ha","capture","targeting","teach","updates","hands:<camera_id>"]}
  {"type":"unsubscribe","topics":["hands:<camera_id>"]}
  ```
  `hands:*` (landmark stream) is opt-in and throttled to **15 Hz**. Everything else is pushed as it happens.
- **Server → client** (all messages carry `"ts"` RFC 3339):

| `type` | Payload | Topic |
|---|---|---|
| `hello` | `{engine_version, api:"v1", pro:boolean}` | always |
| `engine.status` | `EngineStatus` (1 Hz) | `status` |
| `camera.status` | `{camera_id, state:"starting"|"running"|"idle"|"reconnecting"|"error"|"stopped", fps, error?}` | `status` |
| `hands` | `{camera_id, seq, hands:[{track_id, hand, landmarks:[[x,y,z]×21], bbox}], ray?:{origin2d, tip2d, model}}` | `hands:<id>` |
| `gesture.candidate` | `{camera_id, track_id, gesture_id, confidence, progress:0..1}` | `gestures` |
| `gesture.suppressed` | `{camera_id, gesture_id, reason}` | `gestures` (only if `debug.log_suppressed`, i.e. "Log ignored gestures") |
| `gesture.fired` | `{event_id, camera_id, gesture_id, hand, confidence, mapping_ids[], action_summary}` | `gestures` |
| `gesture.update` / `gesture.end` | `{event_id, value?}` (dial) | `gestures` |
| `armed` | `{until}` / `disarmed` | `gestures` |
| `confirm.required` | `{event_id, mapping_id, confirm_gesture_id, expires_at}` | `actions` |
| `action.result` | `{activity_id, event_id, mapping_id, status, error_code?, message?, latency}` | `actions` |
| `ha.status` | `{state:"disconnected"|"connecting"|"ready"|"auth_failed", ha_version?}` (on every connection change) | `ha` |
| `ha.entity` | `{entity_id, state, attributes}` (subscribed entities only) | `ha` |
| `capture.progress` | `{session_id, take, takes, phase:"countdown"|"recording"|"review"|"done", quality_hint?}` | `capture` |
| `engine.paused` / `engine.resumed` | `{until?}` | `status` |
| `target.hover` | `{camera_id, anchor_id, name, score, dwell_progress:0..1, runner_up?}` | `targeting` |
| `target.selected` | `{camera_id, anchor_id, name, domain, expires_at, verbs:[{gesture_id, label}]}` | `targeting` |
| `target.cleared` | `{camera_id, anchor_id, reason:"timeout"|"hand_lost"|"reselected"|"paused"}` | `targeting` |
| `target.ambiguous` | `{camera_id, anchor_ids:[a,b]}` | `targeting` |
| `place.status` | `{camera_id, place_id, state:"ok"|"switched"|"needs_realign", similarity}` | `targeting` |
| `teach.progress` | `{session_id, phase:"aiming"|"capturing"|"captured"|"error", ray_jitter_deg?, confidence?, hint?}` | `teach` |
| `update.available` | `{kind:"app"|"pack", id, version, size}` | `updates` |
| `update.progress` | `{kind, id, phase:"download"|"verify"|"selftest"|"benchmark"|"shadow", bytes?, total?}` | `updates` |
| `update.ready` | `{kind:"app", version}` (restart needed) | `updates` |
| `model.activated` | `{pack_id, version, previous}` | `updates` |
| `model.rolled_back` | `{pack_id, from, to, reason}` | `updates` |

WS event payload schemas are included as OpenAPI components (`WsServerMessage` one-of), so TypeScript types come from the same generator.

## 7. Gesture pack format (`.flickpack.json`)

```jsonc
{
  "format": "flick.gesture-pack",
  "version": 1,
  "name": "Couch essentials",
  "author": "riley",
  "created_at": "2026-10-01T12:00:00Z",
  "landmark_model": { "id": "mediapipe.hand_landmark_full", "version": "<manifest version>" },
  "gestures": [
    {
      "id": "custom.01JABC…",          // re-keyed on import if it collides
      "name": "Rock on", "kind": "static", "hand_constraint": "any", "icon": "🤘", "threshold": 0.75,
      "samples": [
        { "hand": "right", "handedness_score": 0.97,
          "landmarks_image": [[0.51,0.62,0.0], "... 21 total"],
          "landmarks_world": [[0.01,0.07,0.02], "... 21 total"] }
      ]
    }
  ],
  "motion_takes": [],                 // Core: raw motion / two-hand takes (landmark sequences); templates are rebuilt on import
  "mapping_templates": [
    { "name": "Rock on → party scene", "gesture_id": "custom.01JABC…", "mode": "tap",
      "action": { "kind": "call_service", "domain": "scene", "service": "turn_on",
                  "target": { "entity_id": ["${scene}"] }, "data": {} },
      "slots": { "scene": { "domain": "scene", "label": "Party scene" } } }
  ]
}
```
- Packs contain **no images and no HA identifiers**. HA-specific targets are slots the importer fills in.
- Import re-embeds samples with the local embedder, rebuilds motion templates, and retrains the classifier.
- Packs never contain anchors or places. Those are specific to a room and a camera, and are never shared.
- Max pack size 5 MB. Validate with JSON Schema (`schemas/gesture-pack.v1.json`, generated with `schemars`).

## 8. Migrations & compatibility
- `rusqlite_migration` migrations in `crates/flick-store/migrations/`. They are **forward-only**; there are no down migrations.
- **Backward-readable for one minor version:** a release N may only add tables, nullable columns or columns with defaults, and indexes. Destructive changes (drop/rename/type change) ship in N+1, after N has stopped using the column.
  - This lets the OTA rollback (`10-…` §4.3) run version N−1 against a database migrated by N.
  - If a release must break this rule, it is flagged `breaking_db` in the channel index. Rollback then **requires** the DB snapshot restore.
- **Derived data is never migrated; it is rebuilt from the raw data:**
  - `sample_embeddings` and `motion_templates` come from samples/takes;
  - anchor geometry comes from `anchor_observations` when `estimator_version` changes.
- One migration test covers this (not one per migration): a DB snapshot from the previous release → migrate → the queries used by the previous release still succeed (`07-…` §1.2).
