import type { components } from "./schema";

export type Schema<Name extends keyof components["schemas"]> = components["schemas"][Name];

export type Domain =
  | "fan"
  | "light"
  | "media_player"
  | "cover"
  | "switch"
  | "climate"
  | "lock"
  | "scene"
  | "script"
  | string;
export type ThemeMode = "system" | "light" | "dark";
export type LightThemeId = "light" | "light_high_contrast" | "light_colorblind";
export type DarkThemeId = "dark" | "dark_dimmed" | "dark_high_contrast";
export type HaState = "disconnected" | "connecting" | "ready" | "auth_failed" | string;
export type CameraState = "starting" | "running" | "idle" | "reconnecting" | "error" | "stopped" | string;
export type EngineMode = "watching" | "idle" | "paused" | "restarting" | "error";

export type ProblemDetails = Schema<"ProblemJson">;
export type Health = Schema<"HealthResponse">;
export type CameraStatus = Schema<"CameraStatus"> & {
  id?: string;
  name?: string;
  camera_permission?: "authorized" | "denied" | "not_determined" | "restricted" | string;
};
export type EngineStatus = Schema<"EngineStatus"> & {
  mode?: EngineMode;
  version?: string;
  inference_ms_p95?: number;
  cameras: CameraStatus[];
  place?: { id: string; name: string; state: "ok" | "needs_realign" | "switched" | string };
};

export type SettingsMap = {
  "detection.sensitivity"?: "low" | "normal" | "high";
  "detection.vote"?: { n: number; m: number };
  "detection.min_hand_size"?: number;
  "detection.battery_saver"?: boolean;
  "detection.arm"?: { enabled: boolean; gesture: string; hold_ms: number; window_ms: number };
  "detection.pause_gesture"?: { enabled: boolean; gesture: string; hold_ms: number };
  "safety.allow_sensitive"?: boolean;
  "safety.confirm_gesture"?: string;
  "privacy.pause_when_ha_offline"?: boolean;
  "privacy.pause_on_screen_lock"?: boolean;
  "privacy.keep_awake"?: boolean;
  quiet_hours?: null | { from: string; to: string };
  "feedback.hud"?: {
    enabled: boolean;
    position: "top-center" | "top-right" | "bottom-center" | "bottom-right";
    duration_ms: number;
  };
  "feedback.sounds"?: { enabled: boolean; volume: number };
  "ui.theme"?: ThemeMode;
  "ui.light_theme"?: LightThemeId;
  "ui.dark_theme"?: DarkThemeId;
  "debug.log_suppressed"?: boolean;
  "onboarding.completed"?: boolean;
  "gestures.params"?: Record<string, unknown>;
  "gestures.two_hand_separate.axis"?: "any" | "vertical" | "horizontal";
  "targeting.enabled"?: boolean;
  "targeting.tolerance_deg"?: number;
  "targeting.dwell_ms"?: number;
  "targeting.window_ms"?: number;
  "targeting.ray_model"?: "auto" | "eye" | "finger";
  "targeting.dominant_eye"?: "center" | "left" | "right";
  "updates.channel"?: "stable" | "beta" | "nightly";
  "updates.auto_install_app"?: boolean;
  "updates.auto_models"?: boolean;
  "updates.install_when_idle"?: boolean;
  "updates.install_id"?: string;
  "privacy.crash_reports"?: boolean;
  "cloud.setup_assistant"?: { enabled: boolean; provider: null | "foundry" };
  [key: string]: unknown;
};
export type SettingsPatch = Partial<SettingsMap>;

export type HaInstance = Schema<"HaInstance">;
export type HaStatus = Schema<"HaStatus">;
export type HaConnectionUpdate = Schema<"HaConnectionUpdate">;
export type HaClientCertificate = Schema<"HaClientCertificate">;
export type HaClientCertificateUpload = Schema<"HaClientCertificateUpload">;
export type HaDiscovery = Schema<"HaDiscovery">;
export type HaArea = Schema<"HaArea">;
export type HaEntity = Schema<"HaEntity">;
export type HaServiceSchema = Schema<"HaServiceSchema">;
export type Action = Schema<"ActionDto">;
export type ActionOutcome = Schema<"ActionOutcomeDto">;
export type Camera = Schema<"Camera">;
export type CameraAvailable = Schema<"AvailableCamera">;
export type PreviewTicket = Schema<"PreviewTicket">;
export type Gesture = Schema<"Gesture"> & { used_by?: number };
export type CaptureSession = Schema<"CaptureSession">;
export type MotionTake = Schema<"MotionTake"> & { gesture_id?: string };
export type ClassifierReport = Schema<"ClassifierReport"> & {
  loto_accuracy?: number;
  confusions?: Array<{ gesture_id: string; with: string; score: number }>;
};
export type Mapping = Schema<"Mapping"> & {
  gesture_name?: string;
  target_label?: string;
};

export type MappingCreate = Schema<"MappingCreate">;
export type ActivityItem = Schema<"ActivityItem"> & {
  gesture_name?: string;
};
export type Place = Schema<"Place">;
export type Anchor = Schema<"Anchor"> & { uncertainty_deg?: number };
export type AnchorVerb = Schema<"VerbBinding"> & { action?: Action };
export type TeachSession = Schema<"TeachSession">;
export type TeachSpotResponse = Schema<"TeachSpotResponse">;
export type TeachLevelResponse = Schema<"TeachLevelResponse">;
export type TeachCommitResponse = Schema<"TeachCommitResponse">;
export type TeachVerb = Schema<"TeachVerb">;
export type RealignSession = Schema<"RealignSession">;
export type UpdateState = Schema<"UpdateStateDto">;
export type ModelPack = Schema<"ModelPack">;
export type GesturePackPreview = {
  gestures: Array<{ id: string; name: string; conflict?: string }>;
  mapping_templates: Array<{ slots: Record<string, { domain: string; label: string }> }>;
};

export function jsonObject<T extends object>(value: T) {
  return value as unknown as Record<string, never>;
}
