export type Domain = "fan" | "light" | "media_player" | "cover" | "switch" | "climate" | "lock" | "scene" | "script";
export type ThemeMode = "system" | "light" | "dark";
export type HaState = "disconnected" | "connecting" | "ready" | "auth_failed";
export type CameraState = "starting" | "running" | "idle" | "reconnecting" | "error" | "stopped";
export type EngineMode = "watching" | "idle" | "paused" | "restarting" | "error";

export interface ProblemDetails {
  type: string;
  title: string;
  status: number;
  detail?: string;
  code: string;
}

export interface Health {
  status: "ok" | "degraded";
  version: string;
  uptime_s: number;
}

export interface CameraSummary {
  id: string;
  name: string;
  state: CameraState;
  fps: number;
  camera_permission?: "authorized" | "denied" | "not_determined" | "restricted";
}

export interface EngineStatus {
  mode: EngineMode;
  paused: boolean;
  paused_until?: string | null;
  version: string;
  inference_ms_p95: number;
  cameras: CameraSummary[];
  ha: {
    state: HaState;
    ha_version?: string;
  };
  place?: {
    id: string;
    name: string;
    state: "ok" | "needs_realign" | "switched";
  };
}

export interface SettingsMap {
  "detection.sensitivity": "low" | "normal" | "high";
  "detection.vote": { n: number; m: number };
  "detection.min_hand_size": number;
  "detection.battery_saver": boolean;
  "detection.arm": { enabled: boolean; gesture: string; hold_ms: number; window_ms: number };
  "detection.pause_gesture": { enabled: boolean; gesture: string; hold_ms: number };
  "safety.allow_sensitive": boolean;
  "safety.confirm_gesture": string;
  "privacy.pause_when_ha_offline": boolean;
  "privacy.pause_on_screen_lock": boolean;
  "privacy.keep_awake": boolean;
  quiet_hours: null | { from: string; to: string };
  "feedback.hud": { enabled: boolean; position: "top-center" | "top-right" | "bottom-center" | "bottom-right"; duration_ms: number };
  "feedback.sounds": { enabled: boolean; volume: number };
  "ui.theme": ThemeMode;
  "debug.log_suppressed": boolean;
  "onboarding.completed": boolean;
  "gestures.params": Record<string, unknown>;
  "gestures.two_hand_separate.axis": "any" | "vertical" | "horizontal";
  "targeting.enabled": boolean;
  "targeting.tolerance_deg": number;
  "targeting.dwell_ms": number;
  "targeting.window_ms": number;
  "targeting.ray_model": "auto" | "eye" | "finger";
  "targeting.dominant_eye": "center" | "left" | "right";
  "updates.channel": "stable" | "beta" | "nightly";
  "updates.auto_install_app": boolean;
  "updates.auto_models": boolean;
  "updates.install_when_idle": boolean;
  "updates.install_id": string;
  "privacy.crash_reports": boolean;
  "cloud.setup_assistant": { enabled: boolean; provider: null | "foundry" };
}

export type SettingsPatch = Partial<SettingsMap>;

export interface HaInstance {
  id: string;
  name: string;
  base_url: string;
  ha_uuid?: string;
  ha_version?: string;
}

export interface HaDiscovery {
  name: string;
  base_url: string;
  uuid: string;
  version: string;
}

export interface HaArea {
  area_id: string;
  name: string;
  floor_id?: string;
}

export interface HaEntity {
  entity_id: string;
  name: string;
  domain: Domain;
  area_id?: string | null;
  device_id?: string;
  state: string;
  device_class?: string;
  supported_features?: number;
  attributes?: Record<string, unknown>;
}

export interface HaServiceField {
  name: string;
  required?: boolean;
  selector?: Record<string, unknown>;
}

export type Action =
  | {
      kind: "call_service";
      domain: string;
      service: string;
      target: { entity_id?: string[]; device_id?: string[]; area_id?: string[] };
      data?: Record<string, unknown>;
      preset?: string;
    }
  | { kind: "dial"; entity_id: string; property: string; gain: number; min?: number; max?: number }
  | { kind: "verb"; verb: "up" | "down" | "on" | "off" | "stop" | "toggle" | "level_set"; level?: number };

export interface ActionOutcome {
  status: "sent" | "ok" | "error" | "timeout";
  message?: string;
  error_code?: string;
  ha_context_id?: string;
  latency?: { detect_ms?: number; dispatch_ms?: number; ha_ms?: number };
}

