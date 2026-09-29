import { type FormEvent, useState } from "react";
import { useHaConnect, useHaDiscover, useHaStatus, useHaUpdate } from "../../api/hooks";
import type { HaConnectionUpdate, HaStatus } from "../../api/types";
import { Badge, Button, Input } from "../../components/ui";
import { isTauri, openLocationPrivacySettings } from "../../platform/tauri";
import { HaClientCertificateRow } from "./HaClientCertificateRow";
import { extractErrors, type FieldErrors, SettingRow, SettingSection } from "./SettingParts";
import { useLocationPermission, useRequestLocation, useWifiSsid } from "./useHaNetwork";

type Tone = "neutral" | "success" | "warning" | "danger";

const stateBadges: Record<HaStatus["state"], { label: string; tone: Tone }> = {
  ready: { label: "Connected", tone: "success" },
  connecting: { label: "Connecting…", tone: "warning" },
  disconnected: { label: "Disconnected", tone: "neutral" },
  auth_failed: { label: "Token rejected", tone: "danger" },
};

const locationLabels: Record<string, { label: string; tone: Tone }> = {
  authorized: { label: "Allowed", tone: "success" },
  denied: { label: "Denied", tone: "danger" },
  restricted: { label: "Restricted", tone: "danger" },
  not_determined: { label: "Not asked yet", tone: "warning" },
};

const sameList = (a: string[], b: string[]) => a.length === b.length && a.every((item, i) => item === b[i]);

