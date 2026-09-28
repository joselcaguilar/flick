import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  useAnchors,
  useCancelTeach,
  useCommitTeach,
  useHaEntities,
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
import { Badge, Button, GlassPanel, ListRow, Select, Skeleton, Switch } from "../../components/ui";
import { useEventStore } from "../../events/store";
import { formatTime } from "../../lib/utils";

function anchorTargetEntity(anchor: Anchor) {
  const target = anchor.target as unknown as { entity_id?: string; device_id?: string; area_id?: string };
  return target.entity_id ?? target.device_id ?? target.area_id ?? "Unknown target";
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
      {anchors.map((anchor, index) => (
        <i key={anchor.id} style={{ rotate: `${index * 32 - 28}deg` }}>
          <b>{anchor.name}</b>
        </i>
      ))}
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

function TeachPreview({ title, detail, sessionId }: { title: string; detail: string; sessionId?: string }) {
  const hands = useEventStore((state) => state.hands["camera-main"]);
  const teach = useEventStore((state) => state.teach);
  const progress = teach?.session_id === sessionId ? teach : undefined;
  const confidence = progress?.confidence ?? 0.82;
  return (
    <GlassPanel className="teach-preview-panel">
      <div className="preview-header">
        <div>
          <span>Live ray</span>
          <strong>{title}</strong>
        </div>
        <Badge tone={progress?.phase === "error" ? "danger" : "success"}>{progress?.hint ?? detail}</Badge>
      </div>
      <PreviewCanvas alt="Live camera preview with pointing ray" hands={hands?.hands} ray={hands?.ray} />
      <div className="teach-ray-caption">
        <ConfidenceMeter
          value={confidence}
          label={
            progress?.ray_jitter_deg != null ? `Jitter ${progress.ray_jitter_deg.toFixed(1)}°` : "Ray steady"
          }
        />
        <span>{progress?.phase ?? "Median ray only"} · no camera image saved</span>
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

export function DevicesRoute() {
  const places = usePlaces();
  const anchors = useAnchors();
  const grouped = (places.data ?? []).map((place) => ({
    place,
    anchors: (anchors.data ?? []).filter((anchor) => anchor.place_id === place.id),
  }));

  return (
    <section className="feature-screen devices-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <p className="route-path">Devices</p>
          <h1 id="screen-title">Taught devices</h1>
          <p>Point at real objects once, then use reusable gestures like circle or two-hand stop.</p>
        </div>
        <Link className="ui-button ui-button-primary ui-button-md" to="/devices/teach">
          Teach a device
        </Link>
      </header>

      {anchors.isLoading ? <Skeleton /> : null}
      {!anchors.isLoading && !anchors.data?.length ? (
        <GlassPanel className="empty-state-panel">
          <EntityIcon domain="fan" />
          <h2>No devices taught</h2>
          <p>Point at something in the room — Flick will remember it.</p>
        </GlassPanel>
      ) : null}

      <div className="devices-place-grid">
        {grouped.map(({ place, anchors: placeAnchors }) => (
          <GlassPanel className="place-card" key={place.id}>
            <div className="panel-heading">
              <div>
                <span>{place.camera_id}</span>
                <strong>{place.name}</strong>
              </div>
              <Badge tone={statusTone(place.status)}>{place.status}</Badge>
            </div>
            <RoomSketch anchors={placeAnchors} />
            <div className="device-card-list">
              {placeAnchors.map((anchor) => (
                <article className="device-card" key={anchor.id}>
                  <DevicePill name={anchor.name} domain={anchor.domain} detail={anchorTargetEntity(anchor)} />
                  <div className="device-card-meta">
                    <Badge tone={statusTone(anchor.status)}>{anchor.status}</Badge>
                    <span>Last used {formatTime(anchor.last_used_at)}</span>
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
                  <div className="device-card-actions">
                    <Link className="ui-button ui-button-secondary ui-button-sm" to="/devices/teach">
                      Re-teach
                    </Link>
                    <Button variant="ghost" size="sm">
                      Delete
                    </Button>
                  </div>
                </article>
              ))}
            </div>
          </GlassPanel>
        ))}
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
  const entities = useHaEntities("");
  const status = useStatus();
  const startTeach = useStartTeach();
  const ownerFan = pickOwnerFan(entities.data ?? []);
  const [stage, setStage] = useState<TeachStage>("pick");
  const [query, setQuery] = useState("fan");
  const [selectedEntityId, setSelectedEntityId] = useState<string>("");
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
  const cameras = status.data?.cameras ?? [];

  useEffect(() => {
    if (!selectedEntityId && ownerFan) setSelectedEntityId(ownerFan.entity_id);
  }, [ownerFan, selectedEntityId]);

  useEffect(() => {
    const firstCamera = cameras[0]?.camera_id ?? cameras[0]?.id ?? "dev-camera";
    if (!cameraId) setCameraId(firstCamera);
  }, [cameraId, cameras]);

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
  const canCaptureSecondSpot = Boolean(session && spots.length >= 1);
  const canCommit = Boolean(
    session &&
      spots.length >= 2 &&
      (!teachFanLevels || levels.length >= 1) &&
      verbs.some((verb) => verb.enabled),
  );

  async function startSession() {
    if (!selectedEntity) return;
    const started = await startTeach.mutateAsync({
      camera_id: cameraId || "dev-camera",
      target: { entity_id: selectedEntity.entity_id },
    });
    setSession({ id: started.id, prompt: started.prompt });
    setSpots([]);
    setLevels([]);
    setVerbs(defaultVerbDrafts(selectedEntity.domain));
    setCommitResult(null);
    setStage("spots");
  }

  async function captureSpot() {
    const spot = await teachSpot.mutateAsync();
    setSpots((items) => [...items, spot]);
    if (spot.spot_index >= 2) setStage(teachFanLevels ? "levels" : "verbs");
  }

  async function useCurrentSpeed() {
    const response = await teachLevel.mutateAsync({ level: levelToCapture });
    setLevels(response.levels);
    setVerbs(defaultVerbDrafts(domain));
    setLevelToCapture(response.levels.length + 1);
  }

  async function commit() {
    if (!selectedEntity) return;
    const result = await commitTeach.mutateAsync({
      name: selectedEntity.name,
      verbs: verbs
        .filter((verb) => verb.enabled)
        .map<TeachVerb>((verb) => ({ gesture_id: verb.gestureId, action: verb.action })),
    });
    setCommitResult(result);
    setStage("done");
  }

  async function cancel() {
    if (session) await cancelTeach.mutateAsync();
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
          <p className="route-path">Devices / Teach</p>
          <h1 id="screen-title">Teach a device</h1>
          <p>
            Pick the Home Assistant target, capture two pointing spots, teach fine fan levels and test the
            verbs.
          </p>
        </div>
        {stage === "pick" ? (
          <Button
            variant="primary"
            loading={startTeach.isPending}
            disabled={!selectedEntity}
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
                  <Select
                    value={cameraId}
                    onValueChange={setCameraId}
                    label="Camera"
                    items={(cameras.length ? cameras : [{ camera_id: "dev-camera", state: "idle" }]).map(
                      (camera) => ({
                        value: camera.camera_id,
                        label: `${camera.camera_id} · ${camera.state}`,
                      }),
                    )}
                  />
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
            </GlassPanel>
          ) : (
            <TeachPreview
              sessionId={session?.id}
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
            </GlassPanel>
          ) : null}

          {stage === "done" && commitResult ? (
            <GlassPanel className="teach-step-card success-card">
              <Badge tone="success">Done</Badge>
              <h2>{commitResult.anchor.name} is ready.</h2>
              <p>Try these from the room. They are also listed under Mappings → Devices.</p>
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
              <Link className="ui-button ui-button-primary ui-button-md" to="/mappings">
                View mappings
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
  return (
    <section className="feature-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <p className="route-path">Devices / Places</p>
          <h1 id="screen-title">Places</h1>
          <p>Places keep taught anchors tied to a stable camera view without storing camera images.</p>
        </div>
      </header>
      <div className="devices-place-grid">
        {(places.data ?? []).map((place) => (
          <GlassPanel className="place-card" key={place.id}>
            <div className="panel-heading">
              <div>
                <span>{place.camera_id}</span>
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
          <p className="route-path">Devices / Re-align</p>
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
