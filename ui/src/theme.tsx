import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from "react";
import type { ThemeMode } from "./api/types";

const storageKey = "flick.theme";
const highContrastKey = "flick.highContrast";

interface ThemeContextValue {
  mode: ThemeMode;
  highContrast: boolean;
  setMode: (mode: ThemeMode) => void;
  setHighContrast: (enabled: boolean) => void;
  cycleMode: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function getStoredMode(): ThemeMode {
  if (typeof window === "undefined") return "system";
  const stored = window.localStorage.getItem(storageKey);
  return stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
}

function getStoredHighContrast() {
  if (typeof window === "undefined") return false;
  return window.localStorage.getItem(highContrastKey) === "1";
}

function shouldForceOpaque() {
  if (typeof window === "undefined") return false;
  const reducedTransparency = window.matchMedia("(prefers-reduced-transparency: reduce)").matches;
  const unsupported = !CSS.supports("(backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))");
  const platform = navigator.platform.toLowerCase();
  return reducedTransparency || unsupported || platform.includes("linux");
}

function applyTheme(mode: ThemeMode, highContrast: boolean) {
  const root = document.documentElement;
  root.dataset.theme = highContrast ? "high-contrast" : mode === "system" ? "" : mode;
  root.dataset.themeMode = mode;
  root.classList.toggle("high-contrast", highContrast);
  root.classList.toggle("opaque-fallback", shouldForceOpaque());
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(getStoredMode);
  const [highContrast, setHighContrastState] = useState(getStoredHighContrast);

  useEffect(() => {
    applyTheme(mode, highContrast);
    const media = window.matchMedia("(prefers-reduced-transparency: reduce)");
    const onChange = () => applyTheme(mode, highContrast);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [mode, highContrast]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      mode,
      highContrast,
      setMode: (next) => {
        window.localStorage.setItem(storageKey, next);
        setModeState(next);
      },
      setHighContrast: (enabled) => {
        window.localStorage.setItem(highContrastKey, enabled ? "1" : "0");
        setHighContrastState(enabled);
      },
      cycleMode: () => {
        const next = mode === "system" ? "light" : mode === "light" ? "dark" : "system";
        window.localStorage.setItem(storageKey, next);
        setModeState(next);
      },
    }),
    [highContrast, mode],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used inside ThemeProvider");
  return context;
}
