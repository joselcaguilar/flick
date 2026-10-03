import type { Anchor } from "../../api/types";

export const HA_AREA = "__ha__";

export const TOGGLE_DOMAINS = new Set([
  "light",
  "switch",
  "fan",
  "input_boolean",
  "media_player",
  "climate",
  "humidifier",
]);

export function firstId(value: unknown): string | undefined {
  if (Array.isArray(value)) return typeof value[0] === "string" ? value[0] : undefined;
  return typeof value === "string" ? value : undefined;
}

function anchorTargetRecord(anchor: Anchor) {
  return (anchor.target ?? {}) as unknown as Record<string, unknown>;
}

export function anchorEntityIds(anchor: Anchor): string[] {
  const value = anchorTargetRecord(anchor).entity_id;
  if (Array.isArray(value)) return value.filter((id): id is string => typeof id === "string");
  return typeof value === "string" ? [value] : [];
}

export function haAreaOf(anchor: Anchor, entityAreas: Map<string, string | null>): string | null {
  const target = anchorTargetRecord(anchor);
  const entityId = firstId(target.entity_id);
  return (entityId ? entityAreas.get(entityId) : undefined) ?? firstId(target.area_id) ?? null;
}

export function effectiveAreaOf(anchor: Anchor, entityAreas: Map<string, string | null>): string | null {
  return anchor.area_override ?? haAreaOf(anchor, entityAreas);
}
