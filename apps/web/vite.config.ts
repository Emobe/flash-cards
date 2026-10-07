import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
import { buildId } from "../../scripts/lib/build-id";

const cardFrame = fileURLToPath(new URL("../../packages/ui/src/card/frame.html", import.meta.url));

/**
 * Serves the card-frame page (ADR 0005) at `/card-frame.html`: from the dev server's middleware,
 * and as a plain file in the build. It is the same source file the native app embeds, and it must
 * not go through Vite's HTML transforms (no HMR client inside a sandboxed card).
 */
function cardFramePage(): Plugin {
  return {
    name: "card-frame-page",
    configureServer(server) {
      server.middlewares.use("/card-frame.html", (_request, response) => {
        response.setHeader("Content-Type", "text/html; charset=utf-8");
        response.end(readFileSync(cardFrame));
      });
    },
    generateBundle() {
      this.emitFile({
        type: "asset",
        fileName: "card-frame.html",
        source: readFileSync(cardFrame),
      });
    },
  };
}

export default defineConfig(({ command }) => ({
  plugins: [react(), cardFramePage()],
  define: { __BUILD_ID__: JSON.stringify(buildId(command)) },
  worker: { format: "es" },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
}));
