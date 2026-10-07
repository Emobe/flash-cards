import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { usePlatform } from "./platform";

export type ThemePreference = "system" | "light" | "dark";

/** Stored per device, not in the collection. `theme-boot.js` reads the same key (ADR 0010). */
export const THEME_KEY = "fc.theme";

const DARK_QUERY = "(prefers-color-scheme: dark)";

export function readPreference(): ThemePreference {
  try {
    const value = localStorage.getItem(THEME_KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

export function writePreference(preference: ThemePreference): void {
  try {
    if (preference === "system") localStorage.removeItem(THEME_KEY);
    else localStorage.setItem(THEME_KEY, preference);
  } catch {
    // Storage can be blocked. The choice then lasts until the page closes.
  }
}

/** `data-theme` on `<html>` overrides the system setting. It is absent for System. */
export function applyPreference(preference: ThemePreference): void {
  const root = document.documentElement;
  if (preference === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", preference);
}

/** The web client's `setSystemTheme`: the browser's toolbar colour follows the page background. */
export function updateThemeColorMeta(): void {
  const meta = document.querySelector('meta[name="theme-color"]');
  const bg = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim();
  if (meta && bg) meta.setAttribute("content", bg);
}

type ThemeState = {
  preference: ThemePreference;
  setPreference: (preference: ThemePreference) => void;
  /** What is actually showing: the override, or the system setting. */
  effective: "light" | "dark";
};

const ThemeContext = createContext<ThemeState | null>(null);

function systemPrefersDark(): boolean {
  return typeof matchMedia === "function" && matchMedia(DARK_QUERY).matches;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const platform = usePlatform();
  const [preference, setPreferenceState] = useState(readPreference);
  const [systemDark, setSystemDark] = useState(systemPrefersDark);

  useEffect(() => {
    if (typeof matchMedia !== "function") return;
    const query = matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    setSystemDark(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const effective = preference === "system" ? (systemDark ? "dark" : "light") : preference;

  useEffect(() => {
    applyPreference(preference);
    platform.setSystemTheme(effective, preference === "system");
  }, [preference, effective, platform]);

  const setPreference = useCallback((next: ThemePreference) => {
    writePreference(next);
    setPreferenceState(next);
  }, []);

  const value = useMemo(
    () => ({ preference, setPreference, effective }),
    [preference, setPreference, effective],
  );
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeState {
  const state = useContext(ThemeContext);
  if (!state) throw new Error("useTheme must be used inside a ThemeProvider");
  return state;
}
