import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { useActivity, useStatus, useUpdates } from "../api/hooks";
import { ConfidenceMeter, DevicePill, GestureGlyph, PreviewCanvas } from "../components/domain";
import { Badge, Button, GlassPanel, Kbd, ListRow, Skeleton } from "../components/ui";
import { useEventStore } from "../events/store";
import { formatTime } from "../lib/utils";

export interface RouteMeta {
  path: string;
  title: string;
  shortTitle?: string;
  description: string;
  nav?: boolean;
  mobile?: boolean;
  shortcut?: string;
  element: ReactNode;
}

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

function EmptyRoute({ title, description, action }: { title: string; description: string; action?: string }) {
  return (
    <section className="route-panel empty-route" aria-labelledby="screen-title">
      <div>
        <h1 id="screen-title">{title}</h1>
        <p>{description}</p>
      </div>
      <GlassPanel className="empty-state-panel">
        <GestureGlyph name="point" />
        <h2>{action ?? "Ready for the next UI lane"}</h2>
        <p>
          The route, loading state, empty state and command-palette entry are wired. Add screen-specific
          features inside <code>src/features</code>.
        </p>
      </GlassPanel>
    </section>
  );
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

export function HomeRoute() {
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
    <section className="home-grid" aria-labelledby="screen-title">
      <div className="hero-copy">
        <p className="route-path">Home / Dashboard</p>
        <h1 id="screen-title">Point, lock, flick.</h1>
        <p>
          Flick is watching locally. Video stays on this Mac while hand points, selected devices and Home
          Assistant outcomes stay visible.
        </p>
      </div>

      {status.isLoading ? (
        <Skeleton className="hero-preview-skeleton" />
      ) : status.isError ? (
        <GlassPanel className="error-panel" role="alert">
          <strong>Something needs attention</strong>
          <p>Flick will keep trying. Use the command palette for recovery actions.</p>
        </GlassPanel>
      ) : (
        <GlassPanel className="preview-panel">
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

      <section className="status-strip" aria-label="Status">
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
        <GlassPanel className="update-banner">
          <Badge tone="warning">Update ready</Badge>
          <span>Flick 1.3 is ready — restart to update.</span>
          <Button size="sm" variant="ghost">
            Restart
          </Button>
        </GlassPanel>
      ) : null}

      <GlassPanel className="activity-panel">
        <div className="panel-heading">
          <div>
            <span>Recent activity</span>
            <strong>What Flick did</strong>
          </div>
          <Link to="/activity">View all</Link>
        </div>
        <div className="activity-list">
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

      <GlassPanel className="quick-actions">
        <div className="panel-heading">
          <div>
            <span>Quick actions</span>
            <strong>Common command paths</strong>
          </div>
          <Kbd>⌘K</Kbd>
        </div>
        <div className="quick-action-row">
          <Button variant="primary">Pause 15 min</Button>
          <Link className="ui-button ui-button-secondary ui-button-md" to="/mappings/new">
            Add mapping
          </Link>
          <Link className="ui-button ui-button-secondary ui-button-md" to="/gestures/new">
            Record gesture
          </Link>
          <Link className="ui-button ui-button-ghost ui-button-md" to="/devices/teach">
            Teach a device
          </Link>
        </div>
      </GlassPanel>
    </section>
  );
}

