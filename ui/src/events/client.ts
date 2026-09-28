import { getEndpoint } from "../api/client";
import { toWsUrl } from "../api/endpoint";
import { startMockEventStream } from "../mocks/ws";
import { normalizeWsMessage } from "./normalize";
import { useEventStore } from "./store";
import type { WsTopic } from "./types";

export interface EventClient {
  close: () => void;
}

const defaultTopics: WsTopic[] = [
  "status",
  "gestures",
  "actions",
  "ha",
  "capture",
  "targeting",
  "teach",
  "updates",
  "hands:camera-main",
];
let singleton: EventClient | undefined;

export async function startEventStream(topics: WsTopic[] = defaultTopics): Promise<EventClient> {
  if (singleton) return singleton;
  const endpoint = await getEndpoint();

  if (endpoint.mock) {
    singleton = startMockEventStream((message) => useEventStore.getState().ingest(message), topics);
    return singleton;
  }

  let ws: WebSocket | undefined;
  let reconnectTimer: number | undefined;
  let closed = false;
  let attempt = 0;

  const connect = () => {
    useEventStore
      .getState()
      .dispatch({ type: "connection", state: attempt > 0 ? "reconnecting" : "connecting" });
    ws = new WebSocket(toWsUrl(endpoint.baseUrl), ["flick.v1", `bearer.${endpoint.token}`]);

    ws.addEventListener("open", () => {
      attempt = 0;
      useEventStore.getState().dispatch({ type: "connection", state: "open" });
      ws?.send(JSON.stringify({ type: "subscribe", topics }));
    });

    ws.addEventListener("message", (event) => {
      useEventStore.getState().ingest(normalizeWsMessage(JSON.parse(event.data as string)));
    });

    ws.addEventListener("close", () => {
      if (closed) return;
      attempt += 1;
      useEventStore.getState().dispatch({ type: "connection", state: "reconnecting" });
      const delay = Math.min(30000, 500 * 2 ** attempt);
      reconnectTimer = window.setTimeout(connect, delay);
    });

    ws.addEventListener("error", () => {
      useEventStore.getState().dispatch({ type: "connection", state: "error" });
    });
  };

  connect();

  singleton = {
    close: () => {
      closed = true;
      if (reconnectTimer) window.clearTimeout(reconnectTimer);
      ws?.close();
      singleton = undefined;
      useEventStore.getState().dispatch({ type: "connection", state: "closed" });
    },
  };

  return singleton;
}

export async function restartEventStream(topics: WsTopic[] = defaultTopics): Promise<EventClient> {
  singleton?.close();
  return startEventStream(topics);
}
