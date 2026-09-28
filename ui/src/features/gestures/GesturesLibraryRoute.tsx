import { type ChangeEvent, useMemo, useRef, useState } from "react";
import {
  useCommitGesturePack,
  useExportGesturePack,
  useGestures,
  usePatchGesture,
  usePreviewGesturePack,
} from "../../api/hooks";
import type { Gesture } from "../../api/types";
import { GestureGlyph } from "../../components/domain";
import { Badge, Button, GlassPanel, ListRow, Switch, TypeChip } from "../../components/ui";
import { asPercent } from "../../lib/utils";
import "./styles.css";

function gestureType(gesture: Gesture) {
  if ((gesture.hands_required ?? 1) > 1) return "two-hand";
  return gesture.kind === "static" ? "static" : "motion";
}

const gestureTypeLabels: Record<ReturnType<typeof gestureType>, string> = {
  static: "Static",
  motion: "Motion",
  "two-hand": "Two-hand",
};

const gestureDescriptions: Record<string, string> = {
  "builtin.circle_cw":
    "Draw a clockwise circle to step the selected fan or light upward with a deliberate motion.",
  "builtin.circle_ccw":
    "Draw a counter-clockwise circle to step a selected device down without reaching for a remote.",
  "builtin.two_hand_separate":
    "Move both hands apart to stop the selected fan, media player or other safe mapped device.",
  "builtin.thumb_up": "A steady thumbs-up for quick global confirmations like toggling a light.",
  "builtin.thumb_down": "A steady thumbs-down for safe negative actions such as turning a scene off.",
  "builtin.open_palm": "Open palm is easy to hold and works well as an arm, pause or confirmation gesture.",
  "builtin.point": "Pointing selects taught devices before a second gesture decides the action.",
  "custom.rock": "Your local rock-on pose, tuned for scene shortcuts and low-confusion static control.",
  "motion.zorro": "A sharp Z-shaped motion for expressive commands that should not collide with circles.",
  "motion.two_hand_stop_custom":
    "A softer two-hand stop motion for rooms where the built-in stop feels too strict.",
};

function sourceLabel(gesture: Gesture) {
  return gesture.source === "builtin" ? "Built-in" : "Custom";
}

function handLabel(gesture: Gesture) {
  const hand = gesture.hand_constraint ?? "any";
  if (hand === "left") return "Left hand";
  if (hand === "right") return "Right hand";
  return (gesture.hands_required ?? 1) > 1 ? "Two hands" : "Any hand";
}

function gestureDescription(gesture: Gesture) {
  return (
    gestureDescriptions[gesture.id] ??
    `${handLabel(gesture)} ${gestureTypeLabels[gestureType(gesture)].toLowerCase()} gesture with ${
      gesture.used_by ?? 0
    } mapping${(gesture.used_by ?? 0) === 1 ? "" : "s"}.`
  );
}

function toneForGesture(gesture: Gesture) {
  const type = gestureType(gesture);
  if (type === "static") return "accent" as const;
  if (type === "two-hand") return "warning" as const;
  return "success" as const;
}

