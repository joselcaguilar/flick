import type { ActivityItem } from "../../api/types";

export const suppressionReasonOrder = [
  "below_threshold",
  "vote_failed",
  "cooldown",
  "not_armed",
  "no_mapping",
  "blocked_domain",
  "paused",
  "too_small",
  "ambiguous",
  "target_selected",
  "no_target",
  "ambiguous_target",
  "needs_realign",
] as const;

export type SuppressionReasonCode = (typeof suppressionReasonOrder)[number];

export const suppressionReasonCopy: Record<SuppressionReasonCode, string> = {
  below_threshold: "Confidence 0.61 below 0.75",
  vote_failed: "Vote did not pass yet (needs 6 of 8 frames)",
  cooldown: "Cooling down (0.4 s left)",
  not_armed: "Not armed",
  no_mapping: "No mapping for Rock on with left hand",
  blocked_domain: "Locks are blocked in Safety settings",
  paused: "Flick is paused",
  too_small: "Hand too far — move closer",
  ambiguous: "Two gestures looked too similar",
  target_selected: "A device was selected, so the global Thumbs up mapping was skipped",
  no_target: "Circle is a device verb — point at a device first",
  ambiguous_target: "Two taught devices were too close to tell apart",
  needs_realign: "Camera moved — re-align to use pointing",
};

const aliases: Record<string, SuppressionReasonCode> = {
  low_confidence: "below_threshold",
  sensitive_blocked: "blocked_domain",
  hand_too_far: "too_small",
};

export function reasonCopy(itemOrCode: ActivityItem | string) {
  const code = typeof itemOrCode === "string" ? itemOrCode : itemOrCode.reason;
  if (!code) return "No suppression reason recorded";
  const normalized = aliases[code] ?? code;
  if (normalized === "below_threshold" && typeof itemOrCode !== "string" && itemOrCode.confidence) {
    return `Confidence ${itemOrCode.confidence.toFixed(2)} below 0.75`;
  }
  return suppressionReasonCopy[normalized as SuppressionReasonCode] ?? code.replace(/_/g, " ");
}
