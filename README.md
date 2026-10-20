# Flash cards (working name)

A spaced repetition flashcard app in the same space as Anki, built to be easy to use, equally
capable on phone and desktop, and to sync without conflicts.

A Rust core, Tauri apps for desktop and Android, a web client that works offline, and one shared
React UI. In early development.

## Quick start

You need Rust (via rustup), [Bun](https://bun.sh) and the Tauri system libraries. The full list for
each platform is in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#prerequisites).

```sh
bun install
bun run dev          # desktop app
bun run web:dev      # web client
cargo xtask check    # every check; must pass before a PR
```

Bun is used for all JavaScript work: no npm, yarn or pnpm.

## Docs

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): repository layout, prerequisites, commands, Android
  setup and dogfood builds.
- [docs/README.md](docs/README.md): product brief, roadmap, process and architecture decisions.
