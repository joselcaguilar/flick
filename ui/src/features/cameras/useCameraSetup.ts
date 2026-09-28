import { useCameras, useCamerasAvailable, useCreateCamera, useStartCamera } from "../../api/hooks";
import type { Camera } from "../../api/types";
import { type CameraPermission, cameraRequestAccess, isTauri } from "../../platform/tauri";

export type CameraSetupResult = { permission: CameraPermission; camera?: Camera };

export function useCameraSetup() {
  const available = useCamerasAvailable();
  const configured = useCameras();
  const createCamera = useCreateCamera();
  const startCamera = useStartCamera();

  async function ensureStarted(deviceRef?: string): Promise<Camera> {
    const [availableResult, configuredResult] = await Promise.all([
      available.refetch(),
      configured.refetch(),
    ]);
    const availableList = availableResult.data ?? [];
    const configuredList = configuredResult.data ?? [];
    const device =
      availableList.find((camera) => camera.device_ref === deviceRef) ??
      availableList.find((camera) => camera.kind === "local") ??
      availableList[0];
    const existing =
      configuredList.find((camera) => camera.device_ref && camera.device_ref === device?.device_ref) ??
      (deviceRef ? undefined : configuredList[0]);
    const camera =
      existing ??
      (device
        ? await createCamera.mutateAsync({
            name: device.name,
            kind: device.kind,
            device_ref: device.device_ref,
          })
        : undefined);
    if (!camera) throw new Error("No camera was found. Connect a camera, then check again.");
    await startCamera.mutateAsync(camera.id);
    return camera;
  }

  async function allowAndStart(deviceRef?: string): Promise<CameraSetupResult> {
    // The desktop process owns the macOS prompt so it is attributed to Flick.app.
    const permission = isTauri() ? await cameraRequestAccess() : "authorized";
    if (permission !== "authorized") return { permission };
    return { permission, camera: await ensureStarted(deviceRef) };
  }

  return { allowAndStart, ensureStarted, busy: createCamera.isPending || startCamera.isPending };
}
