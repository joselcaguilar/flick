import { Link } from "react-router-dom";
import { useActivity, useCameras, useInstallUpdate, useStatus, useUpdates } from "../../api/hooks";
import type { Camera } from "../../api/types";
import { ConfidenceMeter, DevicePill, PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, Kbd, ListRow, Select, Skeleton } from "../../components/ui";
import { useEventStore } from "../../events/store";
import { useActiveCamera, useCameraLive, useLiveHands } from "../../events/useLiveHands";
import { formatTime } from "../../lib/utils";
import { useCameraPlace } from "../cameras/place";

function statusLabel(status?: string | null) {
  if (!status) return "Unknown";
  const label = status.replace(/_/g, " ");
  return label.charAt(0).toUpperCase() + label.slice(1);
}

function statusTone(status?: string | null): "neutral" | "accent" | "success" | "warning" | "danger" {
  if (status === "ok" || status === "ready" || status === "running") return "success";
  if (status === "error" || status === "auth_failed" || status === "needs_realign") return "danger";
  if (status === "suppressed" || status === "paused") return "warning";
  return "neutral";
}

function StatusTile({
  label,
  value,
  detail,
  tone = "neutral",
}: {
  label: string;
  value: string;
  detail?: string;
  tone?: string;
}) {
  return (
    <div className="status-tile" data-tone={tone}>
      <span>{label}</span>
      <strong>{value}</strong>
      {detail ? <small>{detail}</small> : null}
    </div>
  );
}

const placeSources = {
  flick: "Set in Flick",
  ha: "From Home Assistant",
  none: "Not in Home Assistant",
};

/** Where the live camera is, changeable in Flick without touching Home Assistant. */
function PlaceTile({ camera }: { camera?: Camera }) {
  const place = useCameraPlace(camera);
  if (!place.select) {
    return <StatusTile label="Place" value="Not set" detail="Add a camera to place it" />;
  }
  return (
    <div className="status-tile place-tile">
      <span>Place</span>
      <Select {...place.select} className="ui-select-quiet" display={place.name ?? "Choose a room"} />
      {place.failed ? (
        <small role="alert">Couldn't save. Try again.</small>
      ) : (
        <small>{placeSources[place.source]}</small>
      )}
    </div>
  );
}

