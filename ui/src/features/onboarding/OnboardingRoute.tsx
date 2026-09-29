import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  useCamerasAvailable,
  useHaConnect,
  useHaDiscover,
  useHaEntities,
  usePatchSettings,
  useStatus,
} from "../../api/hooks";
import { ConfidenceMeter, DevicePill, GestureGlyph, PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, Input, ListRow, Select } from "../../components/ui";
import { useActiveCamera, useCameraLive, useLiveHands } from "../../events/useLiveHands";
import {
  type CameraPermission,
  cameraPermissionStatus,
  isTauri,
  openCameraPrivacySettings,
} from "../../platform/tauri";
import { useCameraSetup } from "../cameras/useCameraSetup";
import "./styles.css";
import { OnboardingTeachDeviceStep } from "../devices/Devices";

type StepId = "welcome" | "camera" | "ha" | "try" | "teach" | "control";
type CameraPermissionView = CameraPermission | "unknown";

const devTools = import.meta.env.DEV;

const steps: Array<{ id: StepId; label: string }> = [
  { id: "welcome", label: "Welcome" },
  { id: "camera", label: "Camera" },
  { id: "ha", label: "Home Assistant" },
  { id: "try", label: "First flick" },
  { id: "teach", label: "Teach" },
  { id: "control", label: "Stay in control" },
];

function errorMessage(error: unknown) {
  const problem = (error as { problem?: { title?: string; detail?: string } }).problem;
  return (
    problem?.detail ?? problem?.title ?? (error instanceof Error ? error.message : "Something went wrong.")
  );
}

const permissionLabels: Record<CameraPermissionView, string> = {
  authorized: "Authorized",
  denied: "Denied",
  restricted: "Restricted",
  not_determined: "Not determined",
  unknown: "Checking",
};

function normalizeStatusPermission(value: unknown): CameraPermission | null {
  return value === "authorized" || value === "denied" || value === "restricted" || value === "not_determined"
    ? value
    : null;
}

function cameraStateCopy(permission: CameraPermissionView, running: boolean) {
  if (permission === "unknown") {
    return {
      tone: "neutral" as const,
      title: "Checking camera permission",
      body: "Flick is reading the engine and desktop permission state before it asks for anything.",
    };
  }
  if (permission === "denied") {
    return {
      tone: "danger" as const,
      title: "Camera access is denied",
      body: "Flick needs camera access to see your gestures. Video stays on this Mac. Open macOS Settings → Privacy & Security → Camera, then allow Flick.",
    };
  }
  if (permission === "restricted") {
    return {
      tone: "warning" as const,
      title: "Camera access is restricted",
      body: "A device policy is blocking camera access. Flick can finish setup only after the restriction is removed.",
    };
  }
  if (permission === "not_determined") {
    return {
      tone: "accent" as const,
      title: "Ready to ask macOS",
      body: "The next action opens the system camera prompt. Flick stores hand points, never camera images.",
    };
  }
  if (!running) {
    return {
      tone: "accent" as const,
      title: "Camera access is allowed",
      body: "Start the selected camera and wait for the local preview before continuing.",
    };
  }
  return {
    tone: "success" as const,
    title: "Live preview confirmed",
    body: "The skeleton overlay is live. You can pick another camera if this is not the room you want.",
  };
}

function PromiseItem({ mark, title, body }: { mark: ReactNode; title: string; body: string }) {
  return (
    <div className="onboarding-promise">
      <span aria-hidden="true">{mark}</span>
      <strong>{title}</strong>
      <p>{body}</p>
    </div>
  );
}

