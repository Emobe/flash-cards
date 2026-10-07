# CLAUDE.md

Planning-first project. Anthony owns product decisions and reviews every ADR and PR. You own technical decisions, but propose them and wait for approval before building.

Docs live in `docs/`: start with `docs/README.md`. `PRODUCT.md` is the source of truth for what we build, `PROCESS.md` for how work flows (step format, ADRs, definition of done), `ROADMAP.md` and `phases/` for order of work, `adr/` for decisions.

## Rules

- Read `docs/PRODUCT.md` and all Accepted ADRs before planning a step.
- Propose a plan before writing code. Any significant design decision needs an ADR, written in an Opus `/adr` session, not in a `/step` session. Don't build anything that depends on a Proposed ADR.
- Never read, copy or adapt code from Anki or AnkiDroid (AGPL). Feature ideas come from `docs/`, not Anki's source.
- Keep PRs to one coherent change. Propose a split if a step is too large.
- Don't add dependencies without stating why in the PR.
- Card content is untrusted. Never give rendered card content access to app commands or data.
- Every UI feature must work on mobile and desktop.
- Use Bun for all Node-related work (installs, scripts, tests, dev servers). No npm, yarn or pnpm, and no lockfiles from them. If something does not work with Bun, ask before falling back (see "Tooling constraints" in `docs/PRODUCT.md`).
- Nothing besides Anthony's Claude subscription may cost money: no paid CI, hosting, services or certificates. Flag anything that could incur charges before using it (see `docs/PRODUCT.md`).
- If the docs contradict themselves or look wrong, say so instead of guessing.
- Every step ends with an explicit "Deviations from the plan" list: in the ADR build notes (linked from `docs/STATUS.md`) and in the final report. Say "none" if there are none. A deviation is anything that differs from the plan approved in chat, including size, commit structure, extra or missing items and how an unspecified detail was decided.

## Commands

See `README.md` for prerequisites.

- `cargo xtask check`: run every check. Must pass before opening a PR (there is no CI).
- `cargo xtask fmt`: format Rust and JS/TS.
- `bun run dev`: launch the desktop app.
- `bun run web:dev`: build the wasm core and run the web client (debug wasm has the `debug*` methods; `web:build` is release).
- `cargo xtask doctor-android`: check the Android setup (not part of `check`).
- `bun run android:dev`: run on the connected phone with live reload over USB.
- `bun run android:build` then `bun run android:install`: build, install and launch a standalone debug APK.

## Stop conditions

Work through a step without asking, except stop and ask if:

- the step needs a design decision with no accepted ADR. Do not write the ADR yourself in a `/step` session: ADRs are Opus design sessions (`/adr`). Stop at the start, before any branch, ADR or code, and tell Anthony the step needs `/adr <step>` first. This holds even when the step's Model line says "Sonnet build" and even when the brief says "an ADR or short design doc";
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
- Use the `gh` CLI for everything that goes to GitHub: opening, merging and checking PRs. `gh pr create` cannot push without a terminal prompt, so push the branch first over HTTPS with `gh` as the credential helper, which needs no config change: `git -c credential.helper= -c credential.helper='!gh auth git-credential' push -u https://github.com/Emobe/flash-cards.git <branch>`. Never push over SSH (the `origin` remote) or with a plain `git push`.

## Usage

Anthony is on a limited plan. Commit often, keep `docs/STATUS.md` current, and do not start research or experiments the step does not need.
