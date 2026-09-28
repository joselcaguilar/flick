import { describe, expect, it } from "vitest";
import { applyDialDelta, denormalizeDialValue, normalizeDialValue } from "./dial";

describe("dial value math", () => {
  it("applies gain, step, and bounds for Home Assistant percentage values", () => {
    const config = { min: 1, max: 100, gain: 12, step: 1 };

    expect(applyDialDelta(1, 0.5, config)).toBe(7);
    expect(applyDialDelta(96, 1, config)).toBe(100);
    expect(applyDialDelta(4, -1, config)).toBe(1);
  });

  it("normalizes and denormalizes the same range", () => {
    const config = { min: 1, max: 100, gain: 1, step: 1 };

    expect(normalizeDialValue(1, config)).toBe(0);
    expect(normalizeDialValue(100, config)).toBe(1);
    expect(denormalizeDialValue(0.636, config)).toBe(64);
  });
});
