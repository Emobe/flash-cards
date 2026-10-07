# 0005: Sandboxed card rendering

Status: Accepted
Date: 2026-10-05

## Context

Card content is untrusted. It may come from shared decks or Anki imports, and it contains HTML, CSS,
images, audio and some JavaScript (`PRODUCT.md`). Step 0.6 must show that a card renders correctly on
desktop, Android and the web, and that a malicious card cannot:

- call app commands,
- read app data,
- read other cards,
- navigate the app away.

ADR 0002 makes this sharper. The bridge exposes the whole core through three Tauri commands (`call`,
`subscribe`, `cancel`). Any JavaScript that can call them owns the collection. ADR 0002 also left one
question for this step: how media for display reaches card content.

Templates and field substitution are Phase 1. This ADR covers how a finished card document is shown.

### Findings

Throwaway experiments ran outside the repo, in a copy of it with probe code added:

- **Desktop:** the Tauri 2.12.1 app built with `tauri build --debug --no-bundle` on Manjaro (WebKitGTK).
- **Android:** the debug APK (aarch64) on the local Android emulator (`Small_Phone` AVD, API 37
  x86_64 with arm64 translation). Anthony's phone was not used.
- **Web:** headless Brave (Chromium), driven over the DevTools protocol by a Bun script, against a
  Bun static server.

Tauri and wry source was read too (MIT/Apache-2.0, `tauri-2.12.1`, `wry-0.57.0`).

1. **A sandboxed iframe is isolated by the browser everywhere tested.** The iframe used
   `sandbox="allow-scripts"` with no `allow-same-origin`. In it, `self.origin` is `"null"`, and all
   of these throw `SecurityError`:
   - reading `parent.document`,
   - assigning `top.location`,
   - `localStorage`, `document.cookie` and `indexedDB`,
   - OPFS (`navigator.storage.getDirectory()`, web).

   `window.open` returns `null`. A `BroadcastChannel` message from the frame did not reach the app.
   This holds on WebKitGTK, Android WebView and Chromium.
2. **On Android, Tauri's IPC is reachable from inside the card iframe.** wry 0.57 passes every
   initialisation script to `WebViewCompat.addDocumentStartJavaScript(..., setOf("*"))` and ignores
   `for_main_frame_only` (`android/main_pipe.rs`, `kotlin/RustWebView.kt`). Android's docs say `*`
   injects into all frames.

   So the sandboxed, opaque-origin card frame had `window.__TAURI_INTERNALS__` with the secret
   invoke key, and `window.ipc`. A card calling
   `invoke("call", { method: "debugEmitEvent", ... })` **ran the method in the core**: the main frame
   received the event. This happened twice, once with a frame from the app's own assets and once
   from a custom scheme. The card got no reply (replies go to the main frame), but it could trigger
   any core method blind, including writes.

   Tauri resolves the caller's origin from the main frame URL on Android (`Ipc.kt` passes
   `currentUrl`). It therefore cannot tell the card frame apart from the app.
3. **On desktop Linux, Tauri's IPC is not reachable from the card frame.** It had no
   `__TAURI_INTERNALS__` and no invoke key. Tauri's own scripts are main-frame-only and WebKitGTK
   honours that. `window.webkit.messageHandlers` exists in the frame, but without the key Tauri drops
   the message (`webview/mod.rs`, `on_message`).
4. **Windows is unclear, from docs only.** wry says scripts are "always added to subframes" on
   Windows. The Tauri advisory GHSA-57fm-592m-34r7 says invoke is not reachable from iframes "except
   on Windows when the origin of the Tauri window and the origin of the iFrame are the same". A card
   frame has an opaque origin, so it is probably safe, but this is not verified.
5. **One Tauri command skips the permission system:** `plugin:__TAURI_CHANNEL__|fetch`. It returns
   large IPC replies by sequential ID. From the Android card frame it failed with "Origin header is
   not a valid URL", because an opaque frame sends `Origin: null`. A card CSP without `connect-src`
   blocks it as well.

   Every other command is checked against capabilities, because the app has an ACL manifest
   (ADR 0002). So on Android a card frame can call **every command granted to the main window**,
   today `call`, `subscribe` and `cancel`.
