import { asPercent } from "../../lib/utils";

export function ConfidenceMeter({ value, label = "Confidence" }: { value: number; label?: string }) {
  const bounded = Math.max(0, Math.min(1, value));
  return (
    <div className="confidence-meter">
      <span>{label}</span>
      <div className="confidence-track">
        <span style={{ inlineSize: `${bounded * 100}%` }} />
      </div>
      <strong>{asPercent(bounded)}</strong>
    </div>
  );
}
