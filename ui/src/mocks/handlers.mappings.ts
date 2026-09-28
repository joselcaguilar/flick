import { HttpResponse, http } from "msw";
import { mappings } from "./data";

const api = "*/api/v1";

function ok(body: unknown) {
  return HttpResponse.json(body as Parameters<typeof HttpResponse.json>[0]);
}

function noContent() {
  return new HttpResponse(null, { status: 204 });
}

function outcome(message = "Done · Home Assistant confirmed") {
  return ok({ status: "ok", message, latency: { detect_ms: 214, dispatch_ms: 2, ha_ms: 39 } });
}

export const mappingHandlers = [
  http.get(`${api}/mappings`, () => ok(mappings)),
  http.post(`${api}/mappings`, async ({ request }) =>
    ok({ ...mappings[0], ...((await request.json()) as object), id: "map-new" }),
  ),
  http.patch(`${api}/mappings/:id`, async ({ request, params }) =>
    ok({ ...mappings[0], id: params.id, ...((await request.json()) as object) }),
  ),
  http.delete(`${api}/mappings/:id`, () => noContent()),
  http.post(`${api}/mappings/:id/test`, () => outcome()),
  http.put(`${api}/mappings/order`, () => noContent()),
];
