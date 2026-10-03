import { type FormEvent, useEffect, useId, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import {
  useAnchors,
  useCameras,
  useCancelTeach,
  useCommitTeach,
  useDeleteAnchor,
  useHaAreas,
  useHaEntities,
  useMappings,
  usePatchAnchor,
  usePatchMapping,
  usePlaces,
  useStartRealign,
  useStartTeach,
  useStatus,
  useTeachCurrentLevel,
  useTeachLevelTest,
  useTeachSpot,
} from "../../api/hooks";
import type {
  Action,
  Anchor,
  HaEntity,
  Mapping,
  Place,
  TeachCommitResponse,
  TeachSpotResponse,
  TeachVerb,
} from "../../api/types";
import {
  ConfidenceMeter,
  DevicePill,
  EntityIcon,
  GestureGlyph,
  PreviewCanvas,
} from "../../components/domain";
import { Badge, Button, GlassPanel, Input, ListRow, Select, Skeleton, Switch } from "../../components/ui";
import { useEventStore } from "../../events/store";
import { useActiveCamera, useCameraLive, useLiveHands } from "../../events/useLiveHands";
import { formatTime } from "../../lib/utils";
import { effectiveAreaOf, HA_AREA, haAreaOf } from "./area";

function anchorTarget(anchor: Anchor) {
  return anchor.target as unknown as { entity_id?: string; device_id?: string; area_id?: string };
}

function anchorTargetEntity(anchor: Anchor) {
  const target = anchorTarget(anchor);
  return target.entity_id ?? target.device_id ?? target.area_id ?? "Unknown target";
}

function reteachHref(anchor: Anchor) {
  const params = new URLSearchParams({ anchor: anchor.id });
  const entityId = anchorTarget(anchor).entity_id;
  if (entityId) params.set("entity", entityId);
  return `/devices/teach?${params}`;
}

function statusLabel(status?: string | null) {
  if (!status) return "Unknown";
  const label = status.replace(/_/g, " ");
  return label.charAt(0).toUpperCase() + label.slice(1);
}

function useCameraNames() {
  const cameras = useCameras();
  return (cameraId: string) => cameras.data?.find((camera) => camera.id === cameraId)?.name ?? "Camera";
}

function pickOwnerFan(entities: HaEntity[]) {
  return entities.find((entity) => entity.entity_id === "fan.ventilador_dormitorio") ?? entities[0];
}

function statusTone(status?: string | null): "neutral" | "accent" | "success" | "warning" | "danger" {
  if (status === "ok" || status === "ready" || status === "running") return "success";
  if (status === "needs_realign" || status === "needs_reteach" || status === "error") return "danger";
  if (status === "capturing" || status === "aiming") return "warning";
  return "neutral";
}

function actionLabel(label: string) {
  return label.replace(/^(Circle clockwise|Two hands apart|Thumbs up)\s*→\s*/i, "");
}

function VerbChip({ gestureId, label }: { gestureId: string; label: string }) {
  return (
    <span className="verb-chip">
      <GestureGlyph name={gestureId} animated={false} />
      {actionLabel(label)}
    </span>
  );
}

function RoomSketch({ anchors }: { anchors: Anchor[] }) {
  return (
    <div className="room-sketch" role="img" aria-label={anchors.map((anchor) => anchor.name).join(", ")}>
      <span className="camera-dot">Camera</span>
      {anchors.map((anchor, index) => {
        const angle = index * 32 - 28;
        return (
          <i key={anchor.id} style={{ rotate: `${angle}deg` }} data-side={angle < 0 ? "start" : "end"}>
            <b style={{ rotate: `${-angle}deg` }}>{anchor.name}</b>
          </i>
        );
      })}
    </div>
  );
}

function entityAttributes(entity?: HaEntity) {
  return (entity?.attributes ?? {}) as Record<string, unknown>;
}

function numberAttribute(entity: HaEntity | undefined, key: string) {
  const value = entityAttributes(entity)[key];
  return typeof value === "number" ? value : undefined;
}

function isSensitiveDomain(domain?: string) {
  return domain === "lock" || domain === "alarm_control_panel" || domain === "valve" || domain === "siren";
}

function isSensitiveEntity(entity?: HaEntity) {
  if (!entity) return false;
  return (
    isSensitiveDomain(entity.domain) ||
    (entity.domain === "cover" && ["garage", "door", "gate"].includes(entity.device_class ?? ""))
  );
}

function entityDetail(entity: HaEntity) {
  const step = numberAttribute(entity, "percentage_step");
  const state = entity.state ? `${entity.state}` : "unknown";
  const meta = step ? `percentage step ${step}` : (entity.device_class ?? entity.domain);
  return `${entity.entity_id} · ${state} · ${meta}`;
}

function levelLabel(action: Action) {
  if (action.kind !== "verb") return "service";
  if (action.verb === "level_set") return `speed ${action.level ?? 1}`;
  return action.verb.replace(/_/g, " ");
}

function TeachPreview({
  title,
  detail,
  sessionId,
  cameraId,
}: {
  title: string;
  detail: string;
  sessionId?: string;
  cameraId?: string;
}) {
  const activeCamera = useActiveCamera();
  const id = cameraId || activeCamera.id;
  const running = useCameraLive(id)?.state === "running";
  const hands = useLiveHands(id);
  const teach = useEventStore((state) => state.teach);
  const progress = teach?.session_id === sessionId ? teach : undefined;
  return (
    <GlassPanel className="teach-preview-panel">
      <div className="preview-header">
        <div>
          <span>Live ray</span>
          <strong>{title}</strong>
        </div>
        <Badge tone={progress?.phase === "error" ? "danger" : "success"}>{progress?.hint ?? detail}</Badge>
      </div>
      <PreviewCanvas
        cameraId={running ? id : undefined}
        alt={running ? "Live camera preview with pointing ray" : "Camera is off"}
        hands={running ? hands?.hands : []}
        ray={running ? hands?.ray : undefined}
      />
      {running ? null : (
        <p className="camera-note">
          This camera is off. <Link to="/cameras">Start it from Cameras</Link>, then capture the spot.
        </p>
      )}
      <div className="teach-ray-caption">
        <ConfidenceMeter
          value={progress?.confidence ?? 0}
          label={
            progress?.ray_jitter_deg != null
              ? `Jitter ${progress.ray_jitter_deg.toFixed(1)}°`
              : "Waiting for a steady ray"
          }
        />
        <span>{progress ? statusLabel(progress.phase) : "Median ray only"} · no camera image saved</span>
      </div>
    </GlassPanel>
  );
}

type TeachStage = "pick" | "spots" | "levels" | "verbs" | "done";

interface VerbDraft {
  id: string;
  label: string;
  gestureId: string;
  enabled: boolean;
  action: Action;
}

const gestureChoices = [
  { value: "builtin.circle_any", label: "Circle" },
  { value: "builtin.circle_cw", label: "Circle clockwise" },
  { value: "builtin.two_hand_separate", label: "Two hands apart" },
  { value: "builtin.thumb_up", label: "Thumbs up" },
  { value: "builtin.thumb_down", label: "Thumbs down" },
];

function actionChoices(domain: string, levels: number[]) {
  if (domain === "fan") {
    return [
      {
        value: "level_set:1",
        label: `Speed 1 · ${levels[0] ?? 1}%`,
        action: { kind: "verb", verb: "level_set", level: 1 } as Action,
      },
      { value: "off", label: "Off", action: { kind: "verb", verb: "off" } as Action },
      { value: "on", label: "On", action: { kind: "verb", verb: "on" } as Action },
      { value: "toggle", label: "Toggle", action: { kind: "verb", verb: "toggle" } as Action },
    ];
  }
  return [
    { value: "on", label: "On", action: { kind: "verb", verb: "on" } as Action },
    { value: "off", label: "Off", action: { kind: "verb", verb: "off" } as Action },
    { value: "stop", label: "Stop", action: { kind: "verb", verb: "stop" } as Action },
  ];
}

function defaultVerbDrafts(domain: string): VerbDraft[] {
  if (domain === "fan") {
    return [
      {
        id: "circle",
        label: "Point + circle → speed 1",
        gestureId: "builtin.circle_any",
        enabled: true,
        action: { kind: "verb", verb: "level_set", level: 1 },
      },
      {
        id: "stop",
        label: "Point + two hands apart → off",
        gestureId: "builtin.two_hand_separate",
        enabled: true,
        action: { kind: "verb", verb: "off" },
      },
      {
        id: "on",
        label: "Point + thumbs up → on",
        gestureId: "builtin.thumb_up",
        enabled: true,
        action: { kind: "verb", verb: "on" },
      },
    ];
  }
  return [
    {
      id: "circle",
      label: "Point + circle → on",
      gestureId: "builtin.circle_cw",
      enabled: true,
      action: { kind: "verb", verb: "on" },
    },
    {
      id: "stop",
      label: "Point + two hands apart → off",
      gestureId: "builtin.two_hand_separate",
      enabled: true,
      action: { kind: "verb", verb: "off" },
    },
  ];
}

function verbDraftsFromMappings(mappings: Mapping[], anchorId: string): VerbDraft[] {
  return mappings
    .filter((mapping) => mapping.anchor_id === anchorId)
    .map((mapping) => {
      const gesture =
        gestureChoices.find((choice) => choice.value === mapping.gesture_id)?.label ??
        mapping.gesture_name ??
        mapping.gesture_id;
      return {
        id: mapping.id,
        label: `Point + ${gesture.toLowerCase()} → ${levelLabel(mapping.action)}`,
        gestureId: mapping.gesture_id,
        enabled: mapping.enabled,
        action: mapping.action,
      };
    });
}

export function DevicesRoute() {
  const places = usePlaces();
  const anchors = useAnchors();
  const areas = useHaAreas();
  const entities = useHaEntities();
  const patchAnchor = usePatchAnchor();
  const cameraName = useCameraNames();

  const areaNames = new Map((areas.data ?? []).map((area) => [area.area_id, area.name]));
  const entityAreas = new Map(
    (entities.data ?? []).map((entity) => [entity.entity_id, entity.area_id ?? null]),
  );
  const placesById = new Map((places.data ?? []).map((place) => [place.id, place]));
  const areaItems = [...(areas.data ?? [])]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((area) => ({ value: area.area_id, label: area.name }));
  const areaName = (areaId: string | null) => (areaId ? (areaNames.get(areaId) ?? areaId) : "No area");
  const groups = new Map<string, Anchor[]>();
  for (const anchor of anchors.data ?? []) {
    const key = effectiveAreaOf(anchor, entityAreas) ?? "";
    const list = groups.get(key);
    if (list) list.push(anchor);
    else groups.set(key, [anchor]);
  }
  const areaGroups = [...groups.entries()]
    .map(([key, groupAnchors]) => ({ areaId: key || null, anchors: groupAnchors }))
    .sort((a, b) => {
      if (!a.areaId) return 1;
      if (!b.areaId) return -1;
      return areaName(a.areaId).localeCompare(areaName(b.areaId));
    });

  return (
    <section className="feature-screen devices-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Devices</h1>
          <p>
            Every taught device by room, with the camera that sees it. Change a device's room here without
            touching Home Assistant.
          </p>
        </div>
        <Link className="ui-button ui-button-primary ui-button-md" to="/devices/teach">
          Teach a device
        </Link>
      </header>

      {anchors.isLoading ? <Skeleton /> : null}
      {anchors.isError ? (
        <p className="device-area-error" role="alert">
          Couldn't load devices.
        </p>
      ) : null}
      {!anchors.isLoading && !anchors.isError && !anchors.data?.length ? (
        <GlassPanel className="empty-state-panel">
          <EntityIcon domain="fan" />
          <h2>No devices taught</h2>
          <p>Point at something in the room — Flick will remember it.</p>
        </GlassPanel>
      ) : null}

      <div className="devices-area-grid">
        {areaGroups.map(({ areaId, anchors: areaAnchors }) => (
          <GlassPanel className="place-card area-card" key={areaId ?? "none"}>
            <div className="panel-heading">
              <div>
                <span>{areaAnchors.length === 1 ? "1 device" : `${areaAnchors.length} devices`}</span>
                <strong>{areaName(areaId)}</strong>
              </div>
            </div>
            <ul className="device-row-list">
              {areaAnchors.map((anchor) => {
                const place = placesById.get(anchor.place_id);
                const cameraId = anchor.camera_id ?? place?.camera_id;
                const haArea = haAreaOf(anchor, entityAreas);
                const override = anchor.area_override ?? null;
                const items = [
                  { value: HA_AREA, label: `Home Assistant (${areaName(haArea)})` },
                  ...areaItems,
                  ...(override && !areaNames.has(override) ? [{ value: override, label: override }] : []),
                ];
                const saving = patchAnchor.isPending && patchAnchor.variables?.id === anchor.id;
                const failed = patchAnchor.isError && patchAnchor.variables?.id === anchor.id;
                return (
                  <li className="device-row" key={anchor.id}>
                    <div className="device-row-main">
                      <DevicePill
                        name={anchor.name}
                        domain={anchor.domain}
                        detail={anchorTargetEntity(anchor)}
                      />
                      <div className="device-card-meta">
                        <span>{cameraId ? cameraName(cameraId) : "No camera"}</span>
                        <span>{place?.name ?? "Unknown place"}</span>
                        <Badge tone={statusTone(anchor.status)}>{anchor.status}</Badge>
                        <span>
                          {anchor.last_used_at
                            ? `Last used ${formatTime(anchor.last_used_at)}`
                            : "Not used yet"}
                        </span>
                      </div>
                      <div className="verb-list">
                        {anchor.verbs.map((verb) => (
                          <VerbChip
                            key={`${anchor.id}-${verb.gesture_id}`}
                            gestureId={verb.gesture_id}
                            label={verb.label}
                          />
                        ))}
                      </div>
                    </div>
                    <div className="device-row-side">
                      <div className="device-area-field">
                        <Select
                          label={`Area for ${anchor.name}`}
                          value={override ?? HA_AREA}
                          items={items}
                          disabled={saving}
                          onValueChange={(value) =>
                            patchAnchor.mutate({
                              id: anchor.id,
                              patch: { area_override: value === HA_AREA ? null : value },
                            })
                          }
                        />
                        <p className="device-area-hint">
                          {override ? "Set in Flick · Home Assistant isn't changed" : "From Home Assistant"}
                        </p>
                        {failed ? (
                          <p className="device-area-error" role="alert">
                            Couldn't change the area. Try again.
                          </p>
                        ) : null}
                      </div>
                      <div className="device-card-actions">
                        <Link className="ui-button ui-button-secondary ui-button-sm" to={reteachHref(anchor)}>
                          Re-teach
                        </Link>
                        <Link
                          className="ui-button ui-button-secondary ui-button-sm"
                          to={`/devices/edit?anchor=${anchor.id}`}
                          aria-label={`Edit ${anchor.name}`}
                        >
                          Edit
                        </Link>
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          </GlassPanel>
        ))}
      </div>
    </section>
  );
}

function problemCode(error: unknown) {
  return (error as { problem?: { code?: string } } | null)?.problem?.code;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : "Something went wrong. Try again.";
}

function gestureLabel(gestureId: string) {
  return gestureChoices.find((choice) => choice.value === gestureId)?.label ?? gestureId;
}

function spotErrorMessage(error: unknown) {
  switch (problemCode(error)) {
    case "camera_not_running":
      return "This camera is off. Start it from Cameras, then capture again.";
    case "camera_mismatch":
      return "Add spots with the camera that taught this device.";
    case "target_mismatch":
      return "This device's Home Assistant target changed. Re-teach it instead.";
    case "anchor_geometry":
      return "Flick couldn't read this device's saved position. Re-teach it instead.";
    default:
      return errorMessage(error);
  }
}

export function DeviceEditRoute() {
  const [searchParams] = useSearchParams();
  const anchorId = searchParams.get("anchor") ?? "";
  const anchors = useAnchors();
  const places = usePlaces();
  const mappings = useMappings();
  const anchor = anchors.data?.find((item) => item.id === anchorId);

  if (!anchor) {
    return (
      <section className="feature-screen teach-screen" aria-labelledby="screen-title">
        <header className="operate-header">
          <div>
            <h1 id="screen-title">Edit device</h1>
          </div>
          <Link className="ui-button ui-button-secondary ui-button-md" to="/devices">
            Back to devices
          </Link>
        </header>
        {anchors.isLoading ? (
          <Skeleton />
        ) : (
          <GlassPanel className="empty-state-panel">
            <h2>Device not found</h2>
            <p>It may have been deleted. Pick another one from Devices.</p>
          </GlassPanel>
        )}
      </section>
    );
  }

  return (
    <DeviceEditor
      key={anchor.id}
      anchor={anchor}
      place={places.data?.find((item) => item.id === anchor.place_id)}
      mappings={(mappings.data ?? []).filter((mapping) => mapping.anchor_id === anchor.id)}
    />
  );
}

function DeviceEditor({ anchor, place, mappings }: { anchor: Anchor; place?: Place; mappings: Mapping[] }) {
  const navigate = useNavigate();
  const cameraName = useCameraNames();
  const patchAnchor = usePatchAnchor();
  const patchMapping = usePatchMapping();
  const deleteAnchor = useDeleteAnchor();
  const startTeach = useStartTeach();
  const [sessionId, setSessionId] = useState<string | null>(null);
  const teachSpot = useTeachSpot(sessionId ?? undefined);
  const commitTeach = useCommitTeach(sessionId ?? undefined);
  const cancelTeach = useCancelTeach(sessionId ?? undefined);
  const nameId = useId();
  const [name, setName] = useState(anchor.name);
  const [nameSaved, setNameSaved] = useState(false);
  const [spots, setSpots] = useState<TeachSpotResponse[]>([]);
  const [spotError, setSpotError] = useState<string | null>(null);
  const [savedSpots, setSavedSpots] = useState<number | null>(null);
  const [mappingError, setMappingError] = useState<string | null>(null);
  const [pendingMappingId, setPendingMappingId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const deleteTrigger = useRef<HTMLButtonElement>(null);
  const keepButton = useRef<HTMLButtonElement>(null);
  const confirmOpened = useRef(false);
  const cancelOnLeave = useRef<() => void>(() => undefined);
  const cameraId = anchor.camera_id ?? place?.camera_id ?? "";
  const camera = cameraId ? cameraName(cameraId) : "—";
  const trimmedName = name.trim();
  const canSaveName = trimmedName.length > 0 && trimmedName !== anchor.name;

  useEffect(() => setName(anchor.name), [anchor.name]);

  useEffect(() => {
    if (confirmDelete) {
      confirmOpened.current = true;
      keepButton.current?.focus();
    } else if (confirmOpened.current) {
      confirmOpened.current = false;
      deleteTrigger.current?.focus();
    }
  }, [confirmDelete]);

  useEffect(() => {
    cancelOnLeave.current = sessionId ? () => cancelTeach.mutate() : () => undefined;
  });

  useEffect(() => () => cancelOnLeave.current(), []);

  function saveName(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSaveName) return;
    patchAnchor.mutate(
      { id: anchor.id, patch: { name: trimmedName } },
      { onSuccess: () => setNameSaved(true) },
    );
  }

  function updateMapping(mapping: Mapping, patch: Partial<Mapping>) {
    setMappingError(null);
    setPendingMappingId(mapping.id);
    patchMapping.mutate(
      { id: mapping.id, patch },
      {
        onError: (error) =>
          setMappingError(
            problemCode(error) === "gesture_conflict" && patch.gesture_id
              ? `${gestureLabel(patch.gesture_id)} is already used for another action on ${anchor.name}. Pick a different gesture.`
              : errorMessage(error),
          ),
        onSettled: () => setPendingMappingId(null),
      },
    );
  }

  async function startSpots() {
    setSpotError(null);
    setSavedSpots(null);
    setSpots([]);
    try {
      const started = await startTeach.mutateAsync({
        camera_id: cameraId,
        target: anchor.target,
        anchor_id: anchor.id,
        append: true,
      });
      setSessionId(started.id);
    } catch (error) {
      setSpotError(spotErrorMessage(error));
    }
  }

  async function captureSpot() {
    setSpotError(null);
    try {
      const spot = await teachSpot.mutateAsync();
      setSpots((current) => [...current, spot]);
    } catch (error) {
      setSpotError(spotErrorMessage(error));
    }
  }

  async function saveSpots() {
    setSpotError(null);
    try {
      await commitTeach.mutateAsync({ verbs: [] });
      setSavedSpots(spots.length);
      setSpots([]);
      setSessionId(null);
    } catch (error) {
      setSpotError(spotErrorMessage(error));
    }
  }

  async function cancelSpots() {
    await cancelTeach.mutateAsync().catch(() => undefined);
    setSpots([]);
    setSessionId(null);
    setSpotError(null);
  }

  function removeDevice() {
    deleteAnchor.mutate(anchor.id, { onSuccess: () => navigate("/devices", { replace: true }) });
  }

  return (
    <section className="feature-screen teach-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">{anchor.name}</h1>
          <p>{`${anchorTargetEntity(anchor)} · ${camera} · ${place?.name ?? "—"}`}</p>
        </div>
        <Link className="ui-button ui-button-secondary ui-button-md" to="/devices">
          Back to devices
        </Link>
      </header>
      <div className="teach-layout">
        <div className="teach-main">
          {sessionId ? (
            <TeachPreview
              title={`Point at ${anchor.name} from a new spot`}
              detail={`new spot ${spots.length + 1} · hold steady`}
              sessionId={sessionId}
              cameraId={cameraId}
            />
          ) : null}
          <GlassPanel className="teach-step-card">
            <h2>Name</h2>
            <form className="device-edit-name" onSubmit={saveName}>
              <label className="sentence-field" htmlFor={nameId}>
                <span>Device name</span>
                <Input
                  id={nameId}
                  value={name}
                  onChange={(event) => {
                    setName(event.currentTarget.value);
                    setNameSaved(false);
                  }}
                />
              </label>
              <Button type="submit" variant="primary" loading={patchAnchor.isPending} disabled={!canSaveName}>
                Save name
              </Button>
            </form>
            {nameSaved ? <p className="inline-result">Name saved.</p> : null}
            {patchAnchor.error ? (
              <p className="inline-error" role="alert">
                {patchAnchor.error.message}
              </p>
            ) : null}
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <h2>Gestures</h2>
            <p>Point at {anchor.name}, then make the gesture. Switch one off to pause it.</p>
            {mappings.length ? (
              <div className="teach-verb-editor">
                {mappings.map((mapping) => {
                  const level = levelLabel(mapping.action);
                  const choices = gestureChoices.some((choice) => choice.value === mapping.gesture_id)
                    ? gestureChoices
                    : [
                        ...gestureChoices,
                        { value: mapping.gesture_id, label: mapping.gesture_name ?? mapping.gesture_id },
                      ];
                  return (
                    <article className="teach-verb-row" key={mapping.id}>
                      <Switch
                        checked={mapping.enabled}
                        disabled={pendingMappingId === mapping.id}
                        aria-label={`Enable gesture for ${level}`}
                        onCheckedChange={(enabled) => updateMapping(mapping, { enabled })}
                      />
                      <GestureGlyph name={mapping.gesture_id} animated={false} />
                      <Select
                        value={mapping.gesture_id}
                        label={`Gesture for ${level}`}
                        items={choices}
                        onValueChange={(value) => {
                          if (value !== mapping.gesture_id) updateMapping(mapping, { gesture_id: value });
                        }}
                      />
                      <strong>{level}</strong>
                    </article>
                  );
                })}
              </div>
            ) : (
              <>
                <p>No gestures yet.</p>
                <div className="level-actions">
                  <Link className="ui-button ui-button-secondary ui-button-sm" to={reteachHref(anchor)}>
                    Re-teach {anchor.name}
                  </Link>
                </div>
              </>
            )}
            {mappingError ? (
              <p className="inline-error" role="alert">
                {mappingError}
              </p>
            ) : null}
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <h2>Add spots from another angle</h2>
            {cameraId ? (
              <p>
                Extra angles help Flick recognize {anchor.name} from more of the room. Use {camera}, the
                camera that taught it.
              </p>
            ) : (
              <p className="camera-note">
                Flick doesn't know which camera taught this device. Re-teach it to add spots.
              </p>
            )}
            {sessionId ? (
              <>
                {spots.length ? (
                  <div className="spot-result-grid">
                    {spots.map((spot, index) => (
                      <div className="spot-result" key={spot.spot_index}>
                        <Badge tone={spot.confidence >= 0.85 ? "success" : "warning"}>
                          New spot {index + 1}
                        </Badge>
                        <ConfidenceMeter
                          value={spot.confidence}
                          label={`${spot.kind} · jitter ${spot.ray_jitter_deg.toFixed(1)}°`}
                        />
                        <span>
                          {spot.residual_deg == null
                            ? "Residual pending"
                            : `${spot.residual_deg.toFixed(1)}° residual`}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : null}
                <div className="level-actions">
                  <Button
                    variant={spots.length ? "secondary" : "primary"}
                    loading={teachSpot.isPending}
                    onClick={() => void captureSpot()}
                  >
                    Capture new spot
                  </Button>
                  <Button
                    variant={spots.length ? "primary" : "secondary"}
                    disabled={!spots.length}
                    loading={commitTeach.isPending}
                    onClick={() => void saveSpots()}
                  >
                    Save new spots
                  </Button>
                  <Button variant="ghost" onClick={() => void cancelSpots()}>
                    Cancel
                  </Button>
                </div>
              </>
            ) : (
              <div className="level-actions">
                <Button
                  variant="primary"
                  disabled={!cameraId}
                  loading={startTeach.isPending}
                  onClick={() => void startSpots()}
                >
                  Add spots
                </Button>
              </div>
            )}
            {savedSpots != null ? (
              <p className="inline-result">
                {`New spots saved. ${anchor.name} now uses ${savedSpots} more ${savedSpots === 1 ? "angle" : "angles"}.`}
              </p>
            ) : null}
            {spotError ? (
              <p className="inline-error" role="alert">
                {spotError}
              </p>
            ) : null}
          </GlassPanel>
        </div>
        <aside className="teach-side">
          <GlassPanel className="picked-device-card">
            <span>Device</span>
            <DevicePill name={anchor.name} domain={anchor.domain} detail={anchorTargetEntity(anchor)} />
            <dl className="device-edit-facts">
              <div>
                <dt>Status</dt>
                <dd>
                  <Badge tone={statusTone(anchor.status)}>{statusLabel(anchor.status)}</Badge>
                </dd>
              </div>
              <div>
                <dt>Camera</dt>
                <dd>{camera}</dd>
              </div>
              <div>
                <dt>Place</dt>
                <dd>{place?.name ?? "—"}</dd>
              </div>
            </dl>
            {isSensitiveDomain(anchor.domain) ? <Badge tone="warning">Sensitive</Badge> : null}
            <div className="level-actions">
              <Link className="ui-button ui-button-secondary ui-button-sm" to={reteachHref(anchor)}>
                Re-teach
              </Link>
            </div>
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <h2>Delete device</h2>
            <p>Flick forgets {anchor.name}'s position and its gestures. Home Assistant isn't changed.</p>
            {confirmDelete ? (
              <div className="device-edit-confirm">
                <strong>Delete {anchor.name}?</strong>
                <div className="level-actions">
                  <Button variant="danger" loading={deleteAnchor.isPending} onClick={removeDevice}>
                    Delete device
                  </Button>
                  <Button ref={keepButton} variant="ghost" onClick={() => setConfirmDelete(false)}>
                    Keep device
                  </Button>
                </div>
              </div>
            ) : (
              <div className="level-actions">
                <Button ref={deleteTrigger} variant="secondary" onClick={() => setConfirmDelete(true)}>
                  Delete device…
                </Button>
              </div>
            )}
            {deleteAnchor.error ? (
              <p className="inline-error" role="alert">
                {deleteAnchor.error.message}
              </p>
            ) : null}
          </GlassPanel>
        </aside>
      </div>
    </section>
  );
}

export function OnboardingTeachDeviceStep() {
  return (
    <GlassPanel className="onboarding-teach-step">
      <div>
        <span>Optional step 5</span>
        <h2>Point at a device</h2>
        <p>Point at something in this room — like the ceiling fan — and Flick will remember it.</p>
      </div>
      <DevicePill name="Ventilador dormitorio" domain="fan" detail="Tuya · percentage step 1" />
      <div className="verb-list">
        <VerbChip gestureId="builtin.circle_cw" label="Circle clockwise → Speed 1" />
        <VerbChip gestureId="builtin.two_hand_separate" label="Two hands apart → Off" />
      </div>
    </GlassPanel>
  );
}

export function TeachDeviceRoute() {
  const [searchParams] = useSearchParams();
  const reteachEntityId = searchParams.get("entity") ?? "";
  const reteachAnchorId = searchParams.get("anchor") ?? "";
  const entities = useHaEntities("");
  const status = useStatus();
  const configuredCameras = useCameras();
  const anchors = useAnchors();
  const mappings = useMappings();
  const eventCameras = useEventStore((state) => state.cameras);
  const startTeach = useStartTeach();
  const ownerFan = pickOwnerFan(entities.data ?? []);
  const [stage, setStage] = useState<TeachStage>("pick");
  const [query, setQuery] = useState(reteachEntityId || "fan");
  const [selectedEntityId, setSelectedEntityId] = useState<string>(reteachEntityId);
  const [cameraId, setCameraId] = useState("");
  const [session, setSession] = useState<{ id: string; prompt: string } | null>(null);
  const [spots, setSpots] = useState<TeachSpotResponse[]>([]);
  const [levels, setLevels] = useState<number[]>([]);
  const [levelToCapture, setLevelToCapture] = useState(1);
  const [verbs, setVerbs] = useState<VerbDraft[]>([]);
  const [commitResult, setCommitResult] = useState<TeachCommitResponse | null>(null);
  const teachSpot = useTeachSpot(session?.id);
  const teachLevel = useTeachCurrentLevel(session?.id);
  const testLevel = useTeachLevelTest(session?.id);
  const commitTeach = useCommitTeach(session?.id);
  const cancelTeach = useCancelTeach(session?.id);

  const cameraOptions = useMemo(() => {
    const live = status.data?.cameras ?? [];
    const stateOf = (id: string) =>
      eventCameras[id]?.state ?? live.find((camera) => camera.camera_id === id)?.state ?? "stopped";
    const options = (configuredCameras.data ?? []).map((camera) => ({
      value: camera.id,
      name: camera.name,
      state: stateOf(camera.id),
    }));
    for (const camera of live) {
      if (!options.some((option) => option.value === camera.camera_id)) {
        options.push({
          value: camera.camera_id,
          name: camera.name ?? "Camera",
          state: stateOf(camera.camera_id),
        });
      }
    }
    return options.map((option) => ({ ...option, label: `${option.name} · ${statusLabel(option.state)}` }));
  }, [configuredCameras.data, eventCameras, status.data?.cameras]);

  useEffect(() => {
    if (!selectedEntityId && ownerFan) setSelectedEntityId(ownerFan.entity_id);
  }, [ownerFan, selectedEntityId]);

  useEffect(() => {
    if (cameraId || status.isLoading || configuredCameras.isLoading) return;
    const preferred = cameraOptions.find((option) => option.state === "running") ?? cameraOptions[0];
    if (preferred) setCameraId(preferred.value);
  }, [cameraId, cameraOptions, configuredCameras.isLoading, status.isLoading]);

  const visibleEntities = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return (entities.data ?? [])
      .filter((entity) =>
        normalized
          ? `${entity.name} ${entity.entity_id} ${entity.domain}`.toLowerCase().includes(normalized)
          : true,
      )
      .slice(0, 12);
  }, [entities.data, query]);

  const selectedEntity =
    (entities.data ?? []).find((entity) => entity.entity_id === selectedEntityId) ?? ownerFan;
  const domain = selectedEntity?.domain ?? "fan";
  const sensitive = isSensitiveEntity(selectedEntity);
  const teachFanLevels = domain === "fan";
  const replacesAnchorId =
    reteachAnchorId && selectedEntity?.entity_id === reteachEntityId ? reteachAnchorId : undefined;
  const reteachName = replacesAnchorId
    ? (anchors.data?.find((anchor) => anchor.id === replacesAnchorId)?.name ?? selectedEntity?.name)
    : undefined;
  const canCaptureSecondSpot = Boolean(session && spots.length >= 1);
  const canCommit = Boolean(
    session &&
      spots.length >= 2 &&
      (!teachFanLevels || levels.length >= 1) &&
      verbs.some((verb) => verb.enabled),
  );

  async function startSession() {
    if (!selectedEntity || !cameraId) return;
    const started = await startTeach
      .mutateAsync({
        camera_id: cameraId,
        target: { entity_id: selectedEntity.entity_id },
        anchor_id: replacesAnchorId,
      })
      .catch(() => null);
    if (!started) return;
    const existingVerbs = replacesAnchorId
      ? verbDraftsFromMappings(mappings.data ?? [], replacesAnchorId)
      : [];
    setSession({ id: started.id, prompt: started.prompt });
    setSpots([]);
    setLevels([]);
    setVerbs(existingVerbs.length ? existingVerbs : defaultVerbDrafts(selectedEntity.domain));
    setCommitResult(null);
    setStage("spots");
  }

  async function captureSpot() {
    const spot = await teachSpot.mutateAsync().catch(() => null);
    if (!spot) return;
    setSpots((items) => [...items, spot]);
    if (spot.spot_index >= 2) setStage(teachFanLevels ? "levels" : "verbs");
  }

  async function useCurrentSpeed() {
    const response = await teachLevel.mutateAsync({ level: levelToCapture }).catch(() => null);
    if (!response) return;
    setLevels(response.levels);
    setLevelToCapture(response.levels.length + 1);
  }

  async function commit() {
    if (!selectedEntity) return;
    const result = await commitTeach
      .mutateAsync({
        name: reteachName ?? selectedEntity.name,
        verbs: verbs
          .filter((verb) => verb.enabled)
          .map<TeachVerb>((verb) => ({ gesture_id: verb.gestureId, action: verb.action })),
      })
      .catch(() => null);
    if (!result) return;
    setCommitResult(result);
    setStage("done");
  }

  async function cancel() {
    if (session) await cancelTeach.mutateAsync().catch(() => undefined);
    setSession(null);
    setSpots([]);
    setLevels([]);
    setStage("pick");
    setCommitResult(null);
  }

  function updateVerb(index: number, patch: Partial<VerbDraft>) {
    setVerbs((items) => items.map((item, itemIndex) => (itemIndex === index ? { ...item, ...patch } : item)));
  }

  return (
    <section className="feature-screen teach-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">{reteachName ? `Re-teach ${reteachName}` : "Teach a device"}</h1>
          <p>
            {reteachName
              ? "Capture two new pointing spots. Flick replaces the old position and keeps its gestures."
              : "Pick the Home Assistant target, capture two pointing spots, teach fine fan levels and test the verbs."}
          </p>
        </div>
        {stage === "pick" ? (
          <Button
            variant="primary"
            loading={startTeach.isPending}
            disabled={!selectedEntity || !cameraId}
            onClick={startSession}
          >
            Start capture
          </Button>
        ) : (
          <Button variant="ghost" loading={cancelTeach.isPending} onClick={cancel}>
            Cancel
          </Button>
        )}
      </header>

      <div className="teach-layout">
        <div className="teach-main">
          {stage === "pick" ? (
            <GlassPanel className="teach-step-card">
              <Badge tone="accent">1 · Pick device</Badge>
              <h2>Choose the Home Assistant device in this camera view.</h2>
              <div className="teach-picker-grid">
                <label className="sentence-field">
                  <span>Search entity</span>
                  <input
                    value={query}
                    onChange={(event) => setQuery(event.currentTarget.value)}
                    placeholder="fan, lamp, media player…"
                  />
                </label>
                <div className="sentence-field">
                  <span>Camera</span>
                  {cameraOptions.length ? (
                    <Select
                      value={cameraId}
                      onValueChange={setCameraId}
                      label="Camera"
                      items={cameraOptions.map(({ value, label }) => ({ value, label }))}
                    />
                  ) : (
                    <p className="field-help">
                      No camera yet. <Link to="/cameras">Add one in Cameras</Link>.
                    </p>
                  )}
                </div>
              </div>
              <div className="teach-entity-list">
                {visibleEntities.map((entity) => (
                  <button
                    key={entity.entity_id}
                    type="button"
                    data-active={entity.entity_id === selectedEntity?.entity_id}
                    onClick={() => setSelectedEntityId(entity.entity_id)}
                  >
                    <DevicePill name={entity.name} domain={entity.domain} detail={entityDetail(entity)} />
                    {isSensitiveEntity(entity) ? <Badge tone="warning">Sensitive</Badge> : null}
                  </button>
                ))}
              </div>
              {startTeach.error ? (
                <p className="inline-error" role="alert">
                  {startTeach.error.message}
                </p>
              ) : null}
            </GlassPanel>
          ) : (
            <TeachPreview
              sessionId={session?.id}
              cameraId={cameraId}
              title={`Point at ${selectedEntity?.name ?? "the device"} and hold still`}
              detail={spots.length < 1 ? "spot 1 · hold steady" : "move ~25° · spot 2"}
            />
          )}

          {stage === "spots" ? (
            <GlassPanel className="teach-step-card">
              <Badge tone="accent">{spots.length < 1 ? "2 · Spot 1" : "3 · Spot 2"}</Badge>
              <h2>
                {spots.length < 1
                  ? "Point from your first spot."
                  : "Take one or two steps to the side and point again."}
              </h2>
              <p>
                Capture when the ray is steady. Two spots let Flick triangulate the target; a third spot is
                optional when confidence is low.
              </p>
              <div className="spot-result-grid">
                {spots.map((spot) => (
                  <div className="spot-result" key={spot.spot_index}>
                    <Badge tone={spot.confidence >= 0.85 ? "success" : "warning"}>
                      Spot {spot.spot_index}
                    </Badge>
                    <ConfidenceMeter
                      value={spot.confidence}
                      label={`${spot.kind} · jitter ${spot.ray_jitter_deg.toFixed(1)}°`}
                    />
                    <span>
                      {spot.residual_deg == null
                        ? "Residual pending"
                        : `${spot.residual_deg.toFixed(1)}° residual`}
                    </span>
                  </div>
                ))}
              </div>
              <div className="level-actions">
                <Button variant="primary" loading={teachSpot.isPending} onClick={captureSpot}>
                  {spots.length < 1 ? "Capture spot 1" : "Capture spot 2"}
                </Button>
                <Button
                  variant="ghost"
                  disabled={!canCaptureSecondSpot}
                  loading={teachSpot.isPending}
                  onClick={captureSpot}
                >
                  Add optional spot
                </Button>
              </div>
              {teachSpot.error ? (
                <p className="inline-error" role="alert">
                  {teachSpot.error.message}
                </p>
              ) : null}
            </GlassPanel>
          ) : null}

          {stage === "levels" ? (
            <GlassPanel className="teach-step-card">
              <Badge tone="warning">4 · Fan levels</Badge>
              <h2>Set speed {levelToCapture} in Home Assistant, then use the current speed.</h2>
              <p>
                {selectedEntity?.name ?? "This fan"} reports percentage step{" "}
                <strong>{numberAttribute(selectedEntity, "percentage_step") ?? 1}</strong>. Level 1 should be{" "}
                <strong>{levels[0] ?? numberAttribute(selectedEntity, "percentage") ?? 1}%</strong> for the
                owner fan.
              </p>
              <div className="level-actions">
                <Button variant="primary" loading={teachLevel.isPending} onClick={useCurrentSpeed}>
                  Use current speed
                </Button>
                <Button
                  variant="secondary"
                  disabled={!levels.length}
                  loading={testLevel.isPending}
                  onClick={() => testLevel.mutate({ level: 1 })}
                >
                  Try level 1
                </Button>
                <Button variant="ghost" disabled={!levels.length} onClick={() => setStage("verbs")}>
                  Continue to verbs
                </Button>
              </div>
              {testLevel.data?.message ? <p className="inline-result">{testLevel.data.message}</p> : null}
              {teachLevel.error ? (
                <p className="inline-error" role="alert">
                  {teachLevel.error.message}
                </p>
              ) : null}
            </GlassPanel>
          ) : null}

          {stage === "verbs" ? (
            <GlassPanel className="teach-step-card">
              <Badge tone="accent">5 · Verbs</Badge>
              <h2>Choose the default point-and-gesture sentences.</h2>
              <div className="teach-verb-editor">
                {verbs.map((verb, index) => (
                  <article className="teach-verb-row" key={verb.id}>
                    <Switch
                      checked={verb.enabled}
                      onCheckedChange={(enabled) => updateVerb(index, { enabled })}
                      aria-label={`Enable ${verb.label}`}
                    />
                    <GestureGlyph name={verb.gestureId} animated={false} />
                    <Select
                      value={verb.gestureId}
                      onValueChange={(gestureId) => updateVerb(index, { gestureId })}
                      label="Gesture"
                      items={gestureChoices}
                    />
                    <Select
                      value={
                        verb.action.kind === "verb" && verb.action.verb === "level_set"
                          ? "level_set:1"
                          : verb.action.kind === "verb"
                            ? verb.action.verb
                            : "on"
                      }
                      onValueChange={(value) => {
                        const option = actionChoices(domain, levels).find((item) => item.value === value);
                        if (option)
                          updateVerb(index, {
                            action: option.action,
                            label: `Point + ${value} → ${option.label}`,
                          });
                      }}
                      label="Action"
                      items={actionChoices(domain, levels).map((item) => ({
                        value: item.value,
                        label: item.label,
                      }))}
                    />
                    <strong>{levelLabel(verb.action)}</strong>
                  </article>
                ))}
              </div>
              {sensitive ? (
                <Badge tone="warning">
                  Sensitive targets require Safety enabled and a confirmation gesture before dispatch.
                </Badge>
              ) : null}
              <div className="level-actions">
                <Button
                  variant="primary"
                  disabled={!canCommit}
                  loading={commitTeach.isPending}
                  onClick={commit}
                >
                  Commit taught device
                </Button>
                <Button variant="ghost" onClick={() => setStage(teachFanLevels ? "levels" : "spots")}>
                  Back
                </Button>
              </div>
              {commitTeach.error ? (
                <p className="inline-error" role="alert">
                  {commitTeach.error.message}
                </p>
              ) : null}
            </GlassPanel>
          ) : null}

          {stage === "done" && commitResult ? (
            <GlassPanel className="teach-step-card success-card">
              <Badge tone="success">Done</Badge>
              <h2>{commitResult.anchor.name} is ready.</h2>
              <p>Try these from the room. They are also listed under Devices.</p>
              <div className="sentence-stack">
                {verbs
                  .filter((verb) => verb.enabled)
                  .map((verb) => (
                    <p key={verb.id}>
                      Point at {commitResult.anchor.name} +{" "}
                      <GestureGlyph name={verb.gestureId} animated={false} /> → {levelLabel(verb.action)}
                    </p>
                  ))}
              </div>
              <Link className="ui-button ui-button-primary ui-button-md" to="/devices">
                View devices
              </Link>
            </GlassPanel>
          ) : null}
        </div>
        <aside className="teach-side">
          <GlassPanel className="picked-device-card">
            <span>Picked device</span>
            {selectedEntity ? (
              <DevicePill
                name={selectedEntity.name}
                domain={selectedEntity.domain}
                detail={entityDetail(selectedEntity)}
              />
            ) : (
              <p>No matching device found in Home Assistant.</p>
            )}
            {session ? <Badge tone="accent">Session {session.id.slice(0, 6)}</Badge> : null}
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <span>Quality</span>
            <ConfidenceMeter
              value={spots.at(-1)?.confidence ?? 0.0}
              label={spots.length ? "Target confidence" : "Waiting for spot"}
            />
            <p>
              {spots.length >= 2
                ? "Great — Flick has enough geometry. Add another spot only if confidence is low."
                : "Move about 25° between spot 1 and spot 2 for stronger triangulation."}
            </p>
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <span>Default fan set</span>
            <div className="verb-list">
              <VerbChip gestureId="builtin.circle_any" label="Circle → Speed 1" />
              <VerbChip gestureId="builtin.two_hand_separate" label="Two hands apart → Off" />
              <VerbChip gestureId="builtin.thumb_up" label="Thumbs up → On" />
            </div>
            <p>
              Global gestures stay suppressed while this device is selected, so actions do not double-fire.
            </p>
          </GlassPanel>
        </aside>
      </div>
    </section>
  );
}

export function PlacesRoute() {
  const places = usePlaces();
  const anchors = useAnchors();
  const cameraName = useCameraNames();
  return (
    <section className="feature-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Places</h1>
          <p>Places keep taught anchors tied to a stable camera view without storing camera images.</p>
        </div>
      </header>
      <div className="devices-place-grid">
        {(places.data ?? []).map((place) => (
          <GlassPanel className="place-card" key={place.id}>
            <div className="panel-heading">
              <div>
                <span>{cameraName(place.camera_id)}</span>
                <strong>{place.name}</strong>
              </div>
              <Badge tone={statusTone(place.status)}>{place.status}</Badge>
            </div>
            <RoomSketch anchors={(anchors.data ?? []).filter((anchor) => anchor.place_id === place.id)} />
            <ListRow
              title="Scene signature"
              description="Embedding only · no image stored"
              trailing="similarity 0.94"
            />
          </GlassPanel>
        ))}
      </div>
    </section>
  );
}

export function RealignRoute() {
  const startRealign = useStartRealign("place-bedroom-desk");
  return (
    <section className="feature-screen teach-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Re-align devices</h1>
          <p>Your camera moved, so pointing pauses until two known devices confirm the new pose.</p>
        </div>
        <Button variant="primary" loading={startRealign.isPending} onClick={() => startRealign.mutate()}>
          Start re-align
        </Button>
      </header>
      <div className="teach-layout">
        <TeachPreview title="Point at Ventilador dormitorio" detail="prompt 1 of 2" />
        <GlassPanel className="teach-side teach-step-card">
          <Badge tone="warning">Camera moved</Badge>
          <h2>Point at two devices you taught</h2>
          <p>Flick solves the new camera rotation and applies it to every anchor in Bedroom desk.</p>
          <ConfidenceMeter value={0.84} label="Residual quality" />
          <p>Residual 3.1° · no devices need re-teach.</p>
        </GlassPanel>
      </div>
    </section>
  );
}
