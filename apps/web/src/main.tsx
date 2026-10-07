import { CoreClient } from "core-client";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App, CoreProvider, PlatformProvider, updateThemeColorMeta } from "ui";
import { BackupPanel } from "./BackupPanel";
import { SpikePanel } from "./SpikePanel";
import { createWebTransport } from "./webTransport";
import type { WorkerLike } from "./workerProtocol";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Missing #root element in index.html");
}

// A card that navigates its own frame to the app gets a blank page, not a second copy of the app
// with its own core worker (ADR 0005).
if (window.top === window) {
  const client = new CoreClient(
    createWebTransport(
      () =>
        new Worker(new URL("./core.worker.ts", import.meta.url), {
          type: "module",
        }) as unknown as WorkerLike,
    ),
  );

  createRoot(root).render(
    <StrictMode>
      <CoreProvider client={client}>
        <PlatformProvider
          platform={{ cardFrameUrl: "/card-frame.html", setSystemTheme: updateThemeColorMeta }}
        >
          <App
            extraDeveloperTools={
              <>
                <SpikePanel />
                <BackupPanel />
              </>
            }
          />
        </PlatformProvider>
      </CoreProvider>
    </StrictMode>,
  );
}
