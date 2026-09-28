import type {
  ActivityItem,
  Anchor,
  Camera,
  CameraAvailable,
  ClassifierReport,
  EngineStatus,
  Gesture,
  HaArea,
  HaDiscovery,
  HaEntity,
  HaInstance,
  Mapping,
  ModelPack,
  Place,
  SettingsMap,
  UpdateState,
} from "../api/types";

export const now = "2026-09-28T15:39:00.360Z";

export const ownerFan: HaEntity = {
  entity_id: "fan.ventilador_dormitorio",
  name: "Ventilador dormitorio",
  domain: "fan",
  area_id: "bedroom",
  device_id: "tuya-bedroom-fan",
  state: "on",
  supported_features: 53,
  attributes: {
    friendly_name: "Ventilador dormitorio",
    manufacturer: "Tuya",
    percentage: 1,
    percentage_step: 1,
    supported_features: 53,
  },
};

export const haEntities: HaEntity[] = [
  ownerFan,
  {
    entity_id: "light.lampara_dormitorio",
    name: "Lámpara dormitorio",
    domain: "light",
    area_id: "bedroom",
    device_id: "hue-bedroom-lamp",
    state: "off",
    attributes: { brightness: 0, supported_color_modes: ["brightness"] },
  },
  {
    entity_id: "light.living_room_lights",
    name: "Living room lights",
    domain: "light",
    area_id: "living_room",
    device_id: "living-room-group",
    state: "on",
    attributes: { brightness: 184 },
  },
  {
    entity_id: "media_player.living_room_tv",
    name: "Living room TV",
    domain: "media_player",
    area_id: "living_room",
    device_id: "apple-tv-living-room",
    state: "playing",
    attributes: { volume_level: 0.34 },
  },
  {
    entity_id: "switch.coffee_bar",
    name: "Coffee bar",
    domain: "switch",
    area_id: "kitchen",
    device_id: "kasa-coffee-bar",
    state: "off",
  },
];

export const haAreas: HaArea[] = [
  { area_id: "bedroom", name: "Dormitorio" },
  { area_id: "living_room", name: "Living room" },
  { area_id: "kitchen", name: "Kitchen" },
];

export const haDiscovery: HaDiscovery[] = [
  { name: "Home", base_url: "http://homeassistant.local:8123", uuid: "ha-home-demo", version: "2026.9" },
];

export const haInstance: HaInstance = {
  id: "01K5Y8W2D7E5W4EXAMPLEHA",
  name: "Home",
  base_url: "http://homeassistant.local:8123",
  ha_uuid: "ha-home-demo",
  ha_version: "2026.9",
};

export const camerasAvailable: CameraAvailable[] = [
  { device_ref: "avfoundation:0", name: "MacBook Camera", kind: "local", formats: [{ width: 1280, height: 720, fps: 30 }] },
  { device_ref: "continuity:iphone", name: "iPhone Continuity Camera", kind: "local", formats: [{ width: 1920, height: 1080, fps: 30 }] },
];

export const cameras: Camera[] = [
  {
    id: "camera-main",
    name: "MacBook Camera",
    kind: "local",
    enabled: true,
    mirror: true,
    rotation: 0,
    active_fps: 30,
    idle_fps: 5,
    max_hands: 2,
  },
];

export const places: Place[] = [{ id: "place-bedroom-desk", camera_id: "camera-main", name: "Bedroom desk", status: "ok", active: true }];

export const anchors: Anchor[] = [
  {
    id: "anchor-fan-bedroom",
    place_id: "place-bedroom-desk",
    name: "Ventilador dormitorio",
    target: { entity_id: "fan.ventilador_dormitorio" },
    domain: "fan",
    kind: "point3d",
    uncertainty_deg: 3.8,
    verb_params: { levels: [1] },
    sensitive: false,
    status: "ok",
    last_used_at: now,
    verbs: [
      { gesture_id: "builtin.circle_cw", label: "↻ speed 1", action: { kind: "verb", verb: "level_set", level: 1 } },
      { gesture_id: "builtin.two_hand_separate", label: "✋✋ off", action: { kind: "verb", verb: "off" } },
    ],
  },
];

export const gestures: Gesture[] = [
  { id: "builtin.thumb_up", source: "builtin", kind: "static", hands_required: 1, name: "Thumbs up", icon: "thumbs-up", hand_constraint: "any", enabled: true, sample_count: 0, used_by: 1 },
  { id: "builtin.open_palm", source: "builtin", kind: "static", hands_required: 1, name: "Open palm", icon: "open-palm", hand_constraint: "any", enabled: true, sample_count: 0, used_by: 0 },
  { id: "builtin.point", source: "builtin", kind: "static", hands_required: 1, name: "Point", icon: "point", hand_constraint: "any", enabled: true, sample_count: 0, used_by: 0 },
  { id: "builtin.circle_cw", source: "builtin", kind: "motion", hands_required: 1, name: "Circle clockwise", icon: "circle-cw", hand_constraint: "right", enabled: true, sample_count: 0, used_by: 1 },
  { id: "builtin.circle_ccw", source: "builtin", kind: "motion", hands_required: 1, name: "Circle counter-clockwise", icon: "circle-ccw", hand_constraint: "any", enabled: true, sample_count: 0, used_by: 0 },
  { id: "builtin.two_hand_separate", source: "builtin", kind: "motion", hands_required: 2, name: "Two hands apart", icon: "two-hand-separate", hand_constraint: "any", enabled: true, sample_count: 0, used_by: 1 },
  { id: "builtin.pinch_dial", source: "builtin", kind: "dial", hands_required: 1, name: "Pinch dial", icon: "pinch-dial", hand_constraint: "any", enabled: true, sample_count: 0, used_by: 0 },
];

