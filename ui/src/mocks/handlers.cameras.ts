import { HttpResponse, http } from "msw";
import { cameras, camerasAvailable } from "./data";

const api = "*/api/v1";

function ok(body: unknown) {
  return HttpResponse.json(body as Parameters<typeof HttpResponse.json>[0]);
}

function noContent() {
  return new HttpResponse(null, { status: 204 });
}

export const cameraHandlers = [
  http.get(`${api}/cameras/available`, () => ok(camerasAvailable)),
  http.get(`${api}/cameras`, () => ok(cameras)),
  http.post(`${api}/cameras`, async ({ request }) =>
    ok({ ...cameras[0], ...((await request.json()) as object), id: "camera-new" }),
  ),
  http.patch(`${api}/cameras/:id`, async ({ request, params }) =>
    ok({ ...cameras[0], id: params.id, ...((await request.json()) as object) }),
  ),
  http.delete(`${api}/cameras/:id`, () => noContent()),
  http.post(`${api}/cameras/:id/start`, ({ params }) =>
    ok({ camera_id: params.id, state: "running", fps: 30 }),
  ),
  http.post(`${api}/cameras/:id/stop`, ({ params }) =>
    ok({ camera_id: params.id, state: "stopped", fps: 0 }),
  ),
  http.post(`${api}/cameras/test-rtsp`, () =>
    ok({ ok: true, width: 1280, height: 720, codec: "h264", latency_ms: 420 }),
  ),
  http.post(`${api}/cameras/:id/preview-ticket`, ({ params }) =>
    ok({
      url: `/stream/${params.id}.mjpg?ticket=mock`,
      expires_at: new Date(Date.now() + 60000).toISOString(),
    }),
  ),
];