6. **`srcdoc` does not work in Tauri.** A sandboxed frame loaded from the app's own assets gets the
   app's CSP. Its external script was blocked, because `'self'` does not match an opaque origin in
   WebKit. A `srcdoc` frame inherits the parent's CSP in every browser. So card inline JS would only
   run if the app's CSP allowed `'unsafe-inline'` scripts, which would weaken the app itself.
7. **A custom URI scheme gives the card frame its own CSP.** A `card` scheme was registered with
   `register_uri_scheme_protocol`, and the app CSP got `frame-src card: http://card.localhost`. The
   frame page then loaded on Linux (`card://localhost/...`) and Android (`http://card.localhost/...`)
   with the CSP from our response header. Tauri did not add its own policy. Inline card scripts and
   styles ran, and fetching another URL was blocked.
8. **The frame's CSP survives `document.write`.** The frame page receives the card HTML by
   `postMessage` and writes it with `document.open/write/close`. Afterwards (Chromium), the card's
   `<img>`, `<link rel=stylesheet>` and `<script src>` aimed at the server made no request at all.
   Inline scripts and styles in the written card ran. `document.close()` fires a second `load` event
   on the iframe element (all three platforms).
9. **Media.** Results for the image (PNG) and audio (WAV):

   | Where the URL comes from | WebKitGTK | Android | Chromium |
   | --- | --- | --- | --- |
   | Image from the `card` scheme | loads | loads | (no scheme on web) |
   | Audio from the `card` scheme, even with correct `206` Range replies | **fails** (`MEDIA_ERR_SRC_NOT_SUPPORTED`) | plays | (no scheme on web) |
   | Image and audio from blob URLs the frame creates from `Blob`s the parent posts to it | loads and plays | loads and plays | loads and plays |
   | Blob URL created by the parent | (not tested) | (not tested) | **fails** to load in the opaque frame |

**Not verified:**

- Windows (finding 4).
- Firefox, Safari and mobile browsers.
- A real phone (the emulator stood in for it).
- Whether Tauri's page-load event fires only for the main frame. That is from Android, WebKitGTK and
  WebView2 docs, and the build checks it.
- `Content-Security-Policy: sandbox` as a response header.
- Permission prompts (camera, geolocation) from a card frame.
- Performance of creating a frame per card.

## Options considered

### A. Sandboxed iframe in the app webview, a card-frame page with its own CSP, and a session token on the bridge (chosen)

Each card renders in `<iframe sandbox="allow-scripts">`, which gives it an opaque origin. The frame
loads a small, trusted card-frame page that sets a strict CSP. The page receives the card HTML and
its media from the parent by `postMessage`. On native, the page is served by a `card` custom scheme
so that its CSP is ours (findings 6 and 7). Because Android exposes Tauri's IPC to the frame
(finding 2), every bridge command also requires a session token that only the main frame holds.

- Good: one design for all three platforms. The browser's origin isolation does the main work and
  is verified on all three (finding 1).
- Good: the card is laid out inside the React page, so scrolling, gestures, dialogs and the
  keyboard all work as normal.
- Bad: on Android, card JavaScript shares a webview with Tauri's bridge. The token is our own code,
  and every future command must keep it. See the consequences.
- Bad: a card's JavaScript runs on the app's main thread, so an infinite loop freezes the app.

### B. A separate webview for card content

