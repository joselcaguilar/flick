import { useEffect, useMemo, useRef } from "react";
import { EntityIcon, GestureGlyph } from "../../components/domain";
import type { HudState } from "../../events/types";
import { playEarcon } from "./earcons";

export const hudPreviewStates: Record<string, HudState> = {
  aiming: {
    state: "aiming",
    title: "Ventilador dormitorio?",
    detail: "Hold still to select",
    progress: 0.62,
    icon: "fan",
  },
  selected: {
    state: "selected",
    title: "Ventilador dormitorio",
    detail: "↻ speed 1 · ✋✋ off",
    progress: 0.78,
    icon: "fan",
  },
  ambiguous: { state: "ambiguous", title: "Two devices here", detail: "Point more precisely", icon: "?" },
  candidate: {
    state: "candidate",
    title: "Circle clockwise",
    detail: "Gesture confidence 87%",
    progress: 0.87,
    icon: "↻",
  },
  armed: {
    state: "armed",
    title: "Listening… 4 s",
    detail: "Confirming the next gesture",
    progress: 0.42,
    icon: "◆",
  },
  sent: {
    state: "sent",
    title: "Ventilador dormitorio → speed 1",
    detail: "Sending to Home Assistant…",
    icon: "↻",
  },
  done: {
    state: "done",
    title: "Ventilador dormitorio → speed 1",
    detail: "Done · Home Assistant confirmed",
    icon: "✓",
  },
  failed: {
    state: "failed",
    title: "Ventilador dormitorio → off",
    detail: "Failed · Home Assistant unavailable",
    icon: "✕",
  },
  confirm: {
    state: "confirm",
    title: "Confirm with 👍",
    detail: "Sensitive device · 3 s",
    progress: 0.54,
    icon: "🔒",
  },
  dial: { state: "dial", title: "Living room TV", detail: "Volume 64%", progress: 0.64, icon: "◌" },
  camera_moved: { state: "camera_moved", title: "Camera moved", detail: "Re-align your devices", icon: "!" },
  paused: { state: "paused", title: "Flick paused", detail: "Until 14:30", icon: "Ⅱ" },
};

function getPreviewState() {
  if (typeof window === "undefined") return undefined;
  const key = new URLSearchParams(window.location.search).get("state")?.replace(/-/g, "_");
  return key ? hudPreviewStates[key] : undefined;
}

function stateTone(state: HudState["state"]) {
  if (state === "done") return "success";
  if (state === "failed" || state === "camera_moved") return "danger";
  if (state === "confirm" || state === "ambiguous" || state === "paused") return "warning";
  return "accent";
}

export function HudCapsule({ liveState }: { liveState: HudState }) {
  const previewState = useMemo(getPreviewState, []);
  const hud = previewState ?? liveState;
  const lastState = useRef(hud.state);

  useEffect(() => {
    const start = performance.now();
    requestAnimationFrame(() => {
      const latency = performance.now() - start;
      document.documentElement.dataset.hudPaintMs = latency.toFixed(1);
    });
    if (lastState.current !== hud.state) {
      playEarcon(hud.state);
      lastState.current = hud.state;
    }
  }, [hud.state]);

  return (
    <main className="hud-stage" aria-live="polite">
      <section className="flick-glass hud-capsule" data-state={hud.state} data-tone={stateTone(hud.state)}>
        <span className="hud-icon" aria-hidden="true">
          {hud.icon === "fan" ? (
            <EntityIcon domain="fan" />
          ) : hud.state === "candidate" ? (
            <GestureGlyph name="circle-cw" animated={false} />
          ) : (
            hud.icon
          )}
        </span>
        <div className="hud-copy">
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
