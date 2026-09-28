import { cn } from "../../lib/utils";

const labels: Record<string, string> = {
  "thumbs-up": "Thumbs up",
  "open-palm": "Open palm",
  point: "Point",
  "circle-cw": "Circle clockwise",
  "circle-ccw": "Circle counter-clockwise",
  "two-hand-separate": "Two hands apart",
  "pinch-dial": "Pinch dial",
};

export function GestureGlyph({
  name,
  animated = true,
  className,
}: {
  name?: string | null;
  animated?: boolean;
  className?: string;
}) {
  const glyph = name ?? "point";
  return (
    <span
      className={cn("gesture-glyph", animated && "gesture-glyph-animated", className)}
      role="img"
      aria-label={labels[glyph] ?? glyph}
      data-gesture={glyph}
    >
      {glyph === "circle-cw"
        ? "↻"
        : glyph === "circle-ccw"
          ? "↺"
          : glyph === "two-hand-separate"
            ? "⇔"
            : glyph === "pinch-dial"
              ? "◌"
              : glyph === "thumbs-up"
                ? "👍"
                : glyph === "open-palm"
                  ? "✋"
                  : "☞"}
    </span>
  );
}
