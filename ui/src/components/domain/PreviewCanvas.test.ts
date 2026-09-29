import { describe, expect, it } from "vitest";
import { MjpegPartParser } from "./PreviewCanvas";

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
