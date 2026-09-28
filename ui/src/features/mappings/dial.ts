export interface DialConfig {
  min: number;
  max: number;
  gain: number;
  step?: number;
}

export function clampDialValue(value: number, config: DialConfig) {
  const stepped = config.step ? Math.round(value / config.step) * config.step : value;
  return Math.min(config.max, Math.max(config.min, stepped));
}

export function applyDialDelta(current: number, delta: number, config: DialConfig) {
  return clampDialValue(current + delta * config.gain, config);
}

export function normalizeDialValue(value: number, config: DialConfig) {
  if (config.max === config.min) return 0;
  return (clampDialValue(value, config) - config.min) / (config.max - config.min);
}

export function denormalizeDialValue(value: number, config: DialConfig) {
  return clampDialValue(config.min + value * (config.max - config.min), config);
}
