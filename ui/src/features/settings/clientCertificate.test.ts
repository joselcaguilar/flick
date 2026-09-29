import { describe, expect, it } from "vitest";
import { describeSelection, expiryStatus, joinCertificateFiles, toBase64 } from "./clientCertificate";

describe("client certificate import", () => {
  it("asks for a password only for a lone .p12 bundle", () => {
    expect(describeSelection(["home.p12"])).toEqual({ needsPassword: true });
    expect(describeSelection(["client.crt", "client.key"])).toEqual({ needsPassword: false });
    expect(describeSelection(["home.pfx", "client.key"]).error).toMatch(/single \.p12/);
  });

  it("joins PEM files on separate lines and encodes binary safely", () => {
    const encoder = new TextEncoder();
    const joined = joinCertificateFiles([encoder.encode("CERT"), encoder.encode("KEY")]);
    expect(new TextDecoder().decode(joined)).toBe("CERT\nKEY\n");
    expect(toBase64(new Uint8Array([0, 255, 128]))).toBe("AP+A");
  });

  it("flags expired and soon-to-expire certificates", () => {
    const now = Date.parse("2026-09-28T00:00:00Z");
    expect(expiryStatus("2026-09-01T00:00:00Z", false, now).tone).toBe("danger");
    expect(expiryStatus("2026-10-10T00:00:00Z", false, now).tone).toBe("warning");
    expect(expiryStatus("2030-01-01T00:00:00Z", false, now).tone).toBe("neutral");
  });
});
