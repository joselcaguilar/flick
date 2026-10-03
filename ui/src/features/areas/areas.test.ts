import { describe, expect, it } from "vitest";
import type { Anchor, Mapping, MappingCreate } from "../../api/types";
import {
  actionEntityIds,
  areaTagOf,
  buildAreaRoutineMapping,
  buildAreaToggleMapping,
  isAreaMappingStale,
  toggleEntityIds,
} from "./model";

const livingRoom = { area_id: "living_room", name: "Living Room" };

function anchor(id: string, entityId: unknown): Anchor {
  return { id, name: id, target: { entity_id: entityId } } as unknown as Anchor;
}

function asMapping(create: MappingCreate): Mapping {
  return { ...create, id: "map_1", enabled: true } as unknown as Mapping;
}

function withFeedback(feedback: unknown): Mapping {
  return { feedback } as unknown as Mapping;
}

function targetOf(create: MappingCreate) {
  return (create.action as unknown as { target: Record<string, unknown> }).target;
}

describe("area mapping builders", () => {
  it("builds an all on mapping with sorted entity ids and an area tag", () => {
    const create = buildAreaToggleMapping(
      livingRoom,
      "all_on",
      ["switch.fan", "light.lamp", "light.lamp"],
      "thumbs_up",
    );
    const action = create.action as unknown as Record<string, unknown>;

    expect(create.name).toBe("Living Room · All on");
    expect(create.gesture_id).toBe("thumbs_up");
    expect(action.domain).toBe("homeassistant");
    expect(action.service).toBe("turn_on");
    expect(targetOf(create).entity_id).toEqual(["light.lamp", "switch.fan"]);
    expect(targetOf(create)).not.toHaveProperty("area_id");
    expect(create.target_mode).toBe("global");
    expect(create.camera_ids).toEqual([]);
    expect(areaTagOf(asMapping(create))).toEqual({ area_id: "living_room", kind: "all_on" });
  });

  it("builds an all off mapping that calls turn_off", () => {
    const create = buildAreaToggleMapping(livingRoom, "all_off", ["light.lamp"], "thumbs_down");
    expect(create.name).toBe("Living Room · All off");
    expect((create.action as unknown as Record<string, unknown>).service).toBe("turn_off");
  });

  it("routes scripts through the script domain and scenes through the scene domain", () => {
    const script = buildAreaRoutineMapping(
      livingRoom,
      { entity_id: "script.good_night", name: "Good night" },
      "fist",
    );
    const scene = buildAreaRoutineMapping(
      livingRoom,
      { entity_id: "scene.movie_night", name: "Movie night" },
      "palm",
    );

    expect(script.name).toBe("Living Room · Good night");
    expect((script.action as unknown as Record<string, unknown>).domain).toBe("script");
    expect((scene.action as unknown as Record<string, unknown>).domain).toBe("scene");
    expect(targetOf(scene).entity_id).toEqual(["scene.movie_night"]);
    expect(areaTagOf(asMapping(script))).toEqual({
      area_id: "living_room",
      kind: "routine",
      entity_id: "script.good_night",
    });
  });
});

describe("areaTagOf", () => {
  it("reads a valid tag", () => {
    expect(
      areaTagOf(withFeedback({ hud: true, flick_area: { area_id: "office", kind: "all_off" } })),
    ).toEqual({
      area_id: "office",
      kind: "all_off",
    });
  });

  it("rejects an unknown kind", () => {
    expect(areaTagOf(withFeedback({ flick_area: { area_id: "office", kind: "dim" } }))).toBeNull();
  });

  it("rejects a routine without an entity id", () => {
    expect(areaTagOf(withFeedback({ flick_area: { area_id: "office", kind: "routine" } }))).toBeNull();
  });

  it("returns null for missing feedback", () => {
    expect(areaTagOf(withFeedback(null))).toBeNull();
    expect(areaTagOf(withFeedback({ hud: true }))).toBeNull();
  });
});

describe("isAreaMappingStale", () => {
  const mapping = asMapping(
    buildAreaToggleMapping(livingRoom, "all_on", ["light.lamp", "switch.fan"], "thumbs_up"),
  );

  it("ignores order differences", () => {
    expect(isAreaMappingStale(mapping, ["switch.fan", "light.lamp"])).toBe(false);
  });

  it("flags an added or removed device", () => {
    expect(isAreaMappingStale(mapping, ["light.lamp", "switch.fan", "light.ceiling"])).toBe(true);
    expect(isAreaMappingStale(mapping, ["light.lamp"])).toBe(true);
  });

  it("never flags routines", () => {
    const routine = asMapping(
      buildAreaRoutineMapping(livingRoom, { entity_id: "scene.movie_night", name: "Movie night" }, "palm"),
    );
    expect(isAreaMappingStale(routine, ["light.lamp"])).toBe(false);
  });
});

describe("entity id helpers", () => {
  it("keeps only toggleable domains, unique and sorted", () => {
    const anchors = [
      anchor("a1", "light.x"),
      anchor("a2", ["switch.y", "sensor.z"]),
      anchor("a3", "light.x"),
      anchor("a4", undefined),
    ];
    expect(toggleEntityIds(anchors)).toEqual(["light.x", "switch.y"]);
  });

  it("reads entity ids from call_service actions only", () => {
    const create = buildAreaToggleMapping(livingRoom, "all_on", ["light.b", "light.a"], "thumbs_up");
    expect(actionEntityIds(create.action)).toEqual(["light.a", "light.b"]);
    expect(actionEntityIds(null)).toEqual([]);
  });
});
