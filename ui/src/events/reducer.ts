import type { EventStreamState, WsServerMessage } from "./types";

export const initialEventState: EventStreamState = {
  connection: "idle",
  pro: false,
  cameras: {},
  ha: { state: "disconnected" },
  hands: {},
  hud: { state: "idle", title: "Flick is watching", detail: "Show me your hand" },
  updates: [],
};

export type EventStreamAction =
  | { type: "connection"; state: EventStreamState["connection"] }
  | { type: "message"; message: WsServerMessage }
  | { type: "reset" };

function mark<T extends EventStreamState>(state: T, ts?: string): T {
  return { ...state, lastEventAt: ts ?? new Date().toISOString() };
}

export function eventReducer(state: EventStreamState, action: EventStreamAction): EventStreamState {
  if (action.type === "reset") return initialEventState;
  if (action.type === "connection") return { ...state, connection: action.state };

  const message = action.message;
  switch (message.type) {
    case "hello":
      return mark(
        {
          ...state,
          connection: "open",
          engineVersion: message.engine_version,
          api: message.api,
          pro: message.pro,
        },
        message.ts,
      );
    case "engine.status":
      return mark({ ...state, engine: message.status }, message.ts);
    case "camera.status":
      return mark(
        {
          ...state,
          cameras: {
            ...state.cameras,
            [message.camera_id]: { state: message.state, fps: message.fps ?? 0, error: message.error },
          },
        },
        message.ts,
      );
    case "hands":
      return mark(
        {
          ...state,
          hands: {
            ...state.hands,
            [message.camera_id]: { seq: message.seq, hands: message.hands, ray: message.ray },
          },
        },
        message.ts,
      );
    case "gesture.candidate":
      return mark(
        {
          ...state,
          candidate: {
            gesture_id: message.gesture_id,
            confidence: message.confidence,
            progress: message.progress,
            camera_id: message.camera_id,
          },
          hud: {
            state: "candidate",
            title: "Gesture candidate",
            detail: `${message.gesture_id.replace("builtin.", "")} · ${Math.round(message.confidence * 100)}%`,
            progress: message.progress,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "gesture.suppressed":
      return mark(
        {
          ...state,
          suppression: { gesture_id: message.gesture_id, reason: message.reason, ts: message.ts },
          hud:
            message.reason === "no_target"
              ? {
                  state: "aiming",
                  title: "Point at a device first",
                  detail: "Circle is a device verb",
                  updatedAt: message.ts,
                }
              : state.hud,
        },
        message.ts,
      );
    case "gesture.fired":
      return mark(
        {
          ...state,
          hud: {
            state: "sent",
            title: message.action_summary,
            detail: "Sending to Home Assistant…",
            icon: "↻",
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "action.result":
      return mark(
        {
          ...state,
          hud: {
            state: message.status === "ok" ? "done" : "failed",
            title: state.hud.title,
            detail:
              message.message ??
              (message.status === "ok" ? "Done · Home Assistant confirmed" : "Home Assistant unavailable"),
            icon: message.status === "ok" ? "✓" : "✕",
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "gesture.update":
      return mark(
        {
          ...state,
          hud: {
            state: "dial",
            title: state.selectedTarget?.name ?? "Dial",
            detail: typeof message.value === "number" ? `${Math.round(message.value * 100)}%` : "Adjusting",
            icon: "◌",
            progress: typeof message.value === "number" ? message.value : undefined,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "ha.status":
      return mark({ ...state, ha: { state: message.state, ha_version: message.ha_version } }, message.ts);
    case "armed":
      return mark(
        {
          ...state,
          hud: {
            state: "armed",
            title: "Listening… 4 s",
            detail: "Confirming the next gesture",
            icon: "◆",
            expiresAt: message.until,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "confirm.required":
      return mark(
        {
          ...state,
          hud: {
            state: "confirm",
            title: "Confirm with 👍",
            detail: "Sensitive device · 3 s",
            icon: "🔒",
            expiresAt: message.expires_at,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "engine.paused":
      return mark(
        {
          ...state,
          hud: {
            state: "paused",
            title: "Flick paused",
            detail: message.until ? `Until ${message.until}` : "Until resumed",
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "engine.resumed":
      return mark(
        {
          ...state,
          hud: {
            state: "idle",
            title: "Flick is watching",
            detail: "Show me your hand",
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "target.hover":
      return mark(
        {
          ...state,
          hud: {
            state: "aiming",
            title: `${message.name}?`,
            detail: "Hold still to select",
            progress: message.dwell_progress,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "target.selected":
      return mark(
        {
          ...state,
          selectedTarget: message,
          hud: {
            state: "selected",
            title: message.name,
            detail: message.verbs.map((verb) => verb.label).join(" · "),
            icon: message.domain,
            expiresAt: message.expires_at,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "target.cleared":
      return mark(
        {
          ...state,
          selectedTarget: undefined,
          hud: { state: "idle", title: "Flick is watching", detail: "Show me your hand" },
        },
        message.ts,
      );
    case "target.ambiguous":
      return mark(
        {
          ...state,
          hud: {
            state: "ambiguous",
            title: "Two devices here",
            detail: "Point more precisely",
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "place.status":
      return mark(
        {
          ...state,
          place: message,
          hud:
            message.state === "needs_realign"
              ? {
                  state: "camera_moved",
                  title: "Camera moved",
                  detail: "Re-align your devices",
                  updatedAt: message.ts,
                }
              : state.hud,
        },
        message.ts,
      );
    case "update.available":
    case "update.ready": {
      const id = "id" in message && message.id ? message.id : "app";
      return mark(
        {
          ...state,
          updates: [
            ...state.updates.filter((update) => update.id !== id),
            { id, kind: message.kind, version: message.version },
          ],
        },
        message.ts,
      );
    }
    case "update.progress":
      return mark(
        {
          ...state,
          updates: [
            ...state.updates.filter((update) => update.id !== message.id),
            { id: message.id, kind: message.kind, version: "", phase: message.phase },
          ],
        },
        message.ts,
      );
    case "model.activated":
    case "model.rolled_back":
    case "ha.entity":
    case "capture.progress":
    case "gesture.end":
    case "disarmed":
    case "teach.progress":
    case "resync":
      return mark(state, message.ts);
    default:
      return state;
  }
}
