import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { queryKeys } from "../api/hooks";
import { onServerMessage } from "./client";

const statusEvents = new Set(["hello", "engine.status", "camera.status", "engine.paused", "engine.resumed"]);

/** Refetches engine status when the engine reports a change, so the shell never shows a stale camera state. */
export function useStatusSync() {
  const client = useQueryClient();
  useEffect(
    () =>
      onServerMessage((message) => {
        if (statusEvents.has(message.type)) void client.invalidateQueries({ queryKey: queryKeys.status });
      }),
    [client],
  );
}
