import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { queryKeys } from "../api/hooks";
import { onServerMessage } from "./client";

const statusEvents = new Set([
  "hello",
  "engine.status",
  "camera.status",
  "engine.paused",
  "engine.resumed",
  "ha.status",
]);

/** Refetches engine status and activity when the engine reports a change, so no view shows stale state. */
export function useStatusSync() {
  const client = useQueryClient();
  useEffect(
    () =>
      onServerMessage((message) => {
        if (statusEvents.has(message.type)) void client.invalidateQueries({ queryKey: queryKeys.status });
        if (message.type === "action.result") void client.invalidateQueries({ queryKey: ["activity"] });
      }),
    [client],
  );
}