export function HaConnectionSection({
  onNotice,
  onDisconnect,
}: {
  onNotice: (message: string | null) => void;
  onDisconnect: () => void;
}) {
  const haStatus = useHaStatus();
  const haDiscover = useHaDiscover();
  const haConnect = useHaConnect();
  const haUpdate = useHaUpdate();
  const tauri = isTauri();
  const location = useLocationPermission();
  const requestLocation = useRequestLocation();
  const permission = location.data;
  const wifi = useWifiSsid(permission === "authorized");

  const [remoteDraft, setRemoteDraft] = useState<string | null>(null);
  const [homeDraft, setHomeDraft] = useState<string | null>(null);
  const [ssidsDraft, setSsidsDraft] = useState<string[] | null>(null);
  const [ssidInput, setSsidInput] = useState("");
  const [token, setToken] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [tokenError, setTokenError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});

  const status = haStatus.data;
  const instance = status?.instance;
  const savedRemote = instance?.base_url ?? "";
  const savedHome = instance?.internal_url ?? "";
  const savedSsids = instance?.trusted_ssids ?? [];
  const remote = remoteDraft ?? savedRemote;
  const home = homeDraft ?? savedHome;
  const ssids = ssidsDraft ?? savedSsids;
  const currentSsid = (tauri ? wifi.data : null) ?? status?.network_ssid ?? null;

  const changes: HaConnectionUpdate = {};
  if (remote.trim() !== savedRemote) changes.base_url = remote.trim();
  if (home.trim() !== savedHome) changes.internal_url = home.trim();
  if (!sameList(ssids, savedSsids)) changes.trusted_ssids = ssids;
  const dirty = Object.keys(changes).length > 0;

  const badge = stateBadges[status?.state ?? "disconnected"];
  const discovered = (haDiscover.data ?? []).filter((item) => item.base_url !== home.trim());

  function addSsid(value: string) {
    const ssid = value.trim();
    if (!ssid || ssids.includes(ssid)) return;
    setSsidsDraft([...ssids, ssid]);
    setSsidInput("");
  }

  function removeSsid(ssid: string) {
    setSsidsDraft(ssids.filter((item) => item !== ssid));
  }

  async function saveConnection() {
    setSaveError(null);
    setFieldErrors({});
    try {
      await haUpdate.mutateAsync(changes);
      setRemoteDraft(null);
      setHomeDraft(null);
      setSsidsDraft(null);
      onNotice("Home Assistant connection saved.");
    } catch (error) {
      const { detail, errors } = extractErrors(error);
      setSaveError(detail);
      setFieldErrors(errors);
    }
  }

  async function reauthenticate(event: FormEvent) {
    event.preventDefault();
    setTokenError(null);
    try {
      await haConnect.mutateAsync({ base_url: remote.trim(), token: token.trim() });
      setToken("");
      onNotice("Home Assistant re-authenticated.");
    } catch (error) {
      setTokenError(extractErrors(error).detail);
    }
  }

  const routeLabel = status?.connection === "home" ? "Home URL" : "Remote URL";
  const locationBadge = permission ? locationLabels[permission] : undefined;

  return (
    <SettingSection
      title="Home Assistant connection"
      subtitle="How Flick reaches Home Assistant, at home and away."
    >
      <div className="ha-status-card">
        <Badge tone={badge.tone}>{badge.label}</Badge>
        <div className="ha-status-main">
          <strong>{instance?.name ?? "No instance connected"}</strong>
          {status?.state === "ready" && status.active_url ? (
            <span className="ha-route">
              {routeLabel} · <code>{status.active_url}</code>
              {status.network_ssid ? ` · on Wi-Fi “${status.network_ssid}”` : null}
            </span>
          ) : null}
          {status?.state !== "ready" && status?.last_error ? <em role="alert">{status.last_error}</em> : null}
        </div>
        {status?.ha_version ? <span>{status.ha_version}</span> : null}
      </div>

      <SettingRow
        label="Remote URL"
        description="Used away from home, e.g. your Nabu Casa or HTTPS address."
        error={fieldErrors.base_url}
      >
        <Input
          aria-label="Remote URL"
          className="ha-wide-input"
          type="url"
          inputMode="url"
          spellCheck={false}
          placeholder="https://example.ui.nabu.casa"
          value={remote}
          onChange={(event) => setRemoteDraft(event.target.value)}
        />
      </SettingRow>

      <SettingRow
        label="Home URL"
        description={
          <>
            Optional. Used on your home Wi-Fi.
            {discovered.length > 0 ? (
              <span className="ha-discovered">
                Found on this network:{" "}
                {discovered.map((item, index) => (
                  <span key={item.uuid ?? item.base_url}>
                    {index > 0 ? ", " : null}
                    <button type="button" className="link-button" onClick={() => setHomeDraft(item.base_url)}>
                      {item.base_url}
                    </button>
                  </span>
                ))}
              </span>
            ) : null}
          </>
        }
        error={fieldErrors.internal_url}
      >
        <Input
          aria-label="Home URL"
          className="ha-wide-input"
          type="url"
          inputMode="url"
          spellCheck={false}
          placeholder="http://homeassistant.local:8123"
          value={home}
          onChange={(event) => setHomeDraft(event.target.value)}
        />
      </SettingRow>

      <SettingRow
        label="Home Wi-Fi networks"
        description="On these networks Flick uses the Home URL; anywhere else it uses the Remote URL."
        error={fieldErrors.trusted_ssids}
      >
        <div className="ssid-editor">
          {ssids.length > 0 ? (
            <ul className="ssid-chips" aria-label="Home Wi-Fi networks">
              {ssids.map((ssid) => (
                <li key={ssid} className="ssid-chip">
                  <span>{ssid}</span>
                  <button type="button" aria-label={`Remove ${ssid}`} onClick={() => removeSsid(ssid)}>
                    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                      <path
                        d="M4 4l8 8M12 4l-8 8"
                        stroke="currentColor"
                        strokeWidth="1.5"
                        strokeLinecap="round"
                      />
                    </svg>
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          <form
            className="ssid-add"
            onSubmit={(event) => {
              event.preventDefault();
              addSsid(ssidInput);
            }}
          >
            <Input
              aria-label="Wi-Fi network name"
              placeholder="Network name"
              spellCheck={false}
              value={ssidInput}
              onChange={(event) => setSsidInput(event.target.value)}
            />
            <Button type="submit" disabled={!ssidInput.trim()}>
              Add
            </Button>
          </form>
          {currentSsid && !ssids.includes(currentSsid) ? (
            <button type="button" className="link-button ssid-current" onClick={() => addSsid(currentSsid)}>
              Add current network “{currentSsid}”
            </button>
          ) : null}
        </div>
      </SettingRow>

      {tauri && permission && permission !== "unsupported" ? (
        <SettingRow
          label="Wi-Fi name access"
          description="macOS only shares the Wi-Fi name with apps that have Location access. Without it, Flick looks for Home Assistant on your local network."
        >
          <div className="setting-control-stack">
            {locationBadge ? <Badge tone={locationBadge.tone}>{locationBadge.label}</Badge> : null}
            {permission === "not_determined" ? (
              <Button size="sm" loading={requestLocation.isPending} onClick={() => requestLocation.mutate()}>
                Allow
              </Button>
            ) : null}
            {permission === "denied" || permission === "restricted" ? (
              <Button size="sm" onClick={() => void openLocationPrivacySettings()}>
                Open System Settings
              </Button>
            ) : null}
          </div>
        </SettingRow>
      ) : null}

      <div className="settings-action-row ha-save-row">
        <Button
          variant="primary"
          disabled={!dirty}
          loading={haUpdate.isPending}
          onClick={() => void saveConnection()}
        >
          Save connection
        </Button>
        {dirty ? (
          <Button
            variant="ghost"
            onClick={() => {
              setRemoteDraft(null);
              setHomeDraft(null);
              setSsidsDraft(null);
              setSaveError(null);
              setFieldErrors({});
            }}
          >
            Discard changes
          </Button>
        ) : null}
        {saveError ? (
          <em role="alert" className="settings-inline-error">
            {saveError}
          </em>
        ) : null}
      </div>

      <HaClientCertificateRow onNotice={onNotice} />

      <form onSubmit={(event) => void reauthenticate(event)}>
        <SettingRow
          label="Long-lived token"
          description="Stored in your keychain. Create it from a non-admin Home Assistant user."
          error={tokenError ?? undefined}
        >
          <Input
            aria-label="Long-lived token"
            className="ha-wide-input"
            type="password"
            autoComplete="off"
            placeholder="Paste a new long-lived token"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </SettingRow>
        <div className="settings-action-row">
          <Button type="submit" disabled={!token.trim() || !remote.trim()} loading={haConnect.isPending}>
            Re-authenticate
          </Button>
          <Button type="button" variant="ghost" onClick={onDisconnect}>
            Disconnect
          </Button>
        </div>
      </form>
    </SettingSection>
  );
}
