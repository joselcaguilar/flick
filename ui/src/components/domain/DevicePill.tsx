import { EntityIcon } from "./EntityIcon";

export function DevicePill({
  name,
  domain,
  detail,
}: {
  name: string;
  domain?: string | null;
  detail?: string;
}) {
  return (
    <span className="device-pill">
      <EntityIcon domain={domain} />
      <span>
        <strong>{name}</strong>
        {detail ? <small>{detail}</small> : null}
      </span>
    </span>
  );
}
