/// <reference types="bun" />
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { RouterProvider } from "../router";
import { AppShell } from "./AppShell";
import { BottomAction } from "./slot";

function renderShell(path: string, content: React.ReactNode = <p>page</p>) {
  window.history.replaceState(null, "", `/#${path}`);
  return render(
    <RouterProvider>
      <AppShell>{content}</AppShell>
    </RouterProvider>,
  );
}

function press(key: string, target: Element = document.body, init: KeyboardEventInit = {}) {
  fireEvent.keyDown(target, { key, ...init });
}

beforeEach(() => window.history.replaceState(null, "", "/#/decks"));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("navigation", () => {
  test("a nav labelled Main lists the four destinations in order", () => {
    renderShell("/decks");
    const nav = screen.getByRole("navigation", { name: "Main" });
    const labels = within(nav)
      .getAllByRole("link")
      .map((a) => a.textContent);
    expect(labels).toEqual(["Decks", "Add", "Browse", "Settings"]);
    expect(screen.getByRole("main")).toBeDefined();
  });

  test("the current destination has aria-current and the others do not", () => {
    renderShell("/browse");
    const links = within(screen.getByRole("navigation", { name: "Main" })).getAllByRole("link");
    expect(links.map((a) => a.getAttribute("aria-current"))).toEqual([null, null, "page", null]);
  });

  test("the developer screen counts as Settings", () => {
    renderShell("/settings/developer");
    expect(screen.getByRole("link", { name: "Settings" }).getAttribute("aria-current")).toBe(
      "page",
    );
  });

  test("clicking a destination goes there", () => {
    renderShell("/decks");
    fireEvent.click(screen.getByRole("link", { name: "Add" }));
    expect(window.location.hash).toBe("#/add");
    expect(screen.getByRole("link", { name: "Add" }).getAttribute("aria-current")).toBe("page");
  });

  test("study is full screen: no navigation, a back control", () => {
    const back = vi.spyOn(window.history, "back").mockImplementation(() => {});
    renderShell("/study/7");
    expect(screen.queryByRole("navigation")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(back).toHaveBeenCalled();
  });
});

describe("shortcuts", () => {
  test("g then a letter goes to that destination", () => {
    renderShell("/decks");
    press("g");
    press("b");
    expect(window.location.hash).toBe("#/browse");
    press("g");
    press("s");
    expect(window.location.hash).toBe("#/settings");
    press("g");
    press("d");
    expect(window.location.hash).toBe("#/decks");
    press("g");
    press("a");
    expect(window.location.hash).toBe("#/add");
  });

  test("a letter alone, or after g with an unknown letter, does nothing", () => {
    renderShell("/decks");
    press("b");
    expect(window.location.hash).toBe("#/decks");
    press("g");
    press("x");
    press("b");
    expect(window.location.hash).toBe("#/decks");
  });

  test("g expires after a moment", () => {
    vi.useFakeTimers();
    renderShell("/decks");
    press("g");
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    press("b");
    expect(window.location.hash).toBe("#/decks");
  });

  test("ignored while a text field has focus", () => {
    renderShell(
      "/decks",
      <>
        <input aria-label="name" />
        <textarea aria-label="notes" />
        <div contentEditable suppressContentEditableWarning data-testid="rich" />
      </>,
    );
    const fields = [
      screen.getByLabelText("name"),
      screen.getByLabelText("notes"),
      screen.getByTestId("rich"),
    ];
    for (const field of fields) {
      press("g", field);
      press("b", field);
    }
    expect(window.location.hash).toBe("#/decks");
  });

  test("ignored with Ctrl, Alt or Meta held", () => {
    renderShell("/decks");
    press("g");
    press("b", document.body, { ctrlKey: true });
    expect(window.location.hash).toBe("#/decks");
  });

  test("not active on the full-screen study route", () => {
    renderShell("/study/1");
    press("g");
    press("b");
    expect(window.location.hash).toBe("#/study/1");
  });

  test("? opens the shortcuts list", () => {
    renderShell("/decks");
    expect(screen.queryByRole("dialog")).toBeNull();
    press("?", document.body, { shiftKey: true });
    const dialog = screen.getByRole("dialog", { name: "Keyboard shortcuts" });
    expect(dialog.textContent).toContain("Go to Browse");
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("keyboard inset", () => {
  /** A `visualViewport` the test can resize, in a window 800 px tall. */
  function fakeViewport(height: number, scale = 1) {
    const listeners = new Map<string, Set<() => void>>();
    const viewport = {
      height,
      offsetTop: 0,
      scale,
      addEventListener: (type: string, fn: () => void) => {
        const set = listeners.get(type) ?? new Set();
        set.add(fn);
        listeners.set(type, set);
      },
      removeEventListener: (type: string, fn: () => void) => listeners.get(type)?.delete(fn),
      fire: (type: string) => {
        for (const fn of listeners.get(type) ?? []) fn();
      },
    };
    vi.stubGlobal("visualViewport", viewport);
    vi.stubGlobal("innerHeight", 800);
    return viewport;
  }

  const shell = () => document.querySelector(".shell") as HTMLElement;

  test("no keyboard: inset 0 and the bar shows", () => {
    fakeViewport(800);
    renderShell("/decks");
    expect(shell().style.getPropertyValue("--keyboard-inset")).toBe("0px");
    expect(shell().dataset.keyboard).toBe("closed");
  });

  test("the keyboard writes --keyboard-inset and marks the shell open", () => {
    const viewport = fakeViewport(800);
    renderShell("/decks");
    viewport.height = 500;
    act(() => viewport.fire("resize"));
    expect(shell().style.getPropertyValue("--keyboard-inset")).toBe("300px");
    expect(shell().dataset.keyboard).toBe("open");

    viewport.height = 800;
    act(() => viewport.fire("resize"));
    expect(shell().style.getPropertyValue("--keyboard-inset")).toBe("0px");
    expect(shell().dataset.keyboard).toBe("closed");
  });

  test("a scrolled visual viewport counts its offset", () => {
    const viewport = fakeViewport(500);
    viewport.offsetTop = 100;
    renderShell("/decks");
    expect(shell().style.getPropertyValue("--keyboard-inset")).toBe("200px");
  });

  test("pinch zoom is not a keyboard", () => {
    fakeViewport(400, 2);
    renderShell("/decks");
    expect(shell().style.getPropertyValue("--keyboard-inset")).toBe("0px");
  });

  test("a window with no visualViewport has inset 0", () => {
    vi.stubGlobal("visualViewport", null);
    renderShell("/decks");
    expect(shell().dataset.keyboard).toBe("closed");
  });
});

test("BottomAction renders in the shell's action slot, outside the page", () => {
  renderShell(
    "/decks",
    <BottomAction>
      <button type="button">Save</button>
    </BottomAction>,
  );
  const button = screen.getByRole("button", { name: "Save" });
  expect(button.closest(".shell-action")).not.toBeNull();
  expect(button.closest(".shell-main")).toBeNull();
});

test("only the shell mentions safe areas or the visual viewport", async () => {
  const { readdirSync, readFileSync } = await import("node:fs");
  const { join } = await import("node:path");
  const root = join(import.meta.dirname, "..");
  const offenders: string[] = [];
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (/\.(css|tsx?)$/.test(entry.name) && !/\.test\./.test(entry.name)) {
        const text = readFileSync(path, "utf8");
        if (/safe-area|visualViewport/.test(text) && !path.includes(`${join(root, "shell")}`)) {
          offenders.push(path);
        }
      }
    }
  };
  walk(root);
  expect(offenders).toEqual([]);
});
