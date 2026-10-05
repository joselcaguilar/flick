import { useHaAreas, usePatchCamera } from "../../api/hooks";
import type { Camera } from "../../api/types";
import { HA_AREA } from "../devices/area";

export function useAreaName() {
  const areas = useHaAreas();
  const names = new Map((areas.data ?? []).map((area) => [area.area_id, area.name]));
  return (areaId?: string | null) => (areaId ? (names.get(areaId) ?? areaId) : null);
}

/** The room a camera is in: set in Flick, else the area of this computer in Home Assistant. */
export function cameraAreaId(camera?: Camera) {
  return camera?.area_override ?? camera?.ha_area_id ?? null;
}

/** A camera's place with a picker that changes it in Flick only, never in Home Assistant. */
export function useCameraPlace(camera?: Camera) {
  const areas = useHaAreas();
  const patchCamera = usePatchCamera();
  const areaName = useAreaName();
  const override = camera?.area_override ?? null;
  const haArea = areaName(camera?.ha_area_id);
  const items = [
    { value: HA_AREA, label: `Home Assistant (${haArea ?? "no area"})` },
    ...[...(areas.data ?? [])]
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((area) => ({ value: area.area_id, label: area.name })),
    ...(override && !areas.data?.some((area) => area.area_id === override)
      ? [{ value: override, label: override }]
      : []),
  ];
  let hint = "Not found in Home Assistant · pick a room";
  let source: "flick" | "ha" | "none" = "none";
  if (override) {
    hint = "Set in Flick · Home Assistant isn't changed";
    source = "flick";
  } else if (haArea) {
    hint = `From Home Assistant · ${camera?.ha_device_name ?? "this Mac"}`;
    source = "ha";
  } else if (camera?.ha_device_name) {
    hint = `${camera.ha_device_name} has no area in Home Assistant`;
  }
  return {
    name: areaName(cameraAreaId(camera)),
    hint,
    source,
    failed: patchCamera.isError,
    select: camera
      ? {
          label: `Place of ${camera.name}`,
          value: override ?? HA_AREA,
          items,
          disabled: patchCamera.isPending,
          onValueChange: (value: string) =>
            patchCamera.mutate({
              id: camera.id,
              patch: { area_override: value === HA_AREA ? null : value },
            }),
        }
      : null,
  };
}
