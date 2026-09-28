import { describe, expect, it } from "vitest";
import { validateSentenceDraft, type SentenceBuilderDraft } from "./sentence-builder";

const baseDraft: SentenceBuilderDraft = {
  kind: "targeted",
  gestureId: "builtin.circle_cw",
  hand: "right",
  cameras: ["camera-main"],
  target: { type: "anchor", anchorId: "anchor-fan-bedroom", domain: "fan", label: "Ventilador dormitorio" },
  action: { kind: "verb", verb: "level_set", level: 1 },
  mode: "tap",
  cooldownMs: 1000,
  requireArmed: false,
  sensitive: false,
  sensitiveAck: false,
};

describe("validateSentenceDraft", () => {
  it("accepts the owner's targeted fan speed sentence", () => {
    expect(validateSentenceDraft(baseDraft)).toEqual([]);
  });

  it("rejects targeted verbs in global mappings and protects sensitive targets", () => {
    const issues = validateSentenceDraft({
      ...baseDraft,
      kind: "global",
      sensitive: true,
      sensitiveAck: false,
      confirmGestureId: undefined,
    });

    expect(issues.map((issue) => issue.code)).toEqual([
      "verb_requires_target",
      "sensitive_ack_required",
      "confirmation_required",
    ]);
  });

  it("rejects dial properties unsupported by the target domain", () => {
    const issues = validateSentenceDraft({
      ...baseDraft,
      action: { kind: "dial", entity_id: "$selected", property: "volume_level", gain: 1 },
    });

    expect(issues.map((issue) => issue.code)).toContain("dial_requires_numeric_target");
  });
});
