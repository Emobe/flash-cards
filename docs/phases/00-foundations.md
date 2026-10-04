# Phase 0: Foundations and spikes

## Goal

A working repo and toolchain, the app shell running on Linux, Windows and Android, and the biggest technical risks tested before real code is written. Ends with accepted ADRs for the core architecture.

Spike code may be thrown away. ADRs are the real output.

## Exit criteria

- All checks pass locally with one command on Manjaro.
- Placeholder app runs on Manjaro, Windows and Anthony's Android phone, showing data that came from the Rust core.
- ADRs accepted for: workspace layout, UI-to-core bridge, web approach, scheduling, card sandbox, data model and sync strategy.
- A short risks list in the last ADR or a `docs/RISKS.md`.

---

## 0.1 Repository and workspace boilerplate

**Goal:** a monorepo with a place for the Rust core, the Tauri app, the React UI and the future sync server, plus the tooling to work on them.

**Scope:**
- In: workspace structure, formatting, linting, type checking, test runners, one command to run all checks, `CLAUDE.md`, these docs committed, ADR 0001 for the layout. Bun is the package manager and script runner for all JS and TS tooling (see Tooling constraints in `PRODUCT.md`).
- Out: any real features. Android (next step). CI (deferred, sorted out later).

**Acceptance criteria:**
- A fresh clone can run all checks with documented commands on Manjaro and Windows.
- The desktop app launches and shows a placeholder React screen.
- `CLAUDE.md` follows the starter rules in `PROCESS.md` and stays short.
- Only a Bun lockfile is committed. Installs, scripts, dev server and tests all run through Bun.
- Any tool that does not work cleanly with Bun is reported in the PR with the options, not silently replaced with Node.

**Review focus:** the layout ADR. Does the structure make it easy to add a server, a web client and add-ons later? Is the tooling something you are happy to live with?

**Notes:** large PR allowed for this step only.

---

## 0.2 Android build

**Goal:** the same app builds and runs on Anthony's Android phone.

**Scope:**
- In: Android target set up, debug build installed on a physical device, setup instructions for Manjaro (and Windows if practical), live reload against the dev server if available.
- Out: release signing, Play Store.

**Acceptance criteria:**
- Following the written instructions, a debug build installs and runs on the phone.
- The placeholder screen renders correctly on the phone.
- The Android dev and build workflow runs through Bun. Anything that cannot is documented with the reason and the options.

**Review focus:** are the setup instructions complete enough to redo from scratch?

---

## 0.3 UI-to-core bridge

**Goal:** the React UI calls a function in the Rust core and gets typed data back, on desktop and Android.

**Scope:**
- In: one example call and response, error handling path, an approach for keeping Rust and TypeScript types in sync, ADR.
- Out: real domain functions.

**Acceptance criteria:**
- The UI displays a value computed in the core, on desktop and phone.
- A core error is shown in the UI as a readable message.
- Changing a type in Rust causes a type error in TypeScript (or the ADR explains the chosen alternative).

**Review focus:** will this pattern still be pleasant with a hundred functions? Does it leave room for the web client to use the same UI code?

---

## 0.4 Web client spike

**Goal:** prove the web client can exist with a full offline local copy, and decide how.

**Scope:**
- In: a throwaway page that uses the core in the browser and persists data across reloads. ADR comparing approaches, with findings.
- Out: real web app.

**Acceptance criteria:**
- The spike stores data, survives a page reload and a browser restart, and reads it back through the core.
- The spike builds and runs through Bun.
- ADR covers: chosen approach, browser support, storage limits, performance observations, what the UI layer needs from the bridge to support both Tauri and web.

**Review focus:** does the bridge approach from 0.3 hold up, or does it need adjusting now?

---

## 0.5 Scheduling spike

**Goal:** choose how FSRS scheduling is implemented.

**Scope:**
- In: evaluate available FSRS implementations (licence, maintenance, features including parameter optimisation, runs on all targets including web). Small test schedules a sequence of reviews. ADR.
- Out: queues, limits, deck options (Phase 1).

**Acceptance criteria:**
- ADR with the decision and licence check (must be compatible with our own licence choice and must not be AGPL).
- Test demonstrates scheduling a card through several reviews with expected intervals.
- Confirmed working on desktop, Android and in the browser spike.

**Review focus:** licence and maintenance status of whatever is chosen.

---

## 0.6 Sandboxed card rendering spike

**Goal:** prove untrusted card HTML, CSS and JS can render safely on all platforms.

**Scope:**
- In: render a sample card with HTML, CSS, an image, audio and a little JS, isolated from the app. A deliberately malicious test card. ADR.
- Out: templates, field substitution (Phase 1).

**Acceptance criteria:**
- The malicious card cannot call app commands, read app data, read other cards, or navigate the app away.
- The legitimate card renders correctly with images, audio and JS on desktop and Android, and in the browser spike.
- ADR explains the isolation approach and its known limits.

**Review focus:** try to break it yourself. Add your own malicious card.

---

## 0.7 Data model and sync strategy (design only)

**Goal:** an accepted design for the core data model and how sync will meet the requirements, before Phase 1 builds storage.

**Scope:**
- In: ADR proposing the data model and sync strategy against every requirement in the Sync section of `PRODUCT.md`. At least two alternatives considered. Explicit handling of: offline for weeks, same card reviewed on two devices, edit versus delete, clock differences between devices, app version differences, schema changes, media. How shared decks (Phase 9) and add-on data (Phase 10) would fit later.
- Out: implementation.

**Acceptance criteria:**
- Every sync requirement is addressed explicitly, with what happens in each edge case above.
- The design states what Phase 1 must build in from day one so sync does not need a storage rewrite in Phase 4.

**Review focus:** this is the most important review of the project. Push back on anything that would ever require a "choose a side" prompt. Take your time.

**Notes:** the only design hint from planning: a card's scheduling state can be derived from its review history, which may make merging reviews simple. Claude Code is free to use or reject this.