function GestureCard({
  gesture,
  checked,
  onCheckedChange,
  onToggle,
  pending,
  exportMode,
}: {
  gesture: Gesture;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  onToggle: (gesture: Gesture) => void;
  pending: boolean;
  exportMode: boolean;
}) {
  const type = gestureType(gesture);
  const tuned =
    gesture.source === "builtin" && ["builtin.circle_cw", "builtin.two_hand_separate"].includes(gesture.id);
  return (
    <article className="gesture-card" data-disabled={!gesture.enabled}>
      {exportMode ? (
        <label className="gesture-select">
          <input
            type="checkbox"
            checked={checked}
            onChange={(event) => onCheckedChange(event.target.checked)}
          />
          <span>Add to export</span>
        </label>
      ) : null}
      <div className="gesture-card-main">
        <GestureGlyph name={gesture.icon} animated />
        <div className="gesture-card-title">
          <h3>{gesture.name}</h3>
          <p className="gesture-meta-row">
            <span>{handLabel(gesture)}</span>
            <span aria-hidden="true">·</span>
            <span>
              {gesture.used_by ?? 0} mapping{(gesture.used_by ?? 0) === 1 ? "" : "s"}
            </span>
          </p>
        </div>
      </div>
      <div className="gesture-badge-row">
        <TypeChip>{gestureTypeLabels[type]}</TypeChip>
        <Badge tone={toneForGesture(gesture)}>{sourceLabel(gesture)}</Badge>
        {tuned ? <Badge tone="warning">Tuned in v4</Badge> : null}
        {gesture.sample_count ? <Badge tone="neutral">{gesture.sample_count} takes</Badge> : null}
      </div>
      <p className="gesture-fallback-note">{gestureDescription(gesture)}</p>
      {gesture.source === "custom" ? (
        <div className="gesture-quality">
          <span>Accuracy {gesture.accuracy == null ? "—" : asPercent(gesture.accuracy)}</span>
          <span>
            Distinctiveness {gesture.distinctiveness == null ? "—" : asPercent(gesture.distinctiveness)}
          </span>
        </div>
      ) : null}
      <div className="gesture-card-footer">
        <span>{gesture.enabled ? "Enabled" : "Disabled"}</span>
        <Switch
          checked={gesture.enabled}
          disabled={pending}
          aria-label={`Toggle ${gesture.name}`}
          onCheckedChange={() => onToggle(gesture)}
        />
      </div>
    </article>
  );
}

function GestureSection({
  title,
  description,
  gestures,
  selected,
  onSelect,
  onToggle,
  pending,
  exportMode,
}: {
  title: string;
  description: string;
  gestures: Gesture[];
  selected: Set<string>;
  onSelect: (id: string, checked: boolean) => void;
  onToggle: (gesture: Gesture) => void;
  pending: boolean;
  exportMode: boolean;
}) {
  return (
    <GlassPanel className="gestures-section">
      <div className="gestures-section-heading">
        <div>
          <Badge tone="accent">{gestures.length} gestures</Badge>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
      </div>
      <div className="gestures-grid">
        {gestures.map((gesture) => (
          <GestureCard
            key={gesture.id}
            gesture={gesture}
            checked={selected.has(gesture.id)}
            onCheckedChange={(checked) => onSelect(gesture.id, checked)}
            onToggle={onToggle}
            pending={pending}
            exportMode={exportMode}
          />
        ))}
      </div>
    </GlassPanel>
  );
}

