import { type RefObject, useEffect, useRef, useState } from "react";
import { requestPreviewUrl } from "../../api/hooks";
import type { HandObservationEvent } from "../../events/types";

export interface LetterboxInput {
  sourceWidth: number;
  sourceHeight: number;
  boxWidth: number;
  boxHeight: number;
}

export interface LetterboxMapper {
  scale: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
  map: (point: [number, number]) => [number, number];
}

export function createLetterboxMapper(input: LetterboxInput): LetterboxMapper {
  const scale = Math.min(input.boxWidth / input.sourceWidth, input.boxHeight / input.sourceHeight);
  const width = input.sourceWidth * scale;
  const height = input.sourceHeight * scale;
  const offsetX = (input.boxWidth - width) / 2;
  const offsetY = (input.boxHeight - height) / 2;
  return {
    scale,
    offsetX,
    offsetY,
    width,
    height,
    map: ([x, y]) => [offsetX + x * width, offsetY + y * height],
  };
}

const MJPEG_HEADER_LIMIT = 1024;
const headerDecoder = new TextDecoder();

/**
 * Incremental parser for the engine's `multipart/x-mixed-replace; boundary=flick` stream.
 * Each part carries a Content-Length, so frames are sliced by length rather than by
 * scanning JPEG bytes for the boundary.
 */
export class MjpegPartParser {
  private buffer = new Uint8Array(256 * 1024);
  private length = 0;

  push(chunk: Uint8Array): Uint8Array<ArrayBuffer>[] {
    this.reserve(chunk.length);
    this.buffer.set(chunk, this.length);
    this.length += chunk.length;
    const frames: Uint8Array<ArrayBuffer>[] = [];
    for (;;) {
      const headerEnd = this.findHeaderEnd();
      if (headerEnd < 0) {
        // Drop garbage that can never form a header so the buffer cannot grow unbounded.
        if (this.length > MJPEG_HEADER_LIMIT) this.consume(this.length - 3);
        break;
      }
      const bodyStart = headerEnd + 4;
      const header = headerDecoder.decode(this.buffer.subarray(0, headerEnd));
      const size = Number(/content-length:\s*(\d+)/i.exec(header)?.[1] ?? Number.NaN);
      if (!Number.isFinite(size)) {
        this.consume(bodyStart);
        continue;
      }
      if (this.length < bodyStart + size) break;
      frames.push(this.buffer.slice(bodyStart, bodyStart + size));
      this.consume(bodyStart + size);
    }
    return frames;
  }

  private findHeaderEnd(): number {
    const limit = Math.min(this.length, MJPEG_HEADER_LIMIT);
    for (let i = 0; i + 3 < limit; i += 1) {
      if (
        this.buffer[i] === 13 &&
        this.buffer[i + 1] === 10 &&
        this.buffer[i + 2] === 13 &&
        this.buffer[i + 3] === 10
      ) {
        return i;
      }
    }
    return -1;
  }

  private reserve(extra: number) {
    if (this.length + extra <= this.buffer.length) return;
    const next = new Uint8Array(Math.max(this.buffer.length * 2, this.length + extra));
    next.set(this.buffer.subarray(0, this.length));
    this.buffer = next;
  }

  private consume(count: number) {
    this.buffer.copyWithin(0, count, this.length);
    this.length -= count;
  }
}

export interface PreviewConnectionOptions {
  /** Mints a fresh single-use stream URL; null means there is no stream to show (mock mode). */
  requestUrl: () => Promise<string | null>;
  /** Consumes one stream until it ends or fails. */
  stream: (url: string) => Promise<void>;
  signal: AbortSignal;
  wait?: (ms: number, signal: AbortSignal) => Promise<void>;
  now?: () => number;
}

const RECONNECT_MIN_MS = 500;
const RECONNECT_MAX_MS = 5_000;

