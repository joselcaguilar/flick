import { useId } from "react";
import { cn } from "../../lib/utils";

export type BrandMarkVariant = "tile" | "glyph";

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
        <linearGradient id={`${id}-finger`} x1="18" y1="50" x2="45" y2="14" gradientUnits="userSpaceOnUse">
          <stop stopColor="#F8FBFF" />
          <stop offset=".64" stopColor="#DCEBFF" />
          <stop offset="1" stopColor="#FFFFFF" />
        </linearGradient>
        <linearGradient id={`${id}-arc`} x1="43" y1="13" x2="57" y2="50" gradientUnits="userSpaceOnUse">
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
      <g strokeLinecap="round" strokeLinejoin="round">
        <path d="M14.6 51 44.2 17.2" stroke={`url(#${id}-finger)`} strokeWidth="6.2" />
        <path d="M44.2 17.3c-2.7.8-4.9 2.7-6.4 5.1" stroke="#FFFFFF" strokeWidth="1.3" opacity=".6" />
        <path
          d="M44 17c7.8 3 13 10.4 13 18.9 0 5-1.9 9.6-5 13.2"
          stroke={`url(#${id}-arc)`}
          strokeWidth="5"
        />
        <path d="M47.5 22.2c4.2 3.4 6.7 8.6 6.7 14.2" stroke="#DCEBFF" strokeWidth="1.6" opacity=".88" />
      </g>
    </svg>
  );
}
