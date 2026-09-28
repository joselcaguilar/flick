import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { startEventStream } from "./events/client";
import { useEventStore } from "./events/store";
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

  return (
    <main className="hud-stage" aria-live="polite">
      <section className="flick-glass hud-capsule" data-state={hud.state}>
        <span className="hud-icon" aria-hidden="true">
          {hud.icon ?? (hud.state === "selected" ? "◆" : "F")}
        </span>
        <div>
          <h1>{hud.title}</h1>
          {hud.detail ? <p>{hud.detail}</p> : null}
          {typeof hud.progress === "number" ? (
            <span className="hud-progress">
              <i style={{ inlineSize: `${hud.progress * 100}%` }} />
            </span>
          ) : null}
        </div>
      </section>
    </main>
  );
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
