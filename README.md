# Flash cards (working name)

A spaced repetition flashcard app: Rust core, Tauri apps for desktop and mobile, a shared React UI.
Planning docs are in [`docs/`](docs/README.md); the layout is explained in
[ADR 0001](docs/adr/0001-workspace-layout.md).

## Layout

```
crates/fc-core     Rust core library (domain logic)
apps/native        Tauri v2 host for desktop and Android (src-tauri/) and its Vite entry
packages/ui        Shared React UI
xtask              Repository tasks (`cargo xtask ...`)
scripts/           Bun scripts (e.g. the dev launcher)
docs/              Product, process, roadmap, phases and ADRs
```

## Prerequisites

All free. Bun is used for everything JavaScript: do not use npm, yarn or pnpm, and Node is not needed.

### Manjaro / Arch

```sh
# System libraries for Tauri
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg

# Rust (the exact toolchain is pinned in rust-toolchain.toml and installed automatically)
sudo pacman -S --needed rustup && rustup default stable

# Bun (version pinned in .bun-version)
curl -fsSL https://bun.sh/install | bash

# Licence and advisory checker
cargo install --locked cargo-deny
```

### Windows

1. [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with
   "Desktop development with C++" (free).
2. WebView2: preinstalled on Windows 10 (recent) and 11.
3. Rust via [rustup](https://rustup.rs/) (MSVC toolchain).
4. Bun: `powershell -c "irm bun.sh/install.ps1 | iex"`
5. `cargo install --locked cargo-deny`

Not yet verified on Windows: deferred to a manual check by Anthony (see `docs/phases/00-foundations.md`).

## Commands

Run from the repository root.

| Command | What it does |
| --- | --- |
| `bun install` | Install JS dependencies |
| `cargo xtask check` | Run every check (Rust fmt, clippy, tests, cargo-deny; Biome, TypeScript, Vitest). Must pass before a PR. |
| `cargo xtask fmt` | Format all Rust and JS/TS code |
| `bun run dev` | Launch the desktop app with hot reload (via `scripts/dev.ts`) |

## Troubleshooting

- **Blank window on Linux (notably NVIDIA GPUs).** A known WebKitGTK issue with its DMA-BUF
  renderer. `bun run dev` sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` on Linux automatically, unless
  the variable is already set in your environment. If you launch the app any other way, for example the built binary, set it
  manually. How release builds handle this is not decided yet.
