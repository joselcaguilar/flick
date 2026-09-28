import en from "./en.json";

type Dictionary = typeof en;
type NestedKeyOf<T> = {
  [K in keyof T & string]: T[K] extends Record<string, unknown> ? `${K}.${NestedKeyOf<T[K]>}` : K;
}[keyof T & string];

export type I18nKey = NestedKeyOf<Dictionary>;

export function t(key: I18nKey): string {
  const result = key.split(".").reduce<unknown>((current, part) => {
    if (current && typeof current === "object" && part in current) {
      return (current as Record<string, unknown>)[part];
    }
    return undefined;
  }, en);

  return typeof result === "string" ? result : key;
}
