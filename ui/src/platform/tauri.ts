import { isBrowser } from "../lib/utils";

export function isTauri() {
  return isBrowser && Boolean(window.__TAURI_INTERNALS__);
}

export function isMacApp() {
  return isTauri() && navigator.platform.toLowerCase().includes("mac");
}
