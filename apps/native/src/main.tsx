import { CoreClient } from "core-client";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App, CoreProvider } from "ui";
import "./styles.css";
import { createTauriTransport } from "./tauriTransport";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Missing #root element in index.html");
}

const client = new CoreClient(createTauriTransport());

createRoot(root).render(
  <StrictMode>
    <CoreProvider client={client}>
      <App />
    </CoreProvider>
  </StrictMode>,
);