export function OnboardingRoute() {
  const status = useStatus();
  const availableCameras = useCamerasAvailable();
  const cameraSetup = useCameraSetup();
  const [startedCameraId, setStartedCameraId] = useState<string>();
  const discovery = useHaDiscover();
  const lightEntities = useHaEntities("?domain=light");
  const connect = useHaConnect();
  const patchSettings = usePatchSettings();
  const [step, setStep] = useState<StepId>("welcome");
  const [cameraPermission, setCameraPermission] = useState<CameraPermissionView>("unknown");
  const [cameraRef, setCameraRef] = useState("avfoundation:0");
  const [cameraStarted, setCameraStarted] = useState(false);
  const [cameraError, setCameraError] = useState<string | null>(null);
  const [baseUrl, setBaseUrl] = useState("http://homeassistant.local:8123");
  const [token, setToken] = useState(devTools ? "demo-valid-token" : "");
  const [haError, setHaError] = useState<string | null>(null);
  const [haConnected, setHaConnected] = useState(false);
  const [haInstance, setHaInstance] = useState<{ name: string; version?: string | null }>();
  const [demoMode, setDemoMode] = useState(false);
  const [tryState, setTryState] = useState<"idle" | "listening" | "done">("idle");
  const activeIndex = steps.findIndex((item) => item.id === step);
  const selectedLight = lightEntities.data?.[0];
  const cameraBusy = cameraSetup.busy;
  const activeCamera = useActiveCamera();
  // The engine may already have started the saved camera on launch; follow it instead of showing "not started".
  const cameraId = startedCameraId ?? (activeCamera.running ? activeCamera.id : undefined);
  const liveCamera = useCameraLive(cameraId);
  const liveState = liveCamera?.state;
  const liveError = liveCamera?.error;
  const cameraRunning = liveState === "running";
  const cameraFailed = liveState === "error" || liveState === "permission_denied" || Boolean(liveError);
  const cameraStarting = cameraStarted && !cameraRunning && !cameraFailed;
  const cameraCopy = cameraStateCopy(cameraPermission, cameraRunning);
  const liveHands = useLiveHands(cameraId);
  const handTracked = cameraRunning && Boolean(liveHands?.hands.length);
  const cameraInlineError = cameraFailed
    ? `The camera couldn't start: ${liveError ?? "Check the camera connection and try again."}`
    : cameraError;
  const cameraDetail = cameraRunning
    ? "running locally"
    : cameraStarting
      ? "starting…"
      : cameraFailed
        ? "couldn't start"
        : "not started";

  const currentCamera = useMemo(
    () =>
      availableCameras.data?.find((camera) => camera.device_ref === cameraRef) ?? availableCameras.data?.[0],
    [availableCameras.data, cameraRef],
  );

  useEffect(() => {
    const enginePermission = normalizeStatusPermission(status.data?.camera_permission);
    const cameraPermission = normalizeStatusPermission(status.data?.cameras?.[0]?.camera_permission);
    const nextPermission = enginePermission ?? cameraPermission;
    if (nextPermission) setCameraPermission(nextPermission);
  }, [status.data?.camera_permission, status.data?.cameras]);

  useEffect(() => {
    if (!isTauri()) return;
    let canceled = false;
    cameraPermissionStatus()
      .then((permission) => {
        if (!canceled) setCameraPermission(permission);
      })
      .catch((error) => {
        if (!canceled) setCameraError(errorMessage(error));
      });
    return () => {
      canceled = true;
    };
  }, []);

  async function checkCameraPermission() {
    setCameraError(null);
    try {
      if (isTauri()) {
        const permission = await cameraPermissionStatus();
        setCameraPermission(permission);
        if (permission === "authorized") await ensureCameraStarted();
        return;
      }
      const nextStatus = await status.refetch();
      const permission =
        normalizeStatusPermission(nextStatus.data?.camera_permission) ??
        normalizeStatusPermission(nextStatus.data?.cameras?.[0]?.camera_permission) ??
        "not_determined";
      setCameraPermission(permission);
      if (permission === "authorized") await ensureCameraStarted();
    } catch (error) {
      setCameraError(errorMessage(error));
    }
  }

  async function ensureCameraStarted() {
    const camera = await cameraSetup.ensureStarted(cameraRef);
    setCameraRef(camera.device_ref ?? cameraRef);
    setStartedCameraId(camera.id);
    setCameraStarted(true);
  }

  async function allowCameraAndStart() {
    setCameraError(null);
    try {
      const result = await cameraSetup.allowAndStart(cameraRef);
      setCameraPermission(result.permission);
      if (result.camera) {
        setCameraRef(result.camera.device_ref ?? cameraRef);
        setStartedCameraId(result.camera.id);
        setCameraStarted(true);
      }
    } catch (error) {
      setCameraError(errorMessage(error));
    }
  }

  async function openCameraSettings() {
    setCameraError(null);
    try {
      await openCameraPrivacySettings();
    } catch (error) {
      setCameraError(errorMessage(error));
    }
  }

  async function connectHomeAssistant() {
    setHaError(null);
    try {
      const instance = await connect.mutateAsync({ base_url: baseUrl.trim(), token: token.trim() });
      setHaInstance({ name: instance.name, version: instance.ha_version });
      setHaConnected(true);
    } catch (error) {
      setHaError(errorMessage(error));
    }
  }

  async function finish() {
    await patchSettings.mutateAsync({ "onboarding.completed": true, "debug.log_suppressed": demoMode });
  }

  function next() {
    const nextStep = steps[Math.min(activeIndex + 1, steps.length - 1)]?.id;
    if (nextStep) setStep(nextStep);
  }

  return (
    <section className="onboarding-route" aria-labelledby="screen-title">
      <div className="page-header">
        <h1 id="screen-title">Set up Flick</h1>
        <p>
          Connect the camera, prove a gesture works, then decide whether Home Assistant should receive real
          actions or whether Flick should stay in demo mode.
        </p>
      </div>

      <GlassPanel className="onboarding-progress" aria-label="Onboarding progress">
        {steps.map((item, index) => (
          <button
            key={item.id}
            type="button"
            data-active={item.id === step}
            data-complete={index < activeIndex}
            onClick={() => setStep(item.id)}
          >
            <span>{index + 1}</span>
            {item.label}
          </button>
        ))}
      </GlassPanel>

      <div className="onboarding-stage">
        {step === "welcome" ? (
          <GlassPanel className="onboarding-screen welcome-screen">
            <div>
              <Badge tone="accent">About 3 minutes</Badge>
              <h2>Use a hand gesture like a local remote.</h2>
              <p>
                Flick watches on this Mac, recognizes deliberate gestures, and sends Home Assistant service
                calls only when you choose.
              </p>
            </div>
            <div className="onboarding-promises">
              <PromiseItem
                mark="Local"
                title="Private"
                body="Video stays local; Flick stores hand points only."
              />
              <PromiseItem mark="Fast" title="Instant" body="Point, lock, flick, then see the result." />
              <PromiseItem
                mark={<GestureGlyph name="open-palm" animated={false} />}
                title="No training needed"
                body="Built-ins work first; custom gestures come later."
              />
            </div>
            <div className="onboarding-actions">
              <Button variant="primary" onClick={next}>
                Get started
              </Button>
              <Button variant="ghost" onClick={() => setStep("ha")}>
                Skip camera tour
              </Button>
            </div>
          </GlassPanel>
        ) : null}

        {step === "camera" ? (
          <GlassPanel className="onboarding-screen camera-screen">
            <div className="onboarding-split">
              <div>
                <Badge tone={cameraCopy.tone}>{permissionLabels[cameraPermission]}</Badge>
                <h2>Allow camera access.</h2>
                <p>
                  Flick needs a local camera stream only to calculate hand points. It does not save video.
                </p>
                <ul className="camera-prime-list">
                  <li>macOS asks once; you can revoke access in System Settings.</li>
                  <li>After access is allowed, Flick creates a camera row if none exists.</li>
                  <li>The live skeleton preview confirms the selected camera is running.</li>
                </ul>
                <div className="onboarding-field">
                  <span>Camera</span>
                  <Select
                    label="Camera"
                    value={cameraRef}
                    onValueChange={setCameraRef}
                    items={(
                      availableCameras.data ?? [{ device_ref: cameraRef, name: "Checking cameras…" }]
                    ).map((camera) => ({
                      value: camera.device_ref,
                      label: camera.name,
                    }))}
                  />
                </div>
                <div className="camera-state-card" data-tone={cameraCopy.tone}>
                  <strong>{cameraCopy.title}</strong>
                  <p>{cameraCopy.body}</p>
                </div>
                {cameraInlineError ? (
                  <p className="inline-error" role="alert">
                    {cameraInlineError}
                  </p>
                ) : null}
              </div>
              <div>
                <PreviewCanvas
                  alt={`${currentCamera?.name ?? "Camera"} preview with a tracked hand`}
                  cameraId={cameraRunning ? cameraId : undefined}
                  hands={cameraRunning ? liveHands?.hands : []}
                  ray={cameraRunning ? liveHands?.ray : null}
                />
                <div className="preview-overlay-card onboarding-preview-card">
                  <DevicePill name={currentCamera?.name ?? "Camera"} domain="camera" detail={cameraDetail} />
                  <ConfidenceMeter
                    value={handTracked ? 0.92 : cameraRunning ? 0.12 : 0}
                    label={
                      handTracked ? "Hand tracked" : cameraRunning ? "Show a hand" : "Camera not running"
                    }
                  />
                </div>
              </div>
            </div>
            <div className="onboarding-actions">
              {cameraPermission === "denied" || cameraPermission === "restricted" ? (
                <>
                  <Button variant="primary" onClick={() => void openCameraSettings()}>
                    Open System Settings
                  </Button>
                  <Button
                    variant="secondary"
                    onClick={() => void checkCameraPermission()}
                    loading={cameraBusy}
                  >
                    Check again
                  </Button>
                </>
              ) : (
                <Button
                  variant="primary"
                  loading={cameraBusy || cameraStarting}
                  onClick={() => {
                    if (cameraRunning) next();
                    else void allowCameraAndStart();
                  }}
                >
                  {cameraRunning
                    ? "Continue"
                    : cameraFailed
                      ? "Try again"
                      : cameraStarting
                        ? "Starting…"
                        : cameraPermission === "authorized"
                          ? "Start camera"
                          : "Allow camera"}
                </Button>
              )}
              <Button variant="ghost" onClick={() => setStep("ha")}>
                Skip camera for now
              </Button>
            </div>
          </GlassPanel>
        ) : null}

        {step === "ha" ? (
          <GlassPanel className="onboarding-screen ha-screen">
            <div className="onboarding-split">
              <div>
                <Badge tone={demoMode ? "warning" : haConnected ? "success" : "accent"}>
                  {demoMode ? "Demo mode" : haConnected ? "Connected" : "Discovery"}
                </Badge>
                <h2>Connect Home Assistant.</h2>
                <p>
                  Choose a discovered instance or enter the URL manually. Skipping keeps the first journey in
                  demo mode: the HUD changes, but no service calls are sent.
                </p>
                <div className="discovery-list">
                  {(discovery.data ?? []).map((instance) => (
                    <button
                      key={instance.uuid}
                      type="button"
                      className="discovery-card"
                      onClick={() => setBaseUrl(instance.base_url)}
                    >
                      <strong>{instance.name}</strong>
                      <span>{instance.base_url}</span>
                      <Badge tone="success">{instance.version}</Badge>
                    </button>
                  ))}
                </div>
              </div>
              <form
                className="ha-form"
                onSubmit={(event) => {
                  event.preventDefault();
                  void connectHomeAssistant();
                }}
              >
                <label className="onboarding-field" htmlFor="onboarding-ha-url">
                  <span>Home Assistant URL</span>
                  <Input
                    id="onboarding-ha-url"
                    value={baseUrl}
                    onChange={(event) => setBaseUrl(event.target.value)}
                  />
                </label>
                <div className="token-guide">
                  <a href={`${baseUrl}/profile/security`} target="_blank" rel="noreferrer">
                    Open my HA profile
                  </a>
                  <ol>
                    <li>Create a separate Home Assistant user for Flick.</li>
                    <li>Open Security and create a long-lived access token.</li>
                    <li>Paste it here; Flick verifies before saving.</li>
                  </ol>
                </div>
                <label className="onboarding-field" htmlFor="onboarding-ha-token">
                  <span>Long-lived access token</span>
                  <Input
                    id="onboarding-ha-token"
                    value={token}
                    onChange={(event) => setToken(event.target.value)}
                    placeholder="Paste token"
                    type="password"
                  />
                </label>
                {devTools ? (
                  <div className="error-presets">
                    <button type="button" onClick={() => setBaseUrl("http://offline.local:8123")}>
                      Unreachable
                    </button>
                    <button type="button" onClick={() => setToken("invalid")}>
                      Invalid token
                    </button>
                    <button type="button" onClick={() => setBaseUrl("https://self-signed.local:8123")}>
                      TLS
                    </button>
                  </div>
                ) : null}
                {haError ? (
                  <p className="inline-error" role="alert">
                    {haError}
                  </p>
                ) : null}
                {haConnected ? (
                  <p className="inline-success">
                    Connected to {haInstance?.name ?? "Home Assistant"}
                    {haInstance?.version ? ` (${haInstance.version})` : ""}.
                  </p>
                ) : null}
                <div className="onboarding-actions">
                  <Button variant="primary" loading={connect.isPending} type="submit">
                    Connect
                  </Button>
                  <Button
                    variant="ghost"
                    type="button"
                    onClick={() => {
                      setDemoMode(true);
                      setHaError(null);
                      next();
                    }}
                  >
                    Skip HA for demo mode
                  </Button>
                  <Button
                    variant="secondary"
                    type="button"
                    onClick={next}
                    disabled={!haConnected && !demoMode}
                  >
                    Continue
                  </Button>
                </div>
              </form>
            </div>
          </GlassPanel>
        ) : null}

        {step === "try" ? (
          <GlassPanel className="onboarding-screen try-screen">
            <div className="onboarding-split">
              <div>
                <Badge tone={demoMode ? "warning" : "accent"}>
                  {demoMode ? "HUD only" : "Real call ready"}
                </Badge>
                <h2>Try your first flick.</h2>
                <p>
                  Suggested first mapping: thumbs up toggles a light. In demo mode, Flick shows the exact HUD
                  journey without sending a Home Assistant action.
                </p>
                <ListRow
                  leading={<GestureGlyph name="thumbs-up" />}
                  title="Thumbs up → toggle a light"
                  description={
                    selectedLight
                      ? `${selectedLight.name} · ${selectedLight.entity_id}`
                      : "Light picker loading"
                  }
                  trailing={<Badge tone="accent">global</Badge>}
                />
                <div className="onboarding-actions">
                  <Button
                    variant="primary"
                    onClick={() => {
                      setTryState("listening");
                      window.setTimeout(() => setTryState("done"), 500);
                    }}
                  >
                    Try it
                  </Button>
                  <Button variant="ghost" onClick={next}>
                    I saw it work
                  </Button>
                </div>
              </div>
              <div className="hud-demo" data-state={tryState}>
                <GestureGlyph name="thumbs-up" />
                <div>
                  <strong>
                    {tryState === "done"
                      ? demoMode
                        ? "Demo HUD · no action sent"
                        : "Done · Home Assistant confirmed"
                      : tryState === "listening"
                        ? "Listening for Thumbs up"
                        : "Ready for Thumbs up"}
                  </strong>
                  <p>{selectedLight?.name ?? "Living room lights"} → Toggle</p>
                  <span>{tryState === "done" ? "Done" : "Ready"}</span>
                </div>
              </div>
            </div>
          </GlassPanel>
        ) : null}

        {step === "teach" ? (
          <GlassPanel className="onboarding-screen teach-screen">
            <div className="onboarding-split">
              <div>
                <Badge tone="accent">Optional</Badge>
                <h2>Point at a device.</h2>
                <p>
                  This step proves targeted gestures: point at the fan, wait for selection, then draw a circle
                  for speed 1 or separate two hands for off.
                </p>
              </div>
              <OnboardingTeachDeviceStep />
            </div>
            <div className="onboarding-actions">
              <Button variant="primary" onClick={next}>
                Continue
              </Button>
              <Button variant="ghost" onClick={next}>
                Later
              </Button>
            </div>
          </GlassPanel>
        ) : null}

        {step === "control" ? (
          <GlassPanel className="onboarding-screen control-screen">
            <div>
              <Badge tone="success">Ready</Badge>
              <h2>Stay in control.</h2>
              <p>
                Flick can pause from the menu bar, require an arm gesture, and block sensitive devices by
                default.
              </p>
            </div>
            <div className="control-grid">
              <PromiseItem
                mark="Pause"
                title="Pause instantly"
                body="Menu bar or Option-Command-F pauses recognition."
              />
              <PromiseItem
                mark={<GestureGlyph name="open-palm" animated={false} />}
                title="Arm mode optional"
                body="Require a deliberate open-palm gesture first."
              />
              <PromiseItem
                mark="Safe"
                title="Sensitive devices locked"
                body="Locks, alarms and garage doors need Safety enabled plus confirmation."
              />
            </div>
            <div className="onboarding-actions">
              <Button variant="primary" loading={patchSettings.isPending} onClick={() => void finish()}>
                Finish
              </Button>
              {patchSettings.isSuccess ? (
                <span className="inline-success">Flick is watching from the menu bar.</span>
              ) : null}
            </div>
          </GlassPanel>
        ) : null}
      </div>
    </section>
  );
}
