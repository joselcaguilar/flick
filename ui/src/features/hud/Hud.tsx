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
    detail: "Selected for 4 seconds",
    progress: 0.78,
    icon: "fan",
    verbs: [
      { gesture_id: "builtin.circle_cw", label: "Speed 1" },
      { gesture_id: "builtin.two_hand_separate", label: "Off" },
    ],
  },
  ambiguous: { state: "ambiguous", title: "Two devices here", detail: "Point more precisely", icon: "?" },
  candidate: {
    state: "candidate",
    title: "Circle clockwise",
    detail: "Gesture confidence 87%",
    progress: 0.87,
    icon: "circle-cw",
  },
  armed: {
    state: "armed",
    title: "Listening… 4 s",
    detail: "Confirming the next gesture",
    progress: 0.42,
    icon: "open-palm",
  },
  sent: {
    state: "sent",
    title: "Ventilador dormitorio → speed 1",
    detail: "Sending to Home Assistant…",
    icon: "circle-cw",
  },
  done: {
    state: "done",
    title: "Ventilador dormitorio → speed 1",
    detail: "Done · Home Assistant confirmed",
    icon: "ok",
  },
  failed: {
    state: "failed",
    title: "Ventilador dormitorio → off",
    detail: "Failed · Home Assistant unavailable",
    icon: "error",
  },
  confirm: {
    state: "confirm",
    title: "Confirm with Thumbs up",
    detail: "Sensitive device · 3 s",
    progress: 0.54,
    icon: "thumbs-up",
  },
  dial: { state: "dial", title: "Living room TV", detail: "Volume 64%", progress: 0.64, icon: "pinch-dial" },
  camera_moved: { state: "camera_moved", title: "Camera moved", detail: "Re-align your devices", icon: "!" },
  paused: { state: "paused", title: "Flick paused", detail: "Until 14:30", icon: "pause" },
};

function isGestureIcon(icon?: string) {
  return (
    icon === "circle-cw" ||
    icon === "circle-ccw" ||
    icon === "two-hand-separate" ||
    icon === "thumbs-up" ||
    icon === "open-palm" ||
    icon === "pinch-dial" ||
    icon === "point"
  );
}

function HudIcon({ hud }: { hud: HudState }) {
  if (hud.icon === "fan") return <EntityIcon domain="fan" />;
  if (isGestureIcon(hud.icon)) return <GestureGlyph name={hud.icon} animated={false} />;
  if (hud.state === "candidate") return <GestureGlyph name="circle-cw" animated={false} />;
  if (hud.state === "dial") return <GestureGlyph name="pinch-dial" animated={false} />;
  if (hud.state === "confirm") return <GestureGlyph name="thumbs-up" animated={false} />;
  if (hud.state === "done") return <span className="hud-state-mark">OK</span>;
  if (hud.state === "failed" || hud.state === "camera_moved") {
    return <span className="hud-state-mark">!</span>;
  }
  if (hud.state === "paused") return <span className="hud-state-mark">Pause</span>;
  return <span className="hud-state-mark">{hud.icon ?? "•"}</span>;
}

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

  if (!previewState && hud.state === "idle") {
    return <main className="hud-stage" aria-live="polite" />;
  }

  return (
    <main className="hud-stage" aria-live="polite">
      <section className="hud-capsule" data-state={hud.state} data-tone={stateTone(hud.state)}>
        <span className="hud-icon" aria-hidden="true">
          <HudIcon hud={hud} />
        </span>
        <div className="hud-copy">
          <h1>{hud.title}</h1>
          {hud.detail ? <p>{hud.detail}</p> : null}
          {hud.verbs?.length ? (
            <ul className="hud-verb-list" aria-label="Available gestures">
              {hud.verbs.map((verb) => (
                <li key={verb.gesture_id}>
                  <GestureGlyph name={verb.gesture_id} animated={false} />
                  {verb.label}
                </li>
              ))}
            </ul>
          ) : null}
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
