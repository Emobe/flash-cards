/** Shared helpers for the `dist:` scripts (plain Bun and Node APIs: they also run on Windows). */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "bun";

export function fail(script: string, message: string): never {
  console.error(`${script}: ${message}`);
  process.exit(1);
}

/** The app version: `[workspace.package] version` in the root Cargo.toml (ADR 0012, decision 6). */
export function workspaceVersion(script: string): string {
  const manifest = readFileSync(join(import.meta.dir, "../../Cargo.toml"), "utf8");
  const section = manifest.split("[workspace.package]")[1] ?? "";
  const match = section.match(/^version\s*=\s*"(\d+\.\d+\.\d+)"/m);
  if (!match) {
    fail(script, 'no version = "x.y.z" under [workspace.package] in the root Cargo.toml.');
  }
  return match[1];
}

/** Runs a command, echoing it; exits on failure. */
export function run(
  script: string,
  cmd: string[],
  cwd: string,
  env: Record<string, string | undefined> = process.env,
): void {
  console.log(`$ ${cmd.join(" ")}`);
  const { exitCode } = spawnSync(cmd, { cwd, env, stdio: ["inherit", "inherit", "inherit"] });
  if (exitCode !== 0) {
    fail(script, `${cmd[0]} failed (exit code ${exitCode})`);
  }
}
