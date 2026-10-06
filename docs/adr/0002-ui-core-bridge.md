# 0002: UI-to-core bridge

Status: Accepted
Date: 2026-10-05

## Context

The React UI in `packages/ui` must call the Rust core on desktop and Android now (Tauri), and in the
browser later (step 0.4, likely the core compiled to wasm in a worker). ADR 0001 says `packages/ui`
must not import Tauri APIs and leaves two questions to this ADR: the platform interface the UI uses,
and where generated Rust-to-TypeScript bindings live.

Step 0.3 needs one example call, a readable error path and a way to keep Rust and TypeScript types in
sync. The pattern must also cover things later steps need, so the web client in 0.4 can implement the
same interface:

- **Core-to-UI events.** Step 1.12 adds events (note added, card answered, sync completed). The UI must
  hear about changes it did not make, for example a background sync.
- **Long-running operations with progress and cancellation.** Anki import (Phase 5), sync (Phase 4),
  backup and export (1.13), FSRS optimisation (1.8).
- **Binary data.** Adding media (1.10, Phase 3 editor), importing files on the web, exports.
- **Many functions.** The review question is whether this stays pleasant at a hundred functions.
- **Untrusted card content** must never reach app commands (`PRODUCT.md`, step 0.6).

### Findings

Verified with throwaway experiments in a scratch directory outside the repo (Linux, WebKitGTK,
Tauri 2.12.1, the repo's pinned toolchain):

1. **Type generation with ts-rs 12.0.1.** A `Method` trait plus a list of methods generates a
   `Methods` map (`{ getCoreInfo: { input: null; output: CoreInfo } ... }`) and one file per type.
   serde's `rename_all`, `rename_all_fields`, internally tagged enums (`tag = "kind"`) and `()` as
   `null` come out matching serde's JSON. Renaming a Rust field and regenerating made `tsc` (the repo's
   TypeScript 7.0.2) fail at the use site.
2. **64-bit integers.** ts-rs maps `i64`/`u64` to `bigint` by default, but `JSON.parse` yields
   `number`, so the types would lie and IDs above 2^53 would lose precision silently. Setting ts-rs's
   large-int type to an undefined name (`LARGE_INT_NOT_ALLOWED`) makes any such field a `tsc` error.
3. **One generic command works.** A single async `call(method, input, ...)` command dispatching on the
   method name returned typed results. A Rust `Err(ApiError)` rejected the JS promise with
   `{ kind, message }` as a plain object. An unknown method name returned a clean error.
4. **Concurrency and cancellation.** The command body ran on `spawn_blocking`. A second call completed
   while a 1-second operation was running. A separate `cancel(op)` command set a flag the operation
   checks, and the operation stopped within about 10 ms with a `cancelled` error.
5. **Channels for events and progress.** One long-lived `tauri::ipc::Channel` passed to a `subscribe`
   command delivered domain events and per-operation progress. 50 progress messages arrived in order.
6. **Binary out.** `tauri::ipc::Response` (raw bytes) returned 5 MB in about 15 ms as an `ArrayBuffer`.
   200 small sequential calls took 44 to 62 ms, about 0.3 ms each.
7. **Binary in.** 5 MB sent as a base64 string inside the JSON arguments took about 100 ms (decoded
   in Rust). The same bytes as a `Uint8Array` nested in the arguments (Tauri serialises it to a JSON
   array of numbers) took about 1,280 ms. WebKitGTK has `Uint8Array.prototype.toBase64`.
8. **The repo's current CSP breaks fast IPC.** With the CSP from step 0.1
   (`default-src 'self'; ...`), Tauri's custom-protocol IPC fetch is blocked, it logs
   "IPC custom protocol failed, Tauri will now use the postMessage interface instead", and every call
   falls back to the slower path: 5 MB out took about 1,220 ms, and small raw responses arrived as a
   JS `Array` of numbers instead of an `ArrayBuffer`. Adding `connect-src ipc: http://ipc.localhost`
   fixed it (back to 15 ms and `ArrayBuffer`).

Read in Tauri's source (MIT/Apache-2.0, `tauri-2.12.1`, `scripts/ipc-protocol.js`,
`src/ipc/protocol.rs`, `src/ipc/channel.rs`):

