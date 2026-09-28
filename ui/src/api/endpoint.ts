import { isBrowser } from "../lib/utils";

export interface EngineEndpoint {
  baseUrl: string;
  token: string;
  mock: boolean;
}

function normalizeBaseUrl(value: string) {
  return value.replace(/\/$/, "");
}

function queryWantsMock() {
  if (!isBrowser) return false;
  return new URLSearchParams(window.location.search).get("mock") === "1";
}

export async function resolveEngineEndpoint(): Promise<EngineEndpoint> {
  const envUrl = import.meta.env.VITE_ENGINE_URL;
  const envToken = import.meta.env.VITE_ENGINE_TOKEN;
  const mock = import.meta.env.VITE_MOCK === "1" || queryWantsMock();

  if (envUrl) {
    return { baseUrl: normalizeBaseUrl(envUrl), token: envToken ?? "dev-token", mock };
  }

  if (!mock && isBrowser && window.__TAURI_INTERNALS__) {
    const tauri = await import("@tauri-apps/api/core");
    const endpoint = await tauri.invoke<{ base_url: string; token: string }>("engine_endpoint");
    return { baseUrl: normalizeBaseUrl(endpoint.base_url), token: endpoint.token, mock: false };
  }

  return { baseUrl: "http://127.0.0.1:7871", token: envToken ?? "dev-token", mock };
}

export function toWsUrl(baseUrl: string, path = "/api/v1/events") {
  const url = new URL(path, `${baseUrl}/`);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}
