export const certificateAccept = ".p12,.pfx,.pem,.crt,.cer,.key";

const soonMs = 30 * 24 * 60 * 60 * 1000;

const isBundle = (name: string) => /\.(p12|pfx)$/i.test(name);

/** A `.p12` bundle must be imported alone; PEM certificate and key files can be picked together. */
export function describeSelection(names: string[]): { needsPassword: boolean; error?: string } {
  const bundles = names.filter(isBundle).length;
  if (bundles > 0 && names.length > 1) {
    return {
      needsPassword: true,
      error: "Choose a single .p12 file, or the certificate and key PEM files together.",
    };
  }
  return { needsPassword: bundles > 0 };
}

export function joinCertificateFiles(parts: Uint8Array[]): Uint8Array {
  if (parts.length === 1) return parts[0] ?? new Uint8Array();
  const newline = 10;
  const joined = new Uint8Array(parts.reduce((size, part) => size + part.length + 1, 0));
  let offset = 0;
  for (const part of parts) {
    joined.set(part, offset);
    offset += part.length;
    joined[offset] = newline;
    offset += 1;
  }
  return joined;
}

export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary);
}

export function expiryStatus(notAfter: string | null | undefined, expired: boolean, now = Date.now()) {
  if (!notAfter) return { text: null, tone: "neutral" as const };
  const at = new Date(notAfter);
  const date = at.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  if (expired || at.getTime() <= now) return { text: `Expired ${date}`, tone: "danger" as const };
  if (at.getTime() - now <= soonMs) return { text: `Expires ${date}`, tone: "warning" as const };
  return { text: `Expires ${date}`, tone: "neutral" as const };
}
