import { create } from "zustand";
import { eventReducer, initialEventState, type EventStreamAction } from "./reducer";
import type { EventStreamState, WsServerMessage } from "./types";

interface EventStore extends EventStreamState {
  dispatch: (action: EventStreamAction) => void;
  ingest: (message: WsServerMessage) => void;
}

export const useEventStore = create<EventStore>((set) => ({
  ...initialEventState,
  dispatch: (action) => set((state) => eventReducer(state, action)),
  ingest: (message) => set((state) => eventReducer(state, { type: "message", message })),
}));