function abortableWait(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const timer = window.setTimeout(resolve, ms);
    signal.addEventListener(
      "abort",
      () => {
        window.clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

/**
 * Keeps a preview stream connected until aborted. Every connection mints its own ticket:
 * the engine rejects a reused one, so caching a stream URL across mounts or reconnects
 * leaves the preview blank. Backoff grows while connections keep failing fast and resets
 * after a healthy one.
 */
export async function runPreviewConnections({
  requestUrl,
  stream,
  signal,
  wait = abortableWait,
  now = Date.now,
}: PreviewConnectionOptions): Promise<"aborted" | "unavailable"> {
  let delay = RECONNECT_MIN_MS;
  while (!signal.aborted) {
    const startedAt = now();
    try {
      const url = await requestUrl();
      if (signal.aborted) break;
      if (url === null) return "unavailable";
      await stream(url);
    } catch {
      // Retried below; the placeholder stays up until frames arrive again.
    }
    if (signal.aborted) break;
    if (now() - startedAt > RECONNECT_MAX_MS) delay = RECONNECT_MIN_MS;
    await wait(delay, signal);
    delay = Math.min(delay * 2, RECONNECT_MAX_MS);
  }
  return "aborted";
}

type PreviewStatus = { cameraId: string; state: "live" | "unavailable" } | null;

/**
 * Streams MJPEG over fetch and paints the newest decoded frame on a canvas once per
 * display refresh. WebKit's native `<img>` multipart rendering paces frames unevenly,
 * which read as preview lag even with the engine delivering a steady 30 fps.
 */
function useMjpegCanvas(cameraId: string | undefined, canvasRef: RefObject<HTMLCanvasElement | null>) {
  const [status, setStatus] = useState<PreviewStatus>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!cameraId || !canvas) return;
    const controller = new AbortController();
    let disposed = false;
    let live = false;
    let latest: ImageBitmap | null = null;
    let pending: Uint8Array<ArrayBuffer> | null = null;
    let decoding = false;
    let raf = 0;

    const setLive = (next: boolean) => {
      if (live === next || disposed) return;
      live = next;
      setStatus(next ? { cameraId, state: "live" } : null);
    };

    const paint = () => {
      raf = 0;
      const ctx = canvas.getContext("2d");
      if (!latest || !ctx) return;
      const dpr = window.devicePixelRatio || 1;
      const width = Math.round(canvas.clientWidth * dpr);
      const height = Math.round(canvas.clientHeight * dpr);
      if (width === 0 || height === 0) return;
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      const mapper = createLetterboxMapper({
        sourceWidth: latest.width,
        sourceHeight: latest.height,
        boxWidth: width,
        boxHeight: height,
      });
      ctx.clearRect(0, 0, width, height);
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(latest, mapper.offsetX, mapper.offsetY, mapper.width, mapper.height);
      setLive(true);
    };
    const schedulePaint = () => {
      if (!raf && !disposed) raf = requestAnimationFrame(paint);
    };

    const decode = async () => {
      if (decoding) return;
      decoding = true;
      while (pending && !disposed) {
        const jpeg = pending;
        pending = null;
        try {
          const bitmap = await createImageBitmap(new Blob([jpeg], { type: "image/jpeg" }));
          if (disposed) {
            bitmap.close();
            break;
          }
          latest?.close();
          latest = bitmap;
          schedulePaint();
        } catch {
          // A truncated frame is skipped; the next one replaces it within ~33 ms.
        }
      }
      decoding = false;
    };

    const stream = async (src: string) => {
      try {
        // WKWebView's fetch() rejects multipart/x-mixed-replace bodies outright, so ask the
        // engine for the same bytes as application/octet-stream and split parts here.
        const url = new URL(src);
        url.searchParams.set("framing", "raw");
        const response = await fetch(url, { signal: controller.signal, cache: "no-store" });
        if (!response.ok || !response.body) throw new Error(`preview ${response.status}`);
        const reader = response.body.getReader();
        const parser = new MjpegPartParser();
        for (;;) {
          const { done, value } = await reader.read();
          if (done) return;
          const frames = parser.push(value);
          const newest = frames.at(-1);
          if (newest) {
            // Only the newest frame matters; older ones would only add latency.
            pending = newest;
            void decode();
          }
        }
      } finally {
        // A dropped stream must not leave a frozen frame posing as live video.
        if (!controller.signal.aborted) {
          pending = null;
          latest?.close();
          latest = null;
          setLive(false);
        }
      }
    };

    const resize = new ResizeObserver(schedulePaint);
    resize.observe(canvas);
    // Deferred so React StrictMode's mount/unmount probe does not mint a ticket it never uses.
    const start = window.setTimeout(() => {
      void runPreviewConnections({
        requestUrl: () => requestPreviewUrl(cameraId),
        stream,
        signal: controller.signal,
      }).then((result) => {
        if (result === "unavailable" && !disposed) setStatus({ cameraId, state: "unavailable" });
      });
    }, 0);

    return () => {
      disposed = true;
      window.clearTimeout(start);
      controller.abort();
      resize.disconnect();
      if (raf) cancelAnimationFrame(raf);
      latest?.close();
      latest = null;
      setStatus(null);
    };
  }, [cameraId, canvasRef]);

  if (!cameraId) return "off" as const;
  return status?.cameraId === cameraId ? status.state : ("connecting" as const);
}

