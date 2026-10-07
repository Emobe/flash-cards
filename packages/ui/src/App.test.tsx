import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { CoreClient, createFakeTransport } from "core-client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { CoreProvider } from "./core";
import { PlatformProvider } from "./platform";

const collectionInfo = {
  schemaVersion: 1,
  supportedSchemaVersion: 1,
  createdBy: "9.9.9",
  deviceId: "01234567-89ab-7cde-8f01-23456789abcd",
  unsupportedFeatures: [],
};

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});
afterEach(cleanup);

function renderApp(path: string, getCollectionInfo: () => unknown = () => collectionInfo) {
  window.history.replaceState(null, "", `/#${path}`);
  const setSystemTheme = vi.fn();
  const client = new CoreClient(
    createFakeTransport({
      getCoreInfo: () => ({ coreVersion: "9.9.9" }),
      getCollectionInfo: getCollectionInfo as () => typeof collectionInfo,
      getDeckList: () => ({ decks: [], totalCards: 0 }),
      spikeSchedule: ({ ratings }) => ({
        reviews: ratings.map((_, i) => ({ intervalDays: 2 + i, stability: 1, difficulty: 1 })),
      }),
      exampleDivide: ({ dividend, divisor }) => {
        if (divisor === 0) {
          throw {
            kind: "invalidInput",
            message: "Can't divide by zero. Enter a divisor other than 0.",
          };
        }
        return { quotient: dividend / divisor };
      },
    }),
  );
  render(
    <CoreProvider client={client}>
      <PlatformProvider platform={{ cardFrameUrl: "about:blank", setSystemTheme }}>
        <App />
      </PlatformProvider>
    </CoreProvider>,
  );
  return { setSystemTheme };
}

describe("screens", () => {
  test("opens on Decks with a plain empty state and the main navigation", async () => {
    renderApp("/decks");
    expect(await screen.findByRole("heading", { level: 1, name: "Decks" })).toBeDefined();
    expect(
      await screen.findByText("You have no cards yet. Add your first card to start studying."),
    ).toBeDefined();
    expect(screen.getByRole("navigation", { name: "Main" })).toBeDefined();
  });

  test("an unknown hash lands on Decks", async () => {
    renderApp("/nope");
    expect(await screen.findByRole("heading", { level: 1, name: "Decks" })).toBeDefined();
  });

  test("Add and Browse say what is coming", async () => {
    renderApp("/decks");
    fireEvent.click(await screen.findByRole("link", { name: "Add" }));
    expect(screen.getByRole("heading", { level: 1, name: "Add" })).toBeDefined();
    fireEvent.click(screen.getByRole("link", { name: "Browse" }));
    expect(screen.getByRole("heading", { level: 1, name: "Browse" })).toBeDefined();
  });

  test("study is full screen with a back control", async () => {
    renderApp("/study/12");
    expect(await screen.findByRole("heading", { level: 1, name: "Study" })).toBeDefined();
    expect(screen.queryByRole("navigation")).toBeNull();
    expect(screen.getByRole("button", { name: "Back" })).toBeDefined();
  });
});

describe("Settings", () => {
  test("the theme choice sets data-theme, is remembered and reaches the platform", async () => {
    const { setSystemTheme } = renderApp("/settings");
    const system = (await screen.findByRole("radio", { name: "System" })) as HTMLInputElement;
    expect(system.checked).toBe(true);

    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem("fc.theme")).toBe("dark");
    expect(setSystemTheme).toHaveBeenLastCalledWith("dark", false);

    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(setSystemTheme).toHaveBeenLastCalledWith("light", false);

    fireEvent.click(screen.getByRole("radio", { name: "System" }));
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  });

  test("Developer tools opens the developer screen", async () => {
    renderApp("/settings");
    fireEvent.click(await screen.findByRole("link", { name: "Developer tools" }));
    expect(screen.getByRole("heading", { level: 1, name: "Developer tools" })).toBeDefined();
  });
});

