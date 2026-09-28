import createClient, { type Middleware } from "openapi-fetch";
import type { paths } from "./schema";
import type { Action, SettingsPatch } from "./types";
import { resolveEngineEndpoint, type EngineEndpoint } from "./endpoint";

let endpointPromise: Promise<EngineEndpoint> | undefined;

export function getEndpoint() {
  endpointPromise ??= resolveEngineEndpoint();
  return endpointPromise;
}

export function resetEndpointForTests() {
  endpointPromise = undefined;
}

const authMiddleware: Middleware = {
  async onRequest({ request }) {
    const endpoint = await getEndpoint();
    request.headers.set("Authorization", `Bearer ${endpoint.token}`);
    return request;
  },
};

export async function openApiClient() {
  const endpoint = await getEndpoint();
  const client = createClient<paths>({ baseUrl: `${endpoint.baseUrl}/api/v1` });
  client.use(authMiddleware);
  return client;
}

async function requestJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const endpoint = await getEndpoint();
  const response = await fetch(`${endpoint.baseUrl}${path}`, {
    ...init,
    headers: {
      Authorization: `Bearer ${endpoint.token}`,
      "Content-Type": "application/json",
      ...init.headers,
    },
  });

  if (!response.ok) {
    const problem = await response.json().catch(() => ({ title: response.statusText, status: response.status }));
    throw Object.assign(new Error(problem.detail ?? problem.title ?? "Request failed"), { problem });
  }

  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export const api = {
  get: <T>(path: string) => requestJson<T>(path),
  post: <T>(path: string, body?: unknown) =>
    requestJson<T>(path, { method: "POST", body: body === undefined ? undefined : JSON.stringify(body) }),
  patch: <T>(path: string, body?: unknown) =>
    requestJson<T>(path, { method: "PATCH", body: body === undefined ? undefined : JSON.stringify(body) }),
  put: <T>(path: string, body?: unknown) => requestJson<T>(path, { method: "PUT", body: JSON.stringify(body) }),
  delete: <T>(path: string) => requestJson<T>(path, { method: "DELETE" }),
  patchSettings: (body: SettingsPatch) => api.patch("/api/v1/settings", body),
  callAction: (body: Action) => api.post("/api/v1/ha/call", body),
};
