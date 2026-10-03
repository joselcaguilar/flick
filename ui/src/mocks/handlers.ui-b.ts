import { HttpResponse, http } from "msw";
import { type ActivityItem, type Gesture, jsonObject, type SettingsMap } from "../api/types";
import { activityItems, classifierReport, gestures, haInstance, now, settings } from "./data";

const api = "*/api/v1";

const customGestures: Gesture[] = [
  {
    id: "custom.rock",
    source: "custom",
    kind: "static",
    hands_required: 1,
    name: "Rock on",
    icon: "rock-on",
    hand_constraint: "any",
    enabled: true,
    sample_count: 7,
    accuracy: 0.92,
    distinctiveness: 0.81,
    threshold: 0.75,
    used_by: 0,
  },
  {
    id: "motion.zorro",
    source: "custom",
    kind: "motion",
    hands_required: 1,
    name: "Zorro Z",
    icon: "zorro-z",
    hand_constraint: "right",
    enabled: true,
    sample_count: 5,
    accuracy: 0.88,
    distinctiveness: 0.72,
    threshold: 0.7,
    used_by: 0,
  },
  {
    id: "motion.two_hand_stop_custom",
    source: "custom",
    kind: "motion",
    hands_required: 2,
    name: "Soft stop",
    icon: "two-hand-separate",
    hand_constraint: "any",
    enabled: false,
    sample_count: 4,
    accuracy: 0.86,
    distinctiveness: 0.78,
    threshold: 0.72,
    used_by: 0,
  },
];

