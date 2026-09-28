import { useEffect, useRef, useState } from "react";
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
  src,
  alt,
  hands = [],
  ray,
  sourceWidth = 1280,
  sourceHeight = 720,
}: {
  src?: string;
  alt: string;
  hands?: HandObservationEvent[];
  ray?: { origin2d: [number, number]; tip2d: [number, number]; model: string } | null;
  sourceWidth?: number;
  sourceHeight?: number;
}) {
  const frameRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

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
    <div ref={frameRef} className="preview-canvas" data-has-video={src ? "true" : "false"}>
      {src ? <img src={src} alt={alt} /> : <div className="preview-placeholder">{alt}</div>}
      <canvas ref={canvasRef} aria-label="Hand skeleton overlay" />
    </div>
  );
}
