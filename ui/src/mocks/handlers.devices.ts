import { HttpResponse, http } from "msw";
import { anchors, places } from "./data";

const api = "*/api/v1";

function ok(body: unknown) {
  return HttpResponse.json(body as Parameters<typeof HttpResponse.json>[0]);
}

function noContent() {
  return new HttpResponse(null, { status: 204 });
}

export const deviceHandlers = [
  http.get(`${api}/places`, () => ok(places)),
  http.patch(`${api}/places/:id`, async ({ request, params }) =>
    ok({ ...places[0], id: params.id, ...((await request.json()) as object) }),
  ),
  http.delete(`${api}/places/:id`, () => noContent()),
  http.post(`${api}/places/:id/realign`, () =>
    ok({ id: "realign-mock", prompts: ["anchor-fan-bedroom", "anchor-lamp-bedroom"] }),
  ),
  http.post(`${api}/realign/:session_id/point`, () => ok({ captured: true, residual_deg: 2.4 })),
  http.post(`${api}/realign/:session_id/commit`, () =>
    ok({ applied: true, residual_deg: 3.1, needs_reteach: [] }),
  ),
  http.get(`${api}/anchors`, () => ok(anchors)),
  http.patch(`${api}/anchors/:id`, async ({ request, params }) =>
    ok({ ...anchors[0], id: params.id, ...((await request.json()) as object) }),
  ),
  http.delete(`${api}/anchors/:id`, () => noContent()),
  http.post(`${api}/anchors/:id/test`, () =>
    ok({ selected: true, angular_error_deg: 4, runner_up: "Bedroom lamp" }),
  ),
  http.post(`${api}/teach`, () =>
    ok({
      id: "teach-mock",
      camera_id: "camera-main",
      target: { entity_id: "fan.ventilador_dormitorio" },
      phase: "spot1",
    }),
  ),
  http.post(`${api}/teach/:session_id/spot`, () =>
    ok({ spot_index: 1, ray_jitter_deg: 1.4, confidence: 0.92, kind: "point3d", residual_deg: 3.2 }),
  ),
  http.post(`${api}/teach/:session_id/levels/use-current`, () => ok({ levels: [1], current_percentage: 1 })),
  http.post(`${api}/teach/:session_id/levels/test`, () =>
    ok({
      status: "ok",
      message: "Ventilador dormitorio is on · 1%",
      latency: { detect_ms: 214, dispatch_ms: 2, ha_ms: 39 },
    }),
  ),
  http.post(`${api}/teach/:session_id/commit`, () =>
    ok({ anchor: anchors[0], mapping_ids: ["map-fan-speed-1", "map-fan-off"], distinctiveness_warnings: [] }),
  ),
  http.post(`${api}/teach/:session_id/cancel`, () => noContent()),
  http.post(`${api}/setup-assistant/suggest`, () =>
    ok([{ label: "ceiling fan", bbox: [0.72, 0.1, 0.12, 0.12], candidates: ["fan.ventilador_dormitorio"] }]),
  ),
];
