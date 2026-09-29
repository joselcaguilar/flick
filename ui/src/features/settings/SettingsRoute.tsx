import { useState } from "react";
import { Link } from "react-router-dom";
import {
  useAnchors,
  useCameras,
  useCheckUpdates,
  useInstallUpdate,
  useModels,
  usePatchSettings,
  useRollbackUpdate,
  useSettings,
  useStatus,
  useUpdates,
} from "../../api/hooks";
import type { DarkThemeId, LightThemeId, SettingsMap, SettingsPatch, ThemeMode } from "../../api/types";
import { DevicePill } from "../../components/domain";
import { Badge, Button, Input, Select, Switch } from "../../components/ui";
import { isTauri } from "../../platform/tauri";
import { darkThemes, lightThemes, useTheme } from "../../theme";
import { HaConnectionSection } from "./HaConnectionSection";
import { extractErrors, type FieldErrors, SettingRow, SettingSection, sectionId } from "./SettingParts";
import "./styles.css";
import { useAppPreferences, useSetAppPreferences } from "./useAppPreferences";

const themeModeItems = [
  { value: "system", label: "Sync with system" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

// App behavior lives in the desktop shell; plain browser builds have nothing to control.
const showGeneral = isTauri() || import.meta.env.VITE_MOCK === "1" || import.meta.env.DEV;
const onMac = typeof navigator !== "undefined" && navigator.platform.toLowerCase().includes("mac");
const trayPlace = onMac ? "menu bar" : "system tray";
const dockPlace = onMac ? "Dock" : "taskbar";

const sectionTitles = [
  ...(showGeneral ? ["General"] : []),
  "Appearance",
  "Camera",
  "Recognition sensitivity",
  "Safety",
  "Quiet hours",
  "Pointing",
  "Feedback",
  "Updates",
  "Home Assistant connection",
  "Privacy",
  "About",
];

const sensitivityItems = [
  { value: "low", label: "Low" },
  { value: "normal", label: "Normal" },
  { value: "high", label: "High" },
];

const channelItems = [
  { value: "stable", label: "Stable" },
  { value: "beta", label: "Beta" },
  { value: "nightly", label: "Nightly" },
];

const positionItems = [
  { value: "top-center", label: "Top center" },
  { value: "top-right", label: "Top right" },
  { value: "bottom-center", label: "Bottom center" },
  { value: "bottom-right", label: "Bottom right" },
];

const rayModelItems = [
  { value: "auto", label: "Auto" },
  { value: "eye", label: "Eye-rooted" },
  { value: "finger", label: "Finger-only" },
];

const dominantEyeItems = [
  { value: "center", label: "Center" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

function boolValue(settings: SettingsMap, key: string, fallback = false) {
  const value = settings[key];
  return typeof value === "boolean" ? value : fallback;
}

function numberValue(settings: SettingsMap, key: string, fallback: number) {
  const value = settings[key];
  return typeof value === "number" ? value : fallback;
}

function stringValue(settings: SettingsMap, key: string, fallback: string) {
  const value = settings[key];
  return typeof value === "string" ? value : fallback;
}

export function SettingsRoute() {
  const settingsQuery = useSettings();
  const patchSettings = usePatchSettings();
  const updates = useUpdates();
  const checkUpdates = useCheckUpdates();
  const installUpdate = useInstallUpdate();
  const rollbackUpdate = useRollbackUpdate();
  const models = useModels();
  const engineStatus = useStatus();
  const place = engineStatus.data?.place;
  const anchors = useAnchors(place?.id ?? "");
  const taughtNames = place ? (anchors.data ?? []).map((anchor) => anchor.name) : [];
  const cameras = useCameras();
  const theme = useTheme();
  const appPreferences = useAppPreferences();
  const setAppPreferences = useSetAppPreferences();
  const menuBar = appPreferences.data?.menu_bar ?? true;
  const openAtLogin = appPreferences.data?.open_at_login ?? false;
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [notice, setNotice] = useState<string | null>(null);
  const settings = settingsQuery.data ?? {};
  const hud = settings["feedback.hud"] ?? { enabled: true, position: "top-center", duration_ms: 1500 };
  const sounds = settings["feedback.sounds"] ?? { enabled: true, volume: 0.6 };
  const arm = settings["detection.arm"] ?? {
    enabled: false,
    gesture: "builtin.open_palm",
    hold_ms: 600,
    window_ms: 4000,
  };
  const pauseGesture = settings["detection.pause_gesture"] ?? {
    enabled: false,
    gesture: "builtin.open_palm",
    hold_ms: 1500,
  };
  const vote = settings["detection.vote"] ?? { n: 6, m: 8 };
  const quietHours = settings.quiet_hours ?? { from: "23:00", to: "07:00" };

  async function patch(patch: SettingsPatch, success = "Saved") {
    try {
      setFieldErrors({});
      await patchSettings.mutateAsync(patch);
      setNotice(success);
    } catch (error) {
      const details = extractErrors(error);
      setFieldErrors(details.errors);
      setNotice(details.detail);
    }
  }

  async function patchOne(key: string, value: unknown, success?: string) {
    await patch({ [key]: value } as SettingsPatch, success ?? `Saved ${key}`);
  }

  async function patchApp(patch: { menu_bar?: boolean; open_at_login?: boolean }, success: string) {
    try {
      await setAppPreferences.mutateAsync(patch);
      setNotice(success);
    } catch (error) {
      // Shell commands reject with a plain string.
      setNotice(
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "Flick could not change that setting.",
      );
    }
  }

  return (
    <section className="settings-route" aria-labelledby="screen-title">
      <div className="page-header">
        <h1 id="screen-title">Settings</h1>
        <p>
          Changes save as you make them. Flick flags any value the engine can't accept next to the control.
        </p>
      </div>

      {notice ? (
        <p className="settings-notice" role="status">
          {notice}
        </p>
      ) : null}

      <div className="settings-layout">
        <nav className="settings-nav" aria-label="Settings sections">
          {sectionTitles.map((title) => (
            <a key={title} href={`#${sectionId(title)}`}>
              {title}
            </a>
          ))}
        </nav>
        <div className="settings-sections">
          {showGeneral ? (
            <SettingSection title="General" subtitle="How Flick runs on this computer.">
              <SettingRow
                label={`Keep in ${trayPlace}`}
                description={
                  menuBar
                    ? `Closing the window leaves Flick watching from the ${trayPlace}, out of your ${dockPlace}.`
                    : `Closing the window leaves Flick watching from the ${dockPlace}. The ${trayPlace} icon is hidden.`
                }
              >
                <Switch
                  checked={menuBar}
                  disabled={appPreferences.isLoading}
                  onCheckedChange={(checked) =>
                    void patchApp(
                      { menu_bar: checked },
                      checked
                        ? `Flick will stay in the ${trayPlace}.`
                        : `Flick will stay in the ${dockPlace}.`,
                    )
                  }
                  aria-label={`Keep in ${trayPlace}`}
                />
              </SettingRow>
              <SettingRow
                label="Open at login"
                description={`Start Flick when you log in, without opening this window. It waits in the ${menuBar ? trayPlace : dockPlace}.`}
              >
                <Switch
                  checked={openAtLogin}
                  disabled={appPreferences.isLoading}
                  onCheckedChange={(checked) =>
                    void patchApp(
                      { open_at_login: checked },
                      checked ? "Flick will open when you log in." : "Flick won't open at login.",
                    )
                  }
                  aria-label="Open at login"
                />
              </SettingRow>
            </SettingSection>
          ) : null}

          <SettingSection
            title="Appearance"
            subtitle="The same six themes as GitHub. Flick can follow your system's light and dark setting."
          >
            <SettingRow
              label="Theme mode"
              description="Sync with system switches between your light and dark theme automatically."
              error={fieldErrors["ui.theme"]}
            >
              <Select
                value={theme.mode}
                onValueChange={(value) => {
                  theme.setMode(value as ThemeMode);
                  void patchOne("ui.theme", value, "Theme mode saved.");
                }}
                label="Theme mode"
                items={themeModeItems}
              />
            </SettingRow>
            <ThemePicker
              label="Light theme"
              description="Used in light mode, and during the day when synced with the system."
              themes={lightThemes}
              value={theme.light}
              active={theme.resolved === theme.light}
              error={fieldErrors["ui.light_theme"]}
              onChange={(value) => {
                theme.setLightTheme(value as LightThemeId);
                void patchOne("ui.light_theme", value, "Light theme saved.");
              }}
            />
            <ThemePicker
              label="Dark theme"
              description="Used in dark mode, and at night when synced with the system."
              themes={darkThemes}
              value={theme.dark}
              active={theme.resolved === theme.dark}
              error={fieldErrors["ui.dark_theme"]}
              onChange={(value) => {
                theme.setDarkTheme(value as DarkThemeId);
                void patchOne("ui.dark_theme", value, "Dark theme saved.");
              }}
            />
          </SettingSection>

          <SettingSection title="Camera" subtitle="Local camera behavior for the current room.">
            <SettingRow label="Camera" description={cameras.data?.[0]?.device_ref ?? "MacBook Camera"}>
              <DevicePill name={cameras.data?.[0]?.name ?? "MacBook Camera"} domain="camera" detail="local" />
            </SettingRow>
            <SettingRow
              label="Mirror preview"
              description="Useful for laptop cameras."
              error={fieldErrors["camera.mirror"]}
            >
              <Switch
                checked={boolValue(settings, "camera.mirror", true)}
                onCheckedChange={(checked) => void patchOne("camera.mirror", checked, "Camera mirror saved.")}
                aria-label="Mirror preview"
              />
            </SettingRow>
            <SettingRow
              label="Active FPS"
              description="Target frame rate while watching."
              error={fieldErrors["camera.active_fps"]}
            >
              <Input
                aria-label="Active FPS"
                type="number"
                min={10}
                max={60}
                defaultValue={numberValue(settings, "camera.active_fps", 30)}
                onBlur={(event) =>
                  void patchOne("camera.active_fps", Number(event.target.value), "Active FPS saved.")
                }
              />
            </SettingRow>
            <SettingRow
              label="Max hands"
              description="Two is required for the stop gesture."
              error={fieldErrors["camera.max_hands"]}
            >
              <Input
                aria-label="Max hands"
                type="number"
                min={1}
                max={2}
                defaultValue={numberValue(settings, "camera.max_hands", 2)}
                onBlur={(event) =>
                  void patchOne("camera.max_hands", Number(event.target.value), "Max hands saved.")
                }
              />
            </SettingRow>
          </SettingSection>

          <SettingSection
            title="Recognition sensitivity"
            subtitle="How readily Flick accepts a gesture, and the gestures that arm or pause it."
          >
            <SettingRow
              label="Sensitivity preset"
              description="Low/Normal/High scale thresholds and vote counts."
              error={fieldErrors["detection.sensitivity"]}
            >
              <Select
                value={stringValue(settings, "detection.sensitivity", "normal")}
                onValueChange={(value) => void patchOne("detection.sensitivity", value, "Sensitivity saved.")}
                label="Sensitivity"
                items={sensitivityItems}
              />
            </SettingRow>
            <SettingRow
              label="Vote N of M"
              description="Default is 6 of 8 frames."
              error={fieldErrors["detection.vote"]}
            >
              <div className="inline-number-pair">
                <Input
                  aria-label="Vote frames needed"
                  type="number"
                  min={1}
                  max={10}
                  defaultValue={vote.n}
                  onBlur={(event) =>
                    void patchOne(
                      "detection.vote",
                      { ...vote, n: Number(event.target.value) },
                      "Vote window saved.",
                    )
                  }
                />
                <Input
                  aria-label="Vote window frames"
                  type="number"
                  min={1}
                  max={12}
                  defaultValue={vote.m}
                  onBlur={(event) =>
                    void patchOne(
                      "detection.vote",
                      { ...vote, m: Number(event.target.value) },
                      "Vote window saved.",
                    )
                  }
                />
              </div>
            </SettingRow>
            <SettingRow
              label="Minimum hand size"
              description="3%–18% of frame height."
              error={fieldErrors["detection.min_hand_size"]}
            >
              <Input
                aria-label="Minimum hand size"
                type="number"
                min={0.03}
                max={0.18}
                step={0.01}
                defaultValue={numberValue(settings, "detection.min_hand_size", 0.06)}
                onBlur={(event) =>
                  void patchOne("detection.min_hand_size", Number(event.target.value), "Min hand size saved.")
                }
              />
            </SettingRow>
            <SettingRow
              label="Battery saver"
              description="Caps active mode at 15 fps on battery."
              error={fieldErrors["detection.battery_saver"]}
            >
              <Switch
                checked={boolValue(settings, "detection.battery_saver")}
                onCheckedChange={(checked) =>
                  void patchOne("detection.battery_saver", checked, "Battery saver saved.")
                }
                aria-label="Battery saver"
              />
            </SettingRow>
            <SettingRow
              label="Arm mode"
              description="Open palm arms Flick for a short window."
              error={fieldErrors["detection.arm"]}
            >
              <Switch
                checked={arm.enabled}
                onCheckedChange={(checked) =>
                  void patchOne("detection.arm", { ...arm, enabled: checked }, "Arm mode saved.")
                }
                aria-label="Arm mode"
              />
            </SettingRow>
            <SettingRow
              label="Pause gesture"
              description="Hold open palm for 1.5 seconds to pause."
              error={fieldErrors["detection.pause_gesture"]}
            >
              <Switch
                checked={pauseGesture.enabled}
                onCheckedChange={(checked) =>
                  void patchOne(
                    "detection.pause_gesture",
                    { ...pauseGesture, enabled: checked },
                    "Pause gesture saved.",
                  )
                }
                aria-label="Pause gesture"
              />
            </SettingRow>
            <SettingRow
              label="Two-hand stop axis"
              description="Any, vertical, or horizontal separation."
              error={fieldErrors["gestures.two_hand_separate.axis"]}
            >
              <Select
                value={stringValue(settings, "gestures.two_hand_separate.axis", "any")}
                onValueChange={(value) =>
                  void patchOne("gestures.two_hand_separate.axis", value, "Two-hand axis saved.")
                }
                label="Two-hand axis"
                items={[
                  { value: "any", label: "Any" },
                  { value: "vertical", label: "Vertical" },
                  { value: "horizontal", label: "Horizontal" },
                ]}
              />
            </SettingRow>
          </SettingSection>

          <SettingSection
            title="Safety"
            subtitle="Sensitive devices stay blocked unless you opt in and confirm."
          >
            <SettingRow
              label="Allow sensitive devices"
              description="Locks, alarms, garage doors and shells require confirmation."
              error={fieldErrors["safety.allow_sensitive"]}
            >
              <Switch
                checked={boolValue(settings, "safety.allow_sensitive")}
                onCheckedChange={(checked) =>
                  void patchOne("safety.allow_sensitive", checked, "Safety preference saved.")
                }
                aria-label="Allow sensitive devices"
              />
            </SettingRow>
            <SettingRow
              label="Confirmation gesture"
              description="Default is thumbs up within 3 seconds."
              error={fieldErrors["safety.confirm_gesture"]}
            >
              <Select
                value={stringValue(settings, "safety.confirm_gesture", "builtin.thumb_up")}
                onValueChange={(value) =>
                  void patchOne("safety.confirm_gesture", value, "Confirmation gesture saved.")
                }
                label="Confirmation gesture"
                items={[
                  { value: "builtin.thumb_up", label: "Thumbs up" },
                  { value: "builtin.open_palm", label: "Open palm" },
                ]}
              />
            </SettingRow>
          </SettingSection>

          <SettingSection
            title="Quiet hours"
            subtitle="Pause sounds and optional feedback while household members sleep."
          >
            <SettingRow
              label="Quiet hours enabled"
              description="When off, feedback plays at any hour."
              error={fieldErrors.quiet_hours}
            >
              <Switch
                checked={Boolean(settings.quiet_hours)}
                onCheckedChange={(checked) =>
                  void patchOne("quiet_hours", checked ? quietHours : null, "Quiet hours saved.")
                }
                aria-label="Quiet hours enabled"
              />
            </SettingRow>
            <SettingRow
              label="From / to"
              description="Use 24-hour local time."
              error={fieldErrors.quiet_hours}
            >
              <div className="inline-number-pair">
                <Input
                  aria-label="Quiet hours start"
                  type="time"
                  defaultValue={quietHours.from}
                  onBlur={(event) =>
                    void patchOne(
                      "quiet_hours",
                      { ...quietHours, from: event.target.value },
                      "Quiet hours saved.",
                    )
                  }
                />
                <Input
                  aria-label="Quiet hours end"
                  type="time"
                  defaultValue={quietHours.to}
                  onBlur={(event) =>
                    void patchOne(
                      "quiet_hours",
                      { ...quietHours, to: event.target.value },
                      "Quiet hours saved.",
                    )
                  }
                />
              </div>
            </SettingRow>
          </SettingSection>

          <SettingSection
            title="Pointing"
            subtitle="Point at a taught device to select it before you gesture."
          >
            <SettingRow
              label="Enable pointing"
              description="Effective when a device is taught."
              error={fieldErrors["targeting.enabled"]}
            >
              <Switch
                checked={boolValue(settings, "targeting.enabled", true)}
                onCheckedChange={(checked) => void patchOne("targeting.enabled", checked, "Pointing saved.")}
                aria-label="Enable pointing"
              />
            </SettingRow>
            <SettingRow
              label="Aim tolerance"
              description="5°–15°. Wider is easier but less precise."
              error={fieldErrors["targeting.tolerance_deg"]}
            >
              <Input
                aria-label="Aim tolerance"
                type="number"
                min={5}
                max={15}
                defaultValue={numberValue(settings, "targeting.tolerance_deg", 10)}
                onBlur={(event) =>
                  void patchOne("targeting.tolerance_deg", Number(event.target.value), "Aim tolerance saved.")
                }
              />
            </SettingRow>
            <SettingRow
              label="Dwell"
              description="Default 500 ms to select a device."
              error={fieldErrors["targeting.dwell_ms"]}
            >
              <Input
                aria-label="Dwell duration"
                type="number"
                min={250}
                max={1500}
                step={50}
                defaultValue={numberValue(settings, "targeting.dwell_ms", 500)}
                onBlur={(event) =>
                  void patchOne("targeting.dwell_ms", Number(event.target.value), "Dwell saved.")
                }
              />
            </SettingRow>
            <SettingRow
              label="Selection window"
              description="Default 4 seconds, refreshed after a verb."
              error={fieldErrors["targeting.window_ms"]}
            >
              <Input
                aria-label="Selection window"
                type="number"
                min={1000}
                max={10000}
                step={250}
                defaultValue={numberValue(settings, "targeting.window_ms", 4000)}
                onBlur={(event) =>
                  void patchOne("targeting.window_ms", Number(event.target.value), "Selection window saved.")
                }
              />
            </SettingRow>
            <SettingRow
              label="Ray model"
              description="Eye-rooted when face is visible, finger-only fallback."
              error={fieldErrors["targeting.ray_model"]}
            >
              <Select
                value={stringValue(settings, "targeting.ray_model", "auto")}
                onValueChange={(value) => void patchOne("targeting.ray_model", value, "Ray model saved.")}
                label="Ray model"
                items={rayModelItems}
              />
            </SettingRow>
            <SettingRow
              label="Dominant eye"
              description="Used by the eye-rooted ray model."
              error={fieldErrors["targeting.dominant_eye"]}
            >
              <Select
                value={stringValue(settings, "targeting.dominant_eye", "center")}
                onValueChange={(value) =>
                  void patchOne("targeting.dominant_eye", value, "Dominant eye saved.")
                }
                label="Dominant eye"
                items={dominantEyeItems}
              />
            </SettingRow>
            <SettingRow
              label="Show ray in preview"
              description="Makes pointing feedback visible."
              error={fieldErrors["camera.show_ray"]}
            >
              <Switch
                checked={boolValue(settings, "camera.show_ray", true)}
                onCheckedChange={(checked) => void patchOne("camera.show_ray", checked, "Preview ray saved.")}
                aria-label="Show ray in preview"
              />
            </SettingRow>
            <div className="settings-callout">
              {place ? (
                <>
                  <strong>
                    {place.name} · {place.state === "ok" ? "Aligned" : place.state.replace(/_/g, " ")}
                  </strong>
                  <span>
                    {taughtNames.length
                      ? `${taughtNames.join(", ")} ${taughtNames.length === 1 ? "is" : "are"} taught here. Re-align if the camera moved.`
                      : "No devices taught here yet."}
                  </span>
                  <Link className="ui-button ui-button-secondary ui-button-sm" to="/devices/realign">
                    Re-align now
                  </Link>
                </>
              ) : (
                <>
                  <strong>No place yet</strong>
                  <span>Teach a device to create a place for this camera view.</span>
                  <Link className="ui-button ui-button-secondary ui-button-sm" to="/devices/teach">
                    Teach a device
                  </Link>
                </>
              )}
            </div>
          </SettingSection>

          <SettingSection title="Feedback" subtitle="HUD, sound and high-signal confirmation controls.">
            <SettingRow
              label="HUD"
              description="Main result feedback in the desktop overlay."
              error={fieldErrors["feedback.hud"]}
            >
              <Switch
                checked={hud.enabled}
                onCheckedChange={(checked) =>
                  void patchOne("feedback.hud", { ...hud, enabled: checked }, "HUD saved.")
                }
                aria-label="HUD enabled"
              />
            </SettingRow>
            <SettingRow
              label="HUD position"
              description="Desktop HUD only; mobile uses in-window feedback."
              error={fieldErrors["feedback.hud"]}
            >
              <Select
                value={hud.position}
                onValueChange={(value) =>
                  void patchOne("feedback.hud", { ...hud, position: value }, "HUD position saved.")
                }
                label="HUD position"
                items={positionItems}
              />
            </SettingRow>
            <SettingRow
              label="HUD duration"
              description="Milliseconds after the last update."
              error={fieldErrors["feedback.hud"]}
            >
              <Input
                aria-label="HUD duration"
                type="number"
                min={500}
                max={5000}
                step={100}
                defaultValue={hud.duration_ms}
                onBlur={(event) =>
                  void patchOne(
                    "feedback.hud",
                    { ...hud, duration_ms: Number(event.target.value) },
                    "HUD duration saved.",
                  )
                }
              />
            </SettingRow>
            <SettingRow
              label="Sounds"
              description="Earcons are short and optional."
              error={fieldErrors["feedback.sounds"]}
            >
              <Switch
                checked={sounds.enabled}
                onCheckedChange={(checked) =>
                  void patchOne("feedback.sounds", { ...sounds, enabled: checked }, "Sounds saved.")
                }
                aria-label="Sounds enabled"
              />
            </SettingRow>
            <SettingRow label="Sound volume" description="0 to 1." error={fieldErrors["feedback.sounds"]}>
              <Input
                aria-label="Sound volume"
                type="number"
                min={0}
                max={1}
                step={0.05}
                defaultValue={sounds.volume}
                onBlur={(event) =>
                  void patchOne(
                    "feedback.sounds",
                    { ...sounds, volume: Number(event.target.value) },
                    "Sound volume saved.",
                  )
                }
              />
            </SettingRow>
          </SettingSection>

          <SettingSection
            title="Updates"
            subtitle="Signed app, model and gesture catalog updates; privacy-preserving rollout bucketing is local."
          >
            <SettingRow
              label="Channel"
              description="Stable by default; beta/nightly ask for confirmation in shell."
              error={fieldErrors["updates.channel"]}
            >
              <Select
                value={stringValue(settings, "updates.channel", "stable")}
                onValueChange={(value) => void patchOne("updates.channel", value, "Update channel saved.")}
                label="Update channel"
                items={channelItems}
              />
            </SettingRow>
            <SettingRow
              label="Auto-install app updates"
              description="Installs on quit or restart to update."
              error={fieldErrors["updates.auto_install_app"]}
            >
              <Switch
                checked={boolValue(settings, "updates.auto_install_app", true)}
                onCheckedChange={(checked) =>
                  void patchOne("updates.auto_install_app", checked, "Auto-install app saved.")
                }
                aria-label="Auto-install app updates"
              />
            </SettingRow>
            <SettingRow
              label="Install when idle"
              description="Never during confirmation, recording, teach or dial."
              error={fieldErrors["updates.install_when_idle"]}
            >
              <Switch
                checked={boolValue(settings, "updates.install_when_idle", true)}
                onCheckedChange={(checked) =>
                  void patchOne("updates.install_when_idle", checked, "Idle install saved.")
                }
                aria-label="Install when idle"
              />
            </SettingRow>
            <SettingRow
              label="Auto-update models and catalog"
              description="Runs self-tests and shadow checks before activation."
              error={fieldErrors["updates.auto_models"]}
            >
              <Switch
                checked={boolValue(settings, "updates.auto_models", true)}
                onCheckedChange={(checked) =>
                  void patchOne("updates.auto_models", checked, "Model auto-update saved.")
                }
                aria-label="Auto-update models and catalog"
              />
            </SettingRow>
            <div className="updates-card">
              <Badge tone={updates.data?.app.state === "ready" ? "warning" : "success"}>
                {updates.data?.app.state ?? "checking"}
              </Badge>
              <strong>
                App {updates.data?.app.current ?? "0.1.0"}
                {updates.data?.app.available ? ` → ${updates.data.app.available}` : ""}
              </strong>
              <span>
                Last check{" "}
                {updates.data?.last_check_at
                  ? new Date(updates.data.last_check_at).toLocaleString()
                  : "never"}
              </span>
              <div className="settings-action-row">
                <Button
                  variant="secondary"
                  onClick={() => void checkUpdates.mutateAsync()}
                  loading={checkUpdates.isPending}
                >
                  Check now
                </Button>
                <Button
                  variant="primary"
                  onClick={() => void installUpdate.mutateAsync({ kind: "app" })}
                  loading={installUpdate.isPending}
                >
                  Restart to update
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => void rollbackUpdate.mutateAsync({ kind: "app" })}
                  loading={rollbackUpdate.isPending}
                >
                  Roll back
                </Button>
              </div>
            </div>
            <div className="model-list">
              {(models.data ?? []).map((pack) => (
                <div key={`${pack.id}-${pack.version}`}>
                  <span>{pack.kind}</span>
                  <strong>
                    {pack.id} · {pack.version}
                  </strong>
                  <Badge
                    tone={
                      pack.state === "active" ? "success" : pack.state === "removed" ? "neutral" : "warning"
                    }
                  >
                    {pack.state}
                  </Badge>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void rollbackUpdate.mutateAsync({ kind: "pack", id: pack.id })}
                  >
                    Roll back model
                  </Button>
                </div>
              ))}
            </div>
            <button className="link-button" type="button">
              Import offline update…
            </button>
            <a
              className="settings-link"
              href="https://updates.flick.app/changelog"
              target="_blank"
              rel="noreferrer"
            >
              Open changelog
            </a>
          </SettingSection>

          <HaConnectionSection
            onNotice={setNotice}
            onDisconnect={() =>
              void patchOne("ha.disconnect_requested_at", new Date().toISOString(), "Disconnect requested.")
            }
          />

          <SettingSection
            title="Privacy"
            subtitle="Flick stores geometry, settings and local logs — no camera images."
          >
            <SettingRow
              label="Pause when HA offline"
              description="Camera stops when HA is unreachable for more than 60 seconds."
              error={fieldErrors["privacy.pause_when_ha_offline"]}
            >
              <Switch
                checked={boolValue(settings, "privacy.pause_when_ha_offline", true)}
                onCheckedChange={(checked) =>
                  void patchOne("privacy.pause_when_ha_offline", checked, "HA offline privacy saved.")
                }
                aria-label="Pause when HA offline"
              />
            </SettingRow>
            <SettingRow
              label="Pause on screen lock"
              description="Stops watching when this Mac locks."
              error={fieldErrors["privacy.pause_on_screen_lock"]}
            >
              <Switch
                checked={boolValue(settings, "privacy.pause_on_screen_lock")}
                onCheckedChange={(checked) =>
                  void patchOne("privacy.pause_on_screen_lock", checked, "Screen lock privacy saved.")
                }
                aria-label="Pause on screen lock"
              />
            </SettingRow>
            <SettingRow
              label="Keep Mac awake"
              description="Useful for stationary Macs."
              error={fieldErrors["privacy.keep_awake"]}
            >
              <Switch
                checked={boolValue(settings, "privacy.keep_awake")}
                onCheckedChange={(checked) =>
                  void patchOne("privacy.keep_awake", checked, "Awake preference saved.")
                }
                aria-label="Keep Mac awake"
              />
            </SettingRow>
            <SettingRow
              label="Crash reports"
              description="Opt-in; separate from update checks."
              error={fieldErrors["privacy.crash_reports"]}
            >
              <Switch
                checked={boolValue(settings, "privacy.crash_reports")}
                onCheckedChange={(checked) =>
                  void patchOne("privacy.crash_reports", checked, "Crash report preference saved.")
                }
                aria-label="Crash reports"
              />
            </SettingRow>
            <div className="privacy-explainer">
              <strong>What Flick stores</strong>
              <span>
                Gesture samples are hand landmarks. Taught devices are rays and geometry. Scene signatures are
                embeddings. No camera images are stored.
              </span>
            </div>
            <div className="settings-action-row">
              <Button
                variant="danger"
                onClick={() =>
                  void patchOne(
                    "privacy.delete_gesture_data_requested_at",
                    new Date().toISOString(),
                    "Gesture data deletion requested.",
                  )
                }
              >
                Delete all gesture data
              </Button>
              <Button
                variant="danger"
                onClick={() =>
                  void patchOne(
                    "privacy.delete_taught_devices_requested_at",
                    new Date().toISOString(),
                    "Taught device deletion requested.",
                  )
                }
              >
                Delete all taught devices
              </Button>
            </div>
          </SettingSection>

          <SettingSection title="About" subtitle="Local engine and installation identifiers.">
            <div className="about-grid">
              <div>
                <span>Engine</span>
                <strong>{engineStatus.data?.version ?? "0.1.0-dev"}</strong>
              </div>
              <div>
                <span>Mode</span>
                <strong>{engineStatus.data?.mode ?? "watching"}</strong>
              </div>
              <div>
                <span>Install ID</span>
                <strong>{stringValue(settings, "updates.install_id", "local-only")}</strong>
              </div>
              <div>
                <span>Update index</span>
                <strong>{updates.data?.index_version ?? "—"}</strong>
              </div>
            </div>
          </SettingSection>
        </div>
      </div>
    </section>
  );
}

function ThemePicker({
  label,
  description,
  themes,
  value,
  active,
  error,
  onChange,
}: {
  label: string;
  description: string;
  themes: { id: string; label: string }[];
  value: string;
  active: boolean;
  error?: string;
  onChange: (value: string) => void;
}) {
  return (
    <fieldset className="theme-picker">
      <legend>
        <strong>{label}</strong>
        {active ? <Badge tone="accent">Active</Badge> : null}
      </legend>
      <p>{description}</p>
      {error ? <em role="alert">{error}</em> : null}
      <div className="theme-picker-options">
        {themes.map((option) => (
          <label key={option.id} className="theme-option" data-selected={option.id === value}>
            <input
              type="radio"
              name={label}
              value={option.id}
              checked={option.id === value}
              onChange={() => onChange(option.id)}
            />
            <span className="theme-preview" data-theme={option.id} aria-hidden="true">
              <i className="theme-preview-sidebar" />
              <i className="theme-preview-body">
                <b />
                <b />
                <b />
              </i>
            </span>
            <span className="theme-option-label">{option.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}
