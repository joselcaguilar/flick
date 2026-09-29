import { useState } from "react";
import { useCameras, useCamerasAvailable, useStartCamera, useStatus, useStopCamera } from "../../api/hooks";
import type { Camera, CameraAvailable, CameraStatus } from "../../api/types";
import { PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, ListRow, Skeleton, useToast } from "../../components/ui";
import { useCameraLive, useLiveHands } from "../../events/useLiveHands";
import { isTauri } from "../../platform/tauri";
import { useCameraSetup } from "./useCameraSetup";

function permissionTone(permission?: string | null) {
  if (permission === "authorized") return "success";
  if (permission === "denied" || permission === "restricted") return "danger";
  if (permission === "not_determined") return "warning";
  return "neutral";
}

const permissionLabels: Record<string, string> = {
  authorized: "Camera access allowed",
  denied: "Camera access denied",
  restricted: "Camera access restricted",
  not_determined: "Camera access not asked yet",
};

function cameraTone(state?: string | null) {
  if (state === "running") return "success";
  if (state === "error" || state === "permission_denied") return "danger";
  if (state === "starting" || state === "reconnecting" || state === "paused") return "warning";
  return "neutral";
}

function bestFormat(camera: CameraAvailable) {
  const format = camera.formats[0];
  if (!format) return "No format reported";
  return `${format.width}×${format.height} · ${format.fps} fps · ${format.format}`;
}

function ConfiguredCameraCard({ camera, live }: { camera: Camera; live?: CameraStatus }) {
  const start = useStartCamera();
  const stop = useStopCamera();
  const running = live?.state === "running" || live?.state === "starting" || live?.state === "reconnecting";
  const failed = live?.state === "error" || live?.state === "permission_denied";
  const pending = start.isPending || stop.isPending;

  function toggleCamera() {
    if (running) {
      stop.mutate(camera.id);
      return;
    }
    start.mutate(camera.id);
  }

  return (
    <GlassPanel className="camera-card">
      <div className="panel-heading">
        <div>
          <span>{camera.kind}</span>
          <strong>{camera.name}</strong>
        </div>
        <Badge tone={cameraTone(live?.state)}>{live?.state ?? (camera.enabled ? "idle" : "disabled")}</Badge>
      </div>
      <div className="camera-details-grid">
        <ListRow
          title="Device"
          description={camera.device_ref ?? "local camera"}
          trailing={camera.mirror ? "mirrored" : "not mirrored"}
        />
        <ListRow
          title="Capture"
          description={`${camera.active_fps} fps active · ${camera.idle_fps} fps idle`}
          trailing={`${camera.max_hands} hands`}
        />
        <ListRow title="Rotation" description={`${camera.rotation}°`} trailing="ROI saved" />
      </div>
      <div className="camera-actions">
        <Button variant={running ? "danger" : "primary"} size="sm" loading={pending} onClick={toggleCamera}>
          {running ? "Stop camera" : camera.enabled ? "Start camera" : "Enable camera"}
        </Button>
      </div>
      {failed ? (
        <p className="inline-error" role="alert">
          {live?.error ?? "The camera couldn't start. Check the camera connection and try again."}
        </p>
      ) : null}
    </GlassPanel>
  );
}

function ConfiguredCameraCardLive({ camera }: { camera: Camera }) {
  const live = useCameraLive(camera.id);
  return <ConfiguredCameraCard camera={camera} live={live} />;
}

function AvailableCameraRow({ camera }: { camera: CameraAvailable }) {
  return (
    <ListRow
      title={camera.name}
      description={bestFormat(camera)}
      leading={<Badge tone="accent">{camera.kind}</Badge>}
      trailing={camera.kind === "local" ? "Ready" : "Later"}
    />
  );
}

