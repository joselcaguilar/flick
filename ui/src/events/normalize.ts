import type { HandObservationEvent, RawWsServerMessage, WsServerMessage } from "./types";

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
    [key: string]: unknown;
  };
  if (typeof message.type !== "string") throw new Error("Invalid event message");

  const { type, ts: flatTs, payload: envelope, ...flatPayload } = message;
  const payload = (envelope?.payload ?? flatPayload) as Record<string, unknown>;
  const ts = envelope?.ts ?? flatTs ?? new Date().toISOString();

  if (type === "hello") {
    return {
      type: "hello",
      ts,
      ...(payload as Omit<Extract<WsServerMessage, { type: "hello" }>, "type" | "ts">),
    };
  }
  if (type === "engine.status") {
    return {
      type: "engine.status",
      ts,
      status: payload as Extract<WsServerMessage, { type: "engine.status" }>["status"],
    };
  }
  if (type === "camera.status") {
    return {
      type: "camera.status",
      ts,
      ...(payload as Omit<Extract<WsServerMessage, { type: "camera.status" }>, "type" | "ts">),
    };
  }
  if (type === "hands") {
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
  if (type === "engine.resumed" || type === "disarmed") return { type, ts } as WsServerMessage;
  if (type === "update.ready") {
    return {
      type: "update.ready",
      ts,
      id: "app",
      ...(payload as Omit<Extract<WsServerMessage, { type: "update.ready" }>, "type" | "ts">),
    };
  }

  return { type, ts, ...payload } as WsServerMessage;
}
