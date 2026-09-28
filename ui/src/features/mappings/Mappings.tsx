import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  useAnchors,
  useGestures,
  useMappings,
  usePatchSettings,
  useSettings,
  useTestMapping,
} from "../../api/hooks";
import type { Action, Gesture, Mapping } from "../../api/types";
import { DevicePill, GestureGlyph } from "../../components/domain";
import {
  Badge,
  Button,
  GlassPanel,
  ListRow,
  SegmentedControl,
  SliderDial,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "../../components/ui";
import { cn } from "../../lib/utils";
import {
  draftToSentence,
  type HandChoice,
  type MappingKind,
  type SentenceBuilderDraft,
  validateSentenceDraft,
} from "./sentence-builder";

const targetedVerbActions: Array<{ label: string; action: Action; description: string }> = [
  {
    label: "Speed 1 · 1%",
    action: { kind: "verb", verb: "level_set", level: 1 },
    description: "Fan level taught from HA state",
  },
  { label: "Off", action: { kind: "verb", verb: "off" }, description: "fan.turn_off for selected fan" },
  { label: "Toggle", action: { kind: "verb", verb: "toggle" }, description: "Domain default toggle" },
  {
    label: "Dial percentage",
    action: { kind: "dial", entity_id: "$selected", property: "percentage", gain: 1 },
    description: "Pinch dial on selected device",
  },
];

const globalActions: Array<{ label: string; action: Action; description: string }> = [
  {
    label: "Toggle living room lights",
    action: {
      kind: "call_service",
      domain: "light",
      service: "toggle",
      target: { entity_id: ["light.living_room_lights"] },
      data: {},
      preset: "light.toggle",
    },
    description: "Global gesture works anywhere unless a device is selected",
  },
  {
    label: "Set media volume",
    action: { kind: "dial", entity_id: "media_player.living_room_tv", property: "volume_level", gain: 0.05 },
    description: "Pinch dial changes volume in 10% ticks",
  },
];

function mappingTone(mapping: Mapping) {
  if (!mapping.enabled) return "neutral";
  if (mapping.sensitive) return "warning";
  return "success";
}

function mappingSentence(mapping: Mapping) {
  if (mapping.target_mode === "global") {
    return `${mapping.gesture_name ?? mapping.gesture_id} (${mapping.hand} hand) → ${mapping.name.split("→").at(-1)?.trim() ?? "Action"}`;
  }
  return `Point at ${mapping.target_label ?? "device"} + ${mapping.gesture_name ?? mapping.gesture_id} → ${mapping.name.split("→").at(-1)?.trim() ?? "Action"}`;
}

function mappingDescription(mapping: Mapping) {
  const camera = mapping.camera_ids.length ? mapping.camera_ids.join(", ") : "all cameras";
  return `${mapping.mode} · ${camera}${mapping.sensitive ? " · sensitive" : ""}`;
}

function MappingRow({ mapping }: { mapping: Mapping }) {
  const test = useTestMapping(mapping.id);
  return (
    <article className="mapping-row">
      <div className="mapping-row-main">
        <GestureGlyph name={mapping.gesture_id} animated={false} />
        <div>
          <strong>{mappingSentence(mapping)}</strong>
          <span>{mappingDescription(mapping)}</span>
        </div>
      </div>
      <div className="mapping-row-meta">
        <Badge tone={mappingTone(mapping)}>{mapping.enabled ? "enabled" : "off"}</Badge>
        <Badge tone="accent">{mapping.mode}</Badge>
        {mapping.sensitive ? <Badge tone="warning">Sensitive</Badge> : null}
        <Button variant="ghost" size="sm" loading={test.isPending} onClick={() => test.mutate()}>
          Test
        </Button>
      </div>
      {test.data?.message ? <p className="inline-result">{test.data.message}</p> : null}
    </article>
  );
}

function groupTargetedMappings(mappings: Mapping[]) {
  const groups = new Map<string, Mapping[]>();
  for (const mapping of mappings.filter((item) => item.target_mode !== "global")) {
    const key = mapping.target_label ?? mapping.anchor_id ?? "Device";
    groups.set(key, [...(groups.get(key) ?? []), mapping]);
  }
  return [...groups.entries()];
}

function GesturePicker({
  gestures,
  value,
  onChange,
}: {
  gestures: Gesture[];
  value?: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="sentence-field">
      <span>Gesture</span>
      <div className="choice-grid">
        {gestures.slice(0, 5).map((gesture) => (
          <button
            key={gesture.id}
            type="button"
            data-active={gesture.id === value}
            onClick={() => onChange(gesture.id)}
          >
            <GestureGlyph name={gesture.id} animated={false} />
            <strong>{gesture.name}</strong>
          </button>
        ))}
      </div>
    </div>
  );
}

export function MappingsRoute() {
  const mappings = useMappings();
  const targetedGroups = groupTargetedMappings(mappings.data ?? []);
  const globalMappings = (mappings.data ?? []).filter((mapping) => mapping.target_mode === "global");
  return (
    <section className="feature-screen mappings-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <p className="route-path">Mappings</p>
          <h1 id="screen-title">Gesture sentences</h1>
          <p>Targeted device verbs and global shortcuts stay separate, so pointing always wins.</p>
        </div>
        <Link className="ui-button ui-button-primary ui-button-md" to="/mappings/new">
          New mapping
        </Link>
      </header>

      <Tabs defaultValue="devices" className="mapping-tabs">
        <TabsList aria-label="Mapping tabs">
          <TabsTrigger value="devices">Devices</TabsTrigger>
          <TabsTrigger value="global">Global</TabsTrigger>
        </TabsList>
        <TabsContent value="devices" className="mapping-tab-content">
          {targetedGroups.map(([target, group]) => (
            <GlassPanel className="mapping-group" key={target}>
              <div className="panel-heading">
                <DevicePill name={target} domain={group[0]?.target_domain ?? "fan"} detail="Targeted verbs" />
                <Badge tone="accent">{group.length} verbs</Badge>
              </div>
              <div className="mapping-list">
                {group.map((mapping) => (
                  <MappingRow key={mapping.id} mapping={mapping} />
                ))}
              </div>
            </GlassPanel>
          ))}
        </TabsContent>
        <TabsContent value="global" className="mapping-tab-content">
          <GlassPanel className="mapping-group">
            <div className="panel-heading">
              <div>
                <span>Anywhere gestures</span>
                <strong>Global</strong>
              </div>
              <Badge tone="warning">Suppressed while a device is selected</Badge>
            </div>
            <div className="mapping-list">
              {globalMappings.map((mapping) => (
                <MappingRow key={mapping.id} mapping={mapping} />
              ))}
            </div>
          </GlassPanel>
        </TabsContent>
      </Tabs>
    </section>
  );
}

export function MappingEditorRoute() {
  const gestures = useGestures();
  const anchors = useAnchors();
  const settings = useSettings();
  const patchSettings = usePatchSettings();
  const safetyEnabled = settings.data?.["safety.allow_sensitive"] === true;
  const fanAnchor = anchors.data?.find((anchor) => anchor.id === "anchor-fan-bedroom") ?? anchors.data?.[0];
  const [kind, setKind] = useState<MappingKind>("targeted");
  const [gestureId, setGestureId] = useState("builtin.circle_cw");
  const [hand, setHand] = useState<HandChoice>("any");
  const [sensitive, setSensitive] = useState(false);
  const [sensitiveAck, setSensitiveAck] = useState(false);
  const [confirmGestureId, setConfirmGestureId] = useState("builtin.thumb_up");
  const [targetedActionIndex, setTargetedActionIndex] = useState(0);
  const [globalActionIndex, setGlobalActionIndex] = useState(0);
  const [dialValue, setDialValue] = useState(1);

  const activeAction =
    kind === "targeted"
      ? targetedVerbActions[targetedActionIndex]?.action
      : globalActions[globalActionIndex]?.action;
  const draft = useMemo<SentenceBuilderDraft>(
    () => ({
      kind,
      gestureId,
      hand,
      cameras: kind === "targeted" ? ["camera-main"] : [],
      target:
        kind === "targeted"
          ? {
              type: "anchor" as const,
              anchorId: fanAnchor?.id ?? "anchor-fan-bedroom",
              domain: fanAnchor?.domain ?? "fan",
              label: fanAnchor?.name ?? "Ventilador dormitorio",
            }
          : {
              type: "entity" as const,
              entityId: "light.living_room_lights",
              domain: "light",
              label: "Living room lights",
            },
      action: activeAction,
      mode: activeAction?.kind === "dial" ? "dial" : "tap",
      cooldownMs: 600,
      requireArmed: sensitive,
      sensitive,
      sensitiveAck,
      confirmGestureId: sensitive ? confirmGestureId : undefined,
    }),
    [activeAction, confirmGestureId, fanAnchor, gestureId, hand, kind, sensitive, sensitiveAck],
  );
  const issues = validateSentenceDraft(draft);
  const sensitiveBlocked = sensitive && (!safetyEnabled || !sensitiveAck || !confirmGestureId);
  const canEnable = issues.length === 0 && !sensitiveBlocked;

  function setKindSafely(next: MappingKind) {
    setKind(next);
    if (next === "global" && activeAction?.kind === "verb") setGlobalActionIndex(0);
  }

  return (
    <section className="feature-screen mapping-editor-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <p className="route-path">Mappings / Editor</p>
          <h1 id="screen-title">Build a sentence</h1>
          <p>Choose when Flick listens, what it points at, and the Home Assistant action it sends.</p>
        </div>
        <Button variant="primary" disabled={!canEnable}>
          {canEnable ? "Enable mapping" : "Resolve checks"}
        </Button>
      </header>

      <div className="mapping-editor-grid">
        <GlassPanel className="sentence-builder-card">
          <SegmentedControl
            label="Mapping type"
            value={kind}
            onChange={setKindSafely}
            segments={[
              { label: "Devices", value: "targeted" },
              { label: "Global", value: "global" },
            ]}
          />
          <div className="sentence-preview" aria-live="polite">
            {draftToSentence(draft)}
          </div>
          <GesturePicker gestures={gestures.data ?? []} value={gestureId} onChange={setGestureId} />
          <SegmentedControl
            label="Hand"
            value={hand}
            onChange={setHand}
            segments={[
              { label: "Any", value: "any" },
              { label: "Left", value: "left" },
              { label: "Right", value: "right" },
            ]}
          />

          {kind === "targeted" ? (
            <div className="sentence-field">
              <span>Target + verb</span>
              <DevicePill
                name={fanAnchor?.name ?? "Ventilador dormitorio"}
                domain={fanAnchor?.domain ?? "fan"}
                detail="Taught device · Bedroom desk"
              />
              <div className="choice-grid compact">
                {targetedVerbActions.map((preset, index) => (
                  <button
                    key={preset.label}
                    type="button"
                    data-active={index === targetedActionIndex}
                    onClick={() => setTargetedActionIndex(index)}
                  >
                    <strong>{preset.label}</strong>
                    <span>{preset.description}</span>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="sentence-field">
              <span>Global action</span>
              <p className="field-help">
                Device verbs are intentionally unavailable here; pick a service or dial action.
              </p>
              <div className="choice-grid compact">
                {globalActions.map((preset, index) => (
                  <button
                    key={preset.label}
                    type="button"
                    data-active={index === globalActionIndex}
                    onClick={() => setGlobalActionIndex(index)}
                  >
                    <strong>{preset.label}</strong>
                    <span>{preset.description}</span>
                  </button>
                ))}
              </div>
            </div>
          )}

          {activeAction?.kind === "dial" ? (
            <div className="dial-preview-card">
              <SliderDial
                label="Dial preview"
                value={dialValue}
                min={0}
                max={100}
                step={1}
                onValueChange={setDialValue}
              />
              <Badge tone="accent">{dialValue}%</Badge>
            </div>
          ) : null}
        </GlassPanel>

        <aside className="mapping-safety-panel">
          <GlassPanel className={cn("sensitive-card", sensitiveBlocked && "needs-attention")}>
            <div className="panel-heading">
              <div>
                <span>Safety</span>
                <strong>Sensitive flow</strong>
              </div>
              <Switch
                checked={sensitive}
                onCheckedChange={setSensitive}
                aria-label="Mark mapping as sensitive"
              />
            </div>
            <p>
              Locks, covers and high-impact actions require Settings safety, an acknowledgement and a
              confirmation gesture.
            </p>
            {!safetyEnabled ? (
              <Button
                variant="secondary"
                loading={patchSettings.isPending}
                onClick={() => patchSettings.mutate({ "safety.allow_sensitive": true })}
              >
                Enable Safety setting
              </Button>
            ) : (
              <Badge tone="success">Safety enabled</Badge>
            )}
            <label className="ack-row">
              <input
                type="checkbox"
                checked={sensitiveAck}
                onChange={(event) => setSensitiveAck(event.currentTarget.checked)}
              />
              <span>I understand this sends a sensitive Home Assistant action.</span>
            </label>
            <div className="sentence-field compact-field">
              <span>Confirm gesture</span>
              <div className="choice-grid compact">
                {(gestures.data ?? [])
                  .filter((gesture) => gesture.kind === "static")
                  .slice(0, 3)
                  .map((gesture) => (
                    <button
                      key={gesture.id}
                      type="button"
                      data-active={gesture.id === confirmGestureId}
                      onClick={() => setConfirmGestureId(gesture.id)}
                    >
                      <strong>{gesture.name}</strong>
                    </button>
                  ))}
              </div>
            </div>
          </GlassPanel>

          <GlassPanel className="mapping-checks-card">
            <span>Checks</span>
            {issues.length ? (
              <div className="mapping-issues">
                {issues.map((issue) => (
                  <Badge key={`${issue.field}-${issue.code}`} tone="danger">
                    {issue.message}
                  </Badge>
                ))}
              </div>
            ) : sensitiveBlocked ? (
              <Badge tone="warning">Sensitive mappings stay disabled until all safety checks pass.</Badge>
            ) : (
              <Badge tone="success">Ready to enable</Badge>
            )}
            <ListRow
              title="Test button"
              description="Use the saved mapping endpoint before enabling."
              trailing="POST /test"
            />
            <Button variant="ghost">Test draft</Button>
          </GlassPanel>
        </aside>
      </div>
    </section>
  );
}