9. **Android never uses the custom-protocol request path**, because Android WebView cannot read
   request bodies. Every invoke goes through `postMessage` as a JSON string, so a `Uint8Array` argument
   becomes a JSON number array there too. Responses on that path come back through an internal
   channel: under 1 KB (raw) or 8 KB (JSON) by `eval`, larger ones by a `fetch` over the `ipc` custom
   protocol, which Android supports. So Rust-to-JS bytes are fast on Android and JS-to-Rust bytes are
   not. On macOS and iOS raw responses on the fallback path come back as number arrays.
10. **Command access.** By Tauri's docs, every app command is callable from every window and webview
    unless `tauri_build::AppManifest::commands` is used, which turns app commands into permissions
    that capabilities grant per window.

**Not verified:** anything on Android or Windows (findings 9 and 10 are from source and docs only);
the `AppManifest` gating; any web or wasm behaviour (that is 0.4); macOS and iOS.

Package status checked on crates.io and npm (2026-10-05):

| Package | Version | Status | Licence |
| --- | --- | --- | --- |
| ts-rs | 12.0.1 (Jan 2026) | stable, about 7M downloads in 90 days | MIT |
| specta / tauri-specta | 2.0.0-rc.25 (May 2026) | release candidate since 2023 | MIT |
| typeshare | 1.0.5 | stable, needs a separate CLI install | MIT OR Apache-2.0 |
| taurpc | 0.8.2 | about 2k downloads in 90 days | MIT OR Apache-2.0 |
| @tauri-apps/api | 2.12.1 | matches our Tauri crate | Apache-2.0 OR MIT |

## Options considered

### A. One Tauri command per core function, typed by tauri-specta

The common Tauri pattern. Each function is a `#[tauri::command]`, and tauri-specta generates a typed
TypeScript function per command plus typed events.

- Good: idiomatic, and typed events come free. Each command is visible in Tauri's permission system.
- Bad: the generated client calls `invoke` directly, so it is Tauri-specific. The web client would
  need a second, parallel surface for every function, written by hand or with another generator.
- Bad: specta 2 has been a release candidate for about three years, with breaking changes between
  release candidates.
- Bad: each of a hundred functions needs its own Tauri registration and permission entry, and progress,
  cancellation and binary handling get solved per command.

### B. A transport-agnostic API in Rust, one generic Tauri command, types from ts-rs (chosen)

The core's public API is a list of methods defined once in a new Rust crate with no Tauri dependency.
One dispatcher serves every host. Tauri exposes three fixed commands (`call`, `subscribe`, `cancel`)
however many methods there are. TypeScript gets a generated `Methods` map and a small typed client that
works over any transport.

- Good: adding a method touches only Rust (the method and one line in the list). Bindings regenerate,
  and nothing in Tauri, capabilities or TypeScript is written by hand.
- Good: the web client implements the same small `Transport` interface (3 functions), and wasm calls
  the same Rust dispatcher.
- Good: progress, cancellation, events and bytes are solved once, in the transport.
- Bad: method names are strings on the wire, so a typo is caught by TypeScript's types, not by Rust.
  The dispatcher returns `unknownMethod` for anything unrecognised.
- Bad: Tauri's permission system sees one `call` command, so per-function permissions would have to
  be ours. Not needed: only the trusted main webview may call the core at all.

### C. Schema-first with Protocol Buffers

Methods and messages defined in `.proto` files, Rust code from prost (with `protox` to avoid
installing `protoc`) and TypeScript from protobuf-es. Bytes are native and the format has
well-understood versioning rules.

- Good: language-neutral and strongly versioned. Could also serve the sync wire format.
- Bad: the generated Rust types are separate from our domain types, which needs a mapping layer.
  Adds a JS runtime dependency and a code-generation toolchain to maintain. Harder to debug than JSON.
- Bad: heavy for a single client codebase whose server is also ours. Whether sync uses protobuf is
  step 0.7's decision and does not need to be made here.

### D. taurpc

tRPC-style router on top of specta. Shares option A's problems (Tauri-specific, specta RC) and has a
small user base.

### Sub-choices within B

- **Type generator:** ts-rs (chosen; stable, widely used, serde-compatible, no extra install),
  specta-typescript (RC), typeshare (CLI parses source files; separate install and weaker generics).
- **Wire format:** JSON (chosen; readable in devtools, no JS dependency, native `JSON.parse`) or
  MessagePack (bytes native, but adds a JS dependency, and Tauri on Android turns any binary
  argument into a number array anyway, finding 9).
