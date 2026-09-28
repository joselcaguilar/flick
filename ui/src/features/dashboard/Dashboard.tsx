import { Link } from "react-router-dom";
import { useActivity, useStatus, useUpdates } from "../../api/hooks";
import { ConfidenceMeter, DevicePill, PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, Kbd, ListRow, Skeleton } from "../../components/ui";
import { useEventStore } from "../../events/store";
import { formatTime } from "../../lib/utils";

function statusLabel(status?: string | null) {
  if (!status) return "Unknown";
  return status.replace(/_/g, " ");
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

export function DashboardRoute() {
  const status = useStatus();
  const updates = useUpdates();
  const activity = useActivity("?limit=10");
  const eventState = useEventStore();
  const liveHands = eventState.hands["camera-main"];
  const engine = status.data;
  const camera = engine?.cameras[0];
  const inference =
    engine?.inference_ms_p95 ?? engine?.stages.find((stage) => stage.stage === "inference")?.p95_ms;
  const updateReady =
    updates.data?.app.state === "ready" || eventState.updates.some((update) => update.kind === "app");

  return (
    <section className="dashboard-shell" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <p className="route-path">Home / Dashboard</p>
          <h1 id="screen-title">Dashboard</h1>
          <p>Live camera, targeting status and recent Home Assistant outcomes.</p>
        </div>
        <div className="header-actions">
          <Button variant="primary" size="sm">
            Pause 15 min
          </Button>
          <Link className="ui-button ui-button-secondary ui-button-sm" to="/devices/teach">
            Teach device
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
                  <span>Live preview</span>
                  <strong>{camera?.name ?? camera?.camera_id ?? "MacBook Camera"}</strong>
                </div>
                <Badge tone="success">{camera?.fps ?? 0} fps</Badge>
              </div>
              <PreviewCanvas
                alt="Mock camera preview with hand skeleton overlay"
                hands={liveHands?.hands}
                ray={liveHands?.ray}
              />
              <div className="preview-overlay-card">
                <DevicePill
                  name={eventState.selectedTarget?.name ?? "Ventilador dormitorio"}
                  domain={eventState.selectedTarget?.domain ?? "fan"}
                  detail={eventState.selectedTarget ? "selected · 4 s" : "ray steady"}
                />
                <ConfidenceMeter value={eventState.candidate?.confidence ?? 0.88} label="Ray steady" />
              </div>
            </GlassPanel>
          )}

          <GlassPanel className="activity-panel dashboard-activity-panel">
            <div className="panel-heading">
              <div>
                <span>Recent activity</span>
                <strong>What Flick did</strong>
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
                <p>No gestures yet. Try a flick to see it here.</p>
              ) : null}
            </div>
          </GlassPanel>
        </div>

        <aside className="dashboard-side-column" aria-label="Dashboard side panels">
          <section className="status-strip dashboard-status-strip" aria-label="Status">
            <StatusTile
              label="Camera"
              value={statusLabel(camera?.state)}
              detail={camera?.camera_permission ?? "authorized"}
              tone={statusTone(camera?.state)}
            />
            <StatusTile
              label="Home Assistant"
              value={statusLabel(engine?.ha.state)}
              detail={engine?.ha.ha_version ?? "2026.9"}
              tone={statusTone(engine?.ha.state)}
            />
            <StatusTile
              label="Engine"
              value={engine?.paused ? "Paused" : "Watching"}
              detail={inference ? `${inference.toFixed(1)} ms p95` : "local"}
              tone={engine?.paused ? "warning" : "success"}
            />
            <StatusTile
              label="Place"
              value={engine?.place?.name ?? "Bedroom desk"}
              detail={statusLabel(engine?.place?.state ?? "ok")}
              tone={statusTone(engine?.place?.state ?? "ok")}
            />
          </section>

          {updateReady ? (
            <GlassPanel className="update-banner dashboard-update-banner">
              <Badge tone="warning">Update ready</Badge>
              <span>Flick 1.3 is ready — restart to update.</span>
              <Button size="sm" variant="ghost">
                Restart
              </Button>
            </GlassPanel>
          ) : null}

          <GlassPanel className="quick-actions dashboard-quick-actions">
            <div className="panel-heading">
              <div>
                <span>Quick actions</span>
                <strong>Common command paths</strong>
              </div>
              <Kbd>⌘K</Kbd>
            </div>
            <div className="quick-action-row">
              <Link className="ui-button ui-button-primary ui-button-md" to="/mappings/new">
                Add mapping
              </Link>
              <Link className="ui-button ui-button-secondary ui-button-md" to="/devices/teach">
                Teach a device
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
