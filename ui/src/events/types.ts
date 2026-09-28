import type { CameraState, EngineStatus, HaState } from "../api/types";

export type WsTopic = "status" | "gestures" | "actions" | "ha" | "capture" | "targeting" | "teach" | "updates" | `hands:${string}`;

export interface HandPoint {
  x: number;
  y: number;
  z: number;
}

export interface HandObservationEvent {
  track_id: number;
  hand: "left" | "right";
  landmarks: HandPoint[];
  bbox: { x: number; y: number; w: number; h: number };
}

export type WsServerMessage =
  | { type: "hello"; ts: string; engine_version: string; api: "v1"; pro: boolean }
  | { type: "engine.status"; ts: string; status: EngineStatus }
  | { type: "camera.status"; ts: string; camera_id: string; state: CameraState; fps: number; error?: string }
  | { type: "hands"; ts: string; camera_id: string; seq: number; hands: HandObservationEvent[]; ray?: { origin2d: [number, number]; tip2d: [number, number]; model: string } }
  | { type: "gesture.candidate"; ts: string; camera_id: string; track_id: number; gesture_id: string; confidence: number; progress: number }
  | { type: "gesture.suppressed"; ts: string; camera_id: string; gesture_id: string; reason: SuppressionReason }
  | { type: "gesture.fired"; ts: string; event_id: string; camera_id: string; gesture_id: string; hand: "left" | "right"; confidence: number; mapping_ids: string[]; action_summary: string }
  | { type: "gesture.update"; ts: string; event_id: string; value?: number }
  | { type: "gesture.end"; ts: string; event_id: string; value?: number }
  | { type: "armed"; ts: string; until: string }
  | { type: "disarmed"; ts: string }
  | { type: "confirm.required"; ts: string; event_id: string; mapping_id: string; confirm_gesture_id: string; expires_at: string }
  | { type: "action.result"; ts: string; activity_id: string; event_id: string; mapping_id: string; status: "ok" | "error" | "timeout"; error_code?: string; message?: string; latency?: { detect_ms?: number; dispatch_ms?: number; ha_ms?: number } }
  | { type: "ha.status"; ts: string; state: HaState; ha_version?: string }
  | { type: "ha.entity"; ts: string; entity_id: string; state: string; attributes: Record<string, unknown> }
  | { type: "capture.progress"; ts: string; session_id: string; take: number; takes: number; phase: "countdown" | "recording" | "review" | "done"; quality_hint?: string }
  | { type: "engine.paused"; ts: string; until?: string }
  | { type: "engine.resumed"; ts: string }
  | { type: "target.hover"; ts: string; camera_id: string; anchor_id: string; name: string; score: number; dwell_progress: number; runner_up?: string }
  | { type: "target.selected"; ts: string; camera_id: string; anchor_id: string; name: string; domain: string; expires_at: string; verbs: Array<{ gesture_id: string; label: string }> }
  | { type: "target.cleared"; ts: string; camera_id: string; anchor_id: string; reason: "timeout" | "hand_lost" | "reselected" | "paused" }
  | { type: "target.ambiguous"; ts: string; camera_id: string; anchor_ids: [string, string] }
  | { type: "place.status"; ts: string; camera_id: string; place_id: string; state: "ok" | "switched" | "needs_realign"; similarity: number }
  | { type: "teach.progress"; ts: string; session_id: string; phase: "aiming" | "capturing" | "captured" | "error"; ray_jitter_deg?: number; confidence?: number; hint?: string }
  | { type: "update.available"; ts: string; kind: "app" | "pack"; id: string; version: string; size: number }
  | { type: "update.progress"; ts: string; kind: "app" | "pack"; id: string; phase: "download" | "verify" | "selftest" | "benchmark" | "shadow"; bytes?: number; total?: number }
  | { type: "update.ready"; ts: string; kind: "app"; version: string }
  | { type: "model.activated"; ts: string; pack_id: string; version: string; previous: string }
  | { type: "model.rolled_back"; ts: string; pack_id: string; from: string; to: string; reason: string };

export type SuppressionReason =
  | "low_confidence"
  | "cooldown"
  | "not_armed"
  | "no_mapping"
  | "sensitive_blocked"
  | "hand_too_far"
  | "target_selected"
  | "no_target"
  | "ambiguous_target"
  | "needs_realign";

export interface HudState {
  state: "idle" | "aiming" | "selected" | "candidate" | "sent" | "done" | "failed" | "confirm" | "dial" | "camera_moved" | "paused" | "ambiguous";
  title: string;
  detail?: string;
  icon?: string;
  progress?: number;
  expiresAt?: string;
  updatedAt?: string;
}

export interface EventStreamState {
  connection: "idle" | "connecting" | "open" | "reconnecting" | "closed" | "error";
  engineVersion?: string;
  api?: "v1";
  pro: boolean;
  lastEventAt?: string;
  engine?: EngineStatus;
  cameras: Record<string, { state: CameraState; fps: number; error?: string }>;
  ha: { state: HaState; ha_version?: string };
  hands: Record<string, { seq: number; hands: HandObservationEvent[]; ray?: { origin2d: [number, number]; tip2d: [number, number]; model: string } }>;
  candidate?: { gesture_id: string; confidence: number; progress: number; camera_id: string };
  selectedTarget?: { camera_id: string; anchor_id: string; name: string; domain: string; expires_at: string; verbs: Array<{ gesture_id: string; label: string }> };
  suppression?: { gesture_id: string; reason: SuppressionReason; ts: string };
  place?: { camera_id: string; place_id: string; state: "ok" | "switched" | "needs_realign"; similarity: number };
  hud: HudState;
  updates: Array<{ id: string; kind: "app" | "pack"; version: string; phase?: string }>;
}