function drawHand(ctx: CanvasRenderingContext2D, mapper: LetterboxMapper, hand: HandObservationEvent) {
  ctx.strokeStyle = "rgba(100, 181, 255, 0.92)";
  ctx.fillStyle = "rgba(78, 230, 184, 0.94)";
  ctx.lineWidth = 2;
  ctx.lineCap = "round";
  const points = hand.landmarks.map((point) => mapper.map([point.x, point.y]));
  const chains = [
    [0, 1, 2, 3, 4],
    [0, 5, 6, 7, 8],
    [5, 9, 13, 17],
    [0, 17, 18, 19, 20],
    [9, 10, 11, 12],
    [13, 14, 15, 16],
  ];
  for (const chain of chains) {
    ctx.beginPath();
    for (const [index, pointIndex] of chain.entries()) {
      const [x, y] = points[pointIndex] ?? [0, 0];
      if (index === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  for (const [x, y] of points) {
    ctx.beginPath();
    ctx.arc(x, y, 3, 0, Math.PI * 2);
    ctx.fill();
  }
}

export function PreviewCanvas({
  cameraId,
  alt,
  hands = [],
  ray,
  sourceWidth = 1280,
  sourceHeight = 720,
}: {
  /** Streams this camera's preview while set; pass undefined when the camera is off. */
  cameraId?: string;
  alt: string;
  hands?: HandObservationEvent[];
  ray?: { origin2d: [number, number]; tip2d: [number, number]; model: string } | null;
  sourceWidth?: number;
  sourceHeight?: number;
}) {
  const frameRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLCanvasElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const video = useMjpegCanvas(cameraId, videoRef);
  const hasVideo = video === "live";

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    observer.observe(frame);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    if (size.width === 0 || size.height === 0) return;
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, rect.width, rect.height);
    const mapper = createLetterboxMapper({
      sourceWidth,
      sourceHeight,
      boxWidth: rect.width,
      boxHeight: rect.height,
    });
    ctx.strokeStyle = "rgba(255,255,255,.2)";
    ctx.strokeRect(mapper.offsetX, mapper.offsetY, mapper.width, mapper.height);
    if (ray) {
      const [ox, oy] = mapper.map(ray.origin2d);
      const [tx, ty] = mapper.map(ray.tip2d);
      ctx.strokeStyle = "rgba(0, 109, 255, .96)";
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.moveTo(ox, oy);
      ctx.lineTo(tx, ty);
      ctx.stroke();
    }
    for (const hand of hands) drawHand(ctx, mapper, hand);
  }, [hands, ray, size.height, size.width, sourceHeight, sourceWidth]);

  return (
    <div ref={frameRef} className="preview-canvas" data-has-video={hasVideo ? "true" : "false"}>
      {cameraId ? <canvas ref={videoRef} className="preview-video" role="img" aria-label={alt} /> : null}
      {hasVideo ? null : (
        <div className="preview-placeholder">
          {video === "connecting" ? "Connecting to the camera…" : alt}
        </div>
      )}
      <canvas ref={canvasRef} aria-label="Hand skeleton overlay" />
    </div>
  );
}