export const routes: RouteMeta[] = [
  {
    path: "/",
    title: "Home",
    description: "Dashboard, live preview and recent activity.",
    nav: true,
    mobile: true,
    shortcut: "⌘1",
    element: <HomeRoute />,
  },
  {
    path: "/onboarding",
    title: "Onboarding",
    description: "First-run camera, Home Assistant and first flick flow.",
    element: (
      <EmptyRoute
        title="Onboarding"
        description="Get a new household from camera permission to first gesture in under three minutes."
        action="First run scaffold"
      />
    ),
  },
  {
    path: "/gestures",
    title: "Gestures",
    description: "Built-in and custom gesture library.",
    nav: true,
    mobile: true,
    shortcut: "⌘2",
    element: (
      <EmptyRoute
        title="Gestures library"
        description="Built-ins, custom gestures, enable toggles and update notes live here."
        action="No custom gestures yet"
      />
    ),
  },
  {
    path: "/gestures/new",
    title: "Gesture Studio",
    description: "Record static, motion or two-hand gestures.",
    element: (
      <EmptyRoute
        title="Gesture Studio"
        description="Record takes, train locally and test the gesture before mapping it."
        action="Ready to record"
      />
    ),
  },
  {
    path: "/devices",
    title: "Devices",
    description: "Taught devices grouped by place.",
    nav: true,
    mobile: true,
    shortcut: "⌘3",
    element: (
      <EmptyRoute
        title="Devices"
        description="Point at something in the room — Flick will remember it."
        action="No devices taught"
      />
    ),
  },
  {
    path: "/devices/teach",
    title: "Teach a device",
    description: "Pick a device, point from two spots and test verbs.",
    element: (
      <EmptyRoute
        title="Teach a device"
        description="Point at Ventilador dormitorio and hold still. The owner fan scenario is seeded in mocks."
        action="Point from spot 1"
      />
    ),
  },
  {
    path: "/devices/places",
    title: "Places",
    description: "Per-camera places and taught anchor directions.",
    element: <EmptyRoute title="Places" description="Places organize taught devices per camera and room." />,
  },
  {
    path: "/devices/realign",
    title: "Re-align devices",
    description: "Recover pointing after a camera move.",
    element: (
      <EmptyRoute
        title="Re-align devices"
        description="Point at two known devices to recover after the camera moved."
        action="Camera moved recovery"
      />
    ),
  },
  {
    path: "/mappings",
    title: "Mappings",
    description: "Targeted and global gesture sentences.",
    nav: true,
    mobile: true,
    shortcut: "⌘4",
    element: (
      <EmptyRoute
        title="Mappings"
        description="No mappings yet. Pick a gesture and tell Flick what it should do."
        action="Sentence builder scaffold"
      />
    ),
  },
  {
    path: "/mappings/new",
    title: "Mapping editor",
    description: "Sentence builder for actions and safety behavior.",
    element: (
      <EmptyRoute
        title="Mapping editor"
        description="When I point at a taught device and make a gesture, Flick sends a Home Assistant action."
        action="New sentence"
      />
    ),
  },
  {
    path: "/cameras",
    title: "Cameras",
    description: "Local camera list and preview settings.",
    nav: true,
    mobile: false,
    shortcut: "⌘5",
    element: (
      <EmptyRoute
        title="Cameras"
        description="Local cameras are wired now; RTSP and ROI controls come in Phase 2."
      />
    ),
  },
  {
    path: "/activity",
    title: "Activity",
    description: "Debug outcomes and why a gesture did not fire.",
    nav: true,
    mobile: false,
    shortcut: "⌘6",
    element: (
      <EmptyRoute
        title="Activity"
        description="Suppressed reasons and latency details explain why a gesture did or did not fire."
        action="Why didn't it fire?"
      />
    ),
  },
  {
    path: "/settings",
    title: "Settings",
    description: "General, detection, pointing, safety, feedback, privacy, HA and updates.",
    nav: true,
    mobile: true,
    shortcut: "⌘,",
    element: (
      <EmptyRoute
        title="Settings"
        description="Theme, detection, pointing, safety, feedback, privacy, Home Assistant, updates and advanced controls."
        action="Settings scaffold"
      />
    ),
  },
  {
    path: "/pro",
    title: "Pro",
    description: "Future Pro capabilities.",
    nav: true,
    mobile: false,
    element: (
      <EmptyRoute
        title="Pro"
        description="Phase 3 licensing and cloud-assisted setup will appear here when enabled."
      />
    ),
  },
];