let mutableSettings: SettingsMap = {
  ...settings,
  "camera.active_fps": 30,
  "camera.idle_fps": 5,
  "camera.mirror": true,
  "camera.max_hands": 2,
  "camera.show_ray": true,
  "general.start_at_login": true,
  "general.language": "en",
};
let mutableGestures: Gesture[] = [...gestures, ...customGestures];
const mutableActivity: ActivityItem[] = [
  ...activityItems,
  {
    id: "01K5Y8W2D7DBG1",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "custom.rock",
    gesture_name: "Rock on",
    confidence: 0.61,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Rock on",
    status: "suppressed",
    reason: "below_threshold",
    message: "Confidence 0.61 below 0.75",
    latency: { detect_ms: 62, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG2",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.thumb_up",
    gesture_name: "Thumbs up",
    confidence: 0.8,
    mapping_id: "map-living-lights-toggle",
    anchor_id: null,
    action_summary: "Thumbs up",
    status: "suppressed",
    reason: "cooldown",
    message: "Cooling down (0.4 s left)",
    latency: { detect_ms: 51, dispatch_ms: 1, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG3",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.open_palm",
    gesture_name: "Open palm",
    confidence: 0.79,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Open palm",
    status: "suppressed",
    reason: "not_armed",
    message: "Not armed",
    latency: { detect_ms: 48, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG4",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "custom.rock",
    gesture_name: "Rock on",
    confidence: 0.82,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Rock on with left hand",
    status: "suppressed",
    reason: "no_mapping",
    message: "No mapping for Rock on with left hand",
    latency: { detect_ms: 57, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG5",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.thumb_up",
    gesture_name: "Thumbs up",
    confidence: 0.9,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Lock mapping",
    status: "suppressed",
    reason: "blocked_domain",
    message: "Locks are blocked in Safety settings",
    latency: { detect_ms: 63, dispatch_ms: 1, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG6",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.point",
    gesture_name: "Point",
    confidence: 0.52,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Point",
    status: "suppressed",
    reason: "too_small",
    message: "Hand too far — move closer",
    latency: { detect_ms: 70, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG7",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.open_palm",
    gesture_name: "Open palm",
    confidence: 0.76,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Open palm",
    status: "suppressed",
    reason: "vote_failed",
    message: "Vote did not pass yet (needs 6 of 8 frames)",
    latency: { detect_ms: 44, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG8",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.circle_cw",
    gesture_name: "Circle clockwise",
    confidence: 0.83,
    mapping_id: null,
    anchor_id: "anchor-fan-bedroom",
    action_summary: "Circle clockwise",
    status: "suppressed",
    reason: "target_selected",
    message: "A device was selected, so the global Thumbs up mapping was skipped",
    latency: { detect_ms: 82, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG9",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.circle_cw",
    gesture_name: "Circle clockwise",
    confidence: 0.82,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Circle clockwise",
    status: "suppressed",
    reason: "ambiguous_target",
    message: "Two taught devices were too close to tell apart",
    latency: { detect_ms: 79, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG10",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.point",
    gesture_name: "Point",
    confidence: 0.86,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Point",
    status: "suppressed",
    reason: "needs_realign",
    message: "Camera moved — re-align to use pointing",
    latency: { detect_ms: 76, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG11",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.thumb_up",
    gesture_name: "Thumbs up",
    confidence: 0.84,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Thumbs up",
    status: "suppressed",
    reason: "paused",
    message: "Flick is paused",
    latency: { detect_ms: 42, dispatch_ms: 0, ha_ms: 0 },
  },
  {
    id: "01K5Y8W2D7DBG12",
    ts: now,
    camera_id: "camera-main",
    gesture_id: "builtin.thumb_up",
    gesture_name: "Thumbs up",
    confidence: 0.78,
    mapping_id: null,
    anchor_id: null,
    action_summary: "Thumbs up",
    status: "suppressed",
    reason: "ambiguous",
    message: "Two gestures looked too similar",
    latency: { detect_ms: 52, dispatch_ms: 0, ha_ms: 0 },
  },
];

function ok(body: unknown) {
  return HttpResponse.json(body as Parameters<typeof HttpResponse.json>[0]);
}

function problem(status: number, title: string, detail: string, errors?: Record<string, string>) {
  return HttpResponse.json({ title, detail, status, errors }, { status });
}

function validateSettings(patch: SettingsMap) {
  const errors: Record<string, string> = {};
  const minHand = patch["detection.min_hand_size"];
  if (typeof minHand === "number" && (minHand < 0.03 || minHand > 0.18)) {
    errors["detection.min_hand_size"] = "Min hand size must be between 3% and 18%.";
  }
  const tolerance = patch["targeting.tolerance_deg"];
  if (typeof tolerance === "number" && (tolerance < 5 || tolerance > 15)) {
    errors["targeting.tolerance_deg"] = "Aim tolerance must stay between 5° and 15°.";
  }
  const dwell = patch["targeting.dwell_ms"];
  if (typeof dwell === "number" && (dwell < 250 || dwell > 1500)) {
    errors["targeting.dwell_ms"] = "Dwell must stay between 250 ms and 1500 ms.";
  }
  const quiet = patch.quiet_hours;
  if (quiet && typeof quiet === "object" && "from" in quiet && "to" in quiet && quiet.from === quiet.to) {
    errors.quiet_hours = "Quiet hours need a different start and end time.";
  }
  return errors;
}

export const uiBHandlers = [
  http.get(`${api}/settings`, () => ok(mutableSettings)),
  http.patch(`${api}/settings`, async ({ request }) => {
    const patch = (await request.json()) as SettingsMap;
    const errors = validateSettings(patch);
    if (Object.keys(errors).length > 0) {
      return problem(
        422,
        "Invalid settings",
        "Some settings need attention before Flick can save them.",
        errors,
      );
    }
    mutableSettings = { ...mutableSettings, ...patch };
    return ok(mutableSettings);
  }),
  http.get(`${api}/gestures`, () => ok(mutableGestures)),
  http.post(`${api}/gestures`, async ({ request }) => {
    const body = (await request.json()) as Partial<Gesture>;
    const gesture: Gesture = {
      id: `custom.${Date.now().toString(36)}`,
      source: "custom",
      kind: body.kind ?? "static",
      hands_required: body.hands_required ?? 1,
      name: body.name ?? "New gesture",
      icon: body.icon ?? "rock-on",
      hand_constraint: body.hand_constraint ?? "any",
      enabled: true,
      sample_count: 5,
      accuracy: 0.91,
      distinctiveness: 0.82,
      threshold: body.threshold ?? 0.75,
      used_by: 0,
    };
    mutableGestures = [...mutableGestures, gesture];
    return ok(gesture);
  }),
  http.patch(`${api}/gestures/:id`, async ({ request, params }) => {
    const patch = (await request.json()) as Partial<Gesture>;
    const id = String(params.id);
    mutableGestures = mutableGestures.map((gesture) =>
      gesture.id === id ? { ...gesture, ...patch } : gesture,
    );
    return ok(mutableGestures.find((gesture) => gesture.id === id) ?? { ...gestures[0], id, ...patch });
  }),
  http.get(`${api}/activity`, ({ request }) => {
    const url = new URL(request.url);
    const status = url.searchParams.get("status") || null;
    const flag = url.searchParams.get("include_suppressed");
    const includeSuppressed = flag === "true" || flag === "1" || status === "suppressed";
    const limit = Number(url.searchParams.get("limit")) || 50;
    const items = mutableActivity
      .filter((item) => !status || item.status === status)
      .filter((item) => includeSuppressed || item.status !== "suppressed")
      .sort((a, b) => b.ts.localeCompare(a.ts) || b.id.localeCompare(a.id))
      .slice(0, limit);
    return ok({ items, next_before: undefined });
  }),
  http.post(`${api}/ha/connect`, async ({ request }) => {
    const body = (await request.json()) as { base_url?: string; token?: string };
    const baseUrl = body.base_url ?? "";
    const token = body.token ?? "";
    if (baseUrl.includes("offline")) {
      return problem(
        503,
        "Home Assistant unreachable",
        "Flick could not reach Home Assistant on this network.",
      );
    }
    if (baseUrl.startsWith("https://self-signed")) {
      return problem(
        495,
        "TLS certificate needs review",
        "The certificate is self-signed. Trust it only if this is your Home Assistant.",
      );
    }
    if (token.toLowerCase().includes("invalid") || token.length < 8) {
      return problem(401, "Invalid token", "Home Assistant rejected that long-lived access token.");
    }
    return ok({ ...haInstance, base_url: baseUrl || haInstance.base_url });
  }),
  http.post(`${api}/packs/export`, async ({ request }) => {
    const body = (await request.json()) as { gesture_ids?: string[]; include_mapping_templates?: boolean };
    const selected = mutableGestures.filter((gesture) => body.gesture_ids?.includes(gesture.id));
    return ok({
      format: "flick.gesture-pack",
      version: 1,
      name: "Couch essentials",
      author: "local",
      created_at: new Date().toISOString(),
      landmark_model: { id: "hand-v3", version: "2026.10.1" },
      gestures: selected.map((gesture) => ({
        id: gesture.id,
        name: gesture.name,
        kind: gesture.kind,
        hand_constraint: gesture.hand_constraint,
        icon: gesture.icon,
        threshold: gesture.threshold ?? 0.75,
        samples: [],
      })),
      motion_takes: [],
      mapping_templates: body.include_mapping_templates
        ? [{ name: "Rock on → scene", gesture_id: selected[0]?.id ?? "custom.rock", slots: {} }]
        : [],
    });
  }),
  http.post(`${api}/packs/import/preview`, async ({ request }) => {
    const pack = (await request.json()) as { gestures?: Array<{ id?: string; name?: string }> };
    return ok({
      gestures: (pack.gestures ?? [{ id: "custom.imported", name: "Imported gesture" }]).map((gesture) => ({
        id: gesture.id ?? "custom.imported",
        name: gesture.name ?? "Imported gesture",
        conflict: mutableGestures.some((existing) => existing.id === gesture.id) ? "rename" : undefined,
      })),
      mapping_templates: [],
    });
  }),
  http.post(`${api}/packs/import/commit`, async ({ request }) => {
    const body = (await request.json()) as { pack?: { gestures?: Array<Partial<Gesture>> } };
    const imported = (body.pack?.gestures ?? [{ name: "Imported gesture" }]).map((gesture, index) => {
      const id = `custom.imported_${Date.now().toString(36)}_${index}`;
      const next: Gesture = {
        id,
        source: "custom",
        kind: gesture.kind ?? "static",
        hands_required: gesture.hands_required ?? 1,
        name: gesture.name ?? "Imported gesture",
        icon: gesture.icon ?? "rock-on",
        hand_constraint: gesture.hand_constraint ?? "any",
        enabled: true,
        sample_count: gesture.sample_count ?? 3,
        accuracy: gesture.accuracy ?? 0.87,
        distinctiveness: gesture.distinctiveness ?? 0.76,
        threshold: gesture.threshold ?? 0.75,
        used_by: 0,
      };
      mutableGestures.push(next);
      return id;
    });
    return ok({ imported_gesture_ids: imported });
  }),
  http.post(`${api}/classifier/train`, () =>
    ok({
      ...classifierReport,
      trained_at: new Date().toISOString(),
      loto_accuracy: 0.95,
      metrics: jsonObject({ loto_accuracy: 0.95, train_ms: 310 }),
      confusions: [
        { gesture_id: "custom.rock", with: "builtin.open_palm", score: 0.32 },
        { gesture_id: "motion.zorro", with: "builtin.circle_cw", score: 0.26 },
      ],
    }),
  ),
];
