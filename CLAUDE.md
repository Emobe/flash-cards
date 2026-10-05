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
- `cargo xtask doctor-android`: check the Android setup (not part of `check`).
- `bun run android:dev`: run on the connected phone with live reload over USB.
- `bun run android:build` then `bun run android:install`: build, install and launch a standalone debug APK.

## Stop conditions

Work through a step without asking, except stop and ask if:

- the step needs a design decision with no accepted ADR;
- a requirement in `docs/` is unclear, contradictory or looks wrong;
- something could cost money;
- something needs a system-wide install, a change outside the repo, or editing shell config;
- a tool does not work under Bun;
- an acceptance criterion cannot be verified, or the plan needs to deviate.

## Standing rules

- `master` is the branch name.
- There is no CI yet.
- Pin exact versions of key tools.
- Ask before adding dependencies that are not in the plan.
- Never touch Anthony's phone beyond installing the app and taking screenshots. Never unlock it.

## Usage

Anthony is on a limited plan. Commit often, keep `docs/STATUS.md` current, and do not start research or experiments the step does not need.
