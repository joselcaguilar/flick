import { cn } from "../../lib/utils";

export function EntityIcon({ domain, className }: { domain?: string | null; className?: string }) {
  const symbol =
    domain === "fan"
      ? "✺"
      : domain === "light"
        ? "✦"
        : domain === "media_player"
          ? "▻"
          : domain === "switch"
            ? "⏻"
            : domain === "lock"
              ? "⌁"
              : "◇";
  return (
    <span className={cn("entity-icon", className)} aria-hidden="true" data-domain={domain ?? "unknown"}>
      {symbol}
    </span>
  );
}
