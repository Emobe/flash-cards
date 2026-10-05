# 0003: Web client approach and storage on every target

Status: Accepted
Date: 2026-10-05

## Context

`PRODUCT.md` requires a web client that is "a full client that works offline once loaded, not a
cut-down companion", sharing the one React UI. ADR 0002 left the web host for this step: the core
as wasm in a dedicated worker behind the same `Transport`, with cancellation unsolved because a
worker busy in a synchronous wasm call cannot receive a message. Step 1.1 ("Storage works on all
targets per the web ADR") also needs this ADR to say how the core stores data, because whatever the
browser can do limits what every target does.

Questions to answer: how the core runs in the browser, where its data lives, browser support,
storage limits, performance, and what the UI needs from the bridge so the same UI works on Tauri and
the web. The review question is whether the bridge from 0.3 holds up.

### Findings

Verified with a throwaway experiment in a scratch directory outside the repo: a crate depending on
the repo's real `fc-api` by path, plus `rusqlite`, built for `wasm32-unknown-unknown` with the
pinned toolchain (1.98.1), bound with `wasm-bindgen` 0.2.129, run in a module worker. A Bun server
served it, and headless Brave 143 (Chromium) on Linux loaded it with a throwaway profile. Results
were posted back to the server.

1. **The bridge compiles and runs in wasm unchanged.** `fc_api::dispatch` returned `getCoreInfo`,
   the divide-by-zero `invalidInput` error and `unknownMethod` exactly as on native. Nothing in
   `fc-api` or `fc-core` needed a change.
2. **SQLite works in the browser through `rusqlite`.** `rusqlite` 0.40.2 switches to
   `sqlite-wasm-rs` on `wasm32-unknown-unknown` by default. SQLite 3.53.0 compiles from C with
   the system clang (22.1.8, Manjaro). No `llvm-ar` or other extra tools were needed. The same
   `rusqlite` code then runs on native and web.
3. **Persistence via OPFS, using the SyncAccessHandle pool VFS** (`sqlite-wasm-vfs` 0.2.0,
   `opfs-sahpool`, ported from SQLite's own JS build). 10,000 rows written in one transaction
   survived a page reload and a full browser restart (new process, same profile) and were read
   back. This works without cross-origin isolation headers.
4. **Performance** (Chromium, Linux desktop, 10,000 short rows, about 500 KB of text):

   | Operation | OPFS SAH pool | IndexedDB VFS |
   | --- | --- | --- |
   | Install VFS and open, first time | 18 to 38 ms | 15 ms |
   | Open after reload or restart | 10 to 17 ms | 7 to 20 ms |
   | Insert 10,000 rows, one transaction | 14 to 17 ms | 17 ms |
   | Read all rows | 2 to 7 ms | 2 to 7 ms |
   | One-row transaction | 0.2 ms | 0.3 ms |

   A `fc-api` call costs about 1 ms round trip including `postMessage`. The wasm file is 1.35 MB
   (567 KB gzipped) at `opt-level = "s"` with LTO, without `wasm-opt`.
5. **One open per origin.** A second worker (standing in for a second tab) failed to open the same
   database: `NoModificationAllowedError: Access Handles cannot be created if there is another open
   Access Handle`. The SAH pool VFS is exclusive by design.
6. **Cancellation by flag needs cross-origin isolation.** With `Cross-Origin-Opener-Policy:
   same-origin` and `Cross-Origin-Embedder-Policy: require-corp`, a `SharedArrayBuffer` flag read
   with `Atomics.load` inside `OpContext::checkpoint` stopped a busy synchronous wasm call 300 ms
   after the page set it. A checkpoint cost well under a microsecond (4.5 million in 300 ms).
   Without the headers `SharedArrayBuffer` does not exist, and a cancel sent by `postMessage` was
   only handled after the 1.5 s call finished.
7. **Cancellation by terminating the worker works, with a delay.** A worker was terminated in the
   middle of an uncommitted 5,000-row transaction. A new worker could not open the database for
   about 2.0 s (three runs: 2,035 to 2,039 ms, retrying every 25 ms) because the old access handles
   were still held. It then opened cleanly, and the uncommitted rows were gone: SQLite's rollback
   journal undid the transaction. Reloading the page did not hit this delay in any run.
8. **Progress gets through during a synchronous call.** A sink posting from inside a 1 s busy wasm
   call delivered all 10 progress messages to the page as they were sent, not at the end.
9. **Things that trap on `wasm32-unknown-unknown`.** `std::time::Instant::now()`,
   `std::time::SystemTime::now()`, `std::thread::sleep` and any panic all abort with
   `RuntimeError: unreachable`. So the existing `debugSlow` method (it sleeps) traps on the web.
   The instance kept answering later calls, but its Rust state is undefined after a trap: borrow
   flags and locks are not released and destructors did not run.
10. **The workspace lint `forbid(unsafe_code)` holds.** A crate with `#![forbid(unsafe_code)]` and
    `#[wasm_bindgen]` exports compiled. An `EventSink` (which must be `Send + Sync`) works as a
    zero-sized type that reaches a JS callback through a `thread_local`, with no `unsafe`.
11. **Vite under Bun builds it.** `bun --bun vite build` (Vite 8.3.2, the repo's version) bundled
    a module worker created with `new Worker(new URL(...), { type: "module" })` and the
    `wasm-bindgen --target web` output, and the built bundle ran (open, write, reload, read back).
12. **The IndexedDB VFS (`relaxed-idb`) also persisted** across reload and restart (table above). By
    its own documentation it holds the whole database in memory, supports only
    `PRAGMA synchronous=OFF`, and writes changed blocks to IndexedDB asynchronously after SQLite
    syncs. A commit can therefore be reported before it is stored. Not tested by killing the tab
    mid-write.

Observed but not explained: after a browser restart, `navigator.storage.estimate()` reported
1,592 bytes used, when it had reported 6.3 MB (the pool's preallocated files) before the restart.
Headless Brave reported a 2 GiB quota and `persisted: false`.

From documentation (MDN "Storage quotas and eviction criteria", sqlite.org "persistence" page,
the `sqlite-wasm-rs` and `sqlite-wasm-vfs` READMEs, the Rust blog post on sunsetting the `rustwasm`
GitHub organisation, July 2025):

13. **Browser support.** OPFS `createSyncAccessHandle` in dedicated workers: Chrome and Edge 102+,
    Firefox 111+, Safari 16.4+ (sqlite.org lists the SAH pool VFS as working on all major browsers
    since March 2023). Module workers are needed for Vite's worker output (Firefox 114+).
14. **Quotas per origin.** Chromium: 60% of the disk. Firefox: the smaller of 10% of the disk or
    10 GiB by default, up to 50% of the disk once `navigator.storage.persist()` is granted. Safari
    17+: about 60% of the disk in the browser, about 15% in embedded web views.
15. **Eviction.** Under storage pressure, browsers evict "best-effort" origins. Persisted origins
    are skipped. Firefox and Safari can grant `persist()`. Chromium decides from engagement without
    asking. Safari also deletes all script-written storage, OPFS included, for a site with no user
    interaction in 7 days, unless the site is added to the Home Screen or Dock.
16. **`wasm-bindgen` is maintained.** It moved to a new `wasm-bindgen` GitHub organisation with
    extra maintainers when the `rustwasm` organisation was archived. It is not deprecated.
    `wasm-pack` moved to an individual maintainer.

**Not verified:** Firefox, Safari, any mobile browser, and Windows (nothing but Chromium on Linux
was run). The Windows wasm build needs a clang with the wasm32 target, which a standard Windows
setup lacks. Also unverified: `rusqlite` with its `bundled` feature on Android and Windows (step
1.1), random number generation on the web (`getrandom` needs its `wasm_js` backend, which matters
for IDs in 0.7), offline loading of the app itself (a Service Worker), the IndexedDB VFS under a
crash, and large collections (100 MB and over).

Package status checked on crates.io (2026-10-05):

| Package | Version | Licence | Note |
| --- | --- | --- | --- |
| rusqlite | 0.40.2 (Aug 2026) | MIT | about 39M downloads in 90 days |
| sqlite-wasm-rs | 0.5.5 (0.6.1 latest) | MIT | rusqlite 0.40 requires 0.5 |
| sqlite-wasm-vfs | 0.2.0 (0.3.0 latest) | MIT | 0.2.0 is the one compatible with sqlite-wasm-rs 0.5 |
| wasm-bindgen (+ CLI) | 0.2.129 (Sep 2026) | MIT OR Apache-2.0 | CLI must match the crate exactly |
| js-sys, wasm-bindgen-futures | 0.3.106, 0.4.79 | MIT OR Apache-2.0 | |

The whole wasm dependency tree is under licences already on the `deny.toml` allow-list (MIT,
Apache-2.0, Zlib, Unlicense, Unicode-3.0). It pulls in `tokio` (sync primitives only) and
`indexed_db_futures` through `sqlite-wasm-vfs`. SQLite itself is public domain.

## Options considered

### How the core runs in the browser

- **A. Rust core compiled to wasm, in a dedicated worker (chosen).** One core, the same `dispatch`
  as native (finding 1). The main thread stays free, and OPFS sync handles exist only in dedicated
  workers.
- **B. wasm on the main thread.** No worker plumbing, but every core call blocks rendering, and OPFS
  sync access handles are not available there, so storage would have to be async IndexedDB.
- **C. A thin client calling a server.** Fails the offline requirement and costs hosting.
- **D. A TypeScript reimplementation of the core.** Two cores that would drift apart, including the
  scheduler and sync merge. Rejected.

### Where data lives (decides step 1.1 for every target)

- **1. SQLite on every target, through `rusqlite`; OPFS SAH pool VFS on the web (chosen).** The
  same storage code, SQL and migrations everywhere (finding 2). Fast and durable on the web
  (findings 3 and 4). No special headers needed. Cost: one tab at a time (finding 5).
- **2. SQLite with the IndexedDB VFS (`relaxed-idb`) on the web.** Similar speed at this size
  (finding 12). But a reported commit may not be stored yet, and the whole database sits in memory.
  `PRODUCT.md` says review history is never lost, so this is a fallback only, for browsers without
  OPFS.
- **3. SQLite's own "opfs" VFS** (concurrent tabs via file locks). Needs cross-origin isolation, and
  it exists only in SQLite's JS build, not in the Rust crates. Slower than the SAH pool according to
  sqlite.org.
- **4. A storage trait in the core with a key-value backend per platform** (IndexedDB on web,
  SQLite or files on native). Two storage implementations to keep consistent, and search (Phase 1)
  would lose SQL on the web.
- **5. In-memory SQLite with periodic snapshots** to OPFS or IndexedDB. Simple, but anything since
  the last snapshot is lost when a tab is killed.

### Cancelling a running web call (ADR 0002's open question)

- **i. Terminate the worker and start a new one (chosen).** Works with no special headers. Writes
  roll back, as ADR 0002 already requires them to be atomic (finding 7). Costs about 2 s before
  the database can be reopened in Chromium. Anything held only in worker memory is lost.
- **ii. A `SharedArrayBuffer` flag read by `checkpoint()`.** Instant and keeps state (finding 6),
  but needs the page served with COOP and COEP headers. COEP `require-corp` blocks every
  cross-origin resource that does not opt in. That includes images linked from shared decks, the
  card sandbox design in 0.6, and free static hosts that cannot set headers.
- **iii. Long methods yield to the event loop between chunks.** Cancellation and other calls could
  then interleave, but `Method::call` would become async or resumable, which is a large change to
  `fc-api` for one platform.

## Decision

Option A with storage option 1 and cancellation option i.

### Storage on every target

- `fc-core` uses SQLite through `rusqlite`, with the `bundled` feature on native (so desktop and
  Android do not depend on a system SQLite). On `wasm32-unknown-unknown`, `rusqlite` uses
  `sqlite-wasm-rs` automatically. The schema and data model are step 0.7's decision. This ADR only
  fixes the engine.
- `rusqlite`, `sqlite-wasm-rs` and `sqlite-wasm-vfs` move together: upgrade them in one PR and
  pin all three exactly (`=0.40.2`, `=0.5.5`, `=0.2.0` today).
- The web host installs the `opfs-sahpool` VFS before opening the collection. Opening stays in the
  host, so `fc-core` never imports a VFS crate. It takes a path or URI and is told nothing about
  the platform.

### Rules for code that must run in wasm (enforced from step 1.1)

- No `std::time::Instant`, `SystemTime`, `thread::sleep` or threads in `fc-core` or `fc-api`
  (finding 9). The current time comes from the host. How it is passed in is for step 0.7 (which
  handles clocks differing between devices) and 1.1.
- Randomness for IDs uses `getrandom` with its `wasm_js` backend on the web. Unverified; checked
  when 0.7 picks an ID scheme.
- `cargo xtask check` runs clippy for `fc-wasm` on `wasm32-unknown-unknown`, which compiles
  `fc-core` and `fc-api` for that target too, so code that cannot compile for wasm fails the check.
  Code that compiles but traps (finding 9) cannot be caught this way. That is why the rule above
  exists.

### Web host

- **`crates/fc-wasm`**: the web counterpart of `fc-native`, `#![forbid(unsafe_code)]`, with every
  dependency behind `cfg(target_arch = "wasm32")` so the native workspace build is unaffected.
  It exports `open` (installs the VFS, opens the collection) and `call` (method, JSON input,
  optional bytes, optional op ID; returns JSON output and optional bytes, or an `ApiError`). Notices
  go out through a zero-sized `EventSink` (finding 10). A panic hook logs the panic message to the
  console before the trap.
- **Worker and transport (`apps/web`)**: a module worker loads the wasm and answers requests one at
  a time. The `Transport` keeps the queue on the main thread, so it always knows which operation is
  running:
  - `call` sends one request at a time and transfers attachment `ArrayBuffer`s rather than copying
    them.
  - `cancel(op)` on a queued call removes it and rejects it with `cancelled`. On the running call it
    terminates the worker, rejects the call with `cancelled`, then starts a new worker that retries
    `open` for up to 5 s (finding 7) before running the rest of the queue.
  - A trap (finding 9) rejects the call with `internal` ("Something went wrong. Reload the page to
    continue.") and restarts the worker the same way. The old instance is never used again.
  - Progress is throttled in the worker before it is posted, as on native.
  - `checkpoint()` never reports cancellation on the web, since there is no flag to read. Long
    methods still call it for native.
- **Another tab already has the collection open**: the spike shows a readable error. For the real
  web client (Phase 6), the default is one active tab, coordinated with the Web Locks API. Other
  tabs show "Open in another tab" with a "Use here" button that asks the active tab to release it.
  The VFS can pause and resume its handles for this. Forwarding calls from other tabs to the active
  one is possible later, but not planned.
- **Persistence**: the web client calls `navigator.storage.persist()` and tells the user if it was
  refused. The real client shows storage use from its own numbers, not `estimate()` (see the
  observation under Findings). Safari's 7-day eviction (finding 15) means a web user without sync
  can lose data. Phase 6 must cover it: suggest installing to the Home Screen or Dock, offer export,
  and show it in onboarding. With sync (Phase 4) the server holds a copy.
- **Not cross-origin isolated.** The web client does not need COOP or COEP headers. That keeps
  free static hosting open and avoids constraining the card sandbox (0.6).

### Build

- `rust-toolchain.toml` adds `targets = ["wasm32-unknown-unknown"]`, so rustup installs it with
  the toolchain.
- `wasm-bindgen-cli` is installed with `cargo install wasm-bindgen-cli --version 0.2.129 --locked`
  (per-user, like `cargo-deny`). The crate is pinned `=0.2.129`. The build compares the CLI version
  with `Cargo.lock` and stops with the install command if they differ. No `wasm-pack`: it adds a
  tool without adding anything we need.
- A C compiler that targets wasm32 is a prerequisite (clang). On Linux the system clang works. On
  Windows, LLVM's clang is needed (unverified).
- `cargo xtask wasm [--release]` builds `fc-wasm` and runs `wasm-bindgen --target web` into a
  gitignored directory in `apps/web`. `bun run web:dev` and `bun run web:build` run it, then Vite,
  so the web client builds and runs through Bun.
- `wasm-opt` is not used yet (it needs Binaryen). Revisit when bundle size matters (Phase 6).

### What the UI needs from the bridge (answer to the review question)

The bridge holds up. `Transport` and `CoreClient` stay as they are. `fc-api` and `fc-core` ran
in wasm unchanged. The web differs from native in ways the UI must allow for:

- **Calls are serialised on the web.** While a long operation runs, every other call waits. On
  native they can run alongside it (ADR 0002). Long web operations show progress, and the UI must
  not assume reads stay instant during them. If imports block the UI for too long (Phase 5 or 6),
  long methods are split into resumable chunks (cancellation option iii) for those methods only.
- **The core can be unavailable or restart.** It can be open in another tab, restarting after a
  cancel (about 2 s) or a trap. The real client needs an `unavailable` error kind and a host-level
  "core restarted, refetch" signal. Both are additive and are added in Phase 6, not now.
- **Cancel on the web loses in-memory core state.** Any cache the core keeps must be rebuildable
  from the database.
- **Platform differences go in the `Platform` interface** (ADR 0002), not `CoreClient`. On the web
  that covers asking for persistent storage and the tab handover.

## Consequences

- One storage engine and one set of migrations for every target. Search can use SQL everywhere.
- Native builds compile SQLite from C (bundled). That was already the case for the web, but it now
  also applies to desktop and Android (checked in 1.1).
- New prerequisites: the wasm target (automatic via `rust-toolchain.toml`), `wasm-bindgen-cli`
  0.2.129, and clang with wasm32 support. `README.md` lists them.
- New dependencies: `rusqlite` in `fc-core`; `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`,
  `sqlite-wasm-rs`, `sqlite-wasm-vfs` in `fc-wasm` only. All permissive.
- The `rusqlite` version is tied to `sqlite-wasm-rs` 0.5. The newer VFS crate (0.3) waits for a
  `rusqlite` release that accepts `sqlite-wasm-rs` 0.6.
- The web client works in one tab at a time.
- A cancelled web operation makes the core unavailable for about 2 s in Chromium.
- `cargo xtask check` gets slower: one extra clippy run for wasm, which compiles SQLite once.
- The web bundle starts at about 570 KB gzipped for the core alone.
- The rule against `std::time` and threads in the core is enforced only by review and tests run in
  wasm, not by the compiler.

## Revisit if

- Firefox, Safari or a mobile browser fails the spike's checks.
- A `rusqlite` release supports `sqlite-wasm-rs` 0.6, or the `sqlite-wasm-*` crates stop being
  maintained. The fallback is SQLite's own JS build with a JS-side VFS, which would need a bridge
  from Rust.
- Multi-tab use turns out to matter. Then forward calls from other tabs to the active one, or move
  to a concurrent VFS.
- The 2 s lock-out after a cancel is a problem in practice, or COOP and COEP are adopted anyway.
  Then use the `SharedArrayBuffer` flag (finding 6) when `crossOriginIsolated` is true.
- Large collections (100 MB and over, Phase 5 imports) are slow to open or hit quota limits.
- Bundle size or start-up time on phones becomes a problem (add `wasm-opt`, lazy loading).
- Windows cannot build the wasm target with a reasonable setup.
