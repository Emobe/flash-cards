# 0004: Scheduling implementation (FSRS)

Status: Accepted
Date: 2026-10-05

## Context

`PRODUCT.md` requires FSRS with per-user parameter optimisation from review history, on every target
(desktop, Android, web). Step 0.5 chooses how it is implemented. Step 1.7 (queues and answering) and
1.8 (optimisation from history) build on the choice. The licence must be compatible with a not yet
chosen licence for this project and must not be AGPL (`PRODUCT.md`, "Rules about Anki"). No code from
Anki is read or used. The FSRS crates below are separate projects of the open-spaced-repetition
organisation, and Anthony allowed reading their source for this step.

## Findings

Checked 2026-10-05 on crates.io and by building in a scratch crate outside the repo, with the repo's
pinned toolchain and `deny.toml`.

| | `fsrs` (fsrs-rs) | `rs-fsrs` |
| --- | --- | --- |
| Version | 6.6.2 (updated Aug 2026) | 1.2.1 (Oct 2024) |
| Recent downloads | about 79,000 | about 12,000 |
| Licence | BSD-3-Clause | MIT |
| Algorithm | FSRS-6 (21 parameters) | 19 parameters (FSRS-5 or older) |
| Scheduler | yes (`next_states`) | yes |
| Parameter optimiser | yes (`compute_parameters`) | no |
| Simulation, optimal retention | yes | no |

1. **`fsrs` is pure Rust in this version.** `burn` is only a dev-dependency of 6.6.2, so training
   uses `ndarray`, not a tensor framework. The whole normal dependency tree is 43 crates.
2. **It builds for every target.** Native Linux (test ran), `aarch64-linux-android` (`cargo check`),
   and `wasm32-unknown-unknown` (release build, a 35 KB wasm file for a one-function crate before any
   other code). Wasm needs `getrandom` 0.4 with its `wasm_js` feature set, a target-specific
   dependency in our crate, as already noted in ADR 0003.
3. **Scheduling a card through five "Good" reviews** (default parameters, desired retention 0.9,
   each review on the due day) gave intervals 2, 11, 46, 163, 497 days. These are the numbers the
   step's unit test will pin, after checking them by hand against the published FSRS formulas.
4. **Licence tree.** All of it is on our allow-list except one crate: `priority-queue` 2.7.0, which
   `fsrs` pins exactly (`=2.7.0`). Its licence is `LGPL-3.0-or-later OR MPL-2.0`. It is a dual
   licence, so we may take it under MPL-2.0 (file-level copyleft, no AGPL or GPL). `deny.toml` today
   allows MPL-2.0 only for named crates, so this needs a named exception, which is a licence
   decision for Anthony (the step's review focus).
5. **Unverified:** whether `compute_parameters` and `simulate` run in the browser. `fsrs` uses
   `rayon` (`rayon::spawn` in `inference.rs`, `into_par_iter` in `simulation.rs`), and threads do not
   exist on `wasm32-unknown-unknown` without extra setup. The plain scheduler (`next_states`) does
   not call it in the scratch test. To be tested once this ADR is accepted.

## Options considered

1. **`fsrs` 6.6.2, pinned (proposed).** Current algorithm, includes the optimiser, active, the
   reference implementation.
2. **`rs-fsrs`.** MIT and simple, but it is an older algorithm version, has no optimiser (which
   `PRODUCT.md` requires), and has had no release in a year.
3. **Write our own scheduler.** About a day of work for the scheduler, but the optimiser is much
   more, and our results would drift from the reference. Only if option 1 fails on a target.

## Decision (proposed)

Option 1: depend on `fsrs` at an exact version in `fc-core`, behind a small module of our own
(types for card memory state, `next_states` wrapper, our own review rating enum), so the crate is
not part of `fc-api` or the UI. Pin exactly (`=6.6.2`), as for other key crates.

Needs Anthony's approval:
- **Licence:** take `priority-queue` under MPL-2.0 and add it to the `deny.toml` MPL exceptions with
  a reason. If refused, option 3, or asking the `fsrs` maintainers to drop the dependency.
- **Dependency:** `fsrs` and its 43-crate tree are new dependencies of `fc-core`.

## Consequences

- Scheduler and optimiser come from one maintained source and follow FSRS releases.
- We carry a weak-copyleft (MPL-2.0) crate. File-level only: it constrains changes to that crate's
  own files, not our code. Revisit if the project's own licence makes that awkward.
- Parameter optimisation in the browser may need a single-threaded path or a cap on `rayon`. If the
  build step shows it fails, that is a follow-up decision, not hidden.

## Revisit if

- `fsrs` stops being maintained, or a new release adds a dependency with a rejected licence.
- Optimisation cannot run on the web or Android within acceptable time (1.8).
- The project licence is chosen and conflicts with MPL-2.0 for `priority-queue`.

## Not yet verified (filled in at build)

- Scheduler in the browser spike and on the phone. Optimiser on all targets.
