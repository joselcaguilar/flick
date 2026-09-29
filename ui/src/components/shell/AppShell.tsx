import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { Link, NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { retryEngineConnection } from "../../api/client";
import { usePauseEngine, useResumeEngine, useSettings, useStatus } from "../../api/hooks";
import { restartEventStream, startEventStream } from "../../events/client";
import { useEventStore } from "../../events/store";
import { useStatusSync } from "../../events/useStatusSync";
import { isMacApp } from "../../platform/tauri";
import { routes } from "../../routes/routes";
import { BrandMark } from "../brand/BrandMark";
import { Button, Kbd, Sheet, useToast } from "../ui";
import { CommandPalette } from "./CommandPalette";

function connectionTone(value?: string) {
  if (value === "ready" || value === "running" || value === "open") return "online";
  if (value === "error" || value === "auth_failed" || value === "closed") return "error";
  return "warning";
}

function StatusIndicator({ label, value }: { label: string; value: string }) {
  return (
    <span className="status-indicator" data-tone={connectionTone(value)}>
      <i aria-hidden="true" />
      <span>{label}</span>
      <strong>{value.replace(/_/g, " ")}</strong>
    </span>
  );
}

function clockTime(value?: string | null) {
  const date = value ? new Date(value) : undefined;
  if (!date || Number.isNaN(date.getTime())) return undefined;
  return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function liveStatus(cameraState?: string, paused?: boolean, pausedUntil?: string | null) {
  if (paused) {
    const until = clockTime(pausedUntil);
    return {
      label: "Paused",
      detail: until ? `Camera off until ${until}` : "Camera off until you resume",
      tone: "warning",
    };
  }
  if (cameraState === "running" || cameraState === "starting" || cameraState === "reconnecting") {
    return { label: "Watching", detail: "Local camera only", tone: "online" };
  }
  return {
    label: "Camera off",
    detail: "No active capture",
    tone: cameraState === "error" ? "error" : "warning",
  };
}

export function AppShell() {
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [retryingEngine, setRetryingEngine] = useState(false);
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const status = useStatus();
  useStatusSync();
  const settings = useSettings();
  const firstRunChecked = useRef(false);
  const events = useEventStore();
  const pause = usePauseEngine();
  const resume = useResumeEngine();
  const { show } = useToast();
  const navRoutes = useMemo(() => routes.filter((route) => route.nav), []);
  const mobileRoutes = useMemo(() => routes.filter((route) => route.mobile), []);
  const shellOverride = new URLSearchParams(location.search).get("shell");
  const macShell = isMacApp() || shellOverride === "macos";

  useEffect(() => {
    if (firstRunChecked.current || !settings.data) return;
    firstRunChecked.current = true;
    if (settings.data["onboarding.completed"] !== true && location.pathname === "/") {
      navigate("/onboarding", { replace: true });
    }
  }, [settings.data, location.pathname, navigate]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    let closed = false;
    startEventStream().then((client) => {
      if (closed) client.close();
    });
    return () => {
      closed = true;
    };
  }, []);

  const engineValue = status.data?.paused ? "paused" : (status.data?.cameras[0]?.state ?? events.connection);
  const haValue = status.data?.ha.state ?? events.ha.state;
  const engineRecoverable = status.isError || events.connection === "error" || events.connection === "closed";
  const live = liveStatus(status.data?.cameras[0]?.state, status.data?.paused, status.data?.paused_until);

  async function retryEngine() {
    setRetryingEngine(true);
    try {
      await retryEngineConnection();
      await Promise.all([queryClient.invalidateQueries(), restartEventStream().then(() => undefined)]);
      show({
        tone: "success",
        title: "Retrying engine",
        description: "Flick is checking the local engine again.",
      });
    } catch (error) {
      show({
        tone: "danger",
        title: "Engine retry failed",
        description: error instanceof Error ? error.message : "Flick could not reach the local engine.",
      });
    } finally {
      setRetryingEngine(false);
    }
  }

  async function togglePause() {
    if (status.data?.paused) {
      await resume.mutateAsync();
      show({
        tone: "success",
        title: "Flick resumed",
        description: "Gesture recognition is watching again.",
      });
      return;
    }
    await pause.mutateAsync(900);
    show({ tone: "warning", title: "Flick paused", description: "The camera is off for 15 minutes." });
  }

  return (
    <div className="app-shell" data-shell={macShell ? "macos" : undefined}>
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <aside className="shell-sidebar" aria-label="Primary navigation">
        {macShell ? (
          <div className="mac-sidebar-drag-inset" data-tauri-drag-region aria-hidden="true" />
        ) : null}
        <Link className="brand-mark" to="/">
          <BrandMark className="brand-mark-slot" size={24} decorative />
          <strong>Flick</strong>
        </Link>
        <nav aria-label="Main navigation">
          {navRoutes.map((route) => (
            <NavLink key={route.path} to={route.path} end={route.path === "/"}>
              <span>{route.title}</span>
              {route.shortcut ? <Kbd>{route.shortcut}</Kbd> : null}
            </NavLink>
          ))}
        </nav>
        <div className="sidebar-live-status" data-tone={live.tone}>
          <i aria-hidden="true" />
          <div>
            <strong>{live.label}</strong>
            <span>{live.detail}</span>
          </div>
        </div>
      </aside>

      <header className="shell-toolbar">
        {macShell ? <div className="toolbar-drag-region" data-tauri-drag-region aria-hidden="true" /> : null}
        <div className="toolbar-left">
          <button
            className="mobile-menu-button"
            type="button"
            onClick={() => setMobileOpen(true)}
            aria-label="Open navigation"
          >
            Menu
          </button>
          <StatusIndicator label="Engine" value={engineValue} />
          <StatusIndicator label="HA" value={haValue} />
        </div>
        <div className="toolbar-actions">
          <Button size="sm" onClick={togglePause} loading={pause.isPending || resume.isPending}>
            {status.data?.paused ? "Resume" : "Pause 15 min"}
          </Button>
          <button
            className="toolbar-search"
            type="button"
            onClick={() => setPaletteOpen(true)}
            aria-label="Open command palette"
          >
            <span>Jump to…</span>
            <Kbd>⌘K</Kbd>
          </button>
        </div>
      </header>

      <main id="main-content" className="shell-content" data-route={location.pathname}>
        {engineRecoverable ? (
          <section className="engine-recovery-banner" role="alert" aria-live="polite">
            <div>
              <strong>Engine unreachable</strong>
              <span>
                Flick could not reach the local engine. Retry starts the desktop engine again when available.
              </span>
            </div>
            <Button variant="primary" size="sm" onClick={retryEngine} loading={retryingEngine}>
              Retry
            </Button>
          </section>
        ) : null}
        <Outlet />
      </main>

      <nav className="mobile-tabs" aria-label="Mobile navigation">
        {mobileRoutes.map((route) => (
          <NavLink key={route.path} to={route.path} end={route.path === "/"}>
            <span>{route.shortTitle ?? route.title}</span>
          </NavLink>
        ))}
      </nav>

      <Sheet open={mobileOpen} onOpenChange={setMobileOpen} title="Flick navigation">
        <div className="mobile-sheet-nav">
          {navRoutes.map((route) => (
            <NavLink
              key={route.path}
              to={route.path}
              end={route.path === "/"}
              onClick={() => setMobileOpen(false)}
            >
              {route.title}
            </NavLink>
          ))}
        </div>
      </Sheet>

      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        paused={Boolean(status.data?.paused)}
        onTogglePause={() => void togglePause()}
      />
    </div>
  );
}
