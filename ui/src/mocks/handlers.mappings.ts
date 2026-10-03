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
  http.patch(`${api}/mappings/:id`, async ({ request, params }) => {
    const mapping = mappings.find((item) => item.id === params.id) ?? mappings[0];
    const patch = (await request.json()) as { gesture_id?: string };
    const conflict =
      patch.gesture_id &&
      mapping.anchor_id &&
      mappings.some(
        (other) =>
          other.id !== mapping.id &&
          other.anchor_id === mapping.anchor_id &&
          other.gesture_id === patch.gesture_id,
      );
    if (conflict) {
      return HttpResponse.json(
        {
          type: "about:blank",
          title: "Gesture already used",
          status: 422,
          code: "gesture_conflict",
          detail: "This device already uses that gesture for another action. Pick a different gesture.",
        },
        { status: 422 },
      );
    }
    return ok({ ...mapping, ...patch, id: params.id });
  }),
  http.delete(`${api}/mappings/:id`, () => noContent()),
  http.post(`${api}/mappings/:id/test`, () => outcome()),
  http.put(`${api}/mappings/order`, () => noContent()),
];