- **Bytes:** a separate attachment beside the JSON (chosen, below), or bytes inside DTOs as base64
  strings (UI code would handle base64 everywhere), or separate per-method binary commands.

## Decision

Option B.

### Rust: `crates/fc-api`

A new crate holds the public, serialisable API surface. It depends on `fc-core`, serde, serde_json and
ts-rs, and has no Tauri or platform dependency, so it compiles for desktop, Android and wasm. Step 1.12's
"public surface, separate from internals" lives here. Domain logic stays in `fc-core`.

- **Methods.** Each method is a type implementing a `Method` trait: a name (camelCase, for example
  `getCoreInfo`), an `Input` type, an `Output` type, whether it takes or returns an attachment, and a
  function `(core, input, ctx) -> Result<Output, ApiError>`. One list (a macro) registers every
  method and generates both the dispatcher and the TypeScript `Methods` map, so they cannot drift.
- **Dispatcher.** `dispatch(core, request, ctx) -> Result<Reply, ApiError>`. The request is a method
  name, JSON input and an optional attachment. The reply is JSON output and an optional attachment.
  Every host calls this.
- **Operation context.** Every method gets an `OpContext` with `checkpoint() -> Result<(), ApiError>`
  (returns `cancelled` once cancellation is requested), `progress(Progress)` and attachment access.
  The host supplies the implementation. Long methods call `checkpoint()` regularly. Cancellation is
  cooperative: work stops at the next checkpoint and the method returns `cancelled`. A method that
  writes data makes its writes atomic, so a cancelled operation leaves nothing half-done.
- **Errors.** `ApiError { kind, message, details? }`. `kind` is a generated string union
  (`notFound`, `invalidInput`, `cancelled`, `unknownMethod`, `internal`, ...) for code to branch on.
  `message` is a readable English sentence that says what went wrong and what to do, as `PRODUCT.md`
  requires. Core errors convert to `ApiError` in `fc-api`. Panics and unexpected failures become
  `internal` with a generic message, never a stack trace. Translation can map `kind` later.
- **Events.** `Notice` is the single stream from core to UI:
  `{ type: "event", event: CoreEvent } | { type: "progress", op, progress }`. `CoreEvent` is a
  generated tagged union, filled in by step 1.12. The core emits through an `EventSink` trait the host
  implements. Hosts throttle progress to a few updates per second.
  - **Changed by ADR 0009 (step 1.12):** the core cannot call `EventSink` (`fc-api` depends on
    `fc-core`, not the other way round), so it emits through its own `fc_core::events::Listener`, and
    `fc_api::forward_events` adapts that to the host's `EventSink`. The `Notice` shape, `EventSink` and
    the "refetch, do not rely on order with the call's result" rule are unchanged. The list of events
    is in `docs/events.md`.
- **Types.** API types derive serde and ts-rs, with `rename_all = "camelCase"` (and
  `rename_all_fields` on enums). No `i64`/`u64` in API types: the generator sets the large-int type to
  an undefined name, so any 64-bit field fails `tsc` (finding 2). How IDs are represented is for
  step 0.7. API types are hand-written DTOs, not internal structs, so internals can change without
  changing the API.

### TypeScript: `packages/core-client`

A new package with no React and no Tauri dependency, so the UI, the web worker and a future browser
extension can all use it.

- `src/generated/`: ts-rs output plus `Methods.ts`, committed. This answers ADR 0001's open question
  on where bindings live.
- `Transport`, the only thing a platform implements:

  ```ts
  interface Transport {
    call(req: { method: string; input: unknown; bytes?: Uint8Array; op?: number }):
      Promise<{ output: unknown; bytes?: Uint8Array }>;   // rejects with an ApiError-shaped object
    cancel(op: number): void;
    subscribe(onNotice: (notice: Notice) => void): () => void;
  }
  ```

- `CoreClient`, shared by every platform and typed from `Methods`:
  `client.call("getCoreInfo", null)` returns `Promise<CoreInfo>`. Long operations take
  `{ signal?: AbortSignal; onProgress?: (p: Progress) => void }`. The client assigns operation IDs,
  routes progress notices to the right caller, and turns `signal` aborts into `transport.cancel(op)`.
  Methods that take or return an attachment have it in their typed signature. `client.onEvent(fn)`
  subscribes to `CoreEvent`s. Rejections become a `CoreError` (an `Error` subclass with `kind` and
  `message`). The transport stays small and the logic is written once.
