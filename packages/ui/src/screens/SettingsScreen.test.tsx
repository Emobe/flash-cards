import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { CoreClient, createFakeTransport } from "core-client";
import { afterEach, describe, expect, test } from "vitest";
import { CoreProvider } from "../core";
import { PlatformProvider } from "../platform";
import { RouterProvider } from "../router";
import { ThemeProvider } from "../theme";
import { SettingsScreen } from "./SettingsScreen";

afterEach(cleanup);

function renderSettings(getCoreInfo: () => unknown, build?: string) {
  const client = new CoreClient(
    createFakeTransport({
      getCoreInfo,
      getBackupSettings: () => ({ intervalHours: 24, keep: 5, lastError: null }),
      listBackups: () => ({ backups: [] }),
    } as never),
  );
  render(
    <CoreProvider client={client}>
      <PlatformProvider
        platform={{ cardFrameUrl: "", localBackups: false, setSystemTheme: () => undefined }}
      >
        <ThemeProvider>
          <RouterProvider>
            <SettingsScreen developerTools={false} build={build} />
          </RouterProvider>
        </ThemeProvider>
      </PlatformProvider>
    </CoreProvider>,
  );
}

describe("About in Settings", () => {
  test("shows the core version and the build", async () => {
    renderSettings(() => ({ coreVersion: "0.1.0" }), "a1b2c3d-dirty");
    expect(await screen.findByText("Version 0.1.0 (a1b2c3d-dirty)")).toBeDefined();
    expect(screen.getByText("About")).toBeDefined();
  });

  test("shows the version alone when the build is unknown", async () => {
    renderSettings(() => ({ coreVersion: "0.1.0" }));
    expect(await screen.findByText("Version 0.1.0")).toBeDefined();
  });

  test("shows nothing broken while the call is pending", () => {
    renderSettings(() => new Promise(() => undefined), "dev");
    expect(screen.queryByText("About")).toBeNull();
    expect(screen.getByText("Appearance")).toBeDefined();
  });

  test("shows nothing broken when the call fails", async () => {
    renderSettings(() => {
      throw new Error("no core");
    }, "dev");
    await waitFor(() => expect(screen.getByText("Appearance")).toBeDefined());
    expect(screen.queryByText("About")).toBeNull();
    expect(screen.queryByText(/Version/)).toBeNull();
  });
});