Use a Tauri child webview (desktop, behind Tauri's `unstable` multi-webview feature) and, on Android,
a plain `android.webkit.WebView` added by our own Kotlin plugin, with no JavaScript interfaces and no
init scripts. On the web, use a sandboxed iframe.

- Good: on Android the card has no route to Tauri at all, so no token is needed there. A frozen card
  might not freeze the app's UI.
- Bad: three different implementations, one of them native Kotlin.
- Bad: a native view floats above the web UI. React would have to keep it positioned and hidden
  under dialogs, menus and the keyboard. Gestures over the card area would not reach React.
- Bad: a Tauri child webview gets Tauri's init scripts like any other webview, so on desktop it
  still needs capabilities scoped by webview label.
- Bad: much more code, on an `unstable` Tauri feature.

### C. Sanitise card HTML and render it in the app's own DOM

Strip scripts and dangerous attributes with a sanitiser (for example DOMPurify), then render into a
shadow root.

- Bad: no card JavaScript, which `PRODUCT.md` and the step require.
- Bad: one sanitiser bypass gives a card the main frame's JavaScript context, which means the whole
  core. A sanitiser is a filter. An origin boundary enforced by the browser is stronger.

### D. Sandboxed `srcdoc` iframe (no card-frame page, no scheme)

- Bad: the frame inherits the app's CSP (finding 6). Card scripts only run if the app CSP allows
  `'unsafe-inline'` scripts, which would make any injection into the app fatal.
- Bad: it does not address finding 2 either.

### E. Run card JavaScript in a wasm JavaScript engine (QuickJS) with a fake DOM

- Bad: very large, slow, and incompatible with card scripts written for a real DOM.

### Sub-choices within A

- **How card HTML reaches the frame.** Options:
  - Posted to a static card-frame page (chosen; same for native and web).
  - A complete document served per card by the native scheme. The web has no scheme, so it would
    need a second path.
- **Media:**
  - Bytes posted into the frame, which makes frame-local blob URLs (chosen). This works for images
    and audio on every platform tested (finding 9), and a card can only reach the media it was
    given.
  - URLs on the `card` scheme. Audio fails on WebKitGTK, it does not exist on the web, and a card
    could load any other card's media by name.
  - `data:` URLs written into the HTML. These work, but are larger and slower to parse.
- **Fresh frame per card side** (chosen) **or reusing one frame.** A fresh frame gives each card a
  new opaque origin and JavaScript realm, so nothing a card leaves behind reaches the next card.
  Reuse would only help if creating frames proves slow.

## Decision

Option A. Card content is only ever shown through the card frame described here: in study, in
previews, and in the editor. No card HTML is ever inserted into the app's own DOM.

### The card frame

- **Element.** `<iframe sandbox="allow-scripts">`. Never `allow-same-origin` (with `allow-scripts`
  it removes the sandbox), `allow-top-navigation*`, `allow-popups`, `allow-forms` or `allow-modals`.
  No `allow` attribute, so the frame gets no camera, microphone, geolocation, clipboard or fullscreen.
- **The card-frame page** (`frame.html`). One static file, trusted, written by us, and the same for
  every platform. It includes:
  - A CSP:
    `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src blob: data:; media-src blob: data:; font-src blob: data:`.
    No `connect-src`, so no fetch, XHR, WebSocket or beacons. No remote images, styles or scripts.
  - A small bootstrap script. It refuses to run unless `self.origin === "null"`, so it only works
    inside a sandbox. It accepts exactly one message whose `source` is `parent`:
    `{ html, media: { name: Blob } }`.
- **Rendering.** The bootstrap:
  1. Parses the HTML inertly with `DOMParser`.
  2. Rewrites `src` on `img`, `audio`, `video` and `source` elements whose value names a supplied
     media file, to a blob URL created inside the frame.
  3. Writes the result with `document.open/write/close`, so scripts run in order as in a normal page.

  CSS `url()` media and media referenced from scripts are Phase 1 (template rendering), not this step.
  *(Amendment, step 1.10b: both are built. See "Build notes (step 1.10b)" at the end.)*
- **Messages from the frame are untrusted.** The parent:
  - only listens to messages whose `source` is that frame's `contentWindow`;
  - accepts only `ready` and `height` (clamped to a sane range);
  - ignores everything else.

  A card can send fake messages of these types. That can only affect its own frame.
- **Navigation.** A card can navigate its own frame (for example by clicking a link or setting
  `location`), and nothing in a CSP prevents that. The parent counts `load` events on the frame. The
  bootstrap makes exactly two (finding 8). Any further one means the card navigated, so the parent
  removes the frame and shows "This card tried to open another page and was stopped." The sandbox
  still applies to whatever loaded (it has an opaque origin and no storage).

  The app also refuses to start if `window.top !== window`, so a card that navigates its frame to
  the app gets a blank page, not a working copy of the app.
- **One fresh frame per card side.** The frame is discarded when the card changes. Blob URLs are
  revoked with it.

### Native (Tauri)

- **`card` scheme.** `fc-native` registers a `card` URI scheme that serves `frame.html` and nothing
  else. On Linux the URL is `card://localhost/frame.html`, on Android and Windows
  `http://card.localhost/frame.html`. The UI gets it from `convertFileSrc("frame.html", "card")`.

  The response carries the same CSP as the page, plus `sandbox allow-scripts` as a CSP directive.
  The page is then sandboxed even if something loads it outside our iframe.
- **App CSP.** Add `frame-src card: http://card.localhost`.
- **Session token on the bridge** (finding 2). Every bridge command requires a token:
  - A fourth command, `handshake`, returns a random 128-bit token. It returns one only if no token
    has been issued since the main frame last started loading. Tauri's page-load event (main frame
    only) clears the token.
  - `call`, `subscribe` and `cancel` take the token as an argument and reject a missing or wrong
    one. The comparison is constant-time.
  - The Tauri transport calls `handshake` once, before the UI renders, and keeps the token in a
    module-private variable. The main frame starts before any card frame exists, so the main frame
    always wins.

  A card frame cannot read the main frame's memory (finding 1). It does not receive replies either
  (finding 2). If the main frame's handshake ever fails, the app shows "Restart the app." That only
  happens if something else claimed the token first.

  The token is used on Linux and Windows too. It costs nothing, and Windows is unverified
  (finding 4).
