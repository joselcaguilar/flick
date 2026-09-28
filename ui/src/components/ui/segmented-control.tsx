import { cn } from "../../lib/utils";

export interface Segment<T extends string> {
  label: string;
  value: T;
}

export function SegmentedControl<T extends string>({
  label,
  value,
  segments,
  onChange,
  className,
}: {
  label: string;
  value: T;
  segments: Segment<T>[];
  onChange: (value: T) => void;
  className?: string;
}) {
  return (
    <fieldset className={cn("ui-segmented", className)}>
      <legend>{label}</legend>
      {segments.map((segment) => (
        <label key={segment.value}>
          <input
            type="radio"
            name={label}
            value={segment.value}
            checked={segment.value === value}
            onChange={() => onChange(segment.value)}
          />
          <span>{segment.label}</span>
        </label>
      ))}
    </fieldset>
  );
}
