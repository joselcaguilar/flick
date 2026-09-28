import type { EventStreamState, WsServerMessage } from "./types";

export const initialEventState: EventStreamState = {
  connection: "idle",
  pro: false,
  cameras: {},
  ha: { state: "disconnected" },
  hands: {},
  hud: { state: "idle", title: "Flick is watching", detail: "Show me your hand" },
  activity: [],
  updates: [],
};

export type EventStreamAction =
  | { type: "connection"; state: EventStreamState["connection"] }
  | { type: "message"; message: WsServerMessage }
  | { type: "reset" };

function mark<T extends EventStreamState>(state: T, ts?: string): T {
  return { ...state, lastEventAt: ts ?? new Date().toISOString() };
}

function gestureName(gestureId?: string | null) {
  return gestureId
    ?.replace(/^builtin\./, "")
    .replace(/_/g, " ")
    .replace(/\\b\\w/g, (match) => match.toUpperCase());
}

function upsertActivity(activity: EventStreamState["activity"], item: EventStreamState["activity"][number]) {
  return [item, ...activity.filter((entry) => entry.id !== item.id)].slice(0, 50);
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
          activity: upsertActivity(state.activity, {
            id: `suppressed-${message.ts}-${message.gesture_id}`,
            ts: message.ts,
            status: "suppressed",
            gesture_id: message.gesture_id,
            gesture_name: gestureName(message.gesture_id),
            reason: message.reason,
            camera_id: message.camera_id,
          }),
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
          activity: upsertActivity(state.activity, {
            id: message.event_id,
            ts: message.ts,
            status: "sent",
            gesture_id: message.gesture_id,
            gesture_name: gestureName(message.gesture_id),
            confidence: message.confidence,
            mapping_id: message.mapping_ids[0],
            action_summary: message.action_summary,
            camera_id: message.camera_id,
          }),
          hud: {
            state: "sent",
            title: message.action_summary,
            detail: "Sending to Home Assistant…",
            icon: message.gesture_id.replace(/^builtin\./, "").replace(/_/g, "-"),
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "action.result":
      return mark(
        {
          ...state,
          activity: upsertActivity(state.activity, {
            id: message.activity_id,
            ts: message.ts,
            status: message.status,
            mapping_id: message.mapping_id,
            action_summary: state.hud.title,
            message: message.message,
            latency: message.latency,
          }),
          hud: {
            state: message.status === "ok" ? "done" : "failed",
            title: state.hud.title,
            detail:
              message.message ??
              (message.status === "ok" ? "Done · Home Assistant confirmed" : "Home Assistant unavailable"),
            icon: message.status === "ok" ? "ok" : "error",
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
            icon: "pinch-dial",
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
            icon: "open-palm",
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
            title: "Confirm with Thumbs up",
            detail: "Sensitive device · 3 s",
            icon: "thumbs-up",
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
            detail: "Selected for 4 seconds",
            icon: message.domain,
            verbs: message.verbs,
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
      return mark(state, message.ts);
    case "teach.progress":
      return mark(
        {
          ...state,
          teach: {
            session_id: message.session_id,
            phase: message.phase,
            ray_jitter_deg: message.ray_jitter_deg,
            confidence: message.confidence,
            hint: message.hint,
            updatedAt: message.ts,
          },
        },
        message.ts,
      );
    case "resync":
      return mark(state, message.ts);
    default:
      return state;
  }
}
