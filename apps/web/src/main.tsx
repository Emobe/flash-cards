import { CoreClient } from "core-client";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App, CoreProvider } from "ui";
import "./styles.css";
import { SpikePanel } from "./SpikePanel";
import { createWebTransport } from "./webTransport";
import type { WorkerLike } from "./workerProtocol";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Missing #root element in index.html");
}

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
      <App />
      <div className="app">
        <SpikePanel />
      </div>
    </CoreProvider>
  </StrictMode>,
);
