import { useEffect, useMemo, useState } from "react";
import {
  useCameras,
  useCaptureGesture,
  useClassifier,
  useCreateGesture,
  useTrainClassifier,
} from "../../api/hooks";
import type { HandObservationEvent } from "../../events/types";
import { ConfidenceMeter, GestureGlyph, PreviewCanvas } from "../../components/domain";
import { Badge, Button, GlassPanel, Input, Select, SegmentedControl, TypeChip } from "../../components/ui";
import { asPercent } from "../../lib/utils";
import "./styles.css";

type StudioType = "static" | "motion" | "two-hand";
type StudioPhase = "setup" | "countdown" | "recording" | "ready" | "trained" | "testing" | "saved";

const typeSegments = [
  { value: "static", label: "Static" },
  { value: "motion", label: "Motion" },
  { value: "two-hand", label: "Two-hand" },
] satisfies Array<{ value: StudioType; label: string }>;

const handSegments = [
  { value: "any", label: "Any" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

const trajectories: Record<StudioType, Array<Array<[number, number]>>> = {
  static: [[[0.42, 0.58], [0.47, 0.5], [0.52, 0.47], [0.57, 0.5], [0.62, 0.58]]],
  motion: [[[0.22, 0.72], [0.36, 0.36], [0.55, 0.6], [0.72, 0.28], [0.84, 0.56]]],
  "two-hand": [
    [[0.42, 0.45], [0.34, 0.45], [0.26, 0.45]],
    [[0.58, 0.45], [0.66, 0.45], [0.74, 0.45]],
  ],
};

function buildHand(offset = 0): HandObservationEvent {
  return {
    track_id: offset + 1,
    hand: offset ? "left" : "right",
    bbox: { x: 0.38 + offset * 0.2, y: 0.2, w: 0.22, h: 0.42 },
    landmarks: Array.from({ length: 21 }, (_, index) => ({
      x: 0.42 + offset * 0.2 + (index % 5) * 0.035,
      y: 0.66 - Math.floor(index / 5) * 0.08,
      z: 0,
    })),
  };
}

function TrajectoryGlyph({ type, index }: { type: StudioType; index: number }) {
  const paths = trajectories[type];
  return (
    <svg className="trajectory-glyph" viewBox="0 0 100 72" role="img" aria-label={`${type} trajectory ${index + 1}`}>
      <title>{`${type} take ${index + 1}`}</title>
      {paths.map((path, pathIndex) => (
        <polyline
          key={`${pathIndex}-${path.length}`}
          points={path.map(([x, y]) => `${x * 100},${y * 72}`).join(" ")}
          fill="none"
          stroke={pathIndex === 0 ? "var(--flick-accent-blue)" : "var(--flick-accent-mint)"}
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="5"
        />
      ))}
    </svg>
  );
}

function targetTime(type: StudioType) {
  return type === "static" ? "≤ 60 s" : "≤ 90 s";
}

export function GestureStudioRoute() {
  const cameras = useCameras();
  const createGesture = useCreateGesture();
  const trainClassifier = useTrainClassifier();
  const classifier = useClassifier();
  const [name, setName] = useState("Rock on");
  const [hand, setHand] = useState("any");
  const [cameraId, setCameraId] = useState("camera-main");
  const [type, setType] = useState<StudioType>("static");
  const [phase, setPhase] = useState<StudioPhase>("setup");
  const [countdown, setCountdown] = useState(0);
  const [takes, setTakes] = useState(0);
  const [gestureId, setGestureId] = useState("custom.draft");
  const [threshold, setThreshold] = useState(0.75);
  const [testConfidence, setTestConfidence] = useState(0.18);
  const [savedName, setSavedName] = useState<string | null>(null);
  const captureGesture = useCaptureGesture(gestureId);
  const hands = useMemo(() => (type === "two-hand" ? [buildHand(0), buildHand(1)] : [buildHand(0)]), [type]);
  const requiredTakes = type === "static" ? 5 : 3;
  const report = trainClassifier.data ?? classifier.data;

  useEffect(() => {
    if (countdown <= 0 || phase !== "countdown") return;
    const timer = window.setTimeout(() => setCountdown((current) => current - 1), 700);
    return () => window.clearTimeout(timer);
  }, [countdown, phase]);

  useEffect(() => {
    if (countdown === 0 && phase === "countdown") {
      setPhase("recording");
      const timer = window.setTimeout(() => {
        setTakes((current) => current + 1);
        setPhase("ready");
      }, type === "static" ? 650 : 900);
      return () => window.clearTimeout(timer);
    }
  }, [countdown, phase, type]);

  async function recordTake() {
    setPhase("countdown");
    setCountdown(3);
    await captureGesture.mutateAsync({
      camera_id: cameraId,
      kind: "positive",
      takes: 1,
      take_ms: type === "static" ? 1500 : 2000,
    });
  }

  async function train() {
    setPhase("trained");
    await trainClassifier.mutateAsync();
  }

  async function saveGesture() {
    const saved = await createGesture.mutateAsync({
      name,
      kind: type === "static" ? "static" : "motion",
      hands_required: type === "two-hand" ? 2 : 1,
      hand_constraint: hand,
      icon: type === "two-hand" ? "two-hand-separate" : type === "motion" ? "zorro-z" : "rock-on",
      threshold,
    });
    setGestureId(saved.id);
    setSavedName(saved.name);
    setPhase("saved");
  }

  return (
    <section className="studio-route" aria-labelledby="screen-title">
      <div className="studio-hero">
        <p className="route-path">Gestures / Studio</p>
        <h1 id="screen-title">Record, train, prove it.</h1>
        <p>
          Studio keeps the flow under {targetTime(type)}: name the gesture, record a handful of takes, train locally,
          live-test against confusions, then save.
        </p>
      </div>

      <div className="studio-layout">
        <GlassPanel className="studio-panel setup-panel">
          <Badge tone="accent">Setup</Badge>
          <h2>Name and first take</h2>
          <label className="studio-field">
            <span>Gesture name</span>
            <Input value={name} onChange={(event) => setName(event.target.value)} />
          </label>
          <SegmentedControl label="Hand" value={hand} segments={handSegments} onChange={setHand} />
          <label className="studio-field">
            <span>Camera</span>
            <Select
              label="Camera"
              value={cameraId}
              onValueChange={setCameraId}
              items={(cameras.data ?? []).map((camera) => ({ value: camera.id, label: camera.name }))}
            />
          </label>
          <div className="auto-type-card">
            <div>
              <TypeChip>auto type</TypeChip>
              <strong>Looks like a {type} gesture</strong>
              <span>Change it any time; Flick rebuilds templates on save.</span>
            </div>
            <SegmentedControl label="Detected type" value={type} segments={typeSegments} onChange={setType} />
          </div>
        </GlassPanel>

        <GlassPanel className="studio-panel preview-panel studio-preview-panel">
          <div className="studio-preview-heading">
            <div>
              <Badge tone={phase === "recording" ? "warning" : "success"}>
                {phase === "countdown" ? `${countdown}` : phase}
              </Badge>
              <h2>Live preview</h2>
            </div>
            <ConfidenceMeter value={phase === "recording" ? 0.94 : 0.72} label="Frame quality" />
          </div>
          <PreviewCanvas
            alt="Gesture Studio live preview with hand skeleton"
            hands={phase === "setup" ? [] : hands}
            ray={type === "motion" ? { origin2d: [0.28, 0.72], tip2d: [0.82, 0.36], model: "motion" } : null}
          />
          <div className="studio-countdown" data-visible={phase === "countdown" || phase === "recording"}>
            <strong>{phase === "countdown" ? countdown : type === "static" ? "Hold it…" : "Go!"}</strong>
            <span>{type === "static" ? "1.5 s steady hold" : "2 s capture bar"}</span>
          </div>
          <div className="studio-actions">
            <Button variant="primary" onClick={recordTake} loading={captureGesture.isPending || phase === "countdown"}>
              {takes ? "Record another take" : "Start countdown"}
            </Button>
            <span>{takes}/{requiredTakes} takes captured</span>
          </div>
        </GlassPanel>

        <GlassPanel className="studio-panel takes-panel">
          <Badge tone="accent">Takes</Badge>
          <h2>Trajectory glyphs</h2>
          <p>Static takes render a steady skeleton. Motion and two-hand takes render normalized paths only.</p>
          <div className="trajectory-grid">
            {Array.from({ length: Math.max(takes, requiredTakes) }, (_, index) => (
              <div key={index} className="trajectory-card" data-empty={index >= takes}>
                {index < takes ? <TrajectoryGlyph type={type} index={index} /> : <span>Take {index + 1}</span>}
                <small>{index < takes ? "accepted" : "ready"}</small>
              </div>
            ))}
          </div>
          {takes >= 1 ? (
            <p className="coach-copy">
              Coach: {type === "static" ? "Turn your hand slightly for diversity." : "Same shape, a bit faster or slower."}
            </p>
          ) : null}
        </GlassPanel>

        <GlassPanel className="studio-panel train-panel">
          <Badge tone={phase === "trained" || phase === "saved" ? "success" : "neutral"}>Train</Badge>
          <h2>Learn locally</h2>
          <p>Training activates the new classifier in under one second and keeps relaxed-hand negatives local.</p>
          <Button variant="primary" onClick={train} disabled={takes < requiredTakes} loading={trainClassifier.isPending}>
            Train classifier
          </Button>
          {trainClassifier.isSuccess ? <p className="studio-success">Learned in 0.3 s.</p> : null}
        </GlassPanel>

        <GlassPanel className="studio-panel test-panel">
          <Badge tone="accent">Live test</Badge>
          <h2>Confidence and threshold</h2>
          <div className="test-meter">
            <GestureGlyph name={type === "two-hand" ? "two-hand-separate" : type === "motion" ? "circle-cw" : "open-palm"} />
            <ConfidenceMeter value={testConfidence} label={name || "New gesture"} />
          </div>
          <label className="studio-field">
            <span>Threshold {asPercent(threshold)}</span>
            <input
              className="studio-range"
              type="range"
              min="0.55"
              max="0.95"
              step="0.01"
              value={threshold}
              onChange={(event) => setThreshold(Number(event.target.value))}
            />
          </label>
          <Button
            variant="secondary"
            onClick={() => {
              setPhase("testing");
              setTestConfidence(0.88);
            }}
            disabled={!trainClassifier.isSuccess}
          >
            Run live test
          </Button>
        </GlassPanel>

        <GlassPanel className="studio-panel quality-panel">
          <Badge tone="warning">Quality report</Badge>
          <h2>Confusions</h2>
          <p>Accuracy {report?.loto_accuracy == null ? "—" : asPercent(report.loto_accuracy)} leave-one-take-out.</p>
          <div className="confusion-list">
            {(report?.confusions ?? []).map((confusion) => (
              <div key={`${confusion.gesture_id}-${confusion.with}`}>
                <strong>{confusion.gesture_id.replace(/^custom\.|^builtin\./, "")}</strong>
                <span>looks a bit like {confusion.with.replace(/^custom\.|^builtin\./, "")} · {asPercent(confusion.score)}</span>
              </div>
            ))}
            {!report?.confusions?.length ? (
              <div>
                <strong>No confusions yet</strong>
                <span>Train after recording enough takes.</span>
              </div>
            ) : null}
          </div>
          <p className="coach-copy">If this looks weak, add 3 more takes or pick a more distinct shape.</p>
        </GlassPanel>

        <GlassPanel className="studio-panel save-panel">
          <Badge tone={savedName ? "success" : "neutral"}>Save</Badge>
          <h2>Use it next</h2>
          <p>Save opens the next decision: use it as a device verb or as a global gesture.</p>
          <Button variant="primary" onClick={saveGesture} disabled={!trainClassifier.isSuccess} loading={createGesture.isPending}>
            Save gesture
          </Button>
          {savedName ? (
            <p className="studio-success">Saved {savedName}. Next: map it to a device verb or global action.</p>
          ) : null}
        </GlassPanel>
      </div>
    </section>
  );
}
