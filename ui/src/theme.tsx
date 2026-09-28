import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";
import type { DarkThemeId, LightThemeId, ThemeMode } from "./api/types";

export const lightThemes: { id: LightThemeId; label: string }[] = [
  { id: "light", label: "Light default" },
  { id: "light_high_contrast", label: "Light high contrast" },
  { id: "light_colorblind", label: "Light Protanopia & Deuteranopia" },
];

export const darkThemes: { id: DarkThemeId; label: string }[] = [
  { id: "dark", label: "Dark default" },
  { id: "dark_dimmed", label: "Dark dimmed" },
  { id: "dark_high_contrast", label: "Dark high contrast" },
];

const modeKey = "flick.theme";
const lightKey = "flick.lightTheme";
const darkKey = "flick.darkTheme";

export interface ThemePreferences {
  mode: ThemeMode;
  light: LightThemeId;
  dark: DarkThemeId;
}

interface ThemeContextValue extends ThemePreferences {
  resolved: LightThemeId | DarkThemeId;
  setMode: (mode: ThemeMode) => void;
  setLightTheme: (theme: LightThemeId) => void;
  setDarkTheme: (theme: DarkThemeId) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

const isLight = (value: unknown): value is LightThemeId => lightThemes.some((theme) => theme.id === value);
const isDark = (value: unknown): value is DarkThemeId => darkThemes.some((theme) => theme.id === value);
const isMode = (value: unknown): value is ThemeMode =>
  value === "system" || value === "light" || value === "dark";

function readPreferences(): ThemePreferences {
  if (typeof window === "undefined") return { mode: "system", light: "light", dark: "dark" };
  const mode = window.localStorage.getItem(modeKey);
  const light = window.localStorage.getItem(lightKey);
  const dark = window.localStorage.getItem(darkKey);
  return {
    mode: isMode(mode) ? mode : "system",
    light: isLight(light) ? light : "light",
    dark: isDark(dark) ? dark : "dark",
  };
}

const darkQuery = () => window.matchMedia("(prefers-color-scheme: dark)");

export function resolveTheme(preferences: ThemePreferences, systemDark: boolean) {
  const scheme = preferences.mode === "system" ? (systemDark ? "dark" : "light") : preferences.mode;
  return scheme === "dark" ? preferences.dark : preferences.light;
}

function applyTheme(preferences: ThemePreferences) {
  const root = document.documentElement;
  const resolved = resolveTheme(preferences, darkQuery().matches);
  root.dataset.theme = resolved;
  root.dataset.colorMode = preferences.mode === "system" ? "auto" : preferences.mode;
  root.dataset.lightTheme = preferences.light;
  root.dataset.darkTheme = preferences.dark;
  root.classList.toggle("high-contrast", resolved.endsWith("high_contrast"));
  return resolved;
}

// Apply before React renders so the first paint already uses the saved theme.
if (typeof document !== "undefined") applyTheme(readPreferences());

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preferences, setPreferences] = useState<ThemePreferences>(readPreferences);
  const [resolved, setResolved] = useState(() => resolveTheme(preferences, darkQuery().matches));

  useEffect(() => {
    setResolved(applyTheme(preferences));
    const media = darkQuery();
    const onSchemeChange = () => setResolved(applyTheme(preferences));
    media.addEventListener("change", onSchemeChange);
    return () => media.removeEventListener("change", onSchemeChange);
  }, [preferences]);

  // Keep the HUD window and the main window on the same theme.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === modeKey || event.key === lightKey || event.key === darkKey) {
        setPreferences(readPreferences());
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const value = useMemo<ThemeContextValue>(() => {
    const update = (key: string, value: string, next: Partial<ThemePreferences>) => {
      window.localStorage.setItem(key, value);
      setPreferences((current) => ({ ...current, ...next }));
    };
    return {
      ...preferences,
      resolved,
      setMode: (mode) => update(modeKey, mode, { mode }),
      setLightTheme: (light) => update(lightKey, light, { light }),
      setDarkTheme: (dark) => update(darkKey, dark, { dark }),
    };
  }, [preferences, resolved]);

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used inside ThemeProvider");
  return context;
}