- `createFakeTransport(handlers)` for UI tests, so tests never need Tauri.

### UI: `packages/ui`

Gets a `CoreClient` from a React context provider supplied by the app shell, and never imports a
transport. A `CoreError`'s `message` is what the user sees.

### Tauri host: `apps/native`

- **Rust (`fc-native`):** three commands (**amended by ADR 0005: a fourth, `handshake`, issues a
  session token that `call`, `subscribe` and `cancel` must be given**). `call` runs `fc_api::dispatch` on `spawn_blocking` and
  returns a `tauri::ipc::Response` whose body is a small frame:
  `u32 little-endian JSON length | JSON output | attachment bytes`. `subscribe(Channel<Notice>)`
  registers the webview's notice channel, replacing any earlier one from the same webview (a reload).
  `cancel(op)` flags an operation, and records it even if the operation has not started yet.
- **Attachments in (JS to Rust)** travel as a base64 string in the JSON arguments: 12 times faster
  than Tauri's default number array (finding 7), and the only fast option on Android (finding 9).
  Attachments out use the raw frame (finding 6).
- **TypeScript transport** (`apps/native/src`): implements `Transport` with `@tauri-apps/api`'s
  `invoke` and `Channel`. It decodes the frame and also accepts a number `Array` (finding 8: the
  fallback path and macOS return one).
- **CSP:** add `connect-src ipc: http://ipc.localhost` (finding 8).
- **Command access:** use `AppManifest::commands` so the three commands become permissions, granted
  only to the `main` window in `capabilities/default.json`. Card content (step 0.6) renders somewhere
  that can never call them. The bridge exposes the whole core, so the 0.6 sandbox must keep card code
  out of the main webview's JavaScript context.

### Web host (step 0.4, for reference)

The core runs as wasm in a dedicated worker that calls the same `dispatch`. The transport posts
`{ method, input, op, bytes }` to the worker and transfers `ArrayBuffer`s without copying. Notices
come back by `postMessage`. Step 0.4 must solve one thing this ADR cannot: a worker busy in a
synchronous wasm call cannot receive a cancel message. The options are a `SharedArrayBuffer` flag
read by `checkpoint()` (needs cross-origin isolation headers from the host) or long operations that
yield between chunks. The core side (`checkpoint()`) is the same either way.

### What does not go through `call`

- **Media for display.** Images and audio in cards and the editor load by URL, never as bytes
  through `call`. On native this is a custom URI scheme, on the web a Service Worker or blob URLs. The
  shape is decided in step 0.6 with the sandbox, as a `mediaUrl`-style function in a platform
  interface. **Amended by ADR 0005 (step 0.6):** for card content this is reversed. Media bytes go
  through `call` as attachments and into the card frame as `Blob`s.
- **Large files on native.** Import and export on desktop and Android pass a file reference from a
  file picker (a path, or an Android content URI the host resolves), not bytes through JS. This keeps
  big files out of JS memory and avoids Android's slow JS-to-Rust path. Designed in the step that adds
  file pickers (1.13 or Phase 5). On the web, files are bytes and use attachments.
- **Platform features** (file pickers, share target, notifications) go in a separate `Platform`
  interface next to `CoreClient`, added when the first one is needed.

### Checking that the types stay in sync

- A test in `fc-api` regenerates the bindings in memory and fails if they differ from the committed
  files, with the message "TypeScript bindings are stale: run `cargo xtask bindings`".
  `cargo xtask check` already runs `cargo test`, so stale bindings fail the check.
- `cargo xtask bindings` rewrites them (it runs that test with an environment variable set, so
  `xtask` stays standard-library only).
- With the bindings regenerated, `tsc` (also in `check`) fails wherever TypeScript disagrees with Rust.

## Consequences

- Adding a method is Rust only: one type, one line in the list, then `cargo xtask bindings`. Tauri
  registration, capabilities and the transport stay the same at a hundred methods.
- The UI is testable with a fake transport and never sees Tauri. The web client implements three
  functions.
- New dependencies: `ts-rs` (=12.0.1), `serde`, `serde_json` in `fc-api`; `base64` in `fc-native`
  (already in our lockfile through Tauri); `@tauri-apps/api` (=2.12.1) in `apps/native` only. All
  permissively licensed.
- Generated files are committed, so a change to an API type shows up in PR diffs as TypeScript too.
- All methods share one string namespace and one `Methods` type. If `tsc` slows down at hundreds of
  methods, the map can be split by area without changing the wire format.
