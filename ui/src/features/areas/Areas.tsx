import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  useAnchors,
  useCreateMapping,
  useDeleteMapping,
  useGestures,
  useHaAreas,
  useHaEntities,
  useMappings,
  usePatchMapping,
  useTestMapping,
} from "../../api/hooks";
import type { Anchor, Gesture, HaArea, HaEntity, Mapping } from "../../api/types";
import { DevicePill } from "../../components/domain";
import { Badge, Button, GlassPanel, Select, Switch } from "../../components/ui";
import { anchorEntityIds, effectiveAreaOf, TOGGLE_DOMAINS } from "../devices/area";
import { MappingRow } from "../mappings/Mappings";
import {
  AREA_KIND_LABEL,
  AREA_TOGGLE_KINDS,
  type AreaRoutine,
  type AreaToggleKind,
  areaTagOf,
  areaToggleAction,
  buildAreaRoutineMapping,
  buildAreaToggleMapping,
  domainOf,
  isAreaMappingStale,
  toggleEntityIds,
} from "./model";

type AreaModel = {
  area: Pick<HaArea, "area_id" | "name">;
  members: Anchor[];
  toggleIds: string[];
  routines: AreaRoutine[];
  mappings: Mapping[];
};

type GestureItem = { value: string; label: string; disabled?: boolean };

function errorMessage(error: unknown) {
  if (!error) return null;
  return error instanceof Error ? error.message : String(error);
}

function entityLabel(entity: HaEntity) {
  return entity.name ?? entity.entity_id;
}

function gestureLabel(gesture: Gesture) {
  return gesture.name ?? gesture.id;
}

function devicesLabel(count: number) {
  return `${count} ${count === 1 ? "device" : "devices"}`;
}

function gestureItems(gestures: Gesture[], owners: Map<string, Mapping>, current?: Mapping): GestureItem[] {
  return gestures.map((gesture) => {
    const owner = owners.get(gesture.id);
    const disabled = Boolean(owner && owner.id !== current?.id);
    return {
      value: gesture.id,
      label: disabled
        ? `${gestureLabel(gesture)} — used by ${owner?.name ?? "another mapping"}`
        : gestureLabel(gesture),
      disabled,
    };
  });
}

function mappingForKind(mappings: Mapping[], kind: AreaToggleKind) {
  return mappings.find((mapping) => areaTagOf(mapping)?.kind === kind);
}

function compareName(a: { name: string }, b: { name: string }) {
  return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
}

function buildAreaModels(
  areas: HaArea[],
  anchors: Anchor[],
  entities: HaEntity[],
  mappings: Mapping[],
): { areaModels: AreaModel[]; unassigned: Anchor[]; otherMappings: Mapping[] } {
  const areaById = new Map(areas.map((area) => [area.area_id, area]));
  const entityAreas = new Map(entities.map((entity) => [entity.entity_id, entity.area_id ?? null]));
  const membersByArea = new Map<string, Anchor[]>();
  const unassigned: Anchor[] = [];

  for (const anchor of anchors) {
    const areaId = effectiveAreaOf(anchor, entityAreas);
    if (!areaId) {
      unassigned.push(anchor);
      continue;
    }
    membersByArea.set(areaId, [...(membersByArea.get(areaId) ?? []), anchor]);
  }

  const taggedByArea = new Map<string, Mapping[]>();
  const otherMappings: Mapping[] = [];
  for (const mapping of mappings) {
    const tag = areaTagOf(mapping);
    if (tag) {
      taggedByArea.set(tag.area_id, [...(taggedByArea.get(tag.area_id) ?? []), mapping]);
    } else if (mapping.target_mode === "global") {
      otherMappings.push(mapping);
    }
  }

  const routinesByArea = new Map<string, AreaRoutine[]>();
  for (const entity of entities) {
    if (!entity.area_id || !["scene", "script"].includes(entity.domain)) continue;
    routinesByArea.set(entity.area_id, [
      ...(routinesByArea.get(entity.area_id) ?? []),
      { entity_id: entity.entity_id, name: entityLabel(entity) },
    ]);
  }

  const ids = new Set([...membersByArea.keys(), ...taggedByArea.keys()]);
  const areaModels = [...ids]
    .map((areaId) => {
      const area = areaById.get(areaId) ?? { area_id: areaId, name: areaId, floor_id: null };
      const members = membersByArea.get(areaId) ?? [];
      return {
        area,
        members,
        toggleIds: toggleEntityIds(members),
        routines: (routinesByArea.get(areaId) ?? []).sort(compareName),
        mappings: taggedByArea.get(areaId) ?? [],
      };
    })
    .sort((a, b) => a.area.name.localeCompare(b.area.name, undefined, { sensitivity: "base" }));

  return { areaModels, unassigned, otherMappings };
}

