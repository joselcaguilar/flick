import { useMemo, useState } from "react";
import { useActivity, useGestures, useMappings, usePatchSettings, useSettings } from "../../api/hooks";
import type { ActivityItem } from "../../api/types";
import { GestureGlyph } from "../../components/domain";
import { Badge, Button, GlassPanel, Select, Switch } from "../../components/ui";
import { asPercent, formatTime } from "../../lib/utils";
import { reasonCopy, suppressionReasonOrder } from "./reasons";
import "./styles.css";

const statusItems = [
  { value: "all", label: "All statuses" },
  { value: "ok", label: "OK" },
  { value: "suppressed", label: "Suppressed" },
  { value: "error", label: "Error" },
  { value: "timeout", label: "Timeout" },
];

const timeItems = [
  { value: "all", label: "All day" },
  { value: "15m", label: "Last 15 minutes" },
  { value: "1h", label: "Last hour" },
];

function statusTone(status?: string | null): "neutral" | "success" | "warning" | "danger" | "accent" {
  if (status === "ok") return "success";
  if (status === "suppressed") return "warning";
  if (status === "error" || status === "timeout") return "danger";
  return "neutral";
}

function latencyParts(item: ActivityItem) {
  const latency = item.latency;
  if (!latency) return ["detect —", "dispatch —", "HA —"];
  return [
    `detect ${latency.detect_ms ?? "—"} ms`,
    `dispatch ${latency.dispatch_ms ?? "—"} ms`,
    `HA ${latency.ha_ms ?? "—"} ms`,
  ];
}

function ActivityRow({
  item,
  selected,
  onSelect,
}: {
  item: ActivityItem;
  selected: boolean;
  onSelect: () => void;
}) {
  const parts = latencyParts(item);
  return (
    <button type="button" className="activity-row" data-selected={selected} onClick={onSelect}>
      <span className="activity-time">{formatTime(item.ts)}</span>
      <span className="activity-gesture">
        <GestureGlyph
          name={
            item.gesture_id?.includes("circle")
              ? "circle-cw"
              : item.gesture_id?.includes("two")
                ? "two-hand-separate"
                : "thumbs-up"
          }
        />
        <span>
          <strong>{item.gesture_name ?? item.action_summary ?? "Activity"}</strong>
          <small>
            {item.confidence == null ? "confidence —" : `${asPercent(item.confidence)} confidence`}
          </small>
        </span>
      </span>
      <span>
        <strong>{item.action_summary ?? "—"}</strong>
        <small>{item.mapping_id ?? "No mapping"}</small>
      </span>
      <Badge tone={statusTone(item.status)}>{item.status}</Badge>
      <span className="latency-stack" title={parts.join(" · ")}>
        {parts.join(" · ")}
      </span>
    </button>
  );
}

function DebugReasonCard({ code }: { code: string }) {
  return (
    <div className="debug-reason-card">
      <Badge tone={code.includes("target") || code === "needs_realign" ? "warning" : "neutral"}>{code}</Badge>
      <p>{reasonCopy(code)}</p>
    </div>
  );
}

