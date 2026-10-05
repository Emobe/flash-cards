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

## Build notes (step 0.5)

Built to this ADR with no change to the decision. `fc_core::scheduling` wraps `fsrs` with our own
`Rating`, `Memory`, `Scheduler` and `optimise` (no `fsrs` type reaches `fc-api` or the UI).
The spike methods `spikeSchedule` and `spikeOptimise` and the `SchedulingSpike` section in
`packages/ui` are temporary. The optimiser runs on made-up histories from `synthetic_histories`.

- **Test.** Five Good reviews at 0.9 retention give 2, 11, 46, 163 and 497 days. A second test
  recomputes the first two reviews from the published FSRS-6 formulas without the crate (S0, D0,
  forgetting curve, recall stability) and matches within 0.01. Other tests: a lapse shortens the
  interval, Again, Hard, Good and Easy are ordered, higher retention gives shorter intervals,
  invalid retention is rejected, and optimisation returns 21 finite parameters the scheduler accepts.
  A lapse after two Good reviews gives 2 days, not 1 (my first guess was wrong, FSRS keeps some
  stability after a lapse).
- **Browser (finding 5 answered).** Release wasm in headless Brave (Chromium) on Linux, driven over
  the DevTools protocol: intervals 2, 11, 46, 163, 497, identical to native. `compute_parameters`
  **runs on wasm**: 200 cards (1,200 training items) in 11 ms, 2,000 cards in 66 ms. The parameters
  match native to every digit shown (w0 to w2: 0.242, 0.811, 1.883 and 1.419, 1.702, 3.265).
  `rayon` is only used in `evaluate_with_time_series_splits`, `simulate` and the optional
  `cost_adr` code, none of which the spike calls. They are **not** verified on wasm, and are likely
  to need a single-threaded path or a feature gate. Step 1.8 must test them if it uses them.
- **Desktop.** Tauri window (debug build, Linux): same intervals, optimiser on 200 cards in 32 ms.
  Native timings in `cargo test` (debug): 200 cards 29 ms, 2,000 cards 100 ms.
- **Android.** The debug APK built with `fsrs` for `aarch64-linux-android` and installed. The phone
  was locked, and the project rules forbid unlocking it or tapping, so the two checks were **not**
  run on the phone. The same fc-core code is what ran on desktop.
- **Size.** The release wasm grew from 1,836 KB to 2,043 KB (788 KB gzipped) with `fsrs`.
- **Dependencies added.** `fsrs` 6.6.2 and its tree (itertools, log, ndarray, priority-queue,
  rand, rayon, serde, snafu, strum and their dependencies) in `fc-core`. `getrandom` 0.4.3 with
  `wasm_js` (wasm only, pinned to the version `rand` 0.10 already resolves to).
  `cargo deny` passes with one new exception, `priority-queue` under MPL-2.0.
- **Not verified:** phone, Windows, Firefox and Safari, `simulate` and the time-series evaluation
  on wasm, optimiser quality on real review history (the data here is invented), and cancelling a
  long optimisation (`compute_parameters` has no cancel hook, only a progress state).
