import type { ReactNode } from "react";
import { GestureGlyph } from "../components/domain";
import { GlassPanel } from "../components/ui";
import { DashboardRoute } from "../features/dashboard/Dashboard";
import { DevicesRoute, PlacesRoute, RealignRoute, TeachDeviceRoute } from "../features/devices/Devices";

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

export const routes: RouteMeta[] = [
  {
    path: "/",
    title: "Home",
    description: "Dashboard, live preview and recent activity.",
    nav: true,
    mobile: true,
    shortcut: "⌘1",
    element: <DashboardRoute />,
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
    element: <DevicesRoute />,
  },
  {
    path: "/devices/teach",
    title: "Teach a device",
    description: "Pick a device, point from two spots and test verbs.",
    element: <TeachDeviceRoute />,
  },
  {
    path: "/devices/places",
    title: "Places",
    description: "Per-camera places and taught anchor directions.",
    element: <PlacesRoute />,
  },
  {
    path: "/devices/realign",
    title: "Re-align devices",
    description: "Recover pointing after a camera move.",
    element: <RealignRoute />,
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