- **No Tauri plugin commands for JavaScript.** On Android, every command granted to the main window
  is callable from card frames (finding 5). So the main window's capability grants only our own
  token-checked commands: no `core:default` and no plugin permissions.

  Platform features (file pickers, share target, notifications) go through our own token-checked
  commands, or are called from Rust. This is a standing rule for every future step. Breaking it means
  any card can use that plugin on Android.

  *(Amendment, step 2.1b: a fifth token-checked command, `set_system_theme(token, dark,
  followSystem)`, makes the Android bar icons and the desktop title bar follow the app's theme
  (ADR 0010). On Android it calls a local Kotlin plugin, `appearance`, from Rust. The plugin has no
  JS permission and the capability grants only `allow-set-system-theme`, so the rule above holds.)*
- **Media.** The parent gets media bytes from the core through `call`, which returns them as an
  attachment (Rust to JS is fast on both platforms, ADR 0002), and posts them as `Blob`s. Media for
  display in cards does **not** go by URL. This answers ADR 0002's open question, see the amendment
  below.

### Web

- `frame.html` is served as a static file from the web app's own origin, at `/card-frame.html`. The
  sandbox attribute makes the frame's origin opaque, so it cannot reach the app's OPFS, IndexedDB or
  the core worker (finding 1).
- Media bytes come from the worker as attachments and are posted to the frame in the same way.
- When Phase 6 adds a CSP to the web app, it must include `frame-src 'self'`.

### Shared code

- `frame.html` lives once in the repo. `fc-native` embeds it with `include_str!`, and the web build
  copies it into `dist`.
- A `CardFrame` React component in `packages/ui` takes the card HTML, a media loader
  (`(name) => Promise<Blob>`) and the frame URL from the platform. It does everything above that is
  not platform-specific.

## Consequences

- **What a malicious card can do:**
  - Run JavaScript and draw anything inside its own rectangle, including a fake copy of app
    controls.
  - Send `height` messages.
  - Make one request to any URL by navigating its frame. The frame is then removed. This can tell a
    deck's author when someone studies a card.
  - Freeze the app with an infinite loop. Card frames share the app's main thread in these webviews,
    and nothing can interrupt them.
- **What it cannot do:**
  - Call the core (sandbox plus token).
  - Read app storage or the DOM, or other cards' content or media.
  - Open windows or dialogs.
  - Navigate the app.
- **Freeze recovery is needed before shared decks are common.** A card that loops forever would
  freeze the app every time it comes up. Phase 2 should record which card is being rendered. After a
  crash or forced restart, it should offer to show that card with JavaScript off. This is noted for
  step 2.x, not built in 0.6.
- **Android relies on the token.** A future command that skips the token check, or a Tauri plugin
  granted to the main window, would be callable by any card on Android. Code review must check this
  for every new command and capability. A test in `fc-native` checks that each command rejects a
  missing token.
- **Amendment to ADR 0002.** Its section "What does not go through `call`" said that media for
  display loads by URL. For card content this is reversed: media bytes go through `call` as
  attachments and into the frame as `Blob`s. Media that only the trusted UI shows (none exists yet)
  may still use URLs.
- **Card scripts may not load external scripts.** `script-src` allows only inline code. Anki decks
  sometimes ship script files as media. Supporting that (as blob URLs, like images) is Phase 1 or
  Phase 5 work.
