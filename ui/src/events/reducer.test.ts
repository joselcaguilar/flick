import { describe, expect, it } from "vitest";
import { eventReducer, initialEventState } from "./reducer";
import type { WsServerMessage } from "./types";

const ts = "2026-09-28T15:30:00.000Z";

function reduce(messages: WsServerMessage[]) {
  return messages.reduce((state, message) => eventReducer(state, { type: "message", message }), initialEventState);
}

describe("eventReducer", () => {
  it("turns the owner's target sequence into selected, sent, and done HUD states", () => {
    const state = reduce([
      { type: "hello", ts, engine_version: "0.1.0", api: "v1", pro: false },
      {
        type: "target.selected",
        ts,
        camera_id: "camera-main",
        anchor_id: "anchor-fan-bedroom",
        name: "Ventilador dormitorio",
        domain: "fan",
        expires_at: "2026-09-28T15:30:04.000Z",
        verbs: [
          { gesture_id: "builtin.circle_cw", label: "↻ speed 1" },
          { gesture_id: "builtin.two_hand_separate", label: "✋✋ off" },
        ],
      },
      {
        type: "gesture.fired",
        ts,
        event_id: "evt-1",
        camera_id: "camera-main",
        gesture_id: "builtin.circle_cw",
        hand: "right",
        confidence: 0.93,
        mapping_ids: ["map-fan-speed-1"],
        action_summary: "Ventilador dormitorio → speed 1",
      },
      {
        type: "action.result",
        ts,
        activity_id: "act-1",
        event_id: "evt-1",
        mapping_id: "map-fan-speed-1",
        status: "ok",
        message: "Done · Home Assistant confirmed",
        latency: { detect_ms: 214, dispatch_ms: 2, ha_ms: 39 },
      },
    ]);

    expect(state.connection).toBe("open");
    expect(state.selectedTarget?.name).toBe("Ventilador dormitorio");
    expect(state.hud.state).toBe("done");
    expect(state.hud.title).toBe("Ventilador dormitorio → speed 1");
    expect(state.hud.icon).toBe("✓");
  });

  it("shows targeting recovery copy for suppressed targeted verbs", () => {
    const state = reduce([
      { type: "gesture.suppressed", ts, camera_id: "camera-main", gesture_id: "builtin.circle_cw", reason: "no_target" },
    ]);

    expect(state.suppression?.reason).toBe("no_target");
    expect(state.hud.title).toBe("Point at a device first");
  });
});
