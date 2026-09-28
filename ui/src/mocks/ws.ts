import type { EventClient } from "../events/client";
import type { WsServerMessage, WsTopic } from "../events/types";
import { status } from "./data";

const baseTs = "2026-09-28T15:39:00.360Z";
const sequence: WsServerMessage[] = [
  { type: "hello", ts: baseTs, engine_version: "0.1.0-dev", api: "v1", pro: false },
  { type: "engine.status", ts: baseTs, status },
  { type: "camera.status", ts: baseTs, camera_id: "camera-main", state: "running", fps: 30 },
  { type: "ha.status", ts: baseTs, state: "ready", ha_version: "2026.9" },
  {
    type: "hands",
    ts: baseTs,
    camera_id: "camera-main",
    seq: 1,
    hands: [
      {
        track_id: 1,
        hand: "right",
        bbox: { x: 0.38, y: 0.36, w: 0.22, h: 0.34 },
        landmarks: Array.from({ length: 21 }, (_, index) => ({ x: 0.36 + index * 0.014, y: 0.7 - index * 0.018, z: 0 })),
      },
    ],
    ray: { origin2d: [0.58, 0.44], tip2d: [0.82, 0.22], model: "eye" },
  },
  { type: "target.hover", ts: baseTs, camera_id: "camera-main", anchor_id: "anchor-fan-bedroom", name: "Ventilador dormitorio", score: 0.88, dwell_progress: 0.45 },
  {
    type: "target.selected",
    ts: baseTs,
    camera_id: "camera-main",
    anchor_id: "anchor-fan-bedroom",
    name: "Ventilador dormitorio",
    domain: "fan",
    expires_at: "2026-09-28T15:39:04.360Z",
    verbs: [
      { gesture_id: "builtin.circle_cw", label: "↻ speed 1" },
      { gesture_id: "builtin.two_hand_separate", label: "✋✋ off" },
    ],
  },
  { type: "gesture.candidate", ts: baseTs, camera_id: "camera-main", track_id: 1, gesture_id: "builtin.circle_cw", confidence: 0.87, progress: 0.62 },
  {
    type: "gesture.fired",
    ts: baseTs,
    event_id: "evt-circle-fan",
    camera_id: "camera-main",
    gesture_id: "builtin.circle_cw",
    hand: "right",
    confidence: 0.93,
    mapping_ids: ["map-fan-speed-1"],
    action_summary: "Ventilador dormitorio → speed 1",
  },
  {
    type: "action.result",
    ts: baseTs,
    activity_id: "act-circle-fan",
    event_id: "evt-circle-fan",
    mapping_id: "map-fan-speed-1",
    status: "ok",
    message: "Done · Home Assistant confirmed",
    latency: { detect_ms: 214, dispatch_ms: 2, ha_ms: 39 },
  },
  { type: "gesture.suppressed", ts: baseTs, camera_id: "camera-main", gesture_id: "builtin.thumb_up", reason: "target_selected" },
  {
    type: "gesture.fired",
    ts: baseTs,
    event_id: "evt-failure",
    camera_id: "camera-main",
    gesture_id: "builtin.two_hand_separate",
    hand: "right",
    confidence: 0.91,
    mapping_ids: ["map-fan-off"],
    action_summary: "Ventilador dormitorio → off",
  },
  {
    type: "action.result",
    ts: baseTs,
    activity_id: "act-failure",
    event_id: "evt-failure",
    mapping_id: "map-fan-off",
    status: "error",
    error_code: "ha_unavailable",
    message: "Home Assistant unavailable",
  },
  { type: "update.ready", ts: baseTs, kind: "app", id: "app", version: "1.3.0" },
];

export function startMockEventStream(ingest: (message: WsServerMessage) => void, _topics: WsTopic[]): EventClient {
  let index = 0;
  const timers: number[] = [];

  for (const message of sequence) {
    const timer = window.setTimeout(() => ingest(message), 150 + index * 700);
    timers.push(timer);
    index += 1;
  }

  return {
    close: () => timers.forEach((timer) => window.clearTimeout(timer)),
  };
}
