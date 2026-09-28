import { Badge, Button, GlassPanel } from "../../components/ui";

export function TeachStepPlaceholder() {
  return (
    <GlassPanel className="onboarding-teach-slot" aria-label="Teach a device placeholder">
      <div>
        <Badge tone="accent">Step 5 slot</Badge>
        <h3>Teach a device</h3>
        <p>
          Point at something in this room — like Ventilador dormitorio — and Flick will remember it. The
          dedicated Teach component will mount here at merge time.
        </p>
      </div>
      <div className="teach-slot-preview" aria-hidden="true">
        <span className="teach-camera-dot" />
        <span className="teach-ray" />
        <span className="teach-device-dot">fan</span>
      </div>
      <div className="teach-slot-actions">
        <Button variant="primary">Teach Ventilador dormitorio</Button>
        <Button variant="ghost">Later</Button>
      </div>
    </GlassPanel>
  );
}
