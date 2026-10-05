import { useState } from "react";
import { type useAnchors, useDeleteMapping, usePatchMapping, useTestMapping } from "../../api/hooks";
import type { Action, Mapping } from "../../api/types";
import { GestureGlyph } from "../../components/domain";
import { Badge, Button, Select, Switch } from "../../components/ui";

const targetedVerbActions: Array<{ label: string; action: Action; description: string }> = [
  {
    label: "Speed 1 · 1%",
    action: { kind: "verb", verb: "level_set", level: 1 },
    description: "Fan level taught from HA state",
  },
  { label: "Off", action: { kind: "verb", verb: "off" }, description: "fan.turn_off for selected fan" },
  { label: "On", action: { kind: "verb", verb: "on" }, description: "fan.turn_on for selected fan" },
  { label: "Toggle", action: { kind: "verb", verb: "toggle" }, description: "Domain default toggle" },
  {
    label: "Dial percentage",
    action: { kind: "dial", entity_id: "$selected", property: "percentage", gain: 1 },
    description: "Pinch dial on selected device",
  },
];

function mappingTone(mapping: Mapping) {
  if (!mapping.enabled) return "neutral";
  if (mapping.sensitive) return "warning";
  return "success";
}

function mappingSentence(mapping: Mapping, targetLabel?: string) {
  if (mapping.target_mode === "global") {
    return `${mapping.gesture_name ?? mapping.gesture_id} (${mapping.hand} hand) → ${mapping.name.split("→").at(-1)?.trim() ?? "Action"}`;
  }
  return `Point at ${targetLabel ?? "device"} + ${mapping.gesture_name ?? mapping.gesture_id} → ${mapping.name.split("→").at(-1)?.trim() ?? "Action"}`;
}

function mappingDescription(mapping: Mapping) {
  const camera = mapping.camera_ids.length ? mapping.camera_ids.join(", ") : "all cameras";
  return `${mapping.mode} · ${camera}${mapping.sensitive ? " · sensitive" : ""}`;
}

function actionKey(action: Action) {
  if (action.kind === "verb") {
    return action.verb === "level_set" ? `level_set:${action.level ?? 1}` : action.verb;
  }
  if (action.kind === "dial") return `dial:${action.property}`;
  return `${action.domain}.${action.service}`;
}

function actionLabelFor(mapping: Mapping) {
  return (
    targetedVerbActions
      .find((option) => actionKey(option.action) === actionKey(mapping.action))
      ?.label.replace(/\s+·.+$/, "") ??
    mapping.name.split("→").at(-1)?.trim() ??
    "Action"
  );
}

function targetOptions(anchors: ReturnType<typeof useAnchors>["data"], mapping: Mapping) {
  const anchorItems =
    anchors?.map((anchor) => ({
      value: `anchor:${anchor.id}`,
      label: `Device · ${anchor.name}`,
    })) ?? [];
  const domainItems = ["fan", "light", "media_player", "cover", mapping.target_domain]
    .filter((domain): domain is string => Boolean(domain))
    .filter((domain, index, domains) => domains.indexOf(domain) === index)
    .map((domain) => ({ value: `domain:${domain}`, label: `Selected ${domain.replace(/_/g, " ")}` }));
  return [...anchorItems, ...domainItems];
}

function targetValueFor(mapping: Mapping) {
  if (mapping.target_mode === "anchor" && mapping.anchor_id) return `anchor:${mapping.anchor_id}`;
  return `domain:${mapping.target_domain ?? "fan"}`;
}

function targetPatch(value: string): Partial<Mapping> {
  const [kind, id] = value.split(":", 2);
  if (kind === "anchor") {
    return { target_mode: "anchor", anchor_id: id };
  }
  return { target_mode: "domain", target_domain: id };
}

export function MappingRow({
  mapping,
  targetLabel,
  anchors,
}: {
  mapping: Mapping;
  targetLabel?: string;
  anchors?: ReturnType<typeof useAnchors>["data"];
}) {
  const test = useTestMapping(mapping.id);
  const patchMapping = usePatchMapping();
  const deleteMapping = useDeleteMapping();
  const [editing, setEditing] = useState(false);
  const canEditTarget = mapping.target_mode !== "global";
  const options = targetOptions(anchors, mapping);
  const actionValue = actionKey(mapping.action);

  function patchAction(value: string) {
    const option = targetedVerbActions.find((item) => actionKey(item.action) === value);
    if (!option) return;
    patchMapping.mutate({
      id: mapping.id,
      patch: {
        action: option.action,
        name: `${targetLabel ?? "Device"} + ${mapping.gesture_name ?? mapping.gesture_id} → ${option.label.replace(/\s+·.+$/, "").toLowerCase()}`,
      },
    });
  }

  function patchTarget(value: string) {
    const selected = options.find((option) => option.value === value);
    patchMapping.mutate({
      id: mapping.id,
      patch: {
        ...targetPatch(value),
        name: `${selected?.label.replace(/^Device · |^Selected /, "") ?? targetLabel ?? "Device"} + ${mapping.gesture_name ?? mapping.gesture_id} → ${actionLabelFor(mapping).toLowerCase()}`,
      },
    });
  }

  return (
    <article className="mapping-row">
      <div className="mapping-row-main">
        <GestureGlyph name={mapping.gesture_id} animated={false} />
        <div>
          <strong>{mappingSentence(mapping, targetLabel)}</strong>
          <span>{mappingDescription(mapping)}</span>
        </div>
      </div>
      <div className="mapping-row-meta">
        <Badge tone={mappingTone(mapping)}>{mapping.enabled ? "enabled" : "off"}</Badge>
        <Badge tone="accent">{mapping.mode}</Badge>
        {mapping.sensitive ? <Badge tone="warning">Sensitive</Badge> : null}
        <Switch
          checked={mapping.enabled}
          aria-label={`Enable ${mapping.name}`}
          onCheckedChange={(enabled) => patchMapping.mutate({ id: mapping.id, patch: { enabled } })}
        />
        <Button variant="ghost" size="sm" loading={test.isPending} onClick={() => test.mutate()}>
          Test
        </Button>
        {canEditTarget ? (
          <Button variant="secondary" size="sm" onClick={() => setEditing((value) => !value)}>
            {editing ? "Done" : "Edit"}
          </Button>
        ) : null}
        <Button
          variant="danger"
          size="sm"
          loading={deleteMapping.isPending}
          onClick={() => deleteMapping.mutate(mapping.id)}
        >
          Delete
        </Button>
      </div>
      {editing && canEditTarget ? (
        <div className="mapping-edit-row">
          <div className="compact-field">
            <span>Target</span>
            <Select
              value={targetValueFor(mapping)}
              onValueChange={patchTarget}
              label="Mapping target"
              items={options}
            />
          </div>
          <div className="compact-field">
            <span>Action</span>
            <Select
              value={actionValue}
              onValueChange={patchAction}
              label="Mapping action"
              items={targetedVerbActions.map((option) => ({
                value: actionKey(option.action),
                label: option.label,
              }))}
            />
          </div>
        </div>
      ) : null}
      {test.data?.message ? <p className="inline-result">{test.data.message}</p> : null}
    </article>
  );
}
