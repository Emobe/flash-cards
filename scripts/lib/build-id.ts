/**
 * The build ID shown in Settings next to the version (ADR 0012): the short git commit.
 * Plain Node APIs: Vite loads its config with whichever runtime runs it.
 */

import { execFileSync } from "node:child_process";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

function git(args: string[]): string | null {
  try {
    const cwd = dirname(fileURLToPath(import.meta.url));
    return execFileSync("git", args, { cwd, stdio: ["ignore", "pipe", "ignore"] })
      .toString()
      .trim();
  } catch {
    return null;
  }
}

/**
 * `dev` under the Vite dev server; otherwise the short commit, with `-dirty` when the working
 * tree has uncommitted changes. `unknown` when git cannot answer (a source archive).
 */
export function buildId(command: "serve" | "build"): string {
  if (command === "serve") return "dev";
  const commit = git(["rev-parse", "--short", "HEAD"]);
  if (commit === null) return "unknown";
  const status = git(["status", "--porcelain"]);
  return status ? `${commit}-dirty` : commit;
}
