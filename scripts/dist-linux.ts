/**
 * Builds the Linux release package (`bun run dist:linux`, ADR 0012, decision 3).
 *
 * `tauri build --bundles deb` makes a .deb, which `packaging/arch/PKGBUILD` repackages as a pacman
 * package with `makepkg`. The package is copied to `release/`. The .deb is only the PKGBUILD's
 * input, not an installer.
 */

import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "bun";
import { fail, run, workspaceVersion } from "./lib/dist";

const SCRIPT = "dist-linux";
const ROOT = join(import.meta.dir, "..");
const DEB_DIR = join(ROOT, "target/release/bundle/deb");

if (process.platform !== "linux") {
  fail(SCRIPT, "this builds the Linux package and runs on Linux only.");
}
if (spawnSync(["which", "makepkg"]).exitCode !== 0) {
  fail(SCRIPT, "makepkg not found. It is part of pacman on Arch-based systems (Manjaro).");
}

const version = workspaceVersion(SCRIPT);

// Remove old .debs so the one found below is this build's.
rmSync(DEB_DIR, { recursive: true, force: true });
run(SCRIPT, ["bun", "run", "--cwd", "apps/native", "tauri", "build", "--bundles", "deb"], ROOT);

const deb = existsSync(DEB_DIR) ? readdirSync(DEB_DIR).find((f) => f.endsWith(".deb")) : undefined;
if (!deb) {
  fail(SCRIPT, `no .deb in ${DEB_DIR} after the Tauri build.`);
}

const work = mkdtempSync(join(tmpdir(), "flash-cards-pkg-"));
try {
  cpSync(join(ROOT, "packaging/arch"), work, { recursive: true });
  cpSync(join(DEB_DIR, deb), join(work, "flash-cards.deb"));
  run(SCRIPT, ["makepkg", "-f", "--nodeps", "--skipinteg"], work, {
    ...process.env,
    FC_VERSION: version,
  });

  const built = readdirSync(work).find((f) => /^flash-cards-.*\.pkg\.tar(\..+)?$/.test(f));
  if (!built) {
    fail(SCRIPT, "makepkg produced no package.");
  }
  const suffix = built.slice(built.indexOf(".pkg.tar"));
  const outDir = join(ROOT, "release");
  mkdirSync(outDir, { recursive: true });
  const out = join(outDir, `flash-cards-${version}-linux-x86_64${suffix}`);
  cpSync(join(work, built), out);
  console.log(`\nBuilt ${out}\nInstall: sudo pacman -U ${out}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