- **Keyboard shortcuts during study.** When focus is inside the card frame, keys go to the card. Any
  key forwarding from the frame would be spoofable, so a card could "press" answer keys for itself.
  That would affect only its own scheduling. This is decided in Phase 2 with the study screen.
- **Cost:** `handshake`, a token argument on every bridge call, the `card` scheme, `frame.html`, and
  the `CardFrame` component. No new dependencies.

## Revisit if

- wry or Tauri start honouring main-frame-only scripts on Android. The token stays, but this ADR's
  risk notes change.
- Windows testing shows Tauri's IPC in the card frame. The token already covers this, so just record
  it.
- Creating a frame per card is too slow on the phone (measured in the build). Then consider reusing
  frames.
- Card freezes become a real problem. Then consider option B on Android, which isolates the card in a
  separate webview.
- Tauri adds per-frame IPC origins or a way to disable IPC in subframes.
- Shared decks need remote images. Then allow specific hosts in `img-src` as a user setting, with
  the privacy cost stated.
- Video in cards needs streaming rather than whole `Blob`s.

## Build notes (step 0.6)

What was built is what the Decision describes. Differences and results:

- **Malicious card.** 54 attempts (bridge, parent and top, storage, network, prompts and
  permissions), each reported inside the card as "blocked", "SUCCEEDED" or "other". Results:

  | Platform | Blocked | SUCCEEDED | Other | CSP violations seen by the card |
  | --- | --- | --- | --- | --- |
  | Linux desktop, built debug app (WebKitGTK) | 38 | 0 | 16 | 38 |
  | Android, Anthony's phone (debug APK) | 38 | 0 | 16 | 40 |
  | Web, headless Brave, `web:dev` | 38 | 0 | 16 | 19 |
  | Web, headless Brave, `web:build` + `web:preview` | 38 | 0 | 16 | 19 |

  "Other" means the outcome is not visible from inside the card: the bridge on web and Linux ("no
  bridge in frame", except `webkit.messageHandlers.ipc`, which exists on Linux and is dropped by Tauri
  without the key, finding 3), or attempts the app checks from outside (`top.location`, a form or
  link with `target="_top"`, `BroadcastChannel`, a huge `height` message, `alert`).
- **The bridge on Android.** All seven `invoke` attempts and `window.ipc.postMessage` ran with no
  error and no answer within 2.5 s. That confirms finding 2 on the phone: the frame has Tauri's
  invoke. The panel's "A card reached the core" banner never appeared, and `getCoreInfo` still
  answered afterwards. **Negative control:** a temporary build with the token check removed from
  `call` (not committed) showed the banner on the phone, so the check does stop a real attack, and
  the banner can fire. The real build was reinstalled afterwards.
- **Two false "SUCCEEDED" results, fixed in the test card.** A frame fires `load` for its CSP error
  page, and `sendBeacon` returns `true` when it only queues the request. The CSP blocked both
  (console: "Framing ... violates ... default-src 'none'"). The card now waits for the matching
  `securitypolicyviolation` event before it reports. Lesson for the Phase 1 tests: "no error" is not
  "got through".
- **Outside checks.** On every platform, after the malicious card: the app page was not reloaded
  (same `performance.timeOrigin`), the core answered, the frame height was clamped (largest 10,000 px
  after a card sent `1e9`), and no banner appeared. The `BroadcastChannel` from the card did not
  reach the app.
