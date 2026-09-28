import { type APIRequestContext, expect, type Page } from "playwright/test";

export const engineUrl = process.env.E2E_ENGINE_URL ?? "http://127.0.0.1:7871";
export const authHeaders = { Authorization: "Bearer dev-token" };

export interface MockHaCall {
  domain: string;
  service: string;
  target?: { entity_id?: string[] | string | null };
  service_data?: Record<string, unknown>;
}

export async function engineGet<T>(request: APIRequestContext, path: string): Promise<T> {
  const response = await request.get(`${engineUrl}${path}`, { headers: authHeaders });
  expect(response.ok(), `${path} returned ${response.status()}`).toBeTruthy();
  return (await response.json()) as T;
}

export async function enginePost<T>(
  request: APIRequestContext,
  path: string,
  data?: unknown,
  expectedStatus = 200,
): Promise<T> {
  const response = await request.post(`${engineUrl}${path}`, {
    headers: authHeaders,
    data,
  });
  expect(response.status(), `${path} status`).toBe(expectedStatus);
  if (response.status() === 204) return undefined as T;
  return (await response.json()) as T;
}

export async function replay(request: APIRequestContext, fixture: string) {
  await enginePost(request, "/api/v1/dev/replay", { fixture }, 202);
}

export async function mockHaCalls(request: APIRequestContext): Promise<MockHaCall[]> {
  return engineGet<MockHaCall[]>(request, "/api/v1/dev/mock-ha/calls");
}

export async function waitForMockHaCall(
  request: APIRequestContext,
  baseline: number,
  predicate: (call: MockHaCall) => boolean,
) {
  await expect
    .poll(async () => (await mockHaCalls(request)).slice(baseline).some(predicate), { timeout: 10_000 })
    .toBe(true);
}

export function hasServiceCall(
  domain: string,
  service: string,
  entityId: string,
  data?: Record<string, unknown>,
) {
  return (call: MockHaCall) => {
    const entities = call.target?.entity_id;
    const entityIds = Array.isArray(entities) ? entities : entities ? [entities] : [];
    return (
      call.domain === domain &&
      call.service === service &&
      entityIds.includes(entityId) &&
      Object.entries(data ?? {}).every(([key, value]) => call.service_data?.[key] === value)
    );
  };
}

export async function expectHudDone(page: Page, text: RegExp) {
  await expect(page.getByText(text)).toBeVisible();
  await expect(page.getByText(/Done|Home Assistant confirmed/i)).toBeVisible();
}
