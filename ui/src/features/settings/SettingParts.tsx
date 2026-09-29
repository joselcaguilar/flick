import type { ReactNode } from "react";

export type FieldErrors = Record<string, string>;

export const sectionId = (title: string) => `settings-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;

export function extractErrors(error: unknown) {
  const problem = (error as { problem?: { detail?: string; errors?: FieldErrors } }).problem;
  return {
    detail:
      problem?.detail ?? (error instanceof Error ? error.message : "Flick could not save that setting."),
    errors: problem?.errors ?? {},
  };
}

export function SettingSection({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  const id = sectionId(title);
  return (
    <section className="settings-section" id={id} aria-labelledby={`${id}-title`}>
      <div className="settings-section-heading">
        <h2 id={`${id}-title`}>{title}</h2>
        <p>{subtitle}</p>
      </div>
      <div className="settings-section-body">{children}</div>
    </section>
  );
}

export function SettingRow({
  label,
  description,
  error,
  children,
}: {
  label: string;
  description?: ReactNode;
  error?: string;
  children: ReactNode;
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
