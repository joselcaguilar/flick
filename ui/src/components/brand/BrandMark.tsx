import { useId } from "react";
import { cn } from "../../lib/utils";

export type BrandMarkVariant = "tile" | "glyph-only" | "glyph";

export interface BrandMarkProps {
  className?: string;
  decorative?: boolean;
  size?: number | string;
  title?: string;
  variant?: BrandMarkVariant;
}

export function BrandMark({
  className,
  decorative = false,
  size = 32,
  title = "Flick",
  variant = "tile",
}: BrandMarkProps) {
  const id = useId();
  const titleId = `${id}-title`;
  const tile = variant === "tile";
  const ariaProps = decorative
    ? { "aria-hidden": true as const }
    : { "aria-labelledby": titleId, role: "img" as const };

  return (
    <svg
      className={cn("brand-mark-svg", className)}
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      {...ariaProps}
    >
      {decorative ? null : <title id={titleId}>{title}</title>}
      {tile ? <rect x="5" y="5" width="54" height="54" rx="16" fill="#161B22" /> : null}
      <g strokeLinecap="round">
        <path
          d="M20.3 47c-3.7-3.8-3.5-9.6.5-13.2l13.9-12.6c4-3.6 10.1-3.4 13.6.4 3.6 3.8 3.4 9.6-.5 13.2L33.9 47.4c-4 3.6-10.1 3.4-13.6-.4Z"
          fill="#FFFFFF"
        />
        <path d="M49 16c3.8 1.8 6.4 5.3 7.2 9.5" stroke="#2F81F7" strokeWidth="4" />
        <path d="M53 11.5c5.3 3 8.8 8.4 9.5 14.4" stroke="#2F81F7" strokeWidth="2.3" />
      </g>
    </svg>
  );
}
