import { useEffect } from "react";
import { useStatus } from "../api/hooks";
import type { CameraStatus } from "../api/types";
import { subscribeTopics } from "./client";
import { useEventStore } from "./store";

const mockCameraId = "camera-main";

function findStatusCamera(cameras: CameraStatus[] | undefined, cameraId?: string) {
  if (!cameraId) return undefined;
  return cameras?.find((item) => item.camera_id === cameraId || item.id === cameraId);
}

function mergeCameraLive(
  cameraId: string | undefined,
  statusCamera: CameraStatus | undefined,
  eventCamera: { state: CameraStatus["state"]; fps?: number | null; error?: string | null } | undefined,
): CameraStatus | undefined {
  if (!statusCamera && !eventCamera) return undefined;
  return {
    camera_id: statusCamera?.camera_id ?? cameraId ?? "camera-main",
    state: statusCamera?.state ?? "idle",
    ...statusCamera,
    ...eventCamera,
  };
}

export function useCameraLive(cameraId?: string) {
  const status = useStatus();
  const eventCamera = useEventStore((state) => (cameraId ? state.cameras[cameraId] : undefined));
  return mergeCameraLive(cameraId, findStatusCamera(status.data?.cameras, cameraId), eventCamera);
}

export function useActiveCamera() {
  const cameras = useStatus().data?.cameras ?? [];
  const eventCameras = useEventStore((state) => state.cameras);
  const merged: CameraStatus[] = cameras
    .map((camera) => mergeCameraLive(camera.camera_id, camera, eventCameras[camera.camera_id]))
    .filter((camera): camera is CameraStatus => Boolean(camera));
  for (const [cameraId, eventCamera] of Object.entries(eventCameras)) {
    if (!merged.some((camera) => camera.camera_id === cameraId)) {
      const camera = mergeCameraLive(cameraId, undefined, eventCamera);
      if (camera) merged.push(camera);
    }
  }
  const camera = merged.find((item) => item.state === "running") ?? merged[0];
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