export function CamerasRoute() {
  const cameras = useCameras();
  const available = useCamerasAvailable();
  const status = useStatus();
  const { show } = useToast();
  const [openingSettings, setOpeningSettings] = useState(false);
  const permission =
    status.data?.camera_permission ?? status.data?.cameras[0]?.camera_permission ?? "unknown";
  const activeCamera = cameras.data?.[0];
  const activeLive = useCameraLive(activeCamera?.id ?? status.data?.cameras[0]?.camera_id);
  const running = activeLive?.state === "running";
  const hands = useLiveHands(activeLive?.camera_id ?? activeCamera?.id);
  const setup = useCameraSetup();
  const blocked = permission === "denied" || permission === "restricted";
  const setupLabel =
    permission !== "authorized" ? "Allow camera" : cameras.data?.length ? "Start camera" : "Set up camera";

  async function setUpCamera() {
    try {
      const result = await setup.allowAndStart(activeCamera?.device_ref ?? undefined);
      if (result.permission !== "authorized") {
        show({
          tone: "warning",
          title: "Camera access was not granted",
          description: "Allow Flick in System Settings → Privacy & Security → Camera.",
        });
      }
    } catch (error) {
      show({
        tone: "danger",
        title: "Could not start the camera",
        description: error instanceof Error ? error.message : "Check the camera connection and try again.",
      });
    }
  }

  async function openCameraPrivacySettings() {
    if (!isTauri()) return;
    setOpeningSettings(true);
    try {
      const tauri = await import("@tauri-apps/api/core");
      await tauri.invoke("open_camera_privacy_settings");
    } catch (error) {
      show({
        tone: "danger",
        title: "Could not open System Settings",
        description: error instanceof Error ? error.message : "Open Privacy & Security → Camera manually.",
      });
    } finally {
      setOpeningSettings(false);
    }
  }

  return (
    <section className="feature-screen cameras-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Cameras</h1>
          <p>Built-in and Continuity cameras. Video never leaves this Mac.</p>
        </div>
        <div className="header-actions">
          {permissionLabels[permission] ? (
            <Badge tone={permissionTone(permission)}>{permissionLabels[permission]}</Badge>
          ) : null}
          {permission === "denied" && isTauri() ? (
            <Button variant="primary" size="sm" loading={openingSettings} onClick={openCameraPrivacySettings}>
              Open System Settings
            </Button>
          ) : null}
          {!blocked && !running ? (
            <Button variant="primary" size="sm" loading={setup.busy} onClick={() => void setUpCamera()}>
              {setupLabel}
            </Button>
          ) : null}
        </div>
      </header>

      <div className="cameras-grid">
        <GlassPanel className="camera-preview-panel">
          <div className="preview-header">
            <div>
              <span>Active camera</span>
              <strong>{activeCamera?.name ?? activeLive?.name ?? "MacBook Camera"}</strong>
            </div>
            <Badge tone={cameraTone(activeLive?.state)}>
              {running ? `${activeLive?.fps ?? 0} fps` : (activeLive?.state ?? "stopped")}
            </Badge>
          </div>
          {permission === "denied" || permission === "restricted" ? (
            <div className="camera-permission-panel" role="alert">
              <strong>Camera permission is {String(permission).replace(/_/g, " ")}.</strong>
              <span>Grant access in macOS System Settings, then start the camera again.</span>
              {isTauri() ? (
                <Button
                  variant="primary"
                  size="sm"
                  loading={openingSettings}
                  onClick={openCameraPrivacySettings}
                >
                  Open System Settings
                </Button>
              ) : null}
            </div>
          ) : (
            <PreviewCanvas
              cameraId={running ? (activeCamera?.id ?? activeLive?.camera_id) : undefined}
              alt={running ? "Live camera preview" : "Start a camera to show live preview"}
              hands={running ? hands?.hands : []}
              ray={running ? hands?.ray : undefined}
            />
          )}
          <p className="camera-note">
            {running
              ? "Video stays on this Mac. Flick keeps hand points, never images."
              : "Start the camera to see the live preview. Network cameras (RTSP, UniFi Protect) come in a later release."}
          </p>
        </GlassPanel>

        <div className="camera-list-column">
          {cameras.isLoading ? (
            <Skeleton />
          ) : (
            cameras.data?.map((camera) => <ConfiguredCameraCardLive key={camera.id} camera={camera} />)
          )}
          <GlassPanel className="camera-card">
            <div className="panel-heading">
              <div>
                <span>Discovered</span>
                <strong>Available local devices</strong>
              </div>
              <Badge tone="accent">{available.data?.length ?? 0}</Badge>
            </div>
            <div className="camera-details-grid">
              {available.data?.map((camera) => (
                <AvailableCameraRow key={camera.device_ref} camera={camera} />
              ))}
            </div>
          </GlassPanel>
        </div>
      </div>
    </section>
  );
}
