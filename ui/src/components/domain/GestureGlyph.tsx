import { cn } from "../../lib/utils";

const labels: Record<string, string> = {
  "thumbs-up": "Thumbs up",
  "thumbs-down": "Thumbs down",
  "open-palm": "Open palm",
  point: "Point",
  "circle-cw": "Circle clockwise",
  "circle-ccw": "Circle counter-clockwise",
  "two-hand-separate": "Two hands apart",
  "pinch-dial": "Pinch dial",
  "swipe-up": "Swipe up",
  "swipe-down": "Swipe down",
  "swipe-left": "Swipe left",
  "swipe-right": "Swipe right",
  "rock-on": "Rock on",
  "zorro-z": "Zorro Z",
};

const glyphPaths: Record<string, string[]> = {
  "thumbs-up": [
    "M8.4 20H5.6a1.8 1.8 0 0 1-1.8-1.8v-6.7a1.8 1.8 0 0 1 1.8-1.8h2.8",
    "m8.4 9.7 3.1-5.6a2 2 0 0 1 2.8 2l-.8 3.6h3.9a2.6 2.6 0 0 1 2.5 3.1l-1.1 5.1A3 3 0 0 1 15.9 20H8.4z",
  ],
  "thumbs-down": [
    "M8.4 4H5.6a1.8 1.8 0 0 0-1.8 1.8v6.7a1.8 1.8 0 0 0 1.8 1.8h2.8",
    "m8.4 14.3 3.1 5.6a2 2 0 0 0 2.8-2l-.8-3.6h3.9a2.6 2.6 0 0 0 2.5-3.1l-1.1-5.1A3 3 0 0 0 15.9 4H8.4z",
  ],
  "open-palm": [
    "M7.4 12.5V6.8a1.55 1.55 0 0 1 3.1 0v5",
    "M10.5 11.8V5.2a1.55 1.55 0 0 1 3.1 0V12",
    "M13.6 12V6.6a1.55 1.55 0 0 1 3.1 0v6.3",
    "M16.7 13v-3.2a1.55 1.55 0 0 1 3.1 0v5.3c0 4.1-3.1 6.3-6.7 6.3h-1.7c-2.6 0-4.5-1.4-5.9-3.6l-1.4-2.4a1.55 1.55 0 0 1 2.7-1.5l1.4 2.1",
  ],
  point: ["M7.2 19.2 16.8 4.8", "M16.8 4.8l1.3 5.1 3.2-3.2", "M9.2 20.2c2.7-2.2 6.1-3.3 10.1-3.2"],
  "circle-cw": ["M17.7 7.4A7.5 7.5 0 1 0 19 15.5", "M17.7 7.4h-4.2", "M17.7 7.4V3.2"],
  "circle-ccw": ["M6.3 7.4A7.5 7.5 0 1 1 5 15.5", "M6.3 7.4h4.2", "M6.3 7.4V3.2"],
  "two-hand-separate": [
    "M5.2 8.4h13.6",
    "M5.2 15.6h13.6",
    "m7.8 5.6-3 2.8 3 2.8",
    "m16.2 12.8 3 2.8-3 2.8",
    "M10.8 5.8h2.4",
    "M10.8 18.2h2.4",
  ],
  "pinch-dial": [
    "M7.5 15.7c2.2-3.5 5.2-5.7 9.2-6.8",
    "M10 17.8c2.6-.8 4.8-.5 6.6 1",
    "M7.8 8.1a8.3 8.3 0 0 1 10.4 10.4",
    "M18.2 18.5h-3.7",
    "M18.2 18.5v-3.7",
  ],
  "swipe-up": ["M12 19V5", "M7.8 9.2 12 5l4.2 4.2", "M7.2 20.3v1.3", "M16.8 20.3v1.3"],
  "swipe-down": ["M12 5v14", "M7.8 14.8 12 19l4.2-4.2", "M7.2 2.4v1.3", "M16.8 2.4v1.3"],
  "swipe-left": ["M19 12H5", "M9.2 7.8 5 12l4.2 4.2", "M19 7.2h1.3", "M19 16.8h1.3"],
  "swipe-right": ["M5 12h14", "M14.8 7.8 19 12l-4.2 4.2", "M3.7 7.2H5", "M3.7 16.8H5"],
  "rock-on": [
    "M6.5 12.7V6.8a1.45 1.45 0 0 1 2.9 0v5.8",
    "M9.4 11.9V5.2a1.45 1.45 0 0 1 2.9 0v8.2",
    "M15.2 12.2V5.7a1.45 1.45 0 0 1 2.9 0v8.8",
    "M12.3 14.6v-3.5a1.45 1.45 0 0 1 2.9 0v4.1",
    "M6.6 14.5 5 12.7a1.45 1.45 0 0 0-2.2 1.8l2.6 3.7c1.4 2 3.2 3 5.7 3h1.8c3.1 0 5.2-2 5.2-5.3v-1.4",
  ],
  "zorro-z": ["M5.2 6.2h13.6L5.2 17.8h13.6", "M6.4 11.9c3.5-1.9 7.1-2.1 10.8-.5"],
};

export function normalizeGestureName(name?: string | null) {
  const raw = (name ?? "point")
    .replace(/^builtin\./, "")
    .replace(/^custom\./, "")
    .replace(/_/g, "-")
    .toLowerCase();
  if (raw === "circle-clockwise") return "circle-cw";
  if (raw === "circle-counter-clockwise" || raw === "circle-counterclockwise") return "circle-ccw";
  if (raw === "two-hands-apart") return "two-hand-separate";
  return glyphPaths[raw] ? raw : "point";
}

export function gestureLabel(name?: string | null) {
  const normalized = normalizeGestureName(name);
  return labels[normalized] ?? normalized;
}

export function GestureGlyph({
  name,
  animated = true,
  className,
}: {
  name?: string | null;
  animated?: boolean;
  className?: string;
}) {
  const glyph = normalizeGestureName(name);
  return (
    <span
      className={cn("gesture-glyph", animated && "gesture-glyph-animated", className)}
      role="img"
      aria-label={gestureLabel(glyph)}
      data-gesture={glyph}
    >
      <svg className="gesture-glyph-svg" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <g stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" fill="none">
          {(glyphPaths[glyph] ?? glyphPaths.point).map((path) => (
            <path key={path} d={path} />
          ))}
        </g>
      </svg>
    </span>
  );
}
