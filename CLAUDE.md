# CLAUDE.md

Planning-first project. Anthony owns product decisions and reviews every ADR and PR. You own technical decisions, but propose them and wait for approval before building.

Docs live in `docs/`: start with `docs/README.md`. `PRODUCT.md` is the source of truth for what we build, `PROCESS.md` for how work flows (step format, ADRs, definition of done), `ROADMAP.md` and `phases/` for order of work, `adr/` for decisions.

## Rules

- Read `docs/PRODUCT.md` and all Accepted ADRs before planning a step.
- Propose a plan before writing code. Write an ADR for any significant design decision. Don't build anything that depends on a Proposed ADR.
- Never read, copy or adapt code from Anki or AnkiDroid (AGPL). Feature ideas come from `docs/`, not Anki's source.
- Keep PRs to one coherent change. Propose a split if a step is too large.
- Don't add dependencies without stating why in the PR.
- Card content is untrusted. Never give rendered card content access to app commands or data.
- Every UI feature must work on mobile and desktop.
- Use Bun for all Node-related work (installs, scripts, tests, dev servers). No npm, yarn or pnpm, and no lockfiles from them. If something does not work with Bun, ask before falling back (see "Tooling constraints" in `docs/PRODUCT.md`).
- Nothing besides Anthony's Claude subscription may cost money: no paid CI, hosting, services or certificates. Flag anything that could incur charges before using it (see `docs/PRODUCT.md`).
- If the docs contradict themselves or look wrong, say so instead of guessing.

## Commands

See `README.md` for prerequisites.

- `cargo xtask check`: run every check. Must pass before opening a PR (there is no CI).
- `cargo xtask fmt`: format Rust and JS/TS.
- `bun run dev`: launch the desktop app.
