import { Link } from "react-router-dom";
import {
  useAnchors,
  useHaEntities,
  usePlaces,
  useStartRealign,
  useStartTeach,
  useTestMapping,
} from "../../api/hooks";
import type { Anchor, HaEntity } from "../../api/types";
import { ConfidenceMeter, DevicePill, EntityIcon, PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, ListRow, Skeleton } from "../../components/ui";
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

function TeachPreview({ title, detail }: { title: string; detail: string }) {
  const hands = useEventStore((state) => state.hands["camera-main"]);
  return (
    <GlassPanel className="teach-preview-panel">
      <div className="preview-header">
        <div>
          <span>Live ray</span>
          <strong>{title}</strong>
        </div>
        <Badge tone="success">{detail}</Badge>
      </div>
      <PreviewCanvas alt="Live camera preview with pointing ray" hands={hands?.hands} ray={hands?.ray} />
      <div className="teach-ray-caption">
        <ConfidenceMeter value={0.92} label="Ray steady" />
        <span>Median ray only · no camera image saved</span>
      </div>
    </GlassPanel>
  );
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
                      <span key={`${anchor.id}-${verb.gesture_id}`}>{verb.label}</span>
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
      <Link className="ui-button ui-button-primary ui-button-md" to="/devices/teach">
        Teach a device
      </Link>
      <Link className="ui-button ui-button-ghost ui-button-md" to="/">
        Later
      </Link>
    </GlassPanel>
  );
}

export function TeachDeviceRoute() {
  const entities = useHaEntities("?domain=fan");
  const startTeach = useStartTeach();
  const ownerFan = pickOwnerFan(entities.data ?? []);
  const testSpeed = useTestMapping("map-fan-speed-1");
  const testOff = useTestMapping("map-fan-off");

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
        <Button
          variant="primary"
          loading={startTeach.isPending}
          onClick={() =>
            ownerFan &&
            startTeach.mutate({ camera_id: "camera-main", target: { entity_id: ownerFan.entity_id } })
          }
        >
          Start capture
        </Button>
      </header>

      <div className="teach-layout">
        <div className="teach-main">
          <TeachPreview title="Point at Ventilador dormitorio and hold still" detail="spot 1 · 1 s" />
          <GlassPanel className="teach-step-card">
            <Badge tone="accent">Spot 2</Badge>
            <h2>Take one or two steps to the side and point again.</h2>
            <p>The confidence meter combines triangulation angle, residual and ray jitter.</p>
            <ConfidenceMeter value={0.91} label="Target confidence" />
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <Badge tone="warning">Fine-step fan</Badge>
            <h2>Use current speed for level 1</h2>
            <p>
              Ventilador dormitorio reports <strong>on · 1%</strong>. Flick stores that as speed 1.
            </p>
            <div className="level-actions">
              <Button variant="secondary">Use current speed</Button>
              <Button variant="ghost">Try level 1</Button>
            </div>
          </GlassPanel>
        </div>
        <aside className="teach-side">
          <GlassPanel className="picked-device-card">
            <span>Picked device</span>
            {ownerFan ? (
              <DevicePill
                name={ownerFan.name}
                domain={ownerFan.domain}
                detail={`${ownerFan.entity_id} · Tuya`}
              />
            ) : (
              <p>No matching fan found in Home Assistant.</p>
            )}
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <span>Default verbs</span>
            <div className="sentence-stack">
              <p>Point + ↻ → speed 1</p>
              <p>Point + ✋✋ apart → off</p>
              <p>Point + 👍 → on</p>
            </div>
          </GlassPanel>
          <GlassPanel className="teach-step-card">
            <span>Test</span>
            <p>Point at it again, then try a verb. The HUD shows angular error and the action result.</p>
            <div className="level-actions">
              <Button variant="primary" loading={testSpeed.isPending} onClick={() => testSpeed.mutate()}>
                Try ↻ speed 1
              </Button>
              <Button variant="ghost" loading={testOff.isPending} onClick={() => testOff.mutate()}>
                Try ✋✋ off
              </Button>
            </div>
            {testSpeed.data?.message ? <Badge tone="success">{testSpeed.data.message}</Badge> : null}
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
