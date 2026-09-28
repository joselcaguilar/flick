import { HttpResponse, http } from "msw";

const api = "*/api/v1";

function ok(body: unknown) {
  return HttpResponse.json(body as Parameters<typeof HttpResponse.json>[0]);
}

export const proHandlers = [
  http.get(`${api}/license`, () => ok({ plan: "core", pro: false })),
  http.post(`${api}/license`, () => ok({ plan: "pro", pro: true })),
];
