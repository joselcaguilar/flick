import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { startEventStream } from "./events/client";
import { useEventStore } from "./events/store";
import { HudCapsule } from "./features/hud";
import { ThemeProvider } from "./theme";
import "./styles/index.css";

function HudApp() {
  const hud = useEventStore((state) => state.hud);
  useEffect(() => {
    let closed = false;
    startEventStream(["status", "gestures", "actions", "targeting", "ha"]).then((client) => {
      if (closed) client.close();
    });
    return () => {
      closed = true;
    };
  }, []);

  return <HudCapsule liveState={hud} />;
}

const root = document.getElementById("hud-root");
if (!root) throw new Error("Missing HUD root");

createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <HudApp />
    </ThemeProvider>
  </StrictMode>,
);