export function GesturesLibraryRoute() {
  const gestures = useGestures();
  const patchGesture = usePatchGesture();
  const exportPack = useExportGesturePack();
  const previewPack = usePreviewGesturePack();
  const commitPack = useCommitGesturePack();
  const fileRef = useRef<HTMLInputElement>(null);
  const [selected, setSelected] = useState<Set<string>>(
    new Set(["builtin.circle_cw", "builtin.two_hand_separate"]),
  );
  const [exportMode, setExportMode] = useState(false);
  const [packStatus, setPackStatus] = useState<string | null>(null);
  const allGestures = gestures.data ?? [];
  const builtIns = useMemo(
    () => allGestures.filter((gesture) => gesture.source === "builtin"),
    [allGestures],
  );
  const custom = useMemo(() => allGestures.filter((gesture) => gesture.source === "custom"), [allGestures]);

  function setSelectedGesture(id: string, checked: boolean) {
    setSelected((current) => {
      const next = new Set(current);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  }

  async function toggleGesture(gesture: Gesture) {
    await patchGesture.mutateAsync({ id: gesture.id, patch: { enabled: !gesture.enabled } });
    setPackStatus(`${gesture.name} ${gesture.enabled ? "disabled" : "enabled"}.`);
  }

  async function exportSelected() {
    const pack = await exportPack.mutateAsync({
      gesture_ids: Array.from(selected),
      include_mapping_templates: true,
    });
    const blob = new Blob([JSON.stringify(pack, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "couch-essentials.flickpack.json";
    link.click();
    URL.revokeObjectURL(url);
    setPackStatus(`Exported ${selected.size} gesture${selected.size === 1 ? "" : "s"} as .flickpack.json.`);
    setExportMode(false);
  }

  async function importPack(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    try {
      const pack = JSON.parse(await file.text()) as Record<string, unknown>;
      const preview = await previewPack.mutateAsync(pack);
      const result = await commitPack.mutateAsync({ pack, choices: {} });
      setPackStatus(
        `Imported ${result.imported_gesture_ids.length || preview.gestures.length} gesture${preview.gestures.length === 1 ? "" : "s"} from ${file.name}.`,
      );
    } catch (error) {
      setPackStatus(error instanceof Error ? error.message : "That pack could not be imported.");
    } finally {
      event.target.value = "";
    }
  }

  return (
    <section className="gestures-route" aria-labelledby="screen-title">
      <div className="gestures-hero">
        <p className="route-path">Gestures / Library</p>
        <h1 id="screen-title">Every flick Flick understands.</h1>
        <p>
          Built-ins work without training. Custom gestures keep their takes, quality and thresholds local, and
          packs share gestures without Home Assistant identifiers.
        </p>
      </div>

      <GlassPanel className="gesture-toolbar">
        <div>
          <strong>Gesture packs</strong>
          <span>Import packs, or enter selection mode only when exporting a portable .flickpack.json.</span>
        </div>
        <div className="gesture-toolbar-actions">
          <Button variant="secondary" onClick={() => setExportMode(true)} disabled={exportMode}>
            Select for export
          </Button>
          <Button
            variant="secondary"
            onClick={() => fileRef.current?.click()}
            loading={commitPack.isPending || previewPack.isPending}
          >
            Import pack
          </Button>
          <a className="ui-button ui-button-ghost ui-button-md" href="/gestures/new">
            Record new gesture
          </a>
          <input
            ref={fileRef}
            className="visually-hidden-file"
            type="file"
            aria-label="Import gesture pack file"
            accept=".flickpack.json,application/json"
            onChange={importPack}
          />
        </div>
      </GlassPanel>

      {exportMode ? (
        <GlassPanel className="gesture-export-bar" aria-label="Gesture export selection">
          <div>
            <strong>{selected.size} selected for export</strong>
            <span>Select only the gestures you want in this pack.</span>
          </div>
          <div className="gesture-toolbar-actions">
            <Button
              variant="primary"
              onClick={exportSelected}
              disabled={selected.size === 0}
              loading={exportPack.isPending}
            >
              Export
            </Button>
            <Button variant="ghost" onClick={() => setExportMode(false)}>
              Cancel
            </Button>
          </div>
        </GlassPanel>
      ) : null}

      {packStatus ? (
        <p className="gestures-status" role="status">
          {packStatus}
        </p>
      ) : null}

      <GestureSection
        title="Built-in"
        description="Static, motion and two-hand gestures that ship with Flick. Catalog updates can tune defaults, but your overrides win."
        gestures={builtIns}
        selected={selected}
        onSelect={setSelectedGesture}
        onToggle={(gesture) => void toggleGesture(gesture)}
        pending={patchGesture.isPending}
        exportMode={exportMode}
      />

      <GestureSection
        title="Custom"
        description="Your recorded static, motion and two-hand gestures. Thumbnails are rendered from landmarks, never camera images."
        gestures={custom}
        selected={selected}
        onSelect={setSelectedGesture}
        onToggle={(gesture) => void toggleGesture(gesture)}
        pending={patchGesture.isPending}
        exportMode={exportMode}
      />

      {gestures.isLoading ? (
        <GlassPanel className="gestures-empty">Loading gesture catalog…</GlassPanel>
      ) : null}
      {!gestures.isLoading && custom.length === 0 ? (
        <GlassPanel className="gestures-empty">
          <GestureGlyph name="open-palm" animated={false} />
          <ListRow
            title="No custom gestures yet"
            description="Record a gesture in Studio, then train and save it locally."
            trailing={
              <a className="ui-button ui-button-primary ui-button-sm" href="/gestures/new">
                Record
              </a>
            }
          />
        </GlassPanel>
      ) : null}
    </section>
  );
}
