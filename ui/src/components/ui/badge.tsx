import type { HTMLAttributes } from "react";
import { cn } from "../../lib/utils";

type Tone = "neutral" | "accent" | "success" | "warning" | "danger";

export function Badge({
  className,
  tone = "neutral",
  ...props
}: HTMLAttributes<HTMLSpanElement> & { tone?: Tone }) {
  return <span className={cn("ui-badge", `ui-badge-${tone}`, className)} {...props} />;
}

export function TypeChip({ className, ...props }: HTMLAttributes<HTMLSpanElement>) {
  return <Badge className={cn("ui-type-chip", className)} tone="accent" {...props} />;
}