export function AreasRoute() {
  const areas = useHaAreas();
  const entities = useHaEntities();
  const anchors = useAnchors();
  const gestures = useGestures();
  const mappings = useMappings();

  const loading =
    areas.isLoading || entities.isLoading || anchors.isLoading || gestures.isLoading || mappings.isLoading;
  const error = areas.error ?? entities.error ?? anchors.error ?? gestures.error ?? mappings.error;
  const { areaModels, unassigned, otherMappings } = useMemo(
    () => buildAreaModels(areas.data ?? [], anchors.data ?? [], entities.data ?? [], mappings.data ?? []),
    [areas.data, anchors.data, entities.data, mappings.data],
  );
  const globalOwners = useMemo(() => {
    const owners = new Map<string, Mapping>();
    for (const mapping of mappings.data ?? []) {
      if (mapping.target_mode === "global") owners.set(mapping.gesture_id, mapping);
    }
    return owners;
  }, [mappings.data]);

  return (
    <section className="feature-screen areas-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Areas</h1>
          <p>Room gestures for all on, all off and Home Assistant routines.</p>
        </div>
        <Link className="ui-button ui-button-primary ui-button-md" to="/devices/teach">
          Teach a device
        </Link>
      </header>

      {loading ? <p role="status">Loading areas…</p> : null}
      {error ? (
        <p className="device-area-error" role="alert">
          Couldn't load areas: {errorMessage(error) ?? "unknown error"}. Check the engine is running, then
          reload.
        </p>
      ) : null}
      {!loading && !error && areaModels.length === 0 && otherMappings.length === 0 ? (
        <GlassPanel className="empty-panel">
          <h2>No room gestures yet</h2>
          <p>Teach at least one device so Flick can group gestures by room.</p>
          <Link className="ui-button ui-button-primary ui-button-md" to="/devices/teach">
            Teach a device
          </Link>
        </GlassPanel>
      ) : null}

      {!loading && !error && (areaModels.length > 0 || otherMappings.length > 0) ? (
        <>
          <p className="areas-hint">
            Room gestures fire when you aren't pointing at a device. Pointing always wins. Changing an area in
            Flick never edits Home Assistant.
          </p>
          {unassigned.length ? (
            <p className="areas-note">
              {unassigned.length} taught {unassigned.length === 1 ? "device is" : "devices are"} not assigned
              to an area. <Link to="/devices">Review devices</Link>.
            </p>
          ) : null}
          {areaModels.length ? (
            <div className="areas-grid">
              {areaModels.map((area) => (
                <AreaPanel
                  key={area.area.area_id}
                  model={area}
                  gestures={gestures.data ?? []}
                  globalOwners={globalOwners}
                />
              ))}
            </div>
          ) : null}
          <GlassPanel
            className="mapping-group other-gestures-panel"
            role="region"
            aria-labelledby="other-gestures-title"
          >
            <div className="panel-heading">
              <div>
                <h2 id="other-gestures-title">Other gestures</h2>
                <p>Global mappings that are not managed by Areas.</p>
              </div>
            </div>
            <div className="mapping-list">
              {otherMappings.length ? (
                otherMappings.map((mapping) => (
                  <MappingRow key={mapping.id} mapping={mapping} anchors={anchors.data} />
                ))
              ) : (
                <p className="area-empty-row">No other global gestures.</p>
              )}
            </div>
          </GlassPanel>
        </>
      ) : null}
    </section>
  );
}

