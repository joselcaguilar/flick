import { isBrowser } from "../lib/utils";

export type CameraPermission = "not_determined" | "restricted" | "denied" | "authorized";

const cameraPermissions = new Set<CameraPermission>(["not_determined", "restricted", "denied", "authorized"]);

export function isTauri() {
  return isBrowser && Boolean(window.__TAURI_INTERNALS__);
}

export function isMacApp() {
  return isTauri() && navigator.platform.toLowerCase().includes("mac");
}

function normalizeCameraPermission(value: unknown): CameraPermission {
  return cameraPermissions.has(value as CameraPermission) ? (value as CameraPermission) : "not_determined";
}

async function invokeTauri<T>(command: string, args?: Record<string, unknown>) {
  if (!isTauri()) throw new Error("This action is available in the Flick desktop app.");
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

/** Opens a web page in the default browser. The desktop shell only accepts http(s) URLs. */
export async function openExternal(url: string) {
  if (isTauri()) {
    await invokeTauri<void>("open_external", { url });
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

export type AppPreferences = { menu_bar: boolean; open_at_login: boolean };

export async function getAppPreferences() {
  return invokeTauri<AppPreferences>("app_preferences");
}

export async function setAppPreferences(patch: Partial<AppPreferences>) {
  return invokeTauri<AppPreferences>("set_app_preferences", { patch });
}

export async function cameraPermissionStatus() {
  return normalizeCameraPermission(await invokeTauri<unknown>("camera_permission_status"));
}

export async function cameraRequestAccess() {
  return normalizeCameraPermission(await invokeTauri<unknown>("camera_request_access"));
}

export async function openCameraPrivacySettings() {
  await invokeTauri<void>("open_camera_privacy_settings");
}

export type LocationPermission = CameraPermission | "unsupported";

function normalizeLocationPermission(value: unknown): LocationPermission {
  return value === "unsupported" ? "unsupported" : normalizeCameraPermission(value);
}

// macOS only reveals the Wi-Fi name to apps with Location access.
export async function locationPermissionStatus() {
  return normalizeLocationPermission(await invokeTauri<unknown>("location_permission_status"));
}

export async function locationRequestAccess() {
  return normalizeLocationPermission(await invokeTauri<unknown>("location_request_access"));
}

export async function openLocationPrivacySettings() {
  await invokeTauri<void>("open_location_privacy_settings");
}

export async function currentWifiSsid() {
  return invokeTauri<string | null>("current_wifi_ssid");
}
