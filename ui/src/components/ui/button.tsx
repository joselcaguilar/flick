import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cn } from "../../lib/utils";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  leading?: ReactNode;
  trailing?: ReactNode;
  loading?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "secondary", size = "md", leading, trailing, loading, children, disabled, ...props }, ref) => (
    <button
      ref={ref}
      className={cn("ui-button", `ui-button-${variant}`, `ui-button-${size}`, className)}
      disabled={disabled || loading}
      data-loading={loading ? "true" : undefined}
      {...props}
    >
      {leading}
      <span>{loading ? "Loading…" : children}</span>
      {trailing}
    </button>
  ),
);
Button.displayName = "Button";
