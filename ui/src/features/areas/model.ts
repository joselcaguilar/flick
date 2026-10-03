import type { Action, Anchor, HaArea, Mapping, MappingCreate } from "../../api/types";
import { jsonObject } from "../../api/types";
import { anchorEntityIds, TOGGLE_DOMAINS } from "../devices/area";

export type AreaToggleKind = "all_on" | "all_off";
export type AreaGestureKind = AreaToggleKind | "routine";

export type AreaTag = {
  area_id: string;
  kind: AreaGestureKind;
  entity_id?: string;
};

export type AreaRef = Pick<HaArea, "area_id" | "name">;
export type AreaRoutine = { entity_id: string; name: string };

export const AREA_TOGGLE_KINDS: AreaToggleKind[] = ["all_on", "all_off"];

export const AREA_KIND_LABEL: Record<AreaToggleKind, string> = {
  all_on: "All on",
  all_off: "All off",
};

export const ROUTINE_DOMAINS = new Set(["scene", "script"]);

export function domainOf(entityId: string) {
  const dot = entityId.indexOf(".");
  return dot > 0 ? entityId.slice(0, dot) : "";
}

function sortedUnique(ids: Iterable<string>) {
  return [...new Set(ids)].sort();
}

function stringIds(value: unknown) {
  if (Array.isArray(value)) return value.filter((id): id is string => typeof id === "string");
  return typeof value === "string" ? [value] : [];
}

export function areaTagOf(mapping: Pick<Mapping, "feedback">): AreaTag | null {
  const feedback = mapping.feedback as unknown as Record<string, unknown> | null | undefined;
  const raw = feedback?.flick_area;
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const tag = raw as Record<string, unknown>;
  if (typeof tag.area_id !== "string" || tag.area_id.length === 0) return null;
  if (tag.kind === "all_on" || tag.kind === "all_off") return { area_id: tag.area_id, kind: tag.kind };
  if (tag.kind === "routine" && typeof tag.entity_id === "string" && tag.entity_id.length > 0) {
    return { area_id: tag.area_id, kind: "routine", entity_id: tag.entity_id };
  }
  return null;
}

export function toggleEntityIds(anchors: Anchor[]) {
  return sortedUnique(anchors.flatMap(anchorEntityIds).filter((id) => TOGGLE_DOMAINS.has(domainOf(id))));
}

export function actionEntityIds(action: Action | null | undefined) {
  if (!action) return [];
  const record = action as unknown as Record<string, unknown>;
  if (record.kind !== "call_service") return [];
  const target = record.target as Record<string, unknown> | null | undefined;
  return sortedUnique(stringIds(target?.entity_id));
}

export function areaToggleAction(kind: AreaToggleKind, entityIds: string[]): Action {
  return {
    kind: "call_service",
    domain: "homeassistant",
    service: kind === "all_on" ? "turn_on" : "turn_off",
    target: { entity_id: sortedUnique(entityIds) },
    data: jsonObject({}),
  } as Action;
}

export function routineAction(entityId: string): Action {
  return {
    kind: "call_service",
    domain: domainOf(entityId) === "script" ? "script" : "scene",
    service: "turn_on",
    target: { entity_id: [entityId] },
    data: jsonObject({}),
  } as Action;
}

function areaMapping(name: string, gestureId: string, action: Action, tag: AreaTag): MappingCreate {
  return {
    name,
    gesture_id: gestureId,
    hand: "any",
    camera_ids: [],
    target_mode: "global",
    mode: "tap",
    action,
    sensitive_ack: false,
    feedback: jsonObject({ hud: true, sound: true, flick_area: tag }),
    active_hours: jsonObject({}),
  };
}

export function buildAreaToggleMapping(
  area: AreaRef,
  kind: AreaToggleKind,
  entityIds: string[],
  gestureId: string,
): MappingCreate {
  return areaMapping(
    `${area.name} · ${AREA_KIND_LABEL[kind]}`,
    gestureId,
    areaToggleAction(kind, entityIds),
    { area_id: area.area_id, kind },
  );
}

export function buildAreaRoutineMapping(
  area: AreaRef,
  routine: AreaRoutine,
  gestureId: string,
): MappingCreate {
  return areaMapping(`${area.name} · ${routine.name}`, gestureId, routineAction(routine.entity_id), {
    area_id: area.area_id,
    kind: "routine",
    entity_id: routine.entity_id,
  });
}

export function isAreaMappingStale(mapping: Pick<Mapping, "feedback" | "action">, entityIds: string[]) {
  const tag = areaTagOf(mapping);
  if (!tag || tag.kind === "routine") return false;
  const current = actionEntityIds(mapping.action);
  const expected = sortedUnique(entityIds);
  return current.length !== expected.length || current.some((id, index) => id !== expected[index]);
}
