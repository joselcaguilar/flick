import { useCameras, useCamerasAvailable, useStatus } from "../../api/hooks";
import type { Camera, CameraAvailable } from "../../api/types";
import { PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, ListRow, Skeleton } from "../../components/ui";
import { useEventStore } from "../../events/store";

function permissionTone(permission?: string | null) {
  if (permission === "authorized") return "success";
  if (permission === "denied" || permission === "restricted") return "danger";
  if (permission === "not_determined") return "warning";
  return "neutral";
}

function cameraTone(state?: string | null) {
  if (state === "running") return "success";
  if (state === "error") return "danger";
  if (state === "starting" || state === "reconnecting") return "warning";
  return "neutral";
}

function bestFormat(camera: CameraAvailable) {
  const format = camera.formats[0];
  if (!format) return "No format reported";
  return `${format.width}×${format.height} · ${format.fps} fps · ${format.format}`;
}

function ConfiguredCameraCard({ camera }: { camera: Camera }) {
  const status = useStatus();
  const live = status.data?.cameras.find(
    (item) => item.camera_id === camera.id || (item as { id?: string }).id === camera.id,
  );
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
        <Button variant="secondary" size="sm">
          Set as active
        </Button>
        <Button variant="ghost" size="sm">
          Open preview
        </Button>
      </div>
    </GlassPanel>
  );
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
  const hands = useEventStore((state) => state.hands["camera-main"]);
  const permission =
    status.data?.camera_permission ?? status.data?.cameras[0]?.camera_permission ?? "unknown";

  return (
    <section className="feature-screen cameras-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <p className="route-path">Cameras</p>
          <h1 id="screen-title">Local cameras</h1>
          <p>Phase 1 keeps setup local: built-in and Continuity cameras, permission state and live health.</p>
        </div>
        <Badge tone={permissionTone(permission)}>Permission · {String(permission).replace(/_/g, " ")}</Badge>
      </header>

      <div className="cameras-grid">
        <GlassPanel className="camera-preview-panel">
          <div className="preview-header">
            <div>
              <span>Active camera</span>
              <strong>{cameras.data?.[0]?.name ?? "MacBook Camera"}</strong>
            </div>
            <Badge tone={cameraTone(status.data?.cameras[0]?.state)}>
              {status.data?.cameras[0]?.fps ?? 0} fps
            </Badge>
          </div>
          <PreviewCanvas alt="Active camera preview" hands={hands?.hands} ray={hands?.ray} />
          <p className="camera-note">
            MJPEG preview uses the engine ticket flow. RTSP setup, ROI drawing and Unifi guidance are Phase 2.
          </p>
        </GlassPanel>

        <div className="camera-list-column">
          {cameras.isLoading ? (
            <Skeleton />
          ) : (
            cameras.data?.map((camera) => <ConfiguredCameraCard key={camera.id} camera={camera} />)
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