function AreaPanel({
  model,
  gestures,
  globalOwners,
}: {
  model: AreaModel;
  gestures: Gesture[];
  globalOwners: Map<string, Mapping>;
}) {
  const excluded = model.members.filter((anchor) =>
    anchorEntityIds(anchor).every((id) => !TOGGLE_DOMAINS.has(domainOf(id))),
  );
  return (
    <GlassPanel className="area-panel">
      <header className="area-panel-header">
        <div className="area-panel-title">
          <h2>{model.area.name}</h2>
          <Badge tone="accent">{devicesLabel(model.members.length)}</Badge>
        </div>
        {model.members.length ? (
          <ul className="area-members" aria-label={`Taught devices in ${model.area.name}`}>
            {model.members.map((anchor) => (
              <li key={anchor.id}>
                <DevicePill name={anchor.name} domain={anchor.domain} />
              </li>
            ))}
          </ul>
        ) : (
          <p className="area-empty-row">No taught devices in this area yet.</p>
        )}
      </header>
      {excluded.length ? (
        <p className="area-excluded">
          Not included in All on/off: {excluded.map((anchor) => anchor.name).join(", ")}.
        </p>
      ) : null}
      <ul className="area-gesture-list">
        {AREA_TOGGLE_KINDS.map((kind) => (
          <AreaGestureRow
            key={kind}
            area={model.area}
            kind={kind}
            mapping={mappingForKind(model.mappings, kind)}
            gestures={gestures}
            globalOwners={globalOwners}
            toggleIds={model.toggleIds}
          />
        ))}
        {model.mappings
          .filter((mapping) => areaTagOf(mapping)?.kind === "routine")
          .map((mapping) => {
            const tag = areaTagOf(mapping);
            const routine = model.routines.find((item) => item.entity_id === tag?.entity_id);
            return (
              <RoutineGestureRow
                key={mapping.id}
                area={model.area}
                mapping={mapping}
                routine={
                  routine ?? { entity_id: tag?.entity_id ?? "", name: tag?.entity_id ?? "Missing routine" }
                }
                missing={!routine}
                gestures={gestures}
                globalOwners={globalOwners}
              />
            );
          })}
        <NewRoutineRow
          area={model.area}
          routines={model.routines}
          gestures={gestures}
          globalOwners={globalOwners}
        />
      </ul>
    </GlassPanel>
  );
}

function AreaGestureRow({
  area,
  kind,
  mapping,
  gestures,
  globalOwners,
  toggleIds,
}: {
  area: Pick<HaArea, "area_id" | "name">;
  kind: AreaToggleKind;
  mapping?: Mapping;
  gestures: Gesture[];
  globalOwners: Map<string, Mapping>;
  toggleIds: string[];
}) {
  const createMapping = useCreateMapping();
  const patchMapping = usePatchMapping();
  const stale = mapping ? isAreaMappingStale(mapping, toggleIds) : false;
  const blocked = toggleIds.length === 0;
  const items = gestureItems(gestures, globalOwners, mapping);

  function selectGesture(gestureId: string) {
    if (blocked) return;
    if (mapping) {
      patchMapping.mutate({ id: mapping.id, patch: { gesture_id: gestureId } });
    } else {
      createMapping.mutate(buildAreaToggleMapping(area, kind, toggleIds, gestureId));
    }
  }

  function updateDevices() {
    if (!mapping) return;
    patchMapping.mutate({ id: mapping.id, patch: { action: areaToggleAction(kind, toggleIds) } });
  }

  return (
    <li className="area-gesture-row">
      <div className="area-gesture-main">
        <strong>{AREA_KIND_LABEL[kind]}</strong>
        <span>{blocked ? "No toggleable devices in this area." : devicesLabel(toggleIds.length)}</span>
      </div>
      <Select
        value={mapping?.gesture_id ?? ""}
        onValueChange={selectGesture}
        label={`${area.name} ${AREA_KIND_LABEL[kind]} gesture`}
        items={items}
        placeholder="Choose a gesture"
        disabled={blocked || createMapping.isPending || patchMapping.isPending}
      />
      <div className="area-gesture-actions">
        {stale ? <Badge tone="warning">Out of sync</Badge> : null}
        {stale ? (
          <Button variant="secondary" size="sm" loading={patchMapping.isPending} onClick={updateDevices}>
            Update devices
          </Button>
        ) : null}
        {mapping ? <MappingControls mapping={mapping} /> : null}
      </div>
      {errorMessage(createMapping.error ?? patchMapping.error) ? (
        <p className="inline-error">{errorMessage(createMapping.error ?? patchMapping.error)}</p>
      ) : null}
    </li>
  );
}

