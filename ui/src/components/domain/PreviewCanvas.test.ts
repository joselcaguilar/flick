import { describe, expect, it } from "vitest";
import { MjpegPartParser, runPreviewConnections } from "./PreviewCanvas";

function part(body: number[]) {
  const header = `--flick\r\nContent-Type: image/jpeg\r\nContent-Length: ${body.length}\r\n\r\n`;
  return new Uint8Array([...new TextEncoder().encode(header), ...body, 13, 10]);
}

describe("MjpegPartParser", () => {
  it("reassembles frames split at arbitrary chunk boundaries", () => {
    const stream = new Uint8Array([...part([1, 13, 10, 13, 10, 2]), ...part([3, 4])]);
    const parser = new MjpegPartParser();
    const frames: number[][] = [];
    for (let i = 0; i < stream.length; i += 7) {
      for (const frame of parser.push(stream.subarray(i, i + 7))) frames.push([...frame]);
    }
    expect(frames).toEqual([
      [1, 13, 10, 13, 10, 2],
      [3, 4],
    ]);
  });
});

describe("runPreviewConnections", () => {
  it("mints a fresh ticket for every reconnect and backs off while streams fail fast", async () => {
    const controller = new AbortController();
    const urls: string[] = [];
    const waits: number[] = [];
    let minted = 0;
    const result = await runPreviewConnections({
      signal: controller.signal,
      now: () => 0,
      requestUrl: async () => `ticket-${++minted}`,
      stream: async (url) => {
        urls.push(url);
        if (urls.length === 3) controller.abort();
      },
      wait: async (ms) => {
        waits.push(ms);
      },
    });
    expect(result).toBe("aborted");
    expect(urls).toEqual(["ticket-1", "ticket-2", "ticket-3"]);
    expect(waits).toEqual([500, 1_000]);
  });

  it("stops without streaming when no preview is available", async () => {
    let streamed = false;
    const result = await runPreviewConnections({
      signal: new AbortController().signal,
      requestUrl: async () => null,
      stream: async () => {
        streamed = true;
      },
    });
    expect(result).toBe("unavailable");
    expect(streamed).toBe(false);
  });
});
