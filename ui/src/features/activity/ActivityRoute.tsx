import { useMemo, useState } from "react";
import {
  useActivity,
  useAnchors,
  useCameras,
  useGestures,
  useMappings,
  usePatchSettings,
  useSettings,
} from "../../api/hooks";
import type { ActivityItem } from "../../api/types";
import { GestureGlyph, gestureLabel } from "../../components/domain";
import { Badge, Button, Select, Skeleton, Switch } from "../../components/ui";
import { useEventStore } from "../../events/store";
import { asPercent, formatTime } from "../../lib/utils";
import { reasonCopy, suppressionReasonOrder } from "./reasons";
import "./styles.css";

type Tone = "neutral" | "accent" | "success" | "warning" | "danger";
type Named = { id: string; name?: string | null };

const ALL = "all";
const statusItems = [
  { value: ALL, label: "All statuses" },
  { value: "ok", label: "OK" },
  { value: "sent", label: "Sent" },
  { value: "fired", label: "Fired" },
  { value: "suppressed", label: "Ignored" },
  { value: "error", label: "Error" },
  { value: "timeout", label: "Timeout" },
  { value: "stale", label: "Stale" },
];
const timeItems = [
  { value: ALL, label: "All time" },
  { value: "15m", label: "Last 15 minutes" },
  { value: "1h", label: "Last hour" },
];
const windowMs: Record<string, number> = { "15m": 900_000, "1h": 3_600_000 };
const tones: Record<string, Tone> = {
  ok: "success",
  sent: "accent",
  fired: "accent",
  suppressed: "warning",
  stale: "warning",
  error: "danger",
  timeout: "danger",
};
const skeletonKeys = ["one", "two", "three", "four"];

const statusLabel = (status: string) => statusItems.find((item) => item.value === status)?.label ?? status;
const ms = (value?: number | null) => (value == null ? "—" : `${Math.round(value)} ms`);
const isSuppressed = (item: ActivityItem) =>
  item.status === "suppressed" || item.id.startsWith("suppressed-");
const nameMap = (list: Named[] = []) => new Map(list.map((entry) => [entry.id, entry.name ?? entry.id]));
const toItems = (all: string, names: Map<string, string>) => [
  { value: ALL, label: all },
  ...[...names].map(([value, label]) => ({ value, label })),
];

function detailCopy(item: ActivityItem) {
  if (item.reason) return reasonCopy(item);
  if (item.message) return item.message;
  if (item.status === "ok") return "Home Assistant confirmed the action.";
  if (item.status === "sent" || item.status === "fired")
    return "Sent to Home Assistant; waiting for its reply.";
  return "Flick recorded no reason for this event.";
}

function DebugReasonCard({ code }: { code: string }) {
  const tone: Tone = code.includes("target") || code === "needs_realign" ? "warning" : "neutral";
  return (
    <li className="debug-reason-card">
      <Badge tone={tone}>{code}</Badge>
      <p>{reasonCopy(code)}</p>
    </li>
  );
}

type RowProps = {
  item: ActivityItem;
  gestureName: string;
  mappingName: string;
  selected: boolean;
  onSelect: () => void;
};

function ActivityRow({ item, gestureName, mappingName, selected, onSelect }: RowProps) {
  const latency = item.latency;
  return (
    <li>
      <button
        type="button"
        className="activity-row"
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
      >
        <span className="activity-time">{formatTime(item.ts)}</span>
        <span className="activity-gesture">
          <GestureGlyph name={item.gesture_id ?? "point"} animated={false} />
          <span className="activity-cell">
            <strong>{gestureName}</strong>
            <span>
              {item.confidence == null ? "Confidence —" : `${asPercent(item.confidence)} confidence`}
            </span>
          </span>
        </span>
        <span className="activity-action activity-cell">
          <strong>{item.action_summary ?? "No action"}</strong>
          <span>{mappingName}</span>
        </span>
        <span className="activity-status">
          <Badge tone={tones[item.status] ?? "neutral"}>{statusLabel(item.status)}</Badge>
        </span>
        <span className="latency-stack">
          {latency?.detect_ms ? <span>detect {ms(latency.detect_ms)}</span> : null}
          <span>dispatch {ms(latency?.dispatch_ms)}</span>
          <span>HA {ms(latency?.ha_ms)}</span>
        </span>
      </button>
    </li>
  );
}