export const mappings: Mapping[] = [
  {
    id: "map-fan-speed-1",
    name: "Point at Ventilador dormitorio + ↻ → Speed 1",
    enabled: true,
    gesture_id: "builtin.circle_cw",
    gesture_name: "Circle clockwise",
    hand: "right",
    target_mode: "anchor",
    target_label: "Ventilador dormitorio",
    target_domain: "fan",
    mode: "tap",
    camera_ids: ["camera-main"],
    action: { kind: "verb", verb: "level_set", level: 1 },
    sensitive: false,
    sensitive_ack: false,
    sort_order: 1,
  },
  {
    id: "map-fan-off",
    name: "Point at Ventilador dormitorio + ✋✋ apart → Off",
    enabled: true,
    gesture_id: "builtin.two_hand_separate",
    gesture_name: "Two hands apart",
    hand: "any",
    target_mode: "anchor",
    target_label: "Ventilador dormitorio",
    target_domain: "fan",
    mode: "tap",
    camera_ids: ["camera-main"],
    action: { kind: "verb", verb: "off" },
    sensitive: false,
    sensitive_ack: false,
    sort_order: 2,
  },
  {
    id: "map-living-lights-toggle",
    name: "👍 Thumbs up → Toggle Living room lights",
    enabled: true,
    gesture_id: "builtin.thumb_up",
    gesture_name: "Thumbs up",
    hand: "any",
    target_mode: "global",
    target_label: "Living room lights",
    mode: "tap",
    camera_ids: [],
    action: { kind: "call_service", domain: "light", service: "toggle", target: { entity_id: ["light.living_room_lights"] }, data: {}, preset: "light.toggle" },
    sensitive: false,
    sensitive_ack: false,
    sort_order: 3,
  },
];

export const activityItems: ActivityItem[] = [
  {
    id: "01K5Y8W2D7ACT1",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.circle_cw",
    gesture_name: "Circle clockwise",
    confidence: 0.93,
    mapping_id: "map-fan-speed-1",
    anchor_id: "anchor-fan-bedroom",
    action_summary: "Ventilador dormitorio · speed 1",
    status: "ok",
    message: "Done · Home Assistant confirmed",
    latency: { detect_ms: 214, dispatch_ms: 2, ha_ms: 39 },
  },
  {
    id: "01K5Y8W2D7ACT2",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.circle_cw",
    gesture_name: "Circle clockwise",
    confidence: 0.88,
    action_summary: "Circle clockwise",
    status: "suppressed",
    reason: "no_target",
    message: "Waiting for a selected device",
  },
  {
    id: "01K5Y8W2D7ACT3",
    ts: now,
    action_summary: "Home Assistant",
    status: "error",
    reason: "ha_offline",
    message: "Can't reach Home Assistant. Flick will keep trying — your camera is paused meanwhile.",
  },
];

export const status: EngineStatus = {
  mode: "watching",
  paused: false,
  version: "0.1.0-dev",
  inference_ms_p95: 8.4,
  cameras: [{ id: "camera-main", name: "MacBook Camera", state: "running", fps: 30, camera_permission: "authorized" }],
  ha: { state: "ready", ha_version: "2026.9" },
  place: { id: "place-bedroom-desk", name: "Bedroom desk", state: "ok" },
};

export const settings: SettingsMap = {
  "detection.sensitivity": "normal",
  "detection.vote": { n: 6, m: 8 },
  "detection.min_hand_size": 0.06,
  "detection.battery_saver": false,
  "detection.arm": { enabled: false, gesture: "builtin.open_palm", hold_ms: 600, window_ms: 4000 },
  "detection.pause_gesture": { enabled: false, gesture: "builtin.open_palm", hold_ms: 1500 },
  "safety.allow_sensitive": false,
  "safety.confirm_gesture": "builtin.thumb_up",
  "privacy.pause_when_ha_offline": true,
  "privacy.pause_on_screen_lock": false,
  "privacy.keep_awake": false,
  quiet_hours: null,
  "feedback.hud": { enabled: true, position: "top-center", duration_ms: 1500 },
  "feedback.sounds": { enabled: true, volume: 0.6 },
  "ui.theme": "system",
  "debug.log_suppressed": false,
  "onboarding.completed": true,
  "gestures.params": {},
  "gestures.two_hand_separate.axis": "any",
  "targeting.enabled": true,
  "targeting.tolerance_deg": 10,
  "targeting.dwell_ms": 500,
  "targeting.window_ms": 4000,
  "targeting.ray_model": "auto",
  "targeting.dominant_eye": "center",
  "updates.channel": "stable",
  "updates.auto_install_app": true,
  "updates.auto_models": true,
  "updates.install_when_idle": true,
  "updates.install_id": "01K5Y8W2D7INSTALL",
  "privacy.crash_reports": false,
  "cloud.setup_assistant": { enabled: false, provider: null },
};

export const updates: UpdateState = {
  channel: "stable",
  app: { current: "0.1.0", available: "1.3.0", state: "ready" },
  packs: [
    { id: "hand-landmarker", version: "2026.10.1", kind: "model", state: "active", on_demand: false, installed_at: now },
    { id: "catalog", version: "2026.10.3", kind: "catalog", state: "active", on_demand: false, installed_at: now },
    { id: "owlv2-base", version: "2026.09.1", kind: "model", state: "removed", on_demand: true },
  ],
  index_version: 42,
  last_check_at: now,
};

export const models: ModelPack[] = updates.packs;

export const classifierReport: ClassifierReport = {
  id: "classifier-dev",
  algorithm: "proto_knn",
  trained_at: now,
  loto_accuracy: 0.94,
  confusions: [{ gesture_id: "custom.rock", with: "builtin.open_palm", score: 0.32 }],
};
