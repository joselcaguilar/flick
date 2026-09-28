import type { CSSProperties } from "react";
import circleCcwUrl from "../../../../design/glyphs/circle-ccw.svg";
import circleCwUrl from "../../../../design/glyphs/circle-cw.svg";
import openPalmUrl from "../../../../design/glyphs/open-palm.svg";
import pinchDialUrl from "../../../../design/glyphs/pinch-dial.svg";
import pointUrl from "../../../../design/glyphs/point.svg";
import swipeDownUrl from "../../../../design/glyphs/swipe-down.svg";
import swipeLeftUrl from "../../../../design/glyphs/swipe-left.svg";
import swipeRightUrl from "../../../../design/glyphs/swipe-right.svg";
import swipeUpUrl from "../../../../design/glyphs/swipe-up.svg";
import thumbsDownUrl from "../../../../design/glyphs/thumbs-down.svg";
import thumbsUpUrl from "../../../../design/glyphs/thumbs-up.svg";
import twoHandSeparateUrl from "../../../../design/glyphs/two-hand-separate.svg";
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
};

const glyphUrls: Record<string, string> = {
  "thumbs-up": thumbsUpUrl,
  "thumbs-down": thumbsDownUrl,
  "open-palm": openPalmUrl,
  point: pointUrl,
  "circle-cw": circleCwUrl,
  "circle-ccw": circleCcwUrl,
  "two-hand-separate": twoHandSeparateUrl,
  "pinch-dial": pinchDialUrl,
  "swipe-up": swipeUpUrl,
  "swipe-down": swipeDownUrl,
  "swipe-left": swipeLeftUrl,
  "swipe-right": swipeRightUrl,
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
  return glyphUrls[raw] ? raw : "point";
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
      style={{ "--gesture-glyph-url": `url(${glyphUrls[glyph] ?? pointUrl})` } as CSSProperties}
    >
      <span className="gesture-glyph-shape" aria-hidden="true" />
    </span>
  );
}