- **Navigating card.** Both variants (to the app's URL and to `https://example.com`) were removed
  after the third `load` with the message, on desktop, Android and web. The top-frame guard was
  checked on web: the app framed inside a frame rendered an empty `#root` and started no worker.
  Native shares the same guard, but on native the app's `frame-src` CSP also refuses to frame the
  app. Not exercised on its own on native.
- **Page-load event.** On desktop (WebKitGTK) it fired for the `main` webview once per main-frame
  load and never for the card frame, including when a card navigated its frame to the app (log
  lines `[page-load] main Started/Finished`, debug builds only). A reload (dev live reload) was
  followed by a new handshake and a working app and sample card. On Android the card frames loaded
  without the token being lost (the post-card `getCoreInfo` succeeded), but the event log is not
  visible there, and a main-frame reload was not exercised.
- **Sample card.** Image (64x64 PNG), audio (0.5 s WAV from a frame-local blob URL, played to the
  end) and JS (a number and a hint) all worked on Linux, Android and web, matching finding 9.
- **Timing, mount to first `height`** (sample card, no frame reuse):

  | Platform | Median | Notes |
  | --- | --- | --- |
  | Linux desktop | 24 ms | 10 runs |
  | Android, phone | 48.5 ms | 12 runs (first 59 ms, last 36 ms) |
  | Web, headless Brave | 21 to 22 ms | 10 runs each on dev and the production build |

  Creating a frame per card is fast enough on all three. No reason to reuse frames yet.
- **Infinite loop.** `<script>while(true){}</script>` froze the whole app on desktop (the web
  process at 100% CPU, the Clear button did nothing) and the card was never removed. This is the
  known limit above. It was a one-off manual check with a temporary button, not left in the code.
  Freeze recovery stays Phase 2 work.
- **`debugEmitEvent` is debug-only.** The panel's banner and the card's `call` attempt can only
  show a breach in debug builds (the web production build answers `unknownMethod`). The Android
  check above used the debug APK, as does the negative control.
- **`CardFrame` reset.** A new `html` or media list creates a new frame. The same card shown twice in a
  row keeps its frame, so the future study screen must give each card side a distinct `html` or a
  key if two identical sides can follow each other.
- **Dependencies.** `getrandom` 0.3 was already in the lockfile through Tauri. It is now a direct
  dependency of `fc-native` for the 128-bit token. Nothing else was added.
- **Not verified:** Windows (finding 4), Firefox, Safari and mobile browsers, `Content-Security-Policy:
  sandbox` on the `card` response is set but only checked by the unit test (the iframe `sandbox`
  attribute is what was tested), a main-frame reload on Android, and the real keyboard shortcut
  and focus behaviour inside a card (Phase 2).

## Build notes (step 1.10b)

Built to the plan Anthony approved in chat (no plan file). The CSP, the message shape (`{ html, media }`),
`CardFrame`, `fc-native` and the web app are unchanged. Only `frame.html` changed in the trusted code.

- **CSS `url()`.** After `DOMParser` parses the card, `rewriteCssUrls` rewrites `url(name)`, `url('name')`
  and `url("name")` to `url("blob:...")` in every `<style>` element and every `style=""` attribute, for names
  the app supplied. Anything else is copied as it is (the CSP blocks it). It is one linear pass, a name is
  read at most 200 characters ahead, and a quoted name must be closed by `)`, so hostile CSS cannot make it
  slow (a 1.5 MB string of `url(` and `url("` is a unit test). It follows the same reading as the core's
  `html::css_media_names`. `@import`, CSS escapes and `url()` in comments are not handled (a name is only
  letters, digits, `-`, `_` and `.`, so none of them can be a file name).
- **Script API: `fcMedia.url(name)`.** A read-only, non-configurable property of `window`, defined before
  the card is written. It returns the frame-local blob URL of a file the card was given, or `null` for
  anything else (other names, `__proto__`, non-strings), and never throws. The same blob URLs are used for
  `src` and CSS, so each file has one blob, revoked with the frame. It is used with `img.src`, `new Audio(url)`
  and the like. A script **cannot read the bytes** (`fetch` and `XMLHttpRequest` of the blob are blocked,
  there is no `connect-src`), so Web Audio's `decodeAudioData` on a media file does not work. A card that
  tries to replace or delete `fcMedia` gets a `TypeError`, and could only harm itself anyway.
- **Which names are supplied.** The core's `RenderedCard.media` now lists the `src` names (front, then back),
  then the `url()` names of the note type's CSS, without repeats. A name used only in a field's own
  `style=""` or `<style>`, or only inside script text, is not listed, so it is not supplied (the frame would
  rewrite it if it were). That matches `check_media` (1.10), which does not count those places as use
  either. So a script can only get URLs for files the card also names in a `src`, `[sound:]` or the note
  type's CSS. Decided in chat to leave it; a `data-media="name"` convention is the way out if it matters
  once the editor exists.
- **Blob URLs are made for every supplied file**, not only for those found in a `src`. The parent only
  supplies names the core listed, so the count is the same as before for cards without CSS media.
- **Tests.** Rust: `render_card` lists CSS names after the HTML names, without repeats, skipping web and
  `data:` URLs, and adds nothing for CSS without `url()` (2 tests, 69 in `template`). TS: `frame.test.ts`
  cuts `rewriteCssUrls` out of `frame.html` between `// <css-rewrite>` markers and tests it (the three forms,
  case and spaces, unknown names, `__proto__`, `constructor`, unterminated input, the speed case; 7 tests).
  The rest of the frame needs a real sandboxed iframe and was checked in browsers.
- **Re-run of the sandbox test (the temporary spike panel, now with a "Media card").** The 54-attempt
  malicious card:

  | Platform | Blocked | SUCCEEDED | Other | CSP violations | Alarm banners | After the card |
  | --- | --- | --- | --- | --- | --- | --- |
  | Linux, built debug app (WebKitGTK) | 38 | 0 | 16 | 38 | none | core answers, page not reloaded |
  | Android, Anthony's phone (debug APK) | 38 | 0 | 16 | 40 | none | core answers, page not reloaded |
  | Web, headless Brave, `web:dev` | 38 | 0 | 16 | 19 | none | core answers, page not reloaded |
  | Web, headless Brave, `web:build` + `web:preview` | 38 | 0 | 16 | 19 | none | core answers, page not reloaded |

  These are the same numbers as step 0.6. The sample card (image, audio played to the end, script) and both
  navigating cards ("This card tried to open another page and was stopped.") behave as in 0.6 on all three.
  The Media card (a CSS background from a `<style>`, a `style=""` background, `fcMedia.url` for an image and
  for audio played to the end, `fcMedia.url("not-given.png")` is `null`) works on all three, so `fcMedia`
  survives `document.open/write/close` on WebKitGTK, Android's WebView and Chromium.
- **New attacks on the new surface** (run in the card on web dev and web production, in headless Brave, not on
  desktop or the phone): assigning, deleting and redefining `fcMedia`, and replacing `fcMedia.url`, all throw
  `TypeError`; `fcMedia.url` with `__proto__`, `constructor`, `toString`, `hasOwnProperty`, `valueOf`, `""`,
  a trailing space, other case, `../`, `undefined`, `null`, numbers, objects (also one with a `toString` that
  returns a real name) and an array all give `null`; `fetch` and a synchronous `XMLHttpRequest` of the blob
  URL are blocked. The blob URL is `blob:null/...` (the opaque origin).
- **Timings** (mount to first `height`, one or a few runs, not a benchmark): Media card 17 ms on desktop and
  60 ms on the phone; the sample card 34 ms median on desktop and about 64 ms on the phone. In line with 0.6.

**Deviations from the plan:**

- `rewriteCssUrls` uses `let` and `const` and a template string, not `var` like the rest of `frame.html`,
  because the lint (Biome `noInnerDeclarations`, `useTemplate`) checks inline scripts too.
- `frame.test.ts` reads `frame.html` with Vite's `?raw` import, so `packages/ui/src/raw.d.ts` (a module
  declaration for `*?raw`) was added. The plan did not mention it. The package has no Node types, so the
  first version (`node:fs`) failed the typecheck. No dependency was added.
- The plan said a script's `fcMedia.url` would use the same blob URLs as the rewriting. It does, and it also
  made blob URLs eager for all supplied names (see above), which the plan did not say.
- Not done from the plan: the `@font-face` check with a scratch system font. No font file goes through the
  spike's media loader (it serves only `sample.png` and `sample.wav`), so a font loading from a blob URL is
  **not verified**. The rewriting of a `@font-face` `url()` is covered by the unit tests only.
- The new attacks ran in headless Brave only, not in the desktop app or on the phone (the plan said all
  three).
- Verification detail, not a plan change: the built desktop app showed a blank window in screenshots at
  first (the log had GBM buffer errors); it was run with `WEBKIT_DISABLE_DMABUF_RENDERER=1` for the checks.
  This is a test-environment setting, and nothing in the repo changed. The panel was driven with `xdotool`
  clicks, the phone with `adb input tap` inside the app (Anthony allowed it, the phone was not unlocked by
  me, and it was already showing the app), and the web with a scratch DevTools script that is not committed.
- Not verified: Windows, Firefox, Safari, a real font from a blob URL, new attacks on desktop and phone,
  keyboard and focus behaviour in a card (Phase 2).