function RoutineGestureRow({
  area,
  routine,
  mapping,
  missing,
  gestures,
  globalOwners,
}: {
  area: Pick<HaArea, "area_id" | "name">;
  routine: AreaRoutine;
  mapping: Mapping;
  missing: boolean;
  gestures: Gesture[];
  globalOwners: Map<string, Mapping>;
}) {
  const patchMapping = usePatchMapping();
  return (
    <li className="area-gesture-row">
      <div className="area-gesture-main">
        <strong>{routine.name}</strong>
        <span>{missing ? "Routine not found in Home Assistant." : routine.entity_id}</span>
      </div>
      <Select
        value={mapping.gesture_id}
        onValueChange={(gestureId) =>
          patchMapping.mutate({ id: mapping.id, patch: { gesture_id: gestureId } })
        }
        label={`${area.name} ${routine.name} gesture`}
        items={gestureItems(gestures, globalOwners, mapping)}
        placeholder="Choose a gesture"
        disabled={patchMapping.isPending}
      />
      <div className="area-gesture-actions">
        {missing ? <Badge tone="warning">Missing</Badge> : null}
        <MappingControls mapping={mapping} />
      </div>
      {errorMessage(patchMapping.error) ? (
        <p className="inline-error">{errorMessage(patchMapping.error)}</p>
      ) : null}
    </li>
  );
}

function NewRoutineRow({
  area,
  routines,
  gestures,
  globalOwners,
}: {
  area: Pick<HaArea, "area_id" | "name">;
  routines: AreaRoutine[];
  gestures: Gesture[];
  globalOwners: Map<string, Mapping>;
}) {
  const [routineId, setRoutineId] = useState("");
  const createMapping = useCreateMapping();
  const routine = routines.find((item) => item.entity_id === routineId);

  function selectGesture(gestureId: string) {
    if (!routine) return;
    createMapping.mutate(buildAreaRoutineMapping(area, routine, gestureId), {
      onSuccess: () => setRoutineId(""),
    });
  }

  return (
    <li className="area-gesture-row new-routine-row">
      <div className="area-gesture-main">
        <strong>Add routine</strong>
        <span>Run a Home Assistant scene or script in this area.</span>
      </div>
      <Select
        value={routineId}
        onValueChange={setRoutineId}
        label={`${area.name} routine`}
        items={routines.map((item) => ({ value: item.entity_id, label: item.name }))}
        placeholder="Choose a scene or script"
        disabled={routines.length === 0 || createMapping.isPending}
      />
      <Select
        value=""
        onValueChange={selectGesture}
        label={`${area.name} new routine gesture`}
        items={gestureItems(gestures, globalOwners)}
        placeholder="Choose a gesture"
        disabled={!routine || createMapping.isPending}
      />
      {routines.length === 0 ? <p className="inline-error">No scene or script in this area.</p> : null}
      {errorMessage(createMapping.error) ? (
        <p className="inline-error">{errorMessage(createMapping.error)}</p>
      ) : null}
    </li>
  );
}

function MappingControls({ mapping }: { mapping: Mapping }) {
  const patchMapping = usePatchMapping();
  const deleteMapping = useDeleteMapping();
  return (
    <>
      <TestMappingControl mapping={mapping} />
      <Switch
        checked={mapping.enabled}
        aria-label={`Enable ${mapping.name}`}
        onCheckedChange={(enabled) => patchMapping.mutate({ id: mapping.id, patch: { enabled } })}
      />
      <Button
        variant="danger"
        size="sm"
        loading={deleteMapping.isPending}
        onClick={() => deleteMapping.mutate(mapping.id)}
      >
        Delete
      </Button>
    </>
  );
}

function TestMappingControl({ mapping }: { mapping: Mapping }) {
  const test = useTestMapping(mapping.id);
  return (
    <>
      <Button variant="ghost" size="sm" loading={test.isPending} onClick={() => test.mutate()}>
        Test
      </Button>
      {test.data?.message ? (
        <span className="inline-result" role="status">
          {test.data.message}
        </span>
      ) : null}
    </>
  );
}
