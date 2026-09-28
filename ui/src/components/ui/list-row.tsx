import type { ReactNode } from "react";
import { cn } from "../../lib/utils";

export interface ListRowProps {
  title: ReactNode;
  description?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
  className?: string;
}

export function ListRow({ title, description, leading, trailing, className }: ListRowProps) {
  return (
    <div className={cn("ui-list-row", !leading && "ui-list-row-plain", className)}>
      {leading ? <div className="ui-list-row-leading">{leading}</div> : null}
      <div className="ui-list-row-body">
        <strong>{title}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      {trailing ? <div className="ui-list-row-trailing">{trailing}</div> : null}
    </div>
  );
}
