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
      <defs>
        <linearGradient id={`${id}-tile`} x1="8" y1="6" x2="58" y2="60" gradientUnits="userSpaceOnUse">
          <stop stopColor="#1B2440" />
          <stop offset=".52" stopColor="#0B1020" />
          <stop offset="1" stopColor="#17112D" />
        </linearGradient>
        <linearGradient id={`${id}-finger`} x1="20" y1="48" x2="46" y2="21" gradientUnits="userSpaceOnUse">
          <stop stopColor="#F8FBFF" />
          <stop offset=".64" stopColor="#DCEBFF" />
          <stop offset="1" stopColor="#FFFFFF" />
        </linearGradient>
        <linearGradient id={`${id}-arc`} x1="46" y1="13" x2="61" y2="29" gradientUnits="userSpaceOnUse">
          <stop stopColor="#00A2FF" />
          <stop offset=".48" stopColor="#006DFF" />
          <stop offset="1" stopColor="#6D5DF7" />
        </linearGradient>
      </defs>
      {tile ? (
        <>
          <rect x="5" y="5" width="54" height="54" rx="16" fill={`url(#${id}-tile)`} />
          <rect x="6.5" y="6.5" width="51" height="51" rx="14.5" stroke="white" strokeOpacity=".2" />
        </>
      ) : null}
      <g strokeLinecap="round">
        <path
          d="M20.3 47c-3.7-3.8-3.5-9.6.5-13.2l13.9-12.6c4-3.6 10.1-3.4 13.6.4 3.6 3.8 3.4 9.6-.5 13.2L33.9 47.4c-4 3.6-10.1 3.4-13.6-.4Z"
          fill={`url(#${id}-finger)`}
        />
        <path d="M42.6 22c-2.8.8-5.1 2.5-6.8 5.1" stroke="#FFFFFF" strokeWidth="1.5" opacity=".55" />
        <path d="M49 16c3.8 1.8 6.4 5.3 7.2 9.5" stroke={`url(#${id}-arc)`} strokeWidth="4" />
        <path
          d="M53 11.5c5.3 3 8.8 8.4 9.5 14.4"
          stroke={`url(#${id}-arc)`}
          strokeWidth="2.3"
          opacity=".88"
        />
      </g>
    </svg>
  );
}
