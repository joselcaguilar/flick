import { type FormEvent, useRef, useState } from "react";
import {
  useHaClientCertificate,
  useHaClientCertificateImport,
  useHaClientCertificateRemove,
} from "../../api/hooks";
import { Badge, Button, Input } from "../../components/ui";
import {
  certificateAccept,
  describeSelection,
  expiryStatus,
  joinCertificateFiles,
  toBase64,
} from "./clientCertificate";
import { extractErrors, SettingRow } from "./SettingParts";

export function HaClientCertificateRow({ onNotice }: { onNotice: (message: string | null) => void }) {
  const certificate = useHaClientCertificate();
  const importCertificate = useHaClientCertificateImport();
  const removeCertificate = useHaClientCertificateRemove();
  const fileInput = useRef<HTMLInputElement>(null);
  const [files, setFiles] = useState<File[]>([]);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  const installed = certificate.data?.installed ? certificate.data : null;
  const selection = describeSelection(files.map((file) => file.name));
  const expiry = expiryStatus(installed?.not_after, installed?.expired ?? false);

  function choose(list: FileList | null) {
    setError(null);
    setPassword("");
    setFiles(list ? Array.from(list) : []);
  }

  function reset() {
    setFiles([]);
    setPassword("");
    setError(null);
    if (fileInput.current) fileInput.current.value = "";
  }

  async function importSelected(event: FormEvent) {
    event.preventDefault();
    if (selection.error || files.length === 0) return;
    setError(null);
    try {
      const parts = await Promise.all(files.map(async (file) => new Uint8Array(await file.arrayBuffer())));
      const result = await importCertificate.mutateAsync({
        data: toBase64(joinCertificateFiles(parts)),
        password: selection.needsPassword && password ? password : null,
      });
      reset();
      onNotice(`Client certificate “${result.subject ?? "certificate"}” imported.`);
    } catch (importError) {
      setError(extractErrors(importError).detail);
    }
  }

  async function remove() {
    setError(null);
    try {
      await removeCertificate.mutateAsync();
      onNotice("Client certificate removed.");
    } catch (removeError) {
      setError(extractErrors(removeError).detail);
    }
  }

  return (
    <SettingRow
      label="Client certificate"
      description="Optional. For a Remote URL protected by mTLS, such as Cloudflare. Stored in your keychain and only sent when the server asks for it."
      error={error ?? selection.error}
    >
      <div className="cert-editor">
        <input
          ref={fileInput}
          type="file"
          hidden
          multiple
          accept={certificateAccept}
          onChange={(event) => choose(event.target.files)}
        />

        {installed && files.length === 0 ? (
          <div className="cert-card" title={installed.sha256 ? `SHA-256 ${installed.sha256}` : undefined}>
            <div className="cert-card-title">
              <strong>{installed.subject}</strong>
              {expiry.tone !== "neutral" ? (
                <Badge tone={expiry.tone}>{expiry.tone === "danger" ? "Expired" : "Expires soon"}</Badge>
              ) : null}
            </div>
            <span>
              Issued by {installed.issuer}
              {expiry.text ? ` · ${expiry.text}` : null}
            </span>
          </div>
        ) : null}

        {files.length > 0 ? (
          <form className="cert-import" onSubmit={(event) => void importSelected(event)}>
            <div className="cert-files">
              <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
                <path
                  d="M4 1.75h5.25L12.5 5v9.25H4zM9 1.75V5.25h3.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.25"
                  strokeLinejoin="round"
                />
              </svg>
              <span>{files.map((file) => file.name).join(", ")}</span>
            </div>
            {selection.needsPassword && !selection.error ? (
              <Input
                aria-label="Certificate password"
                type="password"
                autoComplete="off"
                placeholder="Certificate password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            ) : null}
            <div className="cert-actions">
              <Button type="button" variant="ghost" onClick={reset}>
                Cancel
              </Button>
              <Button type="submit" disabled={Boolean(selection.error)} loading={importCertificate.isPending}>
                Import
              </Button>
            </div>
          </form>
        ) : (
          <div className="cert-actions">
            {installed ? (
              <Button variant="danger" loading={removeCertificate.isPending} onClick={() => void remove()}>
                Remove
              </Button>
            ) : null}
            <Button onClick={() => fileInput.current?.click()}>
              {installed ? "Replace…" : "Choose certificate…"}
            </Button>
          </div>
        )}
      </div>
    </SettingRow>
  );
}