- A long operation runs on its own thread, but whether other calls can run during it depends on how
  `fc-core` locks the collection, which is step 1.1's decision. It should allow reads during a long
  write, or the UI will freeze during an import.
- There is no ordering guarantee between a call's result and the events it caused. Events arrive in
  order, but the UI must treat them as "something changed, refetch" and not as the only source of
  truth.
- JS-to-Rust bytes cost a base64 encode and decode (about 100 ms per 5 MB on desktop, not measured on
  Android). Fine for pasted images and recordings, not for whole-collection imports on native, which
  use file references.
- Per-function permissions would be ours to build. Fine while the only caller is the trusted UI. Add-ons
  (Phase 10) get their own narrower surface, not this one.

## Revisit if

- Step 0.4 finds the web worker cannot implement `Transport` as written, especially cancellation.
- Android measurements in the build show the base64 attachment path or the `ipc::Response` frame is
  too slow on the phone.
- Tauri adds raw request bodies on Android, which would make base64 unnecessary.
- macOS or iOS arrive (Phase 12): raw responses there come back as number arrays (finding 9) and may
  need a different encoding.
- specta 2 becomes stable and its typing is clearly better than ts-rs, or ts-rs stops being maintained.
- `tsc` performance with a large `Methods` map becomes a problem.
- Streaming large outputs (chunked export, sync downloads) is needed. That would add a chunk notice
  type, not a new transport.
- Add-ons (Phase 10) need to call the core. They need a separate, capability-limited surface.

## Build notes (step 0.3a)

- `tauri dev` serves the UI from the Vite dev server and does not apply the CSP, so the CSP fix
  (finding 8) can only be checked in a built app (`tauri build --debug --no-bundle`, or the APK).
  In both, with the new `connect-src`, no "IPC custom protocol failed" warning appeared.
- `AppManifest::commands(&["call"])` gating verified on desktop: with `allow-call` removed from the
  capability, calls from the main window are rejected.
- On Android, a call (`getCoreInfo`) works in the standalone debug APK with the new CSP.

## Build notes (step 0.3b)

The rest of the interface is built: `subscribe`, `cancel`, progress, events and attachments, with
the fake transport supporting all of it. Checked in a built debug app on desktop (Linux) and in the
standalone debug APK on the phone (Samsung S24 Ultra), using a temporary harness that ran on launch
and was removed before the PR.

- **Measured, round trip of an attachment** (base64 in, raw frame out, echoed byte for byte):

  | Attachment | Desktop (Linux) | Phone (Android) |
  | --- | --- | --- |
  | 1 MB | 40 ms | 75 ms |
  | 5 MB | 181 ms | 250 ms |

  Android is slower but well within what pasted images and recordings need, so the "Revisit if"
  condition on attachment speed does not apply. This also covers finding 7 on Android, which was
  previously unmeasured.
- **Progress and cancel work on both.** A 20-step operation delivered 5 throttled updates in order,
  ending with the final one. A cancel sent 300 ms into a 5 s operation stopped it at the next
  checkpoint (352 ms on desktop, 303 ms on the phone). A call with an already-aborted signal never
  reaches the core. Events arrive on both.
- **Gating of the new commands verified on desktop:** with `allow-subscribe` and `allow-cancel`
  removed from the capability, no notices arrived and cancel had no effect.
- **Empty attachment equals no attachment** on the wire (a zero-length tail of the frame). The
  generator also emits a runtime `bytesOutMethods` set, so the client still resolves methods that
  return attachments to `{ output, bytes }` with empty bytes.
- **Operation IDs restart at 1 on page reload.** A cancel for an operation that never starts is
  forgotten after 60 seconds, so a stale one cannot hit a later operation with the same ID for long.
  If IDs ever need to be unique across reloads, the client can seed them from the time.
- **Notices sent while nothing is subscribed are dropped.** The UI treats events as "something
  changed, refetch", as decided above, so a reload loses nothing it cannot refetch.
- **Debug methods** (`debugSlow`, `debugEchoBytes`, `debugEmitEvent`) exist only when
  `debug_assertions` is on, but their TypeScript types are always generated. A release build
  answers them with `unknownMethod`. They are deleted when real long methods arrive (1.12).
- **Not verified:** Windows, a web transport (step 0.4), and the mid-run cancel path through the
  `subscribe` channel after a webview reload on the phone.