export function ActivityRoute() {
  const [status, setStatus] = useState("all");
  const [gesture, setGesture] = useState("all");
  const [mapping, setMapping] = useState("all");
  const [timeRange, setTimeRange] = useState("all");
  const query = useActivity(
    status === "all" ? "?limit=50" : `?limit=50&status=${encodeURIComponent(status)}`,
  );
  const gestures = useGestures();
  const mappings = useMappings();
  const settings = useSettings();
  const patchSettings = usePatchSettings();
  const items = query.data?.items ?? [];
  const filtered = useMemo(
    () =>
      items.filter((item) => {
        const gestureOk = gesture === "all" || item.gesture_id === gesture;
        const mappingOk = mapping === "all" || item.mapping_id === mapping;
        const timeOk = timeRange === "all" || true;
        return gestureOk && mappingOk && timeOk;
      }),
    [gesture, items, mapping, timeRange],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = filtered.find((item) => item.id === selectedId) ?? filtered[0];
  const debugEnabled = Boolean(settings.data?.["debug.log_suppressed"]);

  function exportCsv() {
    const header = [
      "time",
      "status",
      "gesture",
      "mapping",
      "action",
      "reason",
      "detect_ms",
      "dispatch_ms",
      "ha_ms",
    ];
    const rows = filtered.map((item) => [
      item.ts,
      item.status,
      item.gesture_name ?? item.gesture_id ?? "",
      item.mapping_id ?? "",
      item.action_summary ?? "",
      item.reason ?? "",
      item.latency?.detect_ms ?? "",
      item.latency?.dispatch_ms ?? "",
      item.latency?.ha_ms ?? "",
    ]);
    const csv = [header, ...rows]
      .map((row) => row.map((value) => JSON.stringify(String(value ?? ""))).join(","))
      .join("\n");
    const url = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = "flick-activity.csv";
    link.click();
    URL.revokeObjectURL(url);
  }

  return (
    <section className="activity-route" aria-labelledby="screen-title">
      <div className="activity-hero">
        <p className="route-path">Activity / Why didn't it fire?</p>
        <h1 id="screen-title">Every outcome, explainable.</h1>
        <p>
          See fired actions, suppressed gestures and the detect → dispatch → Home Assistant latency chain.
          Debug mode logs one plain-English reason per suppressed candidate.
        </p>
      </div>

      <GlassPanel className="activity-toolbar">
        <Select value={status} onValueChange={setStatus} label="Status" items={statusItems} />
        <Select
          value={gesture}
          onValueChange={setGesture}
          label="Gesture"
          items={[
            { value: "all", label: "All gestures" },
            ...(gestures.data ?? []).map((item) => ({ value: item.id, label: item.name })),
          ]}
        />
        <Select
          value={mapping}
          onValueChange={setMapping}
          label="Mapping"
          items={[
            { value: "all", label: "All mappings" },
            ...(mappings.data ?? []).map((item) => ({ value: item.id, label: item.name })),
          ]}
        />
        <Select value={timeRange} onValueChange={setTimeRange} label="Time range" items={timeItems} />
        <Button variant="secondary" onClick={exportCsv}>
          Export CSV
        </Button>
      </GlassPanel>

      <div className="activity-layout">
        <GlassPanel className="activity-list-panel">
          <div className="activity-list-heading">
            <div>
              <Badge tone="accent">{filtered.length} events</Badge>
              <h2>Activity list</h2>
            </div>
            <span>Latency breakdown appears in every row.</span>
          </div>
          <div className="activity-table">
            {filtered.map((item) => (
              <ActivityRow
                key={item.id}
                item={item}
                selected={selected?.id === item.id}
                onSelect={() => setSelectedId(item.id)}
              />
            ))}
          </div>
        </GlassPanel>

        <GlassPanel className="activity-debug-panel">
          <div className="debug-heading">
            <div>
              <Badge tone={debugEnabled ? "success" : "neutral"}>Debugger</Badge>
              <h2>Why didn't it fire?</h2>
            </div>
            <Switch
              checked={debugEnabled}
              aria-label="Enable suppressed gesture logging"
              onCheckedChange={(checked) =>
                void patchSettings.mutateAsync({ "debug.log_suppressed": checked })
              }
            />
          </div>

          {selected ? (
            <div className="selected-debug-card">
              <Badge tone={statusTone(selected.status)}>{selected.status}</Badge>
              <h3>{selected.action_summary ?? selected.gesture_name ?? "Selected activity"}</h3>
              <p>
                {selected.reason
                  ? reasonCopy(selected)
                  : (selected.message ?? "No suppression — action reached Home Assistant.")}
              </p>
              <div className="latency-grid">
                {latencyParts(selected).map((part) => (
                  <span key={part}>{part}</span>
                ))}
              </div>
              {selected.reason === "below_threshold" ? (
                <Button variant="ghost" size="sm">
                  Lower threshold for this gesture to 0.65
                </Button>
              ) : null}
            </div>
          ) : null}

          <div className="all-reasons">
            <h3>Suppression copy coverage</h3>
            <p>Every reason code from the trigger FSM and targeting layer is represented here.</p>
            <div className="debug-reason-grid">
              {suppressionReasonOrder.map((code) => (
                <DebugReasonCard key={code} code={code} />
              ))}
            </div>
            <div className="debug-aliases">
              <strong>Compatibility aliases</strong>
              <span>
                low_confidence → below_threshold · sensitive_blocked → blocked_domain · hand_too_far →
                too_small
              </span>
            </div>
          </div>
        </GlassPanel>
      </div>
    </section>
  );
}
