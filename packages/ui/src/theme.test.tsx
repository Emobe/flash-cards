/// <reference types="bun" />
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, render, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { type Platform, PlatformProvider } from "./platform";
import {
  applyPreference,
  readPreference,
  THEME_KEY,
  ThemeProvider,
  updateThemeColorMeta,
  useTheme,
  writePreference,
} from "./theme";

type Listener = (event: { matches: boolean }) => void;

/** A `matchMedia` whose dark-mode answer the test controls. */
function fakeMatchMedia(initialDark: boolean) {
  let dark = initialDark;
  const listeners = new Set<Listener>();
  vi.stubGlobal("matchMedia", () => ({
    get matches() {
      return dark;
    },
    addEventListener: (_: string, fn: Listener) => listeners.add(fn),
    removeEventListener: (_: string, fn: Listener) => listeners.delete(fn),
  }));
  return (next: boolean) => {
    dark = next;
    for (const fn of listeners) fn({ matches: next });
  };
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("preference storage", () => {
  test("defaults to system", () => {
    expect(readPreference()).toBe("system");
  });

  test("round-trips light and dark, and system clears the key", () => {
    writePreference("dark");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    expect(readPreference()).toBe("dark");
    writePreference("system");
    expect(localStorage.getItem(THEME_KEY)).toBeNull();
    expect(readPreference()).toBe("system");
  });

  test("ignores an unknown stored value", () => {
    localStorage.setItem(THEME_KEY, "purple");
    expect(readPreference()).toBe("system");
  });

  test("survives a localStorage that throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(readPreference()).toBe("system");
    expect(() => writePreference("dark")).not.toThrow();
  });
});

test("applyPreference sets and removes data-theme", () => {
  applyPreference("dark");
  expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  applyPreference("light");
  expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  applyPreference("system");
  expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
});

describe("ThemeProvider", () => {
  function setup(initialDark: boolean) {
    const setDark = fakeMatchMedia(initialDark);
    const setSystemTheme = vi.fn();
    const platform: Platform = { cardFrameUrl: "about:blank", setSystemTheme };
    const wrapper = ({ children }: { children: ReactNode }) => (
      <PlatformProvider platform={platform}>
        <ThemeProvider>{children}</ThemeProvider>
      </PlatformProvider>
    );
    return { setDark, setSystemTheme, wrapper };
  }

  test("follows the system setting and tells the platform", () => {
    const { setDark, setSystemTheme, wrapper } = setup(false);
    const { result } = renderHook(() => useTheme(), { wrapper });
    expect(result.current.effective).toBe("light");
    expect(setSystemTheme).toHaveBeenLastCalledWith("light");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);

    act(() => setDark(true));
    expect(result.current.effective).toBe("dark");
    expect(setSystemTheme).toHaveBeenLastCalledWith("dark");
  });

  test("an override wins over the system, is stored, and reaches the platform", () => {
    const { setDark, setSystemTheme, wrapper } = setup(true);
    const { result } = renderHook(() => useTheme(), { wrapper });
    expect(setSystemTheme).toHaveBeenLastCalledWith("dark");

    act(() => result.current.setPreference("light"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem(THEME_KEY)).toBe("light");
    expect(setSystemTheme).toHaveBeenLastCalledWith("light");

    // A system change does not move an override.
    setSystemTheme.mockClear();
    act(() => setDark(false));
    expect(result.current.effective).toBe("light");
    expect(setSystemTheme).not.toHaveBeenCalledWith("dark");

    act(() => result.current.setPreference("system"));
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
    expect(result.current.effective).toBe("light");
  });

  test("starts from the stored preference", () => {
    localStorage.setItem(THEME_KEY, "dark");
    const { wrapper } = setup(false);
    const { result } = renderHook(() => useTheme(), { wrapper });
    expect(result.current.preference).toBe("dark");
    expect(result.current.effective).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  test("useTheme needs a provider", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<Probe />)).toThrow();
  });
});

function Probe() {
  useTheme();
  return null;
}

test("updateThemeColorMeta copies the bg token into the theme-color meta", () => {
  document.head.innerHTML = '<meta name="theme-color" content="#000000">';
  document.documentElement.style.setProperty("--bg", " #faf6ee ");
  updateThemeColorMeta();
  expect(document.querySelector('meta[name="theme-color"]')?.getAttribute("content")).toBe(
    "#faf6ee",
  );
  document.documentElement.style.removeProperty("--bg");
  document.head.innerHTML = "";
});

describe("theme-boot.js", () => {
  const root = join(import.meta.dirname, "../../../apps");
  const nativeBoot = readFileSync(join(root, "native/public/theme-boot.js"), "utf8");
  const webBoot = readFileSync(join(root, "web/public/theme-boot.js"), "utf8");

  function run(storage: { getItem(key: string): string | null }) {
    const attributes = new Map<string, string>();
    const doc = {
      documentElement: { setAttribute: (k: string, v: string) => attributes.set(k, v) },
    };
    new Function("localStorage", "document", nativeBoot)(storage, doc);
    return attributes;
  }

  test("the two copies are identical", () => {
    expect(webBoot).toBe(nativeBoot);
  });

  test("sets data-theme for a stored light or dark", () => {
    expect(run({ getItem: () => "dark" }).get("data-theme")).toBe("dark");
    expect(run({ getItem: () => "light" }).get("data-theme")).toBe("light");
  });

  test("sets nothing for system, unknown values or a throwing storage", () => {
    expect(run({ getItem: () => null }).size).toBe(0);
    expect(run({ getItem: () => "purple" }).size).toBe(0);
    expect(
      run({
        getItem: () => {
          throw new Error("blocked");
        },
      }).size,
    ).toBe(0);
  });

  test("reads the same key as the app", () => {
    const keys: string[] = [];
    run({
      getItem: (key) => {
        keys.push(key);
        return null;
      },
    });
    expect(keys).toEqual([THEME_KEY]);
  });
});