export function ActivityRoute() {
  const [status, setStatus] = useState<string>(ALL);
  const [gesture, setGesture] = useState<string>(ALL);
  const [mapping, setMapping] = useState<string>(ALL);
  const [timeRange, setTimeRange] = useState<string>(ALL);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const settings = useSettings();
  const patchSettings = usePatchSettings();
  const logSuppressed = Boolean(settings.data?.["debug.log_suppressed"]);

  const includeSuppressed = logSuppressed || status === "suppressed";
  const search = new URLSearchParams({ limit: "50" });
  if (status !== ALL) search.set("status", status);
  if (includeSuppressed) search.set("include_suppressed", "true");
  const query = useActivity(`?${search}`);
  const live = useEventStore((state) => state.activity);
  const gestures = useGestures().data;
  const mappings = useMappings().data;
  const cameras = useCameras().data;
  const anchors = useAnchors().data;

  const gestureNames = useMemo(() => nameMap(gestures), [gestures]);
  const mappingNames = useMemo(() => nameMap(mappings), [mappings]);
  const cameraNames = useMemo(() => nameMap(cameras), [cameras]);
  const anchorNames = useMemo(() => nameMap(anchors), [anchors]);
  const gestureItems = useMemo(() => toItems("All gestures", gestureNames), [gestureNames]);
  const mappingItems = useMemo(() => toItems("All mappings", mappingNames), [mappingNames]);
  const restItems = query.data?.items;

  const filtered = useMemo(() => {
    const rows = new Map<string, ActivityItem>();
    for (const item of live) {
      if (!includeSuppressed && isSuppressed(item)) continue;
      if (status !== ALL && item.status !== status) continue;
      rows.set(item.id, item);
    }
    for (const item of restItems ?? []) rows.set(item.id, item);
    const maxAge = windowMs[timeRange];
    const now = Date.now();
    return [...rows.values()]
      .filter((item) => gesture === ALL || item.gesture_id === gesture)
      .filter((item) => mapping === ALL || item.mapping_id === mapping)
      .filter((item) => !maxAge || now - Date.parse(item.ts) <= maxAge)
      .sort((a, b) => Date.parse(b.ts) - Date.parse(a.ts));
  }, [live, restItems, includeSuppressed, status, gesture, mapping, timeRange]);

  const nameFor = (item: ActivityItem) => {
    const id = item.gesture_id;
    return (
      (id ? gestureNames.get(id) : undefined) ?? item.gesture_name ?? (id ? gestureLabel(id) : "Gesture")
    );
  };
  const mappingFor = (item: ActivityItem) =>
    item.mapping_id ? (mappingNames.get(item.mapping_id) ?? "Unknown mapping") : "No mapping";
  const cameraFor = (item: ActivityItem) =>
    (item.camera_id ? cameraNames.get(item.camera_id) : undefined) ?? "—";
  const anchorFor = (item: ActivityItem) =>
    item.anchor_id ? (anchorNames.get(item.anchor_id) ?? item.anchor_id) : "—";

  const selected = filtered.find((item) => item.id === selectedId) ?? filtered[0];
  const filtersActive = [status, gesture, mapping, timeRange].some((value) => value !== ALL);
  const clearFilters = () => {
    setStatus(ALL);
    setGesture(ALL);
    setMapping(ALL);
    setTimeRange(ALL);
  };

  const exportCsv = () => {
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
  };

  const listBody = () => {
    if (filtered.length) {
      return (
        <ul className="activity-list">
          {filtered.map((item) => (
            <ActivityRow
              key={item.id}
              item={item}
              gestureName={nameFor(item)}
              mappingName={mappingFor(item)}
              selected={item.id === selected?.id}
              onSelect={() => setSelectedId(item.id)}
            />
          ))}
        </ul>
      );
    }
    if (query.isLoading) {
      return (
        <div className="activity-skeletons" aria-hidden="true">
          {skeletonKeys.map((key) => (
            <Skeleton key={key} className="activity-skeleton" />
          ))}
        </div>
      );
    }
    if (query.isError) return null;
    if (filtersActive) {
      return (
        <div className="activity-notice">
          <p role="status">No activity matches these filters.</p>
          <Button type="button" size="sm" variant="ghost" onClick={clearFilters}>
            Clear filters
          </Button>
        </div>
      );
    }
    return <p className="activity-muted">No activity yet. Point at a device, then gesture.</p>;
  };

  return (
    <section className="feature-screen activity-route" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Activity</h1>
          <p>Every gesture Flick acted on, what reached Home Assistant, and how long it took.</p>
        </div>
        <div className="header-actions">
          <Button type="button" variant="secondary" onClick={exportCsv} disabled={!filtered.length}>
            Export CSV
          </Button>
        </div>
      </header>

      <div className="activity-toolbar">
        <div className="activity-filter">
          <span className="activity-filter-label">Status</span>
          <Select value={status} onValueChange={setStatus} label="Status" items={statusItems} />
        </div>
        <div className="activity-filter">
          <span className="activity-filter-label">Gesture</span>
          <Select value={gesture} onValueChange={setGesture} label="Gesture" items={gestureItems} />
        </div>
        <div className="activity-filter">
          <span className="activity-filter-label">Mapping</span>
          <Select value={mapping} onValueChange={setMapping} label="Mapping" items={mappingItems} />
        </div>
        <div className="activity-filter">
          <span className="activity-filter-label">Time</span>
          <Select value={timeRange} onValueChange={setTimeRange} label="Time" items={timeItems} />
        </div>
        <div className="activity-toggle">
          <label htmlFor="activity-log-suppressed">
            <Switch
              id="activity-log-suppressed"
              checked={logSuppressed}
              disabled={!settings.data || patchSettings.isPending}
              onCheckedChange={(checked) =>
                void patchSettings.mutateAsync({ "debug.log_suppressed": checked })
              }
              aria-describedby="activity-log-suppressed-help"
            />
            Log ignored gestures
          </label>
          <p id="activity-log-suppressed-help">
            Records each gesture Flick ignored and why, and shows it here. Off keeps history lean.
          </p>
          {patchSettings.isError ? (
            <p role="alert">Couldn't save this setting. Check Flick is running, then try again.</p>
          ) : null}
        </div>
      </div>

      <div className="activity-layout">
        <section className="activity-list-panel" aria-labelledby="activity-list-title">
          <div className="activity-panel-heading">
            <h2 id="activity-list-title">Recent activity</h2>
            <span className="activity-muted">
              {filtered.length} {filtered.length === 1 ? "event" : "events"}
            </span>
          </div>
          {query.isError ? (
            <div className="activity-notice">
              <p className="activity-muted" role="alert">
                Couldn't load activity from the Flick engine. Check Flick is running, then retry.
              </p>
              <Button type="button" size="sm" variant="secondary" onClick={() => void query.refetch()}>
                Retry
              </Button>
            </div>
          ) : null}
          {listBody()}
        </section>

        <section className="activity-debug-panel" aria-labelledby="activity-debug-title">
          <h2 id="activity-debug-title">Why didn't it fire?</h2>
          {selected ? (
            <div className="activity-detail">
              <Badge tone={tones[selected.status] ?? "neutral"}>{statusLabel(selected.status)}</Badge>
              <h3>{selected.action_summary ?? nameFor(selected)}</h3>
              <p>{detailCopy(selected)}</p>
              <dl>
                <dt>Time</dt>
                <dd>{new Date(selected.ts).toLocaleString()}</dd>
                <dt>Gesture</dt>
                <dd>{nameFor(selected)}</dd>
                <dt>Mapping</dt>
                <dd>{mappingFor(selected)}</dd>
                <dt>Camera</dt>
                <dd>{cameraFor(selected)}</dd>
                <dt>Confidence</dt>
                <dd>{selected.confidence == null ? "—" : asPercent(selected.confidence, 1)}</dd>
                <dt>Anchor</dt>
                <dd>{anchorFor(selected)}</dd>
              </dl>
              <div className="latency-grid">
                <div>
                  <span>Detect</span>
                  <strong>{ms(selected.latency?.detect_ms)}</strong>
                </div>
                <div>
                  <span>Dispatch</span>
                  <strong>{ms(selected.latency?.dispatch_ms)}</strong>
                </div>
                <div>
                  <span>Home Assistant</span>
                  <strong>{ms(selected.latency?.ha_ms)}</strong>
                </div>
              </div>
            </div>
          ) : (
            <p className="activity-muted">Select an event to see why Flick did or didn't act.</p>
          )}
          <div className="all-reasons">
            <h3>What each reason means</h3>
            <p className="activity-muted">Flick logs one of these reasons whenever it ignores a gesture.</p>
            <ul className="debug-reason-grid">
              {suppressionReasonOrder.map((code) => (
                <DebugReasonCard key={code} code={code} />
              ))}
            </ul>
            <p className="activity-aliases">
              low_confidence → below_threshold · sensitive_blocked → blocked_domain · hand_too_far → too_small
            </p>
          </div>
        </section>
      </div>
    </section>
  );
}
