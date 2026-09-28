import * as Slider from "@radix-ui/react-slider";
import { cn } from "../../lib/utils";

export function SliderDial({
  label,
  value,
  min = 0,
  max = 100,
  step = 1,
  onValueChange,
  className,
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onValueChange: (value: number) => void;
  className?: string;
}) {
  return (
    <label className={cn("ui-slider-field", className)}>
      <span>{label}</span>
      <Slider.Root className="ui-slider" min={min} max={max} step={step} value={[value]} onValueChange={([next]) => onValueChange(next ?? value)}>
        <Slider.Track className="ui-slider-track">
          <Slider.Range className="ui-slider-range" />
        </Slider.Track>
        <Slider.Thumb className="ui-slider-thumb" aria-label={label} />
      </Slider.Root>
    </label>
  );
}
