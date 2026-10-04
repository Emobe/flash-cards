# 0001: Repository and workspace layout

Status: Accepted
Date: 2026-10-04

## Context

The project is a Rust core shared by a Tauri v2 app (desktop and Android), a web client, a dev CLI, and
a self-hostable sync server, with one React UI shared across all platforms (`PRODUCT.md`). We need a
layout that lets us add each of these without restructuring, and that works on Linux and Windows.

Constraints from `PRODUCT.md`:
- Bun for all Node-related work; npm, yarn and pnpm are not allowed.
- Zero running cost. CI is deferred (step 0.1 scope), so checks run locally with one command.

Later ADRs (bridge, web approach, sandbox, data model) will refine this one, so the layout should not
pre-empt them.

## Options considered

1. **Polyglot monorepo: Cargo workspace plus Bun workspace in one repo (chosen).** Rust crates in
   `crates/`, Tauri host in `apps/desktop`, shared React UI in `packages/ui`, task entry point
   `cargo xtask`.
2. **Tauri-conventional single app.** The Tauri template's layout, with the core as a module in
   `src-tauri`. Simplest at first, but the core would be entangled with Tauri, blocking the server,
   CLI and wasm uses.
3. **Separate repositories** for core, app and server. Independent versioning, but cross-cutting
   changes become multi-repo PRs, which is poor for one developer plus Claude Code.
4. **Task runner: `just` or Bun scripts instead of xtask.** `just` adds an install and a Windows shell
   question. Bun scripts can't cleanly orchestrate cargo and Bun in one cross-platform entry point
   without relying on shell behaviour.

JS toolchain options under Bun (all verified to run with no Node on `PATH`):
- Dev server and build: Vite (chosen).
- Tests: Vitest run through Bun (chosen, shares Vite config, mature DOM testing) vs `bun test`
  (faster, but needs a DOM shim set up separately).
- Lint and format: Biome (chosen, native binary) vs ESLint + Prettier (JS tools, more config).

## Decision

Option 1.

```
Cargo.toml              Rust workspace: crates/*, apps/*/src-tauri, xtask
package.json            Bun workspace: packages/*, apps/*
crates/fc-core          domain logic
apps/desktop            Tauri v2 host (src-tauri/) + thin Vite entry
packages/ui             shared React UI
xtask                   `cargo xtask check` and `cargo xtask fmt`
scripts                 Bun scripts (dev launcher)
```

- `crates/fc-core` holds domain logic with no dependency on Tauri, the UI, or the OS beyond what it
  needs, so it can target desktop, Android, wasm, the CLI and the server.
- `apps/desktop` is the Tauri v2 host. Its Rust crate (`fc-desktop`) is mobile-ready (`lib.rs` with
  `mobile_entry_point`), so Android (0.2) adds a target, not a restructure. Its frontend is a thin
  Vite entry mounting `packages/ui`.
- `packages/ui` is the shared React UI. It must not import Tauri APIs directly. Platform access goes
  through an interface to be defined in the bridge ADR (0.3).
- Future members slot in without root edits: `crates/fc-server`, `crates/fc-cli`, `apps/web`.
- Naming: Rust crates use an `fc-` prefix (avoids clashing with Rust's built-in `core`); JS packages
  use neutral unscoped names (`ui`, `desktop`). No app name is baked in, so naming the app later is cheap.
- **Bun** is the package manager, workspace manager, script runner, dev server host and test runner
  for all JS and TS. The only committed JS lockfile is `bun.lock`. `cargo xtask check` fails if a
  lockfile from another package manager exists. Bun is pinned via `packageManager` and `.bun-version`.
  Node is not a project dependency.
- `cargo xtask check` runs every check: lockfile guard, `cargo fmt --check`, clippy with warnings
  denied, `cargo test`, `cargo deny check`, `bun install --frozen-lockfile`, Biome, `tsc --build`, Vitest.
- `cargo-deny` enforces a licence allow-list (permissive licences only; AGPL and GPL fail).
  MPL-2.0 is allowed only for named crates that arrive via Tauri, so a new MPL dependency fails and
  must be reviewed. One advisory (RUSTSEC-2024-0370, unmaintained `proc-macro-error` via Tauri's
  Linux GTK stack) is ignored with a reason, as no upgrade is available to us. Revisit on every
  Tauri upgrade and remove the ignore once it is no longer needed.
- Pinned Rust toolchain (`rust-toolchain.toml`). TypeScript strict mode with project references.
- Exact versions (no ranges) for TypeScript, Biome, Vite and Vitest, so tool behaviour only changes
  in a deliberate upgrade. Biome's config schema is pinned to the same version, so a Biome upgrade
  (and any `biome migrate`) is a reviewed change.
- Biome lint: the recommended preset, plus the React domain (hooks rules: `useHookAtTopLevel`,
  `useExhaustiveDependencies`, `useJsxKeyInIterable` and others) set explicitly rather than
  auto-detected, and the whole `a11y` group enabled as errors.
- `bun run dev` goes through `scripts/dev.ts`, a small Bun wrapper that sets
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux only (blank-window workaround for WebKitGTK on some
  GPUs). Kept out of Rust code. Release builds are not covered; decided later.
- All manifests are private/unpublished and carry no licence field until the licence is decided.

## Consequences

- Core stays reusable and testable without a UI. The server, CLI and web can share it.
- Two ecosystems in one repo means two lockfiles (`Cargo.lock`, `bun.lock`), mitigated by the single
  `xtask check` entry point.
- JS tools ship `#!/usr/bin/env node` shebangs. Under Bun they run on Bun's runtime. Tested on Linux
  for Vite (build and dev server), Vitest, tsc, Biome and the Tauri CLI (including `tauri dev`) with
  Node removed from `PATH`. Any future tool that fails under Bun is reported, not silently run on Node.
- With no CI, nothing stops a commit that fails checks; running `cargo xtask check` before a PR is
  part of the definition of done. Windows is not verified automatically.
- Contributors need Rust, Bun and cargo-deny, plus Tauri system dependencies. Documented in `README.md`.

## Assumptions (kept up to date as later ADRs land)

- Where generated Rust-to-TypeScript bindings live (0.3).
- Whether the core also compiles to a wasm package under `packages/` (0.4).
- The Bun toolchain and `cargo xtask check` behave the same on Windows as on Linux. Not yet
  verified: deferred to a manual check by Anthony (see the 0.1 acceptance criteria).
- Data model and sync strategy (0.7) do not change crate boundaries.

## Revisit if

- The web approach (0.4) requires the core to ship as a JS package with its own build, adding a
  `packages/fc-core-wasm` member.
- Bun incompatibilities appear in Vite, Vitest, Tauri or a later dependency.
- CI is introduced (needs a zero-cost option; see `PRODUCT.md`).
- A Tauri upgrade drops gtk-rs 0.18 (remove the RUSTSEC-2024-0370 ignore).
- A different task runner turns out to be significantly better on Windows.
- The add-on system (Phase 10) needs its own top-level structure.
