import { useState } from "react";
import {
  useCameras,
  useCheckUpdates,
  useHaConnect,
  useHaDiscover,
  useHaStatus,
  useInstallUpdate,
  useModels,
  usePatchSettings,
  useRollbackUpdate,
  useSettings,
  useStatus,
  useUpdates,
} from "../../api/hooks";
import type { SettingsMap, SettingsPatch, ThemeMode } from "../../api/types";
import { DevicePill } from "../../components/domain";
import { Badge, Button, GlassPanel, Input, Select, Switch } from "../../components/ui";
import { useTheme } from "../../theme";
import "./styles.css";

type FieldErrors = Record<string, string>;

const themeItems = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
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

function extractErrors(error: unknown) {
  const problem = (error as { problem?: { detail?: string; errors?: FieldErrors } }).problem;
  return {
    detail:
      problem?.detail ?? (error instanceof Error ? error.message : "Flick could not save that setting."),
    errors: problem?.errors ?? {},
  };
}

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

function SettingSection({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: React.ReactNode;
}) {
  return (
    <GlassPanel className="settings-section">
      <div className="settings-section-heading">
        <h2>{title}</h2>
        <p>{subtitle}</p>
      </div>
      <div className="settings-section-body">{children}</div>
    </GlassPanel>
  );
}

function SettingRow({
  label,
  description,
  error,
  children,
}: {
  label: string;
  description?: string;
  error?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        {description ? <span>{description}</span> : null}
        {error ? <em role="alert">{error}</em> : null}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
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
  const cameras = useCameras();
  const haStatus = useHaStatus();
  const haDiscover = useHaDiscover();
  const haConnect = useHaConnect();
  const { mode, highContrast, setMode, setHighContrast } = useTheme();
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [notice, setNotice] = useState<string | null>(null);
  const [haUrl, setHaUrl] = useState("http://homeassistant.local:8123");
  const [haToken, setHaToken] = useState("demo-valid-token");
  const [haError, setHaError] = useState<string | null>(null);
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

  async function reconnectHa() {
    try {
      setHaError(null);
      await haConnect.mutateAsync({ base_url: haUrl, token: haToken });
      setNotice("Home Assistant re-authenticated.");
    } catch (error) {
      setHaError(extractErrors(error).detail);
    }
  }

  return (
    <section className="settings-route" aria-labelledby="screen-title">
      <div className="settings-hero">
        <p className="route-path">Settings</p>
        <h1 id="screen-title">Tune the local remote.</h1>
        <p>
          Each control saves through PATCH /settings and shows validation inline when the engine rejects a
          value.
        </p>
      </div>

      {notice ? (
        <p className="settings-notice" role="status">
          {notice}
        </p>
      ) : null}

      <div className="settings-layout">
        <SettingSection
          title="Appearance"
          subtitle="Dark-first Liquid Glass, with light and high contrast treated as first-class."
        >
          <SettingRow
            label="Theme"
            description="Applies immediately and saves locally."
            error={fieldErrors["ui.theme"]}
          >
            <Select
              value={mode}
              onValueChange={(value) => {
                setMode(value as ThemeMode);
                void patchOne("ui.theme", value, "Theme saved.");
              }}
              label="Theme"
              items={themeItems}
            />
          </SettingRow>
          <SettingRow
            label="High contrast"
            description="Uses opaque surfaces and high-contrast focus treatment."
            error={fieldErrors["ui.high_contrast"]}
          >
            <Switch
              checked={highContrast}
              onCheckedChange={(checked) => {
                setHighContrast(checked);
                void patchOne("ui.high_contrast", checked, "High contrast saved.");
              }}
              aria-label="High contrast"
            />
          </SettingRow>
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
          subtitle="Preset plus the Phase 1 hard controls from the trigger FSM."
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
            description="Stores null when off."
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
          <SettingRow label="From / to" description="Use 24-hour local time." error={fieldErrors.quiet_hours}>
            <div className="inline-number-pair">
              <Input
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
          subtitle="Point-to-select tuned for taught devices and the owner's fan scenario."
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
              onValueChange={(value) => void patchOne("targeting.dominant_eye", value, "Dominant eye saved.")}
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
            <strong>Bedroom desk · OK</strong>
            <span>Ventilador dormitorio is taught. Re-align if the camera moved.</span>
            <Button variant="ghost" size="sm">
              Re-align now
            </Button>
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
              {updates.data?.last_check_at ? new Date(updates.data.last_check_at).toLocaleString() : "never"}
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

        <SettingSection
          title="Home Assistant connection"
          subtitle="Direct WebSocket calls only; use a non-admin HA user for Flick."
        >
          <div className="ha-status-card">
            <Badge tone={haStatus.data?.state === "ready" ? "success" : "warning"}>
              {haStatus.data?.state ?? "unknown"}
            </Badge>
            <strong>{haStatus.data?.instance?.name ?? "Home"}</strong>
            <span>{haStatus.data?.ha_version ?? "2026.9"}</span>
          </div>
          <div className="discovered-ha-list">
            {(haDiscover.data ?? []).map((instance) => (
              <button key={instance.uuid} type="button" onClick={() => setHaUrl(instance.base_url)}>
                {instance.name} · {instance.base_url}
              </button>
            ))}
          </div>
          <SettingRow
            label="Instance URL"
            description="Used for re-authentication."
            error={fieldErrors["ha.url"]}
          >
            <Input value={haUrl} onChange={(event) => setHaUrl(event.target.value)} />
          </SettingRow>
          <SettingRow
            label="Long-lived token"
            description="Stored by the engine keychain backend, not in settings."
            error={haError ?? undefined}
          >
            <Input type="password" value={haToken} onChange={(event) => setHaToken(event.target.value)} />
          </SettingRow>
          <div className="settings-action-row">
            <Button variant="primary" onClick={() => void reconnectHa()} loading={haConnect.isPending}>
              Re-authenticate
            </Button>
            <Button
              variant="ghost"
              onClick={() =>
                void patchOne("ha.disconnect_requested_at", new Date().toISOString(), "Disconnect requested.")
              }
            >
              Disconnect
            </Button>
          </div>
        </SettingSection>

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
    </section>
  );
}
