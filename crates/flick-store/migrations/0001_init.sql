CREATE TABLE settings (
  key         TEXT PRIMARY KEY,
  value       TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE ha_instances (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  base_url      TEXT NOT NULL,
  ha_uuid       TEXT UNIQUE,
  auth_kind     TEXT NOT NULL CHECK (auth_kind IN ('llat','oauth')),
  keychain_ref  TEXT NOT NULL,
  cert_sha256   TEXT,
  ha_version    TEXT,
  is_default    INTEGER NOT NULL DEFAULT 1,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE ha_cache (
  ha_id       TEXT NOT NULL REFERENCES ha_instances(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,
  payload     TEXT NOT NULL,
  fetched_at  INTEGER NOT NULL,
  PRIMARY KEY (ha_id, kind)
);

CREATE TABLE cameras (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('local','rtsp','file')),
  device_ref    TEXT,
  url_redacted  TEXT,
  keychain_ref  TEXT,
  trust_self_signed INTEGER NOT NULL DEFAULT 0,
  enabled       INTEGER NOT NULL DEFAULT 1,
  mirror        INTEGER NOT NULL DEFAULT 1,
  rotation      INTEGER NOT NULL DEFAULT 0 CHECK (rotation IN (0,90,180,270)),
  active_fps    INTEGER NOT NULL DEFAULT 30,
  idle_fps      INTEGER NOT NULL DEFAULT 5,
  max_hands     INTEGER NOT NULL DEFAULT 2 CHECK (max_hands IN (1,2)),
  roi           TEXT,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE gestures (
  id              TEXT PRIMARY KEY,
  source          TEXT NOT NULL CHECK (source IN ('builtin','custom','pack','system')),
  kind            TEXT NOT NULL CHECK (kind IN ('static','motion','dial','negative')),
  hands_required  INTEGER NOT NULL DEFAULT 1 CHECK (hands_required IN (1,2)),
  name            TEXT NOT NULL,
  icon            TEXT,
  hand_constraint TEXT NOT NULL DEFAULT 'any' CHECK (hand_constraint IN ('any','left','right')),
  threshold       REAL,
  enabled         INTEGER NOT NULL DEFAULT 1,
  pro             INTEGER NOT NULL DEFAULT 0,
  pack_id         TEXT,
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL
);

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
  landmarks_image  BLOB NOT NULL,
  landmarks_world  BLOB NOT NULL,
  quality          REAL NOT NULL,
  created_at       INTEGER NOT NULL
);
CREATE INDEX idx_samples_gesture ON gesture_samples(gesture_id);

CREATE TABLE sample_embeddings (
  sample_id        TEXT NOT NULL REFERENCES gesture_samples(id) ON DELETE CASCADE,
  embedder_version TEXT NOT NULL,
  augmentation     INTEGER NOT NULL,
  embedding        BLOB NOT NULL,
  PRIMARY KEY (sample_id, embedder_version, augmentation)
);

CREATE TABLE motion_takes (
  id            TEXT PRIMARY KEY,
  gesture_id    TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  session_id    TEXT REFERENCES capture_sessions(id) ON DELETE SET NULL,
  take_index    INTEGER NOT NULL,
  hands         INTEGER NOT NULL CHECK (hands IN (1,2)),
  frame_count   INTEGER NOT NULL,
  t_ms          BLOB NOT NULL,
  landmarks_image BLOB NOT NULL,
  landmarks_world BLOB NOT NULL,
  quality       REAL NOT NULL,
  created_at    INTEGER NOT NULL
);
CREATE INDEX idx_motion_takes_gesture ON motion_takes(gesture_id);

CREATE TABLE motion_templates (
  id               TEXT PRIMARY KEY,
  gesture_id       TEXT NOT NULL REFERENCES gestures(id) ON DELETE CASCADE,
  take_id          TEXT NOT NULL REFERENCES motion_takes(id) ON DELETE CASCADE,
  channels         TEXT NOT NULL CHECK (channels IN ('single','two_hand')),
  features_version TEXT NOT NULL,
  trajectory       BLOB NOT NULL,
  threshold        REAL NOT NULL,
  created_at       INTEGER NOT NULL
);

CREATE TABLE classifier_models (
  id               TEXT PRIMARY KEY,
  algorithm        TEXT NOT NULL CHECK (algorithm IN ('proto_knn','logreg')),
  embedder_version TEXT NOT NULL,
  params           BLOB NOT NULL,
  metrics          TEXT NOT NULL,
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
  camera_ids         TEXT NOT NULL DEFAULT '[]',
  zone_id            TEXT,
  target_mode        TEXT NOT NULL DEFAULT 'global' CHECK (target_mode IN ('global','anchor','domain')),
  anchor_id          TEXT REFERENCES anchors(id) ON DELETE CASCADE,
  target_domain      TEXT,
  mode               TEXT NOT NULL DEFAULT 'tap' CHECK (mode IN ('tap','hold','repeat','dial')),
  hold_ms            INTEGER NOT NULL DEFAULT 800,
  repeat_ms          INTEGER NOT NULL DEFAULT 400,
  cooldown_ms        INTEGER NOT NULL DEFAULT 1000,
  require_armed      INTEGER NOT NULL DEFAULT 0,
  active_hours       TEXT,
  action             TEXT NOT NULL,
  sensitive          INTEGER NOT NULL DEFAULT 0,
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

CREATE TABLE activity_log (
  id             TEXT PRIMARY KEY,
  ts             INTEGER NOT NULL,
  camera_id      TEXT,
  gesture_id     TEXT,
  hand           TEXT,
  confidence     REAL,
  mapping_id     TEXT,
  anchor_id      TEXT,
  action_summary TEXT,
  status         TEXT NOT NULL CHECK (status IN ('fired','sent','ok','error','timeout','stale','suppressed')),
  reason         TEXT,
  message        TEXT,
  ha_context_id  TEXT,
  latency        TEXT
);
CREATE INDEX idx_activity_ts ON activity_log(ts DESC);

CREATE TABLE gesture_packs (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  author      TEXT,
  version     TEXT,
  imported_at INTEGER NOT NULL,
  manifest    TEXT NOT NULL
);

CREATE TABLE places (
  id               TEXT PRIMARY KEY,
  camera_id        TEXT NOT NULL REFERENCES cameras(id) ON DELETE CASCADE,
  name             TEXT NOT NULL,
  scene_signature  BLOB NOT NULL,
  embedder_version TEXT NOT NULL,
  intrinsics       TEXT NOT NULL,
  status           TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','needs_realign')),
  active           INTEGER NOT NULL DEFAULT 0,
  created_at       INTEGER NOT NULL,
  updated_at       INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_places_active ON places(camera_id) WHERE active = 1;

CREATE TABLE anchors (
  id                TEXT PRIMARY KEY,
  place_id          TEXT NOT NULL REFERENCES places(id) ON DELETE CASCADE,
  name              TEXT NOT NULL,
  target            TEXT NOT NULL,
  domain            TEXT NOT NULL,
  kind              TEXT NOT NULL CHECK (kind IN ('point3d','direction','region2d')),
  position          BLOB,
  direction         BLOB,
  teach_origin      BLOB,
  covariance        BLOB,
  uncertainty_deg   REAL NOT NULL,
  verb_params       TEXT NOT NULL DEFAULT '{}',
  sensitive         INTEGER NOT NULL DEFAULT 0,
  sensitive_ack     INTEGER NOT NULL DEFAULT 0,
  estimator_version TEXT NOT NULL,
  status            TEXT NOT NULL DEFAULT 'ok' CHECK (status IN ('ok','needs_realign','needs_reteach')),
  last_used_at      INTEGER,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE INDEX idx_anchors_place ON anchors(place_id);

CREATE TABLE anchor_observations (
  id                TEXT PRIMARY KEY,
  anchor_id         TEXT NOT NULL REFERENCES anchors(id) ON DELETE CASCADE,
  spot_index        INTEGER NOT NULL,
  frames            INTEGER NOT NULL,
  hand              TEXT NOT NULL CHECK (hand IN ('left','right')),
  landmarks_image   BLOB NOT NULL,
  landmarks_world   BLOB NOT NULL,
  face_keypoints    BLOB,
  intrinsics_version TEXT NOT NULL,
  created_at        INTEGER NOT NULL
);

CREATE TABLE model_packs (
  id            TEXT NOT NULL,
  version       TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('model','catalog')),
  state         TEXT NOT NULL CHECK (state IN ('staged','shadow','active','previous','rejected','removed')),
  provides      TEXT NOT NULL,
  sha256        TEXT NOT NULL,
  source        TEXT NOT NULL CHECK (source IN ('bundled','ota','import')),
  on_demand     INTEGER NOT NULL DEFAULT 0,
  reject_reason TEXT,
  installed_at  INTEGER NOT NULL,
  activated_at  INTEGER,
  PRIMARY KEY (id, version)
);
CREATE UNIQUE INDEX idx_packs_active ON model_packs(id) WHERE state = 'active';

CREATE TABLE update_state (
  key         TEXT PRIMARY KEY,
  value       TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
);

INSERT OR IGNORE INTO gestures (id, source, kind, hands_required, name, icon, hand_constraint, created_at, updated_at) VALUES
  ('builtin.closed_fist', 'builtin', 'static', 1, 'Closed fist', NULL, 'any', 0, 0),
  ('builtin.open_palm', 'builtin', 'static', 1, 'Open palm', '✋', 'any', 0, 0),
  ('builtin.pointing_up', 'builtin', 'static', 1, 'Pointing up', '☝', 'any', 0, 0),
  ('builtin.thumb_up', 'builtin', 'static', 1, 'Thumb up', '👍', 'any', 0, 0),
  ('builtin.thumb_down', 'builtin', 'static', 1, 'Thumb down', '👎', 'any', 0, 0),
  ('builtin.victory', 'builtin', 'static', 1, 'Victory', '✌', 'any', 0, 0),
  ('builtin.i_love_you', 'builtin', 'static', 1, 'I love you', '🤟', 'any', 0, 0),
  ('builtin.point', 'builtin', 'static', 1, 'Point', '☝', 'any', 0, 0),
  ('builtin.swipe_left', 'builtin', 'motion', 1, 'Swipe left', '←', 'any', 0, 0),
  ('builtin.swipe_right', 'builtin', 'motion', 1, 'Swipe right', '→', 'any', 0, 0),
  ('builtin.swipe_up', 'builtin', 'motion', 1, 'Swipe up', '↑', 'any', 0, 0),
  ('builtin.swipe_down', 'builtin', 'motion', 1, 'Swipe down', '↓', 'any', 0, 0),
  ('builtin.pinch_dial', 'builtin', 'dial', 1, 'Pinch dial', '🤏', 'any', 0, 0),
  ('builtin.circle_cw', 'builtin', 'motion', 1, 'Circle clockwise', '↻', 'any', 0, 0),
  ('builtin.circle_ccw', 'builtin', 'motion', 1, 'Circle counter-clockwise', '↺', 'any', 0, 0),
  ('builtin.circle_any', 'builtin', 'motion', 1, 'Circle', '○', 'any', 0, 0),
  ('builtin.two_hand_separate', 'builtin', 'motion', 2, 'Two-hand separate', '↕', 'any', 0, 0),
  ('system.none', 'system', 'negative', 1, 'None', NULL, 'any', 0, 0);