export function DashboardRoute() {
  const status = useStatus();
  const updates = useUpdates();
  const activity = useActivity("?limit=10");
  const eventState = useEventStore();
  const installUpdate = useInstallUpdate();
  const cameras = useCameras();
  const active = useActiveCamera();
  const camera = useCameraLive(active.id);
  const running = camera?.state === "running";
  const liveHands = useLiveHands(active.id);
  const engine = status.data;
  const activeCamera = cameras.data?.find((item) => item.id === active.id) ?? cameras.data?.[0];
  const cameraName = activeCamera?.name ?? (active.id ? "Camera" : "No camera yet");
  const target = eventState.selectedTarget;
  const candidate = eventState.candidate;
  const inference =
    engine?.inference_ms_p95 ?? engine?.stages.find((stage) => stage.stage === "inference")?.p95_ms;
  const updateReady =
    updates.data?.app.state === "ready" || eventState.updates.some((update) => update.kind === "app");
  const updateVersion = updates.data?.app.available;

  return (
    <section className="dashboard-shell" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Dashboard</h1>
          <p>What Flick sees, what it selected, and what Home Assistant did.</p>
        </div>
        <div className="header-actions">
          <Link className="ui-button ui-button-primary ui-button-md" to="/devices/teach">
            Teach a device
          </Link>
        </div>
      </header>

      <div className="dashboard-grid">
        <div className="dashboard-main-column">
          {status.isLoading ? (
            <Skeleton className="hero-preview-skeleton" />
          ) : status.isError ? (
            <GlassPanel className="error-panel" role="alert">
              <strong>Something needs attention</strong>
              <p>Flick will keep trying. Use the command palette for recovery actions.</p>
            </GlassPanel>
          ) : (
            <GlassPanel className="preview-panel dashboard-preview-panel">
              <div className="preview-header">
                <div>
                  <span>{cameraName}</span>
                  <strong>Live preview</strong>
                </div>
                {running ? (
                  <Badge tone="success">{Math.round(camera?.fps ?? 0)} fps</Badge>
                ) : (
                  <Badge>{statusLabel(camera?.state ?? "stopped")}</Badge>
                )}
              </div>
              <div className="dashboard-stage">
                <PreviewCanvas
                  cameraId={running ? active.id : undefined}
                  alt={running ? "Live camera preview with hand tracking" : "Camera is off"}
                  hands={running ? liveHands?.hands : []}
                  ray={running ? liveHands?.ray : undefined}
                />
                {running && (target || candidate) ? (
                  <div className="preview-overlay-card">
                    {target ? (
                      <DevicePill name={target.name} domain={target.domain} detail="selected" />
                    ) : (
                      <span>Point at a device to select it</span>
                    )}
                    {candidate ? (
                      <ConfidenceMeter
                        value={candidate.confidence}
                        label={candidate.gesture_id.replace("builtin.", "")}
                      />
                    ) : null}
                  </div>
                ) : null}
              </div>
              {running ? null : (
                <p className="camera-note">
                  The camera is off. <Link to="/cameras">Start it from Cameras</Link> to see what Flick sees.
                </p>
              )}
            </GlassPanel>
          )}

          <GlassPanel className="activity-panel dashboard-activity-panel">
            <div className="panel-heading">
              <div>
                <span>Last 10 gestures and their Home Assistant results</span>
                <strong>Recent activity</strong>
              </div>
              <Link to="/activity">View all</Link>
            </div>
            <div className="activity-list dashboard-activity-list">
              {(activity.data?.items ?? []).map((item) => (
                <ListRow
                  key={item.id}
                  leading={<Badge tone={statusTone(item.status)}>{item.status}</Badge>}
                  title={item.action_summary ?? "Activity"}
                  description={item.message ?? item.reason ?? "No details"}
                  trailing={formatTime(item.ts)}
                />
              ))}
              {!activity.isLoading && !activity.data?.items.length ? (
                <p className="empty-note">No gestures yet. Try a flick and it shows up here.</p>
              ) : null}
            </div>
          </GlassPanel>
        </div>

        <aside className="dashboard-side-column" aria-label="Dashboard side panels">
          <section className="status-strip dashboard-status-strip" aria-label="Status">
            <StatusTile
              label="Camera"
              value={statusLabel(camera?.state)}
              detail={camera?.camera_permission ?? engine?.camera_permission ?? undefined}
              tone={statusTone(camera?.state)}
            />
            <StatusTile
              label="Home Assistant"
              value={statusLabel(engine?.ha.state)}
              detail={engine?.ha.ha_version ? `HA ${engine.ha.ha_version}` : undefined}
              tone={statusTone(engine?.ha.state)}
            />
            <StatusTile
              label="Engine"
              value={engine?.paused ? "Paused" : "Watching"}
              detail={inference ? `${inference.toFixed(1)} ms p95` : "local"}
              tone={engine?.paused ? "warning" : "success"}
            />
            <PlaceTile camera={activeCamera} />
          </section>

          {updateReady ? (
            <GlassPanel className="update-banner dashboard-update-banner">
              <Badge tone="warning">Update ready</Badge>
              <span>
                {updateVersion
                  ? `Flick ${updateVersion} is ready to install.`
                  : "An update is ready to install."}
              </span>
              <Button
                size="sm"
                loading={installUpdate.isPending}
                onClick={() => void installUpdate.mutateAsync({ kind: "app" })}
              >
                Restart to update
              </Button>
            </GlassPanel>
          ) : null}

          <GlassPanel className="quick-actions dashboard-quick-actions">
            <div className="panel-heading">
              <div>
                <span>
                  Or press <Kbd>⌘K</Kbd> anywhere
                </span>
                <strong>Quick actions</strong>
              </div>
            </div>
            <div className="quick-action-row">
              <Link className="ui-button ui-button-secondary ui-button-md" to="/devices/teach">
                Add a device
              </Link>
              <Link className="ui-button ui-button-secondary ui-button-md" to="/gestures">
                Record a gesture
              </Link>
              <Link className="ui-button ui-button-secondary ui-button-md" to="/cameras">
                Camera status
              </Link>
            </div>
          </GlassPanel>
        </aside>
      </div>
    </section>
  );
}
