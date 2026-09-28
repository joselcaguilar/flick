import * as SelectPrimitive from "@radix-ui/react-select";
import { cn } from "../../lib/utils";

export function Select({
  value,
  onValueChange,
  label,
  items,
}: {
  value: string;
  onValueChange: (value: string) => void;
  label: string;
  items: Array<{ value: string; label: string }>;
}) {
  return (
    <SelectPrimitive.Root value={value} onValueChange={onValueChange}>
      <SelectPrimitive.Trigger className="ui-select" aria-label={label}>
        <SelectPrimitive.Value />
        <SelectPrimitive.Icon className="ui-select-chevron">
          <svg viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <path d="m4 6 4 4 4-4" />
          </svg>
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content className="flick-glass ui-select-content" position="popper" sideOffset={8}>
          <SelectPrimitive.Viewport>
            {items.map((item) => (
              <SelectPrimitive.Item key={item.value} className={cn("ui-select-item")} value={item.value}>
                <SelectPrimitive.ItemText>{item.label}</SelectPrimitive.ItemText>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