describe("Developer screen", () => {
  test("shows the core and collection versions", async () => {
    renderApp("/settings/developer");
    expect(await screen.findByText("Core version 9.9.9")).toBeDefined();
    expect(
      screen.getByText("Collection storage version 1 (this build supports up to 1)."),
    ).toBeDefined();
  });

  test("shows the quotient computed by the core", async () => {
    renderApp("/settings/developer");
    fireEvent.click(await screen.findByRole("button", { name: "Divide" }));
    expect((await screen.findByRole("status")).textContent).toBe("10 ÷ 4 = 2.5");
  });

  test("shows a readable message when the core returns an error", async () => {
    renderApp("/settings/developer");
    fireEvent.change(await screen.findByLabelText("Divisor"), { target: { value: "0" } });
    fireEvent.click(screen.getByRole("button", { name: "Divide" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Can't divide by zero. Enter a divisor other than 0.",
    );
  });

  test("shows the intervals the core scheduled", async () => {
    renderApp("/settings/developer");
    fireEvent.click(await screen.findByRole("button", { name: "Schedule 5 Good reviews" }));
    expect((await screen.findByTestId("intervals")).textContent).toBe(
      "Intervals in days: 2, 3, 4, 5, 6",
    );
  });

  test("pins an action above the keyboard", async () => {
    renderApp("/settings/developer");
    const button = await screen.findByRole("button", { name: "Clear the field" });
    expect(button.closest(".shell-action")).not.toBeNull();
  });
});

describe("collection problem screen", () => {
  function fail(kind: string, message: string) {
    return () => {
      throw { kind, message };
    };
  }

  test("shows an opening message first", () => {
    renderApp("/decks", () => new Promise(() => {}));
    expect(screen.getByRole("status").textContent).toBe("Opening your cards...");
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  test("a newer collection asks for an update and leaves the cards alone", async () => {
    renderApp("/decks", fail("updateRequired", "ignored"));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("made by a newer version of the app");
    expect(alert.textContent).toContain("Update the app");
    expect(alert.textContent).toContain("Your cards are untouched");
    expect(screen.queryByRole("navigation")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Decks" })).toBeNull();
  });

  test("a collection in use says what to try", async () => {
    renderApp("/decks", fail("unavailable", "The collection is in use."));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The collection is in use.");
    expect(alert.textContent).toContain("Another window or tab");
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  test("any other error shows the message and asks for a restart", async () => {
    renderApp("/decks", fail("internal", "Something broke inside."));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Something broke inside.");
    expect(alert.textContent).toContain("Restart the app");
    expect(screen.queryByRole("navigation")).toBeNull();
  });
});

describe("release builds", () => {
  afterEach(() => vi.unstubAllEnvs());

  test("no Developer tools link, and the screen is not there", async () => {
    vi.stubEnv("DEV", false);
    renderApp("/settings");
    await screen.findByRole("heading", { level: 1, name: "Settings" });
    expect(screen.queryByRole("link", { name: "Developer tools" })).toBeNull();
    cleanup();
    renderApp("/settings/developer");
    expect(await screen.findByRole("heading", { level: 1, name: "Settings" })).toBeDefined();
    expect(screen.queryByText("Core version 9.9.9")).toBeNull();
  });

  test("a Tauri debug build gets them through TAURI_ENV_DEBUG", async () => {
    vi.stubEnv("DEV", false);
    vi.stubEnv("TAURI_ENV_DEBUG", "true");
    renderApp("/settings");
    expect(await screen.findByRole("link", { name: "Developer tools" })).toBeDefined();
  });

  test("the app passes extra developer tools to the Developer screen", async () => {
    window.history.replaceState(null, "", "/#/settings/developer");
    const client = new CoreClient(
      createFakeTransport({
        getCoreInfo: () => ({ coreVersion: "9.9.9" }),
        getCollectionInfo: () => collectionInfo,
      }),
    );
    render(
      <CoreProvider client={client}>
        <PlatformProvider platform={{ cardFrameUrl: "about:blank", setSystemTheme: () => {} }}>
          <App extraDeveloperTools={<p>web panel</p>} />
        </PlatformProvider>
      </CoreProvider>,
    );
    expect(await screen.findByText("web panel")).toBeDefined();
  });
});
