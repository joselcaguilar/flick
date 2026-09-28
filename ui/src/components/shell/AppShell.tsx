import { useEffect, useMemo, useState } from "react";
import { Link, NavLink, Outlet, useLocation } from "react-router-dom";
import { usePauseEngine, useResumeEngine, useStatus } from "../../api/hooks";
import { startEventStream } from "../../events/client";
import { useEventStore } from "../../events/store";
import { routes } from "../../routes/routes";
import { useTheme } from "../../theme";
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

export function AppShell() {
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const location = useLocation();
  const status = useStatus();
  const events = useEventStore();
  const pause = usePauseEngine();
  const resume = useResumeEngine();
  const { show } = useToast();
  const theme = useTheme();
  const navRoutes = useMemo(() => routes.filter((route) => route.nav), []);
  const mobileRoutes = useMemo(() => routes.filter((route) => route.mobile), []);

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
    show({ tone: "warning", title: "Flick paused", description: "Recognition is paused for 15 minutes." });
  }

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <aside className="flick-glass shell-sidebar" aria-label="Primary navigation">
        <Link className="brand-mark" to="/">
          <span aria-hidden="true">F</span>
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
        <button className="sidebar-command" type="button" onClick={() => setPaletteOpen(true)}>
          <span>Command palette</span>
          <Kbd>⌘K</Kbd>
        </button>
      </aside>

      <header className="flick-glass shell-toolbar">
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
          <Button
            variant="ghost"
            size="sm"
            onClick={togglePause}
            loading={pause.isPending || resume.isPending}
          >
            {status.data?.paused ? "Resume" : "Pause 15 min"}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => theme.cycleMode()}>
            Theme: {theme.mode}
          </Button>
          <Button
            variant={theme.highContrast ? "primary" : "ghost"}
            size="sm"
            onClick={() => theme.setHighContrast(!theme.highContrast)}
          >
            High contrast
          </Button>
          <Button variant="primary" size="sm" onClick={() => setPaletteOpen(true)}>
            ⌘K
          </Button>
        </div>
      </header>

      <main id="main-content" className="shell-content" data-route={location.pathname}>
        <Outlet />
      </main>

      <nav className="flick-glass mobile-tabs" aria-label="Mobile navigation">
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

      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
    </div>
  );
}
