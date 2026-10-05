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

// A card that navigates its own frame to the app gets a blank page, not a second copy of the app
// holding a transport (ADR 0005).
if (window.top === window) {
  const client = new CoreClient(createTauriTransport());

  createRoot(root).render(
    <StrictMode>
      <CoreProvider client={client}>
        <App />
      </CoreProvider>
    </StrictMode>,
  );
}