export interface Camera {
  id: string;
  name: string;
  kind: "local" | "rtsp" | "file";
  enabled: boolean;
  mirror: boolean;
  rotation: 0 | 90 | 180 | 270;
  active_fps: number;
  idle_fps: number;
  max_hands: 1 | 2;
}

export interface CameraAvailable {
  device_ref: string;
  name: string;
  kind: "local";
  formats: Array<{ width: number; height: number; fps: number }>;
}

export interface Gesture {
  id: string;
  source: "builtin" | "custom" | "pack" | "system";
  kind: "static" | "motion" | "dial" | "negative";
  hands_required: 1 | 2;
  name: string;
  icon?: string;
  hand_constraint: "any" | "left" | "right";
  threshold?: number | null;
  enabled: boolean;
  sample_count: number;
  accuracy?: number;
  distinctiveness?: number;
  used_by?: number;
}

export interface CaptureSession {
  id: string;
  gesture_id: string;
  camera_id?: string;
  kind: "positive" | "negative";
  target_takes: number;
  status: "running" | "completed" | "cancelled" | "failed";
}

export interface MotionTake {
  id: string;
  gesture_id: string;
  take_index: number;
  hands: 1 | 2;
  trajectory: Array<[number, number]>;
  quality: number;
}

export interface ClassifierReport {
  id: string;
  algorithm: "proto_knn" | "logreg";
  trained_at: string;
  loto_accuracy: number;
  confusions: Array<{ gesture_id: string; with: string; score: number }>;
}

export interface Mapping {
  id: string;
  name: string;
  enabled: boolean;
  gesture_id: string;
  gesture_name: string;
  hand: "any" | "left" | "right";
  target_mode: "global" | "anchor" | "domain";
  target_label: string;
  target_domain?: Domain;
  mode: "tap" | "hold" | "repeat" | "dial";
  camera_ids: string[];
  action: Action;
  sensitive: boolean;
  sensitive_ack: boolean;
  sort_order: number;
}

export interface ActivityItem {
  id: string;
  ts: string;
  camera_id?: string;
  gesture_id?: string;
  gesture_name?: string;
  confidence?: number;
  mapping_id?: string;
  anchor_id?: string;
  action_summary: string;
  status: "fired" | "sent" | "ok" | "error" | "timeout" | "stale" | "suppressed";
  reason?: string;
  message?: string;
  latency?: { detect_ms?: number; dispatch_ms?: number; ha_ms?: number };
}

export interface Place {
  id: string;
  camera_id: string;
  name: string;
  status: "ok" | "needs_realign";
  active: boolean;
}

export interface AnchorVerb {
  gesture_id: string;
  label: string;
  action: Action;
}

export interface Anchor {
  id: string;
  place_id: string;
  name: string;
  target: { entity_id?: string; device_id?: string; area_id?: string };
  domain: Domain;
  kind: "point3d" | "direction" | "region2d";
  uncertainty_deg: number;
  verb_params: { levels?: number[] };
  sensitive: boolean;
  status: "ok" | "needs_realign" | "needs_reteach";
  last_used_at?: string;
  verbs: AnchorVerb[];
}

export interface TeachSession {
  id: string;
  camera_id: string;
  target: { entity_id?: string; device_id?: string; area_id?: string };
  phase: "pick" | "spot1" | "spot2" | "levels" | "verbs" | "test";
}

export interface RealignSession {
  id: string;
  prompts: string[];
}

export interface UpdateState {
  channel: "stable" | "beta" | "nightly";
  app: { current: string; available?: string; state: "idle" | "downloaded" | "ready" | "checking"; rollback_to?: string };
  packs: ModelPack[];
  index_version: number;
  last_check_at?: string;
  busy_reason?: "confirm_pending" | "recording" | "teaching" | "dial";
}

export interface ModelPack {
  id: string;
  version: string;
  kind: "model" | "catalog";
  state: "staged" | "shadow" | "active" | "previous" | "rejected" | "removed";
  on_demand: boolean;
  installed_at?: string;
}

export interface GesturePackPreview {
  gestures: Array<{ id: string; name: string; conflict?: string }>;
  mapping_templates: Array<{ slots: Record<string, { domain: string; label: string }> }>;
}
