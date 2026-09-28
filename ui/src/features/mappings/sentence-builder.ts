import type { Action, Domain } from "../../api/types";

export type MappingKind = "global" | "targeted";
export type MappingMode = "tap" | "hold" | "repeat" | "dial";
export type HandChoice = "any" | "left" | "right";

export interface SentenceBuilderDraft {
  kind: MappingKind;
  gestureId?: string;
  hand: HandChoice;
  cameras: string[];
  target?:
    | { type: "entity"; entityId: string; domain: Domain; label: string }
    | { type: "anchor"; anchorId: string; domain: Domain; label: string }
    | { type: "domain"; domain: Domain; label: string };
  action?: Action;
  mode: MappingMode;
  cooldownMs: number;
  requireArmed: boolean;
  sensitive: boolean;
  sensitiveAck: boolean;
  confirmGestureId?: string;
}

export interface SentenceValidationIssue {
  field: keyof SentenceBuilderDraft | "action.kind" | "target.domain";
  code:
    | "missing_gesture"
    | "missing_target"
    | "missing_action"
    | "verb_requires_target"
    | "selected_requires_target"
    | "sensitive_ack_required"
    | "confirmation_required"
    | "dial_requires_numeric_target";
  message: string;
}

const dialPropertiesByDomain: Partial<Record<Domain, string[]>> = {
  fan: ["percentage"],
  light: ["brightness_pct"],
  media_player: ["volume_level"],
  cover: ["position"],
  climate: ["temperature"],
};

export function validateSentenceDraft(draft: SentenceBuilderDraft): SentenceValidationIssue[] {
  const issues: SentenceValidationIssue[] = [];

  if (!draft.gestureId) {
    issues.push({
      field: "gestureId",
      code: "missing_gesture",
      message: "Pick the gesture that starts this sentence.",
    });
  }

  if (!draft.target) {
    issues.push({ field: "target", code: "missing_target", message: "Pick what Flick should control." });
  }

  if (!draft.action) {
    issues.push({
      field: "action",
      code: "missing_action",
      message: "Pick the Home Assistant action to send.",
    });
  }

  if (draft.kind === "global" && draft.action?.kind === "verb") {
    issues.push({
      field: "action.kind",
      code: "verb_requires_target",
      message: "Device verbs need a selected device.",
    });
  }

  if (draft.kind === "global" && draft.action?.kind === "dial" && draft.action.entity_id === "$selected") {
    issues.push({
      field: "action.kind",
      code: "selected_requires_target",
      message: "Dialing the selected device needs a targeted sentence.",
    });
  }

  if (draft.action?.kind === "dial" && draft.target?.domain) {
    const allowed = dialPropertiesByDomain[draft.target.domain] ?? [];
    if (!allowed.includes(draft.action.property)) {
      issues.push({
        field: "target.domain",
        code: "dial_requires_numeric_target",
        message: "That target does not expose a dial value.",
      });
    }
  }

  if (draft.sensitive && !draft.sensitiveAck) {
    issues.push({
      field: "sensitiveAck",
      code: "sensitive_ack_required",
      message: "Sensitive devices need an explicit acknowledgement.",
    });
  }

  if (draft.sensitive && !draft.confirmGestureId) {
    issues.push({
      field: "confirmGestureId",
      code: "confirmation_required",
      message: "Pick a confirmation gesture for this sensitive mapping.",
    });
  }

  return issues;
}

export function draftToSentence(draft: SentenceBuilderDraft) {
  const gesture = draft.gestureId ?? "gesture";
  const target = draft.target?.label ?? "target";

  if (draft.kind === "targeted") {
    return `When I point at ${target} and ${gesture} → ${describeAction(draft.action)}`;
  }

  return `When ${gesture} with ${draft.hand} hand → ${describeAction(draft.action)} on ${target}`;
}

function describeAction(action?: Action) {
  if (!action) return "action";
  if (action.kind === "call_service") return `${action.domain}.${action.service}`;
  if (action.kind === "dial") return `set ${action.property}`;
  return action.verb === "level_set" ? `speed ${action.level ?? "level"}` : action.verb;
}
