import { useEffect } from "react";
import { useStatus } from "../api/hooks";
import { subscribeTopics } from "./client";
import { useEventStore } from "./store";

const mockCameraId = "camera-main";

export function useActiveCamera() {
  const cameras = useStatus().data?.cameras ?? [];
  const camera = cameras.find((item) => item.state === "running") ?? cameras[0];
  return { id: camera?.camera_id, running: camera?.state === "running" };
}

export function useLiveHands(cameraId?: string) {
  const active = useActiveCamera();
  const id = cameraId ?? active.id;

  useEffect(() => {
    if (id) subscribeTopics([`hands:${id}`]);
  }, [id]);

  return useEventStore((state) => (id ? state.hands[id] : undefined) ?? state.hands[mockCameraId]);
}
