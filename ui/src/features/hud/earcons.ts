import type { HudState } from "../../events/types";

const tones: Partial<
  Record<HudState["state"], { frequency: number; duration: number; type?: OscillatorType }[]>
> = {
  selected: [
    { frequency: 587, duration: 0.08 },
    { frequency: 880, duration: 0.12 },
  ],
  sent: [{ frequency: 720, duration: 0.06, type: "triangle" }],
  done: [
    { frequency: 660, duration: 0.08 },
    { frequency: 990, duration: 0.14 },
  ],
  failed: [{ frequency: 180, duration: 0.16, type: "sawtooth" }],
  confirm: [
    { frequency: 720, duration: 0.05 },
    { frequency: 720, duration: 0.05 },
  ],
  dial: [{ frequency: 520, duration: 0.035, type: "triangle" }],
  paused: [{ frequency: 320, duration: 0.1, type: "triangle" }],
};

let context: AudioContext | undefined;

export function playEarcon(state: HudState["state"]) {
  const sequence = tones[state];
  if (!sequence || typeof window === "undefined") return;
  try {
    context ??= new AudioContext();
    const start = context.currentTime;
    sequence.forEach((tone, index) => {
      const oscillator = context?.createOscillator();
      const gain = context?.createGain();
      if (!oscillator || !gain || !context) return;
      oscillator.type = tone.type ?? "sine";
      oscillator.frequency.value = tone.frequency;
      gain.gain.setValueAtTime(0.0001, start + index * 0.09);
      gain.gain.exponentialRampToValueAtTime(0.045, start + index * 0.09 + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + index * 0.09 + tone.duration);
      oscillator.connect(gain).connect(context.destination);
      oscillator.start(start + index * 0.09);
      oscillator.stop(start + index * 0.09 + tone.duration + 0.02);
    });
  } catch {
    // Browser autoplay policies may block HUD audio until the user has interacted with the app.
  }
}
