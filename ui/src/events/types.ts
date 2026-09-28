import type { components } from "../api/schema";
import type { ActivityItem, CameraState, EngineStatus, HaState } from "../api/types";

export type WsTopic =
  | "status"
  | "gestures"
  | "actions"
  | "ha"
  | "capture"
  | "targeting"
  | "teach"
  | "updates"
  | `hands:${string}`;
export type RawWsServerMessage = components["schemas"]["WsServerMessage"];

export interface HandPoint {
  x: number;
  y: number;
  z: number;
}

export interface HandObservationEvent {
  track_id: number;
  hand: "left" | "right" | string;
  landmarks: HandPoint[];
  bbox: { x: number; y: number; w: number; h: number };
}

export type WsServerMessage =
  | { type: "hello"; ts: string; engine_version: string; api: "v1" | string; pro: boolean }
  | { type: "engine.status"; ts: string; status: EngineStatus }
  | {
      type: "camera.status";
      ts: string;
      camera_id: string;
      state: CameraState;
      fps?: number | null;
      error?: string | null;
    }
  | {
      type: "hands";
      ts: string;
      camera_id: string;
      seq: number;
      hands: HandObservationEvent[];
      ray?: { origin2d: [number, number]; tip2d: [number, number]; model: string } | null;
    }
  | {
      type: "gesture.candidate";
      ts: string;
      camera_id: string;
      track_id: number;
      gesture_id: string;
      confidence: number;
      progress: number;
    }
  | {
      type: "gesture.suppressed";
      ts: string;
      camera_id: string;
      gesture_id: string;
      reason: SuppressionReason;
    }
  | {
      type: "gesture.fired";
      ts: string;
      event_id: string;
      camera_id: string;
      gesture_id: string;
      hand: "left" | "right" | string;
      confidence: number;
      mapping_ids: string[];
      action_summary: string;
    }
  | { type: "gesture.update"; ts: string; event_id: string; value?: number | null }
  | { type: "gesture.end"; ts: string; event_id: string; value?: number | null }
  | { type: "armed"; ts: string; until: string }
  | { type: "disarmed"; ts: string }
  | {
      type: "confirm.required";
      ts: string;
      event_id: string;
      mapping_id: string;
      confirm_gesture_id: string;
      expires_at: string;
    }
  | {
      type: "action.result";
      ts: string;
      activity_id: string;
      event_id: string;
      mapping_id: string;
      status: "ok" | "error" | "timeout" | string;
      error_code?: string | null;
      message?: string | null;
      latency?: { detect_ms?: number | null; dispatch_ms?: number | null; ha_ms?: number | null } | null;
    }
  | { type: "ha.status"; ts: string; state: HaState; ha_version?: string | null }
  | { type: "ha.entity"; ts: string; entity_id: string; state: string; attributes: Record<string, unknown> }
  | {
      type: "capture.progress";
      ts: string;
      session_id: string;
      take: number;
      takes: number;
      phase: "countdown" | "recording" | "review" | "done" | string;
      quality_hint?: string | null;
    }
  | { type: "engine.paused"; ts: string; until?: string | null }
  | { type: "engine.resumed"; ts: string }
  | {
      type: "target.hover";
      ts: string;
      camera_id: string;
      anchor_id: string;
      name: string;
      score: number;
      dwell_progress: number;
      runner_up?: string | null;
    }
  | {
      type: "target.selected";
      ts: string;
      camera_id: string;
      anchor_id: string;
      name: string;
      domain: string;
      expires_at: string;
      verbs: Array<{ gesture_id: string; label: string }>;
    }
  | {
      type: "target.cleared";
      ts: string;
      camera_id: string;
      anchor_id: string;
      reason: "timeout" | "hand_lost" | "reselected" | "paused" | string;
    }
  | { type: "target.ambiguous"; ts: string; camera_id: string; anchor_ids: string[] }
  | {
      type: "place.status";
      ts: string;
      camera_id: string;
      place_id: string;
      state: "ok" | "switched" | "needs_realign" | string;
      similarity: number;
    }
  | {
      type: "teach.progress";
      ts: string;
      session_id: string;
      phase: "aiming" | "capturing" | "captured" | "error" | string;
      ray_jitter_deg?: number | null;
      confidence?: number | null;
      hint?: string | null;
    }
  | {
      type: "update.available";
      ts: string;
      kind: "app" | "pack" | string;
      id: string;
      version: string;
      size: number;
    }
  | {
      type: "update.progress";
      ts: string;
      kind: "app" | "pack" | string;
      id: string;
      phase: "download" | "verify" | "selftest" | "benchmark" | "shadow" | string;
      bytes?: number | null;
      total?: number | null;
    }
  | { type: "update.ready"; ts: string; kind: "app" | string; id?: string; version: string }
  | { type: "model.activated"; ts: string; pack_id: string; version: string; previous: string }
  | { type: "model.rolled_back"; ts: string; pack_id: string; from: string; to: string; reason: string }
  | { type: "resync"; ts: string; reason: string; topics: string[] };

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
  | "needs_realign"
  | string;

export interface HudState {
  state:
    | "idle"
    | "aiming"
    | "selected"
    | "candidate"
    | "armed"
    | "sent"
    | "done"
    | "failed"
    | "confirm"
    | "dial"
    | "camera_moved"
    | "paused"
    | "ambiguous";
  title: string;
  detail?: string;
  icon?: string;
  verbs?: Array<{ gesture_id: string; label: string }>;
  progress?: number;
  expiresAt?: string;
  updatedAt?: string;
}

export interface EventStreamState {
  connection: "idle" | "connecting" | "open" | "reconnecting" | "closed" | "error";
  engineVersion?: string;
  api?: "v1" | string;
  pro: boolean;
  lastEventAt?: string;
  engine?: EngineStatus;
  cameras: Record<string, { state: CameraState; fps?: number | null; error?: string | null }>;
  ha: { state: HaState; ha_version?: string | null };
  hands: Record<
    string,
    {
      seq: number;
      hands: HandObservationEvent[];
      ray?: { origin2d: [number, number]; tip2d: [number, number]; model: string } | null;
    }
  >;
  candidate?: { gesture_id: string; confidence: number; progress: number; camera_id: string };
  selectedTarget?: {
    camera_id: string;
    anchor_id: string;
    name: string;
    domain: string;
    expires_at: string;
    verbs: Array<{ gesture_id: string; label: string }>;
  };
  suppression?: { gesture_id: string; reason: SuppressionReason; ts: string };
  teach?: {
    session_id: string;
    phase: "aiming" | "capturing" | "captured" | "error" | string;
    ray_jitter_deg?: number | null;
    confidence?: number | null;
    hint?: string | null;
    updatedAt: string;
  };
  place?: {
    camera_id: string;
    place_id: string;
    state: "ok" | "switched" | "needs_realign" | string;
    similarity: number;
  };
  hud: HudState;
  activity: ActivityItem[];
  updates: Array<{ id: string; kind: string; version: string; phase?: string }>;
}
