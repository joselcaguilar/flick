import type { ReactNode } from "react";
import { Navigate } from "react-router-dom";
import { features } from "../config/features";
import { ActivityRoute } from "../features/activity/ActivityRoute";
import { CamerasRoute } from "../features/cameras/Cameras";
import { DashboardRoute } from "../features/dashboard/Dashboard";
import { DevicesRoute, PlacesRoute, RealignRoute, TeachDeviceRoute } from "../features/devices/Devices";
import { GesturesLibraryRoute } from "../features/gestures/GesturesLibraryRoute";
import { MappingEditorRoute, MappingsRoute } from "../features/mappings/Mappings";
import { OnboardingRoute } from "../features/onboarding/OnboardingRoute";
import { ProRoute } from "../features/pro/Pro";
import { SettingsRoute } from "../features/settings/SettingsRoute";
import { GestureStudioRoute } from "../features/studio/GestureStudioRoute";

export interface RouteMeta {
  path: string;
  title: string;
  shortTitle?: string;
  description: string;
  nav?: boolean;
  mobile?: boolean;
  command?: boolean;
  shortcut?: string;
  element: ReactNode;
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
    element: <OnboardingRoute />,
  },
  {
    path: "/gestures",
    title: "Gestures",
    description: "Built-in and custom gesture library.",
    nav: true,
    mobile: true,
    shortcut: "⌘2",
    element: <GesturesLibraryRoute />,
  },
  {
    path: "/gestures/new",
    title: "Gesture Studio",
    description: "Record static, motion or two-hand gestures.",
    element: <GestureStudioRoute />,
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
    element: <MappingsRoute />,
  },
  {
    path: "/mappings/new",
    title: "Mapping editor",
    description: "Sentence builder for actions and safety behavior.",
    element: <MappingEditorRoute />,
  },
  {
    path: "/cameras",
    title: "Cameras",
    description: "Local camera list and preview settings.",
    nav: true,
    mobile: false,
    shortcut: "⌘5",
    element: <CamerasRoute />,
  },
  {
    path: "/activity",
    title: "Activity",
    description: "Debug outcomes and why a gesture did not fire.",
    nav: true,
    mobile: false,
    shortcut: "⌘6",
    element: <ActivityRoute />,
  },
  {
    path: "/settings",
    title: "Settings",
    description: "General, detection, pointing, safety, feedback, privacy, HA and updates.",
    nav: true,
    mobile: true,
    shortcut: "⌘,",
    element: <SettingsRoute />,
  },
  {
    path: "/pro",
    title: "Pro",
    description: "Future Pro capabilities.",
    nav: features.pro,
    mobile: false,
    command: features.pro,
    element: features.pro ? <ProRoute /> : <Navigate to="/" replace />,
  },
];
