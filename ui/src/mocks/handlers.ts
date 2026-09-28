import { HttpResponse, http } from "msw";
import {
  activityItems,
  anchors,
  cameras,
  camerasAvailable,
  classifierReport,
  gestures,
  haAreas,
  haDiscovery,
  haEntities,
  haInstance,
  mappings,
  models,
  places,
  settings,
  status,
  updates,
} from "./data";

const api = "*/api/v1";
let mutableSettings = { ...settings };

function ok<T>(body: T) {
  return HttpResponse.json(body);
}

function noContent() {
  return new HttpResponse(null, { status: 204 });
}

function outcome(message = "Done · Home Assistant confirmed") {
  return ok({ status: "ok", message, latency: { detect_ms: 214, dispatch_ms: 2, ha_ms: 39 } });
}

export const handlers = [
  http.get("*/health", () => ok({ status: "ok", version: "0.1.0-dev", uptime_s: 42 })),
  http.get(`${api}/status`, () => ok(status)),
  http.post(`${api}/engine/pause`, async ({ request }) => {
    const body = (await request.json().catch(() => ({}))) as { duration_s?: number };
    return ok({ ...status, paused: true, mode: "paused", paused_until: body.duration_s ? new Date(Date.now() + body.duration_s * 1000).toISOString() : null });
  }),
  http.post(`${api}/engine/resume`, () => ok(status)),
  http.get(`${api}/settings`, () => ok(mutableSettings)),
  http.patch(`${api}/settings`, async ({ request }) => {
    mutableSettings = { ...mutableSettings, ...((await request.json()) as Record<string, unknown>) };
    return ok(mutableSettings);
  }),
  http.get(`${api}/ha/discover`, () => ok(haDiscovery)),
  http.post(`${api}/ha/connect`, () => ok(haInstance)),
  http.get(`${api}/ha/status`, () => ok({ state: "ready", ha_version: "2026.9", instance: haInstance })),
  http.delete(`${api}/ha`, () => noContent()),
  http.get(`${api}/ha/areas`, () => ok(haAreas)),
  http.get(`${api}/ha/entities`, ({ request }) => {
    const url = new URL(request.url);
    const domain = url.searchParams.get("domain");
    const area = url.searchParams.get("area_id");
    const q = url.searchParams.get("q")?.toLowerCase();
    return ok(
      haEntities.filter((entity) => (!domain || entity.domain === domain) && (!area || entity.area_id === area) && (!q || entity.name.toLowerCase().includes(q) || entity.entity_id.includes(q))),
    );
  }),
  http.get(`${api}/ha/services`, () =>
    ok({
      fan: { turn_on: { fields: { percentage: { selector: { number: { min: 1, max: 100, step: 1 } } } } }, turn_off: { fields: {} } },
      light: { toggle: { fields: {} }, turn_on: { fields: { brightness_pct: { selector: { number: { min: 1, max: 100 } } } } } },
    }),
  ),
  http.post(`${api}/ha/call`, () => outcome()),
  http.get(`${api}/cameras/available`, () => ok(camerasAvailable)),
  http.get(`${api}/cameras`, () => ok(cameras)),
  http.post(`${api}/cameras`, async ({ request }) => ok({ ...cameras[0], ...((await request.json()) as object), id: "camera-new" })),
  http.patch(`${api}/cameras/:id`, async ({ request, params }) => ok({ ...cameras[0], id: params.id, ...((await request.json()) as object) })),
  http.delete(`${api}/cameras/:id`, () => noContent()),
  http.post(`${api}/cameras/:id/start`, ({ params }) => ok({ camera_id: params.id, state: "running", fps: 30 })),
  http.post(`${api}/cameras/:id/stop`, ({ params }) => ok({ camera_id: params.id, state: "stopped", fps: 0 })),
  http.post(`${api}/cameras/test-rtsp`, () => ok({ ok: true, width: 1280, height: 720, codec: "h264", latency_ms: 420 })),
  http.post(`${api}/cameras/:id/preview-ticket`, ({ params }) => ok({ url: `/stream/${params.id}.mjpg?ticket=mock`, expires_at: new Date(Date.now() + 60000).toISOString() })),
  http.get(`${api}/gestures`, () => ok(gestures)),
  http.post(`${api}/gestures`, async ({ request }) => ok({ ...gestures[0], ...((await request.json()) as object), id: "custom.01K5Y8W2D7GESTURE", source: "custom", sample_count: 0 })),
  http.patch(`${api}/gestures/:id`, async ({ request, params }) => ok({ ...gestures[0], id: params.id, ...((await request.json()) as object) })),
  http.delete(`${api}/gestures/:id`, () => noContent()),
  http.post(`${api}/gestures/:id/capture`, ({ params }) => ok({ id: "capture-mock", gesture_id: params.id, kind: "positive", target_takes: 5, status: "running" })),
  http.get(`${api}/gestures/:id/motion-takes`, ({ params }) => ok([{ id: "take-1", gesture_id: params.id, take_index: 1, hands: 1, trajectory: [[0.2, 0.5], [0.5, 0.2], [0.8, 0.5]], quality: 0.91 }])),
  http.patch(`${api}/gestures/:id/type`, async ({ request, params }) => ok({ ...gestures[0], id: params.id, ...((await request.json()) as object) })),
  http.post(`${api}/capture/:session_id/cancel`, () => noContent()),
  http.get(`${api}/gestures/:id/samples`, ({ params }) => ok([{ id: "sample-1", gesture_id: params.id, take_index: 1, hand: "right", quality: 0.92 }])),
  http.delete(`${api}/gestures/:id/samples/:sample_id`, () => noContent()),
  http.post(`${api}/classifier/train`, () => ok(classifierReport)),
  http.get(`${api}/classifier`, () => ok(classifierReport)),
  http.get(`${api}/mappings`, () => ok(mappings)),
  http.post(`${api}/mappings`, async ({ request }) => ok({ ...mappings[0], ...((await request.json()) as object), id: "map-new" })),
  http.patch(`${api}/mappings/:id`, async ({ request, params }) => ok({ ...mappings[0], id: params.id, ...((await request.json()) as object) })),
  http.delete(`${api}/mappings/:id`, () => noContent()),
  http.post(`${api}/mappings/:id/test`, () => outcome()),
  http.put(`${api}/mappings/order`, () => noContent()),
  http.get(`${api}/activity`, () => ok({ items: activityItems, next_before: undefined })),
  http.get(`${api}/places`, () => ok(places)),
  http.patch(`${api}/places/:id`, async ({ request, params }) => ok({ ...places[0], id: params.id, ...((await request.json()) as object) })),
  http.delete(`${api}/places/:id`, () => noContent()),
  http.post(`${api}/places/:id/realign`, () => ok({ id: "realign-mock", prompts: ["anchor-fan-bedroom", "anchor-lamp-bedroom"] })),
  http.post(`${api}/realign/:session_id/point`, () => ok({ captured: true, residual_deg: 2.4 })),
  http.post(`${api}/realign/:session_id/commit`, () => ok({ applied: true, residual_deg: 3.1, needs_reteach: [] })),
  http.get(`${api}/anchors`, () => ok(anchors)),
  http.patch(`${api}/anchors/:id`, async ({ request, params }) => ok({ ...anchors[0], id: params.id, ...((await request.json()) as object) })),
  http.delete(`${api}/anchors/:id`, () => noContent()),
  http.post(`${api}/anchors/:id/test`, () => ok({ selected: true, angular_error_deg: 4, runner_up: "Bedroom lamp" })),
  http.post(`${api}/teach`, () => ok({ id: "teach-mock", camera_id: "camera-main", target: { entity_id: "fan.ventilador_dormitorio" }, phase: "spot1" })),
  http.post(`${api}/teach/:session_id/spot`, () => ok({ spot_index: 1, ray_jitter_deg: 1.4, confidence: 0.92, kind: "point3d", residual_deg: 3.2 })),
  http.post(`${api}/teach/:session_id/levels/use-current`, () => ok({ levels: [1], current_percentage: 1 })),
  http.post(`${api}/teach/:session_id/levels/test`, () => outcome("Ventilador dormitorio is on · 1%")),
  http.post(`${api}/teach/:session_id/commit`, () => ok({ anchor: anchors[0], mapping_ids: ["map-fan-speed-1", "map-fan-off"], distinctiveness_warnings: [] })),
  http.post(`${api}/teach/:session_id/cancel`, () => noContent()),
  http.post(`${api}/setup-assistant/suggest`, () => ok([{ label: "ceiling fan", bbox: [0.72, 0.1, 0.12, 0.12], candidates: ["fan.ventilador_dormitorio"] }])),
  http.get(`${api}/updates`, () => ok(updates)),
  http.post(`${api}/updates/check`, () => ok({ ...updates, last_check_at: new Date().toISOString() })),
  http.post(`${api}/updates/install`, () => new HttpResponse(null, { status: 202 })),
  http.post(`${api}/updates/rollback`, () => new HttpResponse(null, { status: 202 })),
  http.post(`${api}/updates/import`, () => ok({ accepted: ["catalog"], rejected: [] })),
  http.get(`${api}/models`, () => ok(models)),
  http.post(`${api}/models/:pack_id/install`, () => new HttpResponse(null, { status: 202 })),
  http.delete(`${api}/models/:pack_id`, () => noContent()),
  http.post(`${api}/packs/export`, () => ok({ format: "flick.gesture-pack", version: 1, gestures: [] })),
  http.post(`${api}/packs/import/preview`, () => ok({ gestures: [], mapping_templates: [] })),
  http.post(`${api}/packs/import/commit`, () => ok({ imported_gesture_ids: [] })),
  http.get(`${api}/license`, () => ok({ plan: "core", pro: false })),
  http.post(`${api}/license`, () => ok({ plan: "pro", pro: true })),
  http.get(`${api}/diagnostics/bundle`, () => new HttpResponse(new Blob(["mock diagnostics"]), { headers: { "Content-Type": "application/zip" } })),
  http.get(`${api}/openapi.json`, () => ok({ openapi: "3.1.0", info: { title: "Flick API", version: "0.1.0" }, paths: {} })),
];
