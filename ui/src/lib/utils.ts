import { type ClassValue, clsx } from "clsx";

export function cn(...inputs: ClassValue[]) {
  return clsx(inputs);
}

export function formatTime(value?: string | null) {
  if (!value) return "—";
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(new Date(value));
}

export function asPercent(value: number, digits = 0) {
  return `${Math.round(value * 100 * 10 ** digits) / 10 ** digits}%`;
}

export const isBrowser = typeof window !== "undefined";
