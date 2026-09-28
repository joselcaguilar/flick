import type { HandObservationEvent, RawWsServerMessage, WsServerMessage } from "./types";

const typeMap: Record<string, WsServerMessage["type"]> = {
  engine_status: "engine.status",
  camera_status: "camera.status",
  gesture_candidate: "gesture.candidate",
  gesture_suppressed: "gesture.suppressed",
  gesture_fired: "gesture.fired",
  gesture_update: "gesture.update",
  gesture_end: "gesture.end",
  confirm_required: "confirm.required",
  action_result: "action.result",
  ha_status: "ha.status",
  ha_entity: "ha.entity",
  capture_progress: "capture.progress",
  engine_paused: "engine.paused",
  engine_resumed: "engine.resumed",
  target_hover: "target.hover",
  target_selected: "target.selected",
  target_cleared: "target.cleared",
  target_ambiguous: "target.ambiguous",
  place_status: "place.status",
  teach_progress: "teach.progress",
  update_available: "update.available",
  update_progress: "update.progress",
  update_ready: "update.ready",
  model_activated: "model.activated",
  model_rolled_back: "model.rolled_back",
};

function handFromArray(hand: {
  bbox?: number[];
  hand?: string;
  landmarks?: number[][];
  track_id?: number;
}): HandObservationEvent {
  const [x = 0, y = 0, w = 0, h = 0] = hand.bbox ?? [];
  return {
    track_id: hand.track_id ?? 0,
    hand: hand.hand ?? "right",
    bbox: { x, y, w, h },
    landmarks: (hand.landmarks ?? []).map(([px = 0, py = 0, pz = 0]) => ({ x: px, y: py, z: pz })),
  };
}

export function normalizeWsMessage(input: RawWsServerMessage | WsServerMessage | unknown): WsServerMessage {
  const message = input as {
    type?: string;
    ts?: string;
    payload?: { payload?: Record<string, unknown>; ts?: string };
  };
  if (typeof message.type !== "string") throw new Error("Invalid event message");
  if (message.type.includes(".")) return input as WsServerMessage;

  const normalizedType = typeMap[message.type] ?? message.type;
  const payload = (message.payload?.payload ?? {}) as Record<string, unknown>;
  const ts = message.payload?.ts ?? message.ts ?? new Date().toISOString();

  if (normalizedType === "hello")
    return {
      type: "hello",
      ts,
      ...(payload as Omit<Extract<WsServerMessage, { type: "hello" }>, "type" | "ts">),
    };
  if (normalizedType === "engine.status")
    return {
      type: "engine.status",
      ts,
      status: payload as Extract<WsServerMessage, { type: "engine.status" }>["status"],
    };
  if (normalizedType === "camera.status")
    return {
      type: "camera.status",
      ts,
      ...(payload as Omit<Extract<WsServerMessage, { type: "camera.status" }>, "type" | "ts">),
    };
  if (normalizedType === "hands") {
    const hands = (
      (payload.hands as Array<{
        bbox?: number[];
        hand?: string;
        landmarks?: number[][];
        track_id?: number;
      }>) ?? []
    ).map(handFromArray);
    const ray = payload.ray as { origin2d?: number[]; tip2d?: number[]; model?: string } | null | undefined;
    return {
      type: "hands",
      ts,
      camera_id: String(payload.camera_id ?? "camera-main"),
      seq: Number(payload.seq ?? 0),
      hands,
      ray: ray
        ? {
            origin2d: [Number(ray.origin2d?.[0] ?? 0), Number(ray.origin2d?.[1] ?? 0)],
            tip2d: [Number(ray.tip2d?.[0] ?? 0), Number(ray.tip2d?.[1] ?? 0)],
            model: String(ray.model ?? "eye"),
          }
        : undefined,
    };
  }
  if (normalizedType === "engine.resumed" || normalizedType === "disarmed")
    return { type: normalizedType, ts } as WsServerMessage;
  if (normalizedType === "update.ready")
    return {
      type: "update.ready",
      ts,
      id: "app",
      ...(payload as Omit<Extract<WsServerMessage, { type: "update.ready" }>, "type" | "ts">),
    };

  return { type: normalizedType, ts, ...payload } as WsServerMessage;
}
