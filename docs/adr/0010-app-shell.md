# 0010: App shell and visual direction

Status: Accepted
Date: 2026-10-07
Accepted: 2026-10-07 (Anthony approved the recommendations, see Decisions on review)

## Context

Step 2.1 asks for navigation, theming and layout rules shared by every later screen. Acceptance
criteria:

- Navigation feels native on a phone (one-handed reachable) and efficient on desktop.
- The theme follows the system setting with a manual override.
- Content is never hidden behind the status bar, navigation bar, display cutout or on-screen
  keyboard. Edge-to-edge drawing and safe areas are handled once in the app shell, not per screen.
- Works with gesture and three-button navigation, in portrait and landscape.
- Checked on the real phone, not only an emulator.

Review focus: look and feel. "Ask for alternatives if it feels generic."

What exists:

- One React UI in `packages/ui`, mounted by `apps/native` (Tauri, desktop and Android) and
  `apps/web`. It is one placeholder screen holding the spikes (`DivideForm`, `SchedulingSpike`,
  `CardSandboxSpike`, and on the web `SpikePanel` and `BackupPanel`). Each app has its own
  throwaway `styles.css`.
- `MainActivity.kt` calls `enableEdgeToEdge()`. The Android theme is
  `Theme.Material3.DayNight.NoActionBar`.
- The CSP (`tauri.conf.json`) is
  `default-src 'self'; img-src 'self' asset: data:; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; frame-src card: http://card.localhost`.
- ADR 0005: the main window's capability grants only our four token-checked commands. No
  `core:default` and no plugin permissions, because on Android any command granted to the main
  window is callable from card frames. Platform features go through our own token-checked commands
  or are called from Rust.
- ADR 0002: platform differences go in the `Platform` interface (`packages/ui/src/platform.tsx`),
  not `CoreClient`.
- STATUS: native start-up logs a collection open failure and carries on, so every method then
  answers "No collection is open." The real error screen (newer collection, file in use) was left
  for this step.
- Screens that follow: deck list (2.2), review (2.3), add note (2.4), deck management (2.5),
  settings and backups (2.6), then browse, edit, inbox and stats (Phase 3), sync status (4.6).

An earlier draft of this ADR was written in a Sonnet session before the rule that ADRs are Opus
`/adr` sessions. This version replaces it. The structure is similar. The differences are the
findings below, which changed decisions 4 and 5 and corrected the CSP claim.

## Findings

What was checked in this session, how, and what was not.

### Verified on Anthony's phone

A throwaway build of `master` with a probe page in place of the UI, built in a git worktree in
the scratchpad (not in the repo) with `bun run android:build`. It was installed with `adb install -r`
and launched, then read from screenshots. Afterwards the phone's previous debug APK was installed
again. Nothing on the phone was tapped or changed.

Phone: Samsung SM-S928B, Android 16, Android System WebView **Chrome 153**. Three-button
navigation, portrait, system dark mode.

1. **`env(safe-area-inset-*)` reports the real insets in Tauri's WebView.** With
   `enableEdgeToEdge()`, top was 35 px and bottom 48 px (the three-button bar), left and right 0. A
   red outline drawn at the insets matched the bottom of the status bar and the top of the
   navigation bar exactly. No native code is needed to get them.
2. **`viewport-fit=cover` makes no difference in the WebView.** The insets were the same with and
   without it, and after switching the meta tag at run time. It is still needed for Chrome on
   Android and Safari (the web client), so it is set anyway.
3. **The Tauri CSP runs both an inline `<script>` in `index.html` and a classic `<script src>`
   file from `public/`** in the built APK. Tauri adds a hash for inline scripts at build time. The
   draft's claim that inline scripts are blocked was wrong.
4. **Inline `style=""` attributes in `index.html` were blocked** once the page also had an inline
   `<style>` element. The likely cause, not checked: Tauri hashes the `<style>` element into
   `style-src`, and a CSP that lists a hash ignores `'unsafe-inline'`. Styles set from JavaScript
   (React's `style` prop) go through the CSSOM and are not affected. Rule: `index.html` has no inline
   styles. All styling is in CSS files.
5. **A Kotlin class in the app module can be called from Rust and changes the system bar icons.**
   A local Tauri plugin (`tauri::plugin::Builder` with `register_android_plugin`, no separate crate
   or Gradle module) ran `WindowInsetsControllerCompat.isAppearanceLightStatusBars`. It was called
   with `run_mobile_plugin` from Rust and returned `Ok`. The screenshot shows the icons switching
   from light to dark. The system also gave the three-button bar a light backing.
6. **The same plugin is not callable from JavaScript.** `invoke("plugin:appearance|setBarStyle")`
   from the page was rejected with "appearance.setBarStyle not allowed. Plugin not found". The main
   window's capability does not grant it. Card frames go through the same check (ADR 0005 finding 5).
7. **`navigator.virtualKeyboard` exists in this WebView.** Calling `show()` without a user tap did
   not open the keyboard, so keyboard behaviour was **not** observed.

### Verified by reading sources and docs

8. **Android back button.** Tauri's `AppPlugin.kt` (`tauri-2.12.1`) registers the back callback.
   With no JS listener, it calls `webView.goBack()` when `canGoBack()`, otherwise the default
   (leave the app). `TauriActivity` turns off wry's own handler. So routes pushed onto the browser
   history work with the back button with no permission and no native code.
9. **Tauri's window theme is not implemented on Android.** `tao-0.37.1` has `set_theme` for Linux,
   Windows and macOS only. Bar icon colour on Android needs our own native call (finding 5).
10. **WebView insets support** (Android developer docs, "Understand window insets in WebView",
    and Chromium's `android_webview/docs/insets.md`):
    - `systemBars()` and `displayCutout()` reach `env()` from M136 when the WebView is full screen,
      and for every WebView from M144.
    - `ime()` resizes the **visual** viewport, not the layout viewport, from M139, at the bottom
      edge only.
    - Chrome's edge-to-edge guide says WebView before 140 could report a wrong top inset.
    - Apps that handle insets natively must zero them, not consume them, or the WebView gets
      "ghost padding" when they change.
11. **wry does not touch insets.** There is no insets code in the generated Kotlin (`WryActivity`,
    `RustWebView`) or `wry-0.57.0`.

### Computed

12. **Contrast** of every proposed text and control colour against the background and surface
    colours, in light and dark, with the WCAG 2 formula (a script in the scratchpad, numbers in
    decision 1). The draft's "Calm blue" control borders were under 3:1 and are corrected here.
13. **Font size.** Literata, a variable serif with optical sizes (OFL), served by Google Fonts as
    woff2: Latin 86 KB plus Latin Extended (Polish letters) 71 KB, upright only. Measured from the
    response headers. Nothing was downloaded into the repo.

### Not verified

- **The keyboard.** Whether the visual viewport shrinks as the docs say, and whether a focused
  field near the bottom scrolls into view. It needs a tap. Checked in the build (Anthony taps).
- **Gesture navigation, landscape and the cutout in landscape.** The phone is in three-button
  portrait, and changing those settings is off limits. Checked in the build by Anthony.
- **Android system font size.** Whether the WebView scales `rem` text with it. Checked in the build
  by Anthony.
- **Desktop.** WebKitGTK and WebView2 were not run in this session. That covers
  `prefers-color-scheme` following the desktop theme, and Tauri's `Window::set_theme` matching the
  title bar to an override. Both are low risk and checked in the build.
- **Whether `pushState` entries count for the WebView's `canGoBack()`.** The docs imply they do.
  Also whether loading card frames adds history entries. Checked in the build with the card spike.
- **Older WebViews** (before M136, or 140 for the top inset). Only one phone was used.

## Decision 1: visual direction (Anthony picks)

Three directions, each with a light and a dark palette. All text pairs are at least 4.5:1 and
control borders and focus rings at least 3:1, against both the background and surface colours.
**Open `docs/plans/2.1-directions.html` in a browser** to see them on a phone-sized deck list.

### A. Paper (recommended)

A study notebook: warm paper and ink, one terracotta accent, serif headings, system sans for the
controls, flat surfaces with a 1 px border instead of shadows, 10 px corners. It suits a learning
app where the card text is the product, and it does not look like a default Material or Bootstrap
app.

| Token | Light | Dark |
|---|---|---|
| `bg` | `#FAF6EE` | `#1C1A17` |
| `surface` | `#FFFDF8` | `#25221E` |
| `text` | `#2A2622` (13.9:1 on bg) | `#EDE6D8` (14.0) |
| `muted` | `#675F54` (5.8) | `#A89F8F` (6.0 on surface) |
| `accent` | `#A93F24` (5.7) | `#E8906F` (6.5 on surface) |
| `on-accent` | `#FFFFFF` (6.1 on accent) | `#1C1A17` (7.2) |
| `control` (borders, focus) | `#8C8274` (3.5) | `#7D7466` (3.4 on surface) |
| `danger` | `#A3261B` (6.8) | `#F08A7E` (6.5 on surface) |
| `line` (decorative dividers) | `#E6DED0` | `#3A352E` |

The lowest ratio of each pair (against bg or surface) is shown when it differs. `line` is for
decoration only and has no contrast requirement.

### B. Calm blue

Cool neutrals, one blue accent, system fonts, 14 px corners and a light shadow. Clean and familiar.
The safest choice, and the most likely to read as generic.

| Token | Light | Dark |
|---|---|---|
| `bg` / `surface` | `#F5F7FA` / `#FFFFFF` | `#111827` / `#1A2333` |
| `text` | `#1B2430` (14.6) | `#E6EBF2` (13.2) |
| `muted` | `#556070` (5.9) | `#9AA6B8` (6.4) |
| `accent` / `on-accent` | `#2057D6` (5.8) / `#FFFFFF` (6.2) | `#7AA7FF` (6.6) / `#111827` (7.4) |
| `control` | `#778496` (3.5) | `#6B778B` (3.5) |
| `danger` | `#B3261E` (6.1) | `#F2867C` (6.4) |

### C. High contrast

Black and white, a blue accent in light and yellow in dark, a larger base size (17 px) and square
corners. The fastest to scan and the strongest on accessibility. Also the most austere.

| Token | Light | Dark |
|---|---|---|
| `bg` = `surface` | `#FFFFFF` | `#000000` |
| `text` | `#000000` (21) | `#FFFFFF` (21) |
| `muted` | `#404040` (10.4) | `#C4C4C4` (12.0) |
| `accent` / `on-accent` | `#0039B3` (9.4) / `#FFFFFF` | `#FFD43B` (14.7) / `#000000` |
| `control` | `#000000` | `#FFFFFF` |
| `danger` | `#A30000` (8.2) | `#FF7B72` (8.3) |

### Type: system fonts or a bundled serif (applies to A)

| Option | For | Against |
|---|---|---|
| **System serif stack (recommended)**: `"Iowan Old Style", "Palatino Linotype", Palatino, "Noto Serif", "Source Serif 4", Georgia, serif` | Nothing to ship. The card frame (opaque origin, `font-src blob: data:`) can use the same system fonts, so app headings and card text match on each device | It looks a little different per platform: Noto Serif on Android and Anthony's Linux (checked with `fc-list`), Palatino Linotype on Windows |
| Bundle Literata (OFL), about 157 KB upright | The same look on every platform | An asset and licence file to carry. Card text cannot use it unless the parent passes the font into the card frame as a blob, which is an ADR 0005 change. Card text and headings would then differ |

Controls use `system-ui, sans-serif` in every direction. Directions B and C use system sans
throughout.

Whichever direction is picked, the rest of this ADR is the same. The direction only changes the
token values in one CSS file. The components are drawn for the chosen direction (corner radius,
icon stroke weight, serif headings), so a late switch costs a day of tidying, not a rewrite.

## Decision 2: navigation

**Destinations:** Decks, Add, Browse, Settings, in that order. Add is a destination rather than a
floating button on Decks, because fast capture is a product priority (`PRODUCT.md`) and it should be
one tap from anywhere. Study is not a destination: it is a full-screen route opened from a deck,
with no navigation chrome and a back control. Stats, Inbox and Sync status arrive later as one more
entry each.

**Layout by window width** (a CSS media query on width, never a platform check):

```
 Compact: under 600 px                 Medium and wide: 600 px and over
 +---------------------------+         +------+---------------------------+
 | Decks                     |         |  ▣   |  Decks                    |
 |                           |         | Decks|                           |
 |   content scrolls         |         |  ⊕   |   content, max ~46rem,    |
 |                           |         |  Add |   centred                 |
 |                           |         |  ⌕   |                           |
 |                           |         |Browse|                           |
 |  [Decks][Add][Browse][Set]|         |  ⚙   |                           |
 +---------------------------+         +------+---------------------------+
   bottom bar in thumb reach            rail; from 1024 px icon and label in a row
```

- **Compact** (phones in portrait, narrow desktop windows): a bottom bar with four equal targets,
  icon above label, at least 56 px tall plus the bottom inset. The page title is at the top and
  scrolls with the content. A screen's primary action sits at the bottom, in thumb reach. Nothing
  important lives in the top corners.
- **600 px and over** (phones in landscape, tablets, desktop): a navigation rail on the left (icon
  above label, about 88 px wide). From 1024 px it widens into a sidebar with icon and label side by
  side. It is the same element restyled by CSS. Phones in landscape get the rail because a bottom
  bar would take too much of a short screen.
- **Keyboard (desktop):** `g` then `d`, `a`, `b` or `s` goes to a destination, and `?` lists the
  shortcuts. Shortcuts are ignored while a text field has focus. Keys pressed while focus is inside
  a card frame go to the card and never reach the app (ADR 0005), which is correct.
- **Routing:** a small router in `packages/ui`. The route lives in the URL hash (`#/decks`,
  `#/study/<deckId>`), and every navigation is a history entry. Back (Android button, browser,
  `Alt+Left`) pops it (finding 8). Hash URLs need no server fallback, so they work on Tauri's
  scheme and on any free static host for the web client. At a top-level destination, Android back
  leaves the app, which is the platform default. How study asks before leaving a half-answered card
  is for 2.3.
- On a route change, focus moves to the page heading and `document.title` is set.

## Decision 3: theme

- Colour, spacing, type scale, radius, focus ring and motion are CSS custom properties in one file,
  `packages/ui/src/styles/tokens.css`. Components use only tokens, never literal colours.
- Setting: **System** (default), **Light**, **Dark**. System follows `prefers-color-scheme` live
  (the Android theme is DayNight, so the WebView reports the system mode; finding 1 showed
  `prefers dark: true` with the phone in dark mode). An override sets `data-theme="light|dark"` on
  `<html>`. `color-scheme` is set to match, so form controls and scrollbars follow.
- **Stored per device** in `localStorage` (try/catch, falls back to System). It is an appearance
  preference like a window size, not collection data. It has to be readable synchronously before
  first paint, which the async core cannot do. It does not sync.
- **No flash of the wrong theme:** a classic script file, `public/theme-boot.js`, loaded by a
  `<script src>` in each app's `index.html` head, sets `data-theme` before first paint. A file and
  not an inline script, so `index.html` stays free of inline code for any CSP the web client adds
  in Phase 6 (finding 3 shows an inline script would also work in Tauri).
- **System bars and window chrome follow an override.** The shell calls
  `platform.setSystemTheme(effectiveTheme)` whenever the effective theme changes. On native this
  goes through a fifth token-checked command, `set_system_theme`:
  - **Android:** Rust calls a local Tauri plugin (`AppearancePlugin.kt` in the app module) with
    `run_mobile_plugin`. It sets light or dark status and navigation bar icons (findings 5 and 6).
    The plugin has no JS permission, and the command checks the token like the other four.
  - **Desktop:** `Window::set_theme`, so the title bar matches. Not verified (see Findings).
  - **Web:** `<meta name="theme-color">` is updated. Nothing else.

  Without this, a Light override on a phone in dark mode would show light icons on a light page.
- **Card content keeps its own theme handling** in the sandboxed frame (ADR 0005). A frame
  document follows the system colour scheme, not our override. The review screen (2.3) has to tell
  the frame which theme is in effect (for example a `night` class set by `frame.html`). That
  changes the frame message, so 2.3 records it as an amendment to ADR 0005. It is not decided here.

## Decision 4: safe areas and the keyboard, once

One component, `AppShell`, owns every inset. Screens render inside the shell's content region and
never mention `env()`, `safe-area` or `visualViewport`.

- Each app's `index.html` has
  `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">`.
  `viewport-fit=cover` matters for browsers (finding 2).
- **System bars and cutout:** the shell's CSS uses `env(safe-area-inset-*)` directly: top inset
  above the title, bottom inset under the bottom bar, left and right insets on the rail and the
  content (the cutout in landscape). Verified on the phone (finding 1). No native inset code.
- **Keyboard:** the shell measures the visual viewport
  (`innerHeight - visualViewport.height - visualViewport.offsetTop`, clamped at 0) on its
  `resize` and `scroll` events and writes `--keyboard-inset` on the shell root. This works in the
  Android WebView (M139 and later, finding 10), Chrome and Safari. While the inset is non-zero:
  - the bottom bar is hidden, because it would cover a third of a landscape screen;
  - the content region gets bottom padding of `--keyboard-inset`, so the last field can scroll
    above the keyboard;
  - a screen that needs an action pinned above the keyboard (2.4's save button) asks the shell for
    a "bottom action" slot that the shell positions. Screens never do the arithmetic themselves.
- No `interactive-widget` in the viewport meta. The WebView docs describe visual viewport
  resizing only, and one mechanism is easier to reason about than two.
- Gesture and three-button navigation differ only in the size of the bottom inset. Nothing in the
  shell branches on the mode.

## Decision 5: accessibility baseline

- Contrast as in decision 1, enforced by a Vitest test that parses `tokens.css` and checks every
  text, control and accent pair in both themes (4.5:1 text, 3:1 controls). A later palette change
  cannot silently break it.
- Sizes in `rem`, so the system and browser font size scale the UI. The layout is checked at 200%
  text size.
- Every control can be reached by keyboard, with a visible focus ring (2 px, `control` or `accent`
  token, 3:1). Tap targets at least 48 × 48 px in the compact layout, 32 px at 600 px and over.
- Landmarks: `nav` labelled "Main", `main`, one `h1` per page. Icon-only controls have an
  `aria-label`. The current destination has `aria-current="page"`.
- `prefers-reduced-motion`: transitions off.
- Errors use `role="alert"`, status messages `role="status"`.

## Decision 6: empty and error states

- `EmptyState` (icon, one plain sentence, at most one button) is what every empty list uses. No
  jargon: "You have no cards yet" with "Add your first card", not "Collection has 0 notes".
- **Collection problem screen.** When the collection cannot be opened, the shell shows a full
  screen instead of the destinations:
  - `updateRequired`: "This collection was made by a newer version of the app. Update the app to
    open it. Your cards are untouched."
  - `unavailable`: the file is in use (another window, or another tab on the web), with what to
    try.
  - anything else: the core's message and "Restart the app".

  The collection is never modified on these paths.
- For this to work on native, the open error has to reach the UI with its kind. Today `fc-native`
  turns it into a string and later calls answer "No collection is open." The fix is small and in the
  host layer: `fc-api`'s `Core` keeps the error from the last failed open, and the "No collection
  is open" error is replaced by that error. The web host already reports its open error to the
  transport.

## Decision 7: what happens to the spikes

`DivideForm`, `SchedulingSpike`, `CardSandboxSpike`, and on the web `SpikePanel` and
`BackupPanel`, are still test rigs until their replacements land (2.3 for the card frame, 2.6 for
backups). They move to a **Developer** screen opened from Settings, shown only in debug builds:
`import.meta.env.DEV` for the dev servers (`tauri dev`, `bun run web:dev`), and Tauri's
`TAURI_ENV_DEBUG`, exposed to Vite through `envPrefix`, for the debug APK and debug desktop
builds. Those are production Vite builds, so `DEV` is false there. Not verified: the build checks
that the screen appears in `bun run android:build` and not in a release build. In a release build it
is not there, so the spikes stop shaping the real layout. They are still deleted by the steps STATUS
names. `apps/web` passes its two panels to the shell through a prop (the UI cannot import from the
app).

## Options considered

### Routing

| Option | For | Against |
|---|---|---|
| **Own hash router (chosen)** | No dependency. About six flat routes. We control back and focus behaviour. Works on any host | About 100 lines to maintain. No nested data loading (not needed: the core owns data) |
| `wouter` (MIT, about 2 KB) | Small, hooks API, hash mode available | A dependency for something this small. We would still write the focus and title handling |
| `react-router` 7 / TanStack Router | Standard, nested routes, loaders, typed routes (TanStack) | Large for six flat routes. Their data APIs pull towards a shape the core already owns |
| Tab state only, no history | Simplest | No back button, no deep link to a deck, Android back would leave the app from any screen |

### Navigation pattern

| Option | For | Against |
|---|---|---|
| **Bottom bar under 600 px, rail and sidebar above (chosen)** | Matches platform habit. The bar is in thumb reach. Landscape phones keep their height. The sidebar scales to more destinations | Two layouts to test |
| Bottom bar everywhere | One layout | Wastes a wide screen. Eats a landscape phone's height |
| Drawer (hamburger) on the phone | Room for many destinations | Hidden navigation, top-left, out of one-handed reach |

### Insets and keyboard source

| Option | For | Against |
|---|---|---|
| **CSS `env()` plus `visualViewport` (chosen)** | Verified on the phone (insets). Works in the WebView, Chrome and Safari with the same code. No native code | Depends on WebView M136 or later (M139 for the keyboard). Older WebViews update from the Play Store |
| Native inset listener in `MainActivity.kt` writing CSS variables into the page | Works on any WebView version. Gives the keyboard height exactly | Kotlin plus a JS injection path. Must zero rather than consume insets (finding 10). Not needed on the target phone |
| VirtualKeyboard API (`overlaysContent` and `env(keyboard-inset-height)`) | Exact keyboard geometry in CSS. Present in the phone's WebView (finding 7) | Chromium only: no Safari, no WebKitGTK. Would need a second path anyway |

### Android bar icon colour for an override

| Option | For | Against |
|---|---|---|
| **Local Tauri plugin called from Rust behind our token-checked command (chosen)** | Verified (findings 5 and 6). No new crate, no JS permission, card frames cannot reach it | A Kotlin file and a fifth command to keep |
| `addJavascriptInterface` in `MainActivity` | Very little code | Exposed to every frame, card frames included. Breaks the ADR 0005 rule |
| Do nothing (icons follow the system) | No native code | Light icons on a light page when the override differs from the system |
| Recreate the activity with `AppCompatDelegate.setDefaultNightMode` | Pure Android | Restarts the WebView and the page. Still needs a call from the page |

### Components and icons

| Option | For | Against |
|---|---|---|
| **Own components, inline SVG icons drawn for the direction (chosen)** | No dependency or licence. Every detail follows the direction. Small | We draw about ten simple icons |
| A component library (Radix, MUI, ...) | Accessible primitives for free | Dependencies. The direction fights the library's look. A larger bundle next to a wasm-heavy web client |
| An icon package (Lucide, ...) | Many consistent icons | A dependency for ten icons. Copying a few with their ISC notice is a fallback if drawing them goes badly |

## Consequences

- Every later screen is written against `AppShell`, `EmptyState` and the tokens, never against
  insets or the keyboard. A screen that needs something the shell does not offer (a sticky action,
  a dialog, a full-screen route) asks for it as a shell feature, not a local workaround.
- Adding a destination is one entry in a list. With more than about six, the compact bar moves the
  extras into a "More" screen. Not decided now.
- `packages/ui` gains CSS (tokens, shell, components). The apps stop owning styles and only mount
  the shell. `index.html` holds no inline styles (finding 4).
- A fifth bridge command, `set_system_theme`, with the same token check and its own entry in the
  "rejects a missing token" test. Code review must keep the Android plugin free of JS permissions.
- The theme preference is per device. If Anthony wants it to follow him between devices, it becomes
  a synced setting in Phase 4.
- The card frame does not follow the app's theme override until 2.3 passes it in (an ADR 0005
  amendment then).
- `fc-api` remembers the last open error. A small change to the host layer, no API change.
- Visual quality and the inset behaviour are checked on the real phone and the desktop app, not in
  `happy-dom` (it has no layout).
- No new dependencies.

## Revisit if

- A WebView older than M136 has to be supported, or `env()` or the keyboard behaves differently on
  another device or Android version. Then add the native inset listener (options table), which only
  changes where the numbers come from.
- More than six destinations are needed, or tablets and foldables become a target (a two-pane
  layout).
- Anthony wants the theme to sync, or more appearance settings than System, Light and Dark.
- The web client (Phase 6) adds a CSP: it must allow `script-src 'self'` for `theme-boot.js`.
- Add-ons (Phase 10) need to add screens or navigation entries.
- The system serif looks poor on a platform Anthony uses: bundle Literata and pass it to the card
  frame (an ADR 0005 change).

## Decisions on review

Anthony approved the ADR on 2026-10-07 with every recommendation as written:

- Direction **A Paper**, with the **system serif stack** (no bundled font).
- Destinations Decks, Add, Browse, Settings, with Add as a destination.
- The theme preference is per device and does not sync.
- The fifth token-checked command and the Kotlin plugin for the Android bar icons.
- A Developer screen in debug builds holds the spikes until 2.3 and 2.6.
- No new dependencies in 2.1.

### Questions as asked

1. Visual direction: **A Paper (recommended)**, B Calm blue or C High contrast. Open
   `docs/plans/2.1-directions.html`. Ask for other directions if all three feel generic.
2. For A: the system serif stack (recommended) or a bundled Literata.
3. Destinations Decks, Add, Browse, Settings, with Add as a destination rather than a floating
   button.
4. The theme preference is per device and does not sync.
5. A fifth token-checked command and a small Kotlin plugin so the Android bar icons follow an
   override.
6. A Developer screen in development and debug builds holds the spikes until 2.3 and 2.6.
7. No new dependencies in 2.1.

## Build notes (step 2.1)

Built on `step/2.1-shell-build`. Split by Anthony's decision in chat: this PR is the UI and the
`fc-api` open error. The native system-bar part is **step 2.1b**, a separate PR (see "Left for
2.1b"). About 2,670 added lines, 875 of them tests, against the plan's 1,300.

### What was built

- **Tokens and base styles** (`packages/ui/src/styles/`): the Paper palette for light, system dark
  and `data-theme="dark"`, spacing, type, radius, focus ring and motion tokens. Both apps' old
  `styles.css` are gone, the spike styles live in `spikes.css`, and the apps get the styles by
  importing `ui`. `tokens.test.ts` parses `tokens.css` and checks every pair (4.5:1 text, 3:1
  controls) in both themes, and that the two dark blocks are identical.
- **Theme** (`theme.tsx`): `System | Light | Dark` in `localStorage` key `fc.theme`, `data-theme` on
  `<html>`, live `matchMedia` for System, `platform.setSystemTheme(effective)` on every change.
  `apps/*/public/theme-boot.js` sets `data-theme` before first paint, loaded by `<script src>` in
  both `index.html` heads. No inline `<style>` or `style=""` in either `index.html`. The web client
  also updates `<meta name="theme-color">` to the `bg` token. Viewport meta has `viewport-fit=cover`.
- **Router** (`router.tsx`, about 165 lines with `Link` and `PageHeading`): hash routes `decks`,
  `add`, `browse`, `settings`, `settings/developer`, `study/:deckId`. Unknown hash is replaced with
  `#/decks`. After a route change the page `h1` takes focus and `document.title` is
  `"<Page> · Flash cards"`.
- **AppShell** (`shell/`): bottom bar under 600 px, rail from 600 px, sidebar from 1024 px, from one
  `destinations` list. `env(safe-area-inset-*)` and `visualViewport` appear only inside `shell/`
  (a test greps for it). `useKeyboardInset` writes `--keyboard-inset` and `data-keyboard`. The bar
  hides under 600 px while the keyboard is open. `BottomAction` is a portal into a slot below the
  content. `g` then `d`/`a`/`b`/`s` and `?` (a `dialog`), ignored in text fields and with Ctrl, Alt
  or Meta. Study is full screen with a back button. Eight inline SVG icons.
- **Screens:** `EmptyState`, Decks, Add, Browse and Study placeholders, Settings with the Appearance
  radio group and (debug builds) a Developer tools row, the collection problem screen, and the
  Developer screen (versions, a keyboard test with a `BottomAction`, the spikes, and the web
  client's two panels through `extraDeveloperTools`).
- **Developer screen only in debug builds:** `isDeveloperBuild()` is `import.meta.env.DEV ||
  TAURI_ENV_DEBUG === "true"`. `envPrefix` in `apps/native/vite.config.ts` was
  `"TAURI_ENV_*"`, which matches nothing (Vite prefixes are not globs); a probe build gave
  `undefined` with it and `true` with `"TAURI_ENV_"`. Fixed.
- **The core remembers why a collection did not open.** `fc_core::Core` keeps the last open error
  and `fc-api` (`getCollectionInfo`, the backup calls) answers with it, so a newer collection gives
  `updateRequired` and a file in use gives `unavailable`, not "No collection is open." Four Rust
  tests, including a collection with `user_version` 99 that is left byte for byte unchanged.

### Verified

- `cargo xtask check` passes (123 Vitest tests, the Rust tests, clippy, fmt, the wasm build).
- **Phone** (SM-S928B, three-button, portrait, dark): `bun run android:build`, `android:install`,
  screenshot of Decks. The heading clears the status bar and the bottom bar sits on top of the
  navigation bar, with nothing hidden. The serif heading renders.
- **Debug flag reaches the APK:** in the built bundle `isDeveloperBuild` is `return!0` for
  `android:build`, and `return!1` for a Vite build with `TAURI_ENV_DEBUG` unset.
- **Desktop** (`bun run dev`, Linux, 1208 x 633 window): sidebar layout, `g` `s` goes to Settings,
  `?` opens the shortcuts list with focus on Close, choosing Dark switches the page.
- **Web** (`bun run web:dev`, Brave on Linux at phone width and at a wider window): the bottom bar
  at phone width, the rail when wider, the Developer screen with both web panels and the
  `BottomAction` button pinned at the bottom, core and collection versions shown.

### Not verified

- Anything on the phone beyond the Decks screen (I may only install and take screenshots, so I
  could not tap to the other destinations): the other destinations, the Developer screen in the
  APK, gesture navigation, landscape and the cutout on each side, the on-screen keyboard and the
  `BottomAction` above it, Android back through routes and out of the app, back after several
  cards in the card spike, the largest system font size. **Anthony checks these.** The pushState
  and `canGoBack()` question from ADR findings is part of the back check.
- Desktop resizing to the compact layout (the window manager ignored `xdotool windowsize`; the
  compact layout was seen in the web client instead), a Windows build, WebKitGTK's console for CSP
  errors, whether the native window title follows `document.title`, and "no flash of the wrong
  theme" (a screenshot cannot show a flash).
- Whether the desktop title bar follows an override: that is 2.1b.
- The keyboard behaviour itself. The tests mock `visualViewport` and check the CSS variable and
  `data-keyboard`; `happy-dom` has no layout, so that the bar really hides and the content really
  shrinks is checked only by CSS reading and the web screenshot above.
- A release build with the Developer route typed by hand. It is covered by a test, not a build.

### Left for 2.1b

`Platform.setSystemTheme` exists and is called on every change of the effective theme. The web
client implements it (`theme-color` meta). **The native apps pass a no-op**, so on the phone the
status bar and navigation bar icons still follow the system, not an override: a Light override on a
phone in dark mode shows light icons on a light page until 2.1b. 2.1b is the fifth command
`set_system_theme(token, dark)`, the `appearance` plugin and `AppearancePlugin.kt`, the
`Platform` wiring in `apps/native`, desktop `Window::set_theme`, the extended token test, and the
ADR 0005 amendment line for the fifth command.

### Deviations from the plan

- **Split.** Native system bars, the fifth command, the Kotlin plugin, `Window::set_theme` and the
  ADR 0005 amendment line moved to 2.1b (Anthony agreed in chat after I stopped at the 1,600 line
  limit).
- **Size.** About 2,670 added lines (875 tests) against the plan's 1,300.
- **The open error is kept in `fc-core`, not `fc-api`.** `Core` lives in `fc-core`, which cannot
  name `ApiError`, so `Core::open_error()` stores the `CollectionError` (now `Clone`) and `fc-api`
  converts it. `fc-native`'s `open_collection` is unchanged, because `Core` keeps the error. A
  failure before the core is asked (no data directory, a config error) still answers "No collection
  is open." The plan said `fc-native` keeps an `ApiError`.
- **`App` makes the `getCollectionInfo` call, not `AppShell`**, and `AppShell` has a `bare` prop
  (no navigation, no back control) for the problem and "Opening your cards..." screens. Same
  result, and the shell stays free of core calls.
- **The keyboard shrinks the shell, not just the content.** The shell root gets bottom padding of
  `--keyboard-inset`, so the content region and the `BottomAction` slot both end above the
  keyboard, instead of only padding the content as ADR decision 4 says.
- **`theme-boot.js`:** two identical files and a test that they match (the plan let me choose).
- **A release build's Developer route shows Settings** (the plan did not say). The web client's
  release build (`web:build`) therefore has no Developer screen either, so no spike panels.
- **Extras:** links use the `accent` token (default blue links looked wrong), a "Opening your
  cards..." status while `getCollectionInfo` is pending, a pinch-zoom guard in `useKeyboardInset`
  (zoom also shrinks the visual viewport), `--target` is 2.5rem from 600 px (the ADR said at least
  32 px).
- **Tests:** the plan's "keyboard inset hides the bar" test checks `data-keyboard` and the CSS
  variable, not the CSS (see Not verified). The contrast test does not cover `line`, which the ADR
  says has no requirement.
- **STATUS:** I replaced the design-session paragraphs in "Current step" with build status. The
  phone findings stay in this ADR.
- `docs/plans/2.1-directions.html` is kept as the record of the choice, as the plan said.

## Build notes (step 2.1b)

Built on `step/2.1b-system-bars`. About 280 added lines of code and tests (about 80 of them tests), plus these docs. No new dependencies
(`tauri`'s `test` feature is enabled for tests only).

### What was built

- **`set_system_theme(token, dark, followSystem)`**, the fifth bridge command
  (`apps/native/src-tauri/src/lib.rs`). It checks the token first, then asks `Appearance` to apply
  the theme. It is registered like the other four: `build.rs`, the generated permission and
  `allow-set-system-theme` in `capabilities/default.json`. (Tauri turns `_` into `-` in permission
  names.)
- **`appearance.rs`**: the local `appearance` plugin and the `Appearance` state. The plugin has no
  commands and no permissions. On Android its `setup` registers `AppearancePlugin` and keeps the
  handle. `apply` calls `Window::set_theme` on desktop and `run_mobile_plugin("setBarStyle")` on
  Android.
- **`AppearancePlugin.kt`**: sets `isAppearanceLightStatusBars` and `isAppearanceLightNavigationBars`
  on the UI thread.
- **`Platform.setSystemTheme(theme, followSystem)`**, called by `ThemeProvider` with
  `followSystem = preference === "system"`. `createTauriTransport()` returns the transport plus
  `setSystemTheme`, because the token lives in its closure. `apps/native/src/main.tsx` passes it
  through. A failure is logged and never thrown.
- **Tests:** a test on Tauri's mock runtime that `set_system_theme` rejects an empty, a wrong and a
  missing token with `internal` and accepts the issued one; transport tests for the arguments and
  for a failed call; the theme and App tests now check `followSystem`.

### Verified

- `cargo xtask check` passes.
- **Phone** (SM-S928B, three-button, portrait, system dark, `bun run android:build`, `adb install`):
  - Choosing **Light** in Settings gave a light page with **dark** status bar icons and a light
    navigation bar backing with dark icons. Choosing **System** again gave light icons on the dark
    page. logcat showed `Tauri/Plugin: pluginId: appearance, command: setBarStyle` for each change.
    This is the whole chain: page, token-checked command, Rust, Kotlin.
  - **Android back** after Settings then Developer tools: back goes to Settings, then Decks, and a
    third back leaves the app. So `pushState` entries count for the WebView's `canGoBack()`.
  - Not asked for but seen: the Developer tools row is in the debug APK.
- **Desktop** (Linux, X11, i3, `bun run dev`): the system theme here is light. Dark to System,
  Light to System and Dark to System again each returned the page to the same light colour, so
  `set_theme(None)` releases the window and nothing is stuck. No `Could not set the system theme`
  errors in the log.

### Not verified

- **The desktop title bar changing colour.** This machine runs i3, which draws the title bar, so a
  GTK theme change cannot show. It needs GNOME, KDE or Windows.
- **A system that is dark while an override is Light on desktop** (the system here is light), and
  Windows (`set_theme` is documented for it, not run).
- **Gesture navigation.** I tried Samsung's `navigation_bar_gesture_while_hidden` setting, which
  did not change the mode (the navigation bar stayed three-button), and put it back to `0`. Switching
  the mode needs system overlay changes, which I did not make. Landscape, the keyboard with
  `BottomAction`, the cutout and the largest font size are also not checked.
- **That card frames cannot call the plugin**: the capability grants only the `allow-` entry for
  our command and the probe in finding 6 showed the rejection. I did not repeat it from a card.
- A release APK. The bar icon code does not depend on the build type.

### Deviations from the plan

- **`setSystemTheme` has a second argument, `followSystem`.** ADR 0010 and the plan have only
  the theme. On desktop `set_theme(Some(...))` pins the window and WebKitGTK's
  `prefers-color-scheme` follows it, so System could not return to the desktop theme. With
  `followSystem` the command calls `set_theme(None)`. Android and web ignore it. The command has
  a third argument for the same reason. (Raised in the plan, and Anthony said go.)
- **The Rust command is generic over the runtime** (`Webview<R>`), so the test can run it on the
  mock runtime. No effect at run time.
- **The token test covers `set_system_theme` only.** The plan said to extend a test of every command,
  but no such test existed (only the `Gate` unit tests), and the other four commands are not generic
  over the runtime. Adding the mock-runtime test for them is not part of this step.
- **`tauri` with the `test` feature is a dev-dependency of `fc-native`.** Not a new crate, but it
  is a Cargo.toml line the plan did not list.
- **The payload is `serde_json::json!`**, not a `Serialize` struct, because `serde` is not a direct
  dependency and I did not add one.
- **I used the phone beyond install and screenshots.** Anthony said "you can do gestures" after the
  plan. I took that to allow taps and key events inside the app (Settings choices, back presses, a
  launch with `monkey`) and one attempt to switch to gesture navigation, which I reversed. I did not
  unlock the phone, which was already awake and unlocked. If Anthony meant something narrower, say so
  and I will not repeat it.
- **`Appearance` is managed on every platform** (an empty struct off Android), not only on Android,
  so the command has one signature everywhere.
- **Size:** about 280 lines of code and tests. No split needed.
- **STATUS:** the "Current step" text from 2.1 (waiting for review) was replaced by the 2.1b status.

## Build notes (step 2.2)

Built on `step/2.2-deck-list`. About 750 added lines, 340 of them tests, 80 of them generated
bindings and docs, against the plan's 500. No ADR of its own: it is one read-only method and one
screen on the 2.1 shell. No new dependencies.

### What was built

- **`getDeckList`** (`crates/fc-api/src/decks.rs`): the decks that are not deleted in tree order,
  each with `id` (UUID string), `name` (the display name), `path`, `parentId`, `depth`, `newCount`,
  `learningCount`, `reviewCount`, plus `totalCards`. It reads `Collection::decks()` and
  `Collection::deck_counts()` in one call, so names and counts agree. A parent's counts include its
  sub-decks (that is how `deck_counts` works). A read failure is logged and answers the generic
  internal error. Bindings regenerated.
- **`DecksScreen`** (`packages/ui/src/screens/DecksScreen.tsx`, replacing the placeholder):
  - a row per deck, indented by depth, with three count columns headed New, Learn and Review. Zero
    counts are muted. Screen readers hear "5 new, 1 learning, 3 to review" from hidden text;
  - the whole row is a link to `#/study/<deckId>`. A separate chevron button (`aria-expanded`)
    collapses the decks inside a deck. The collapsed set is kept per device in `localStorage`
    (`fc.collapsedDecks`, try/catch);
  - no cards in the collection shows `EmptyState`: "You have no cards yet. Add your first card to
    start studying." with a button to Add (still the 2.1 placeholder until 2.4);
  - cards but nothing due shows "You're done for now. Nothing is due today." above the list;
  - the list is read again when the screen opens and on window `focus` and `visibilitychange`. A
    failed refresh keeps the list on screen. A failed first load shows an alert with "Try again".
- `ChevronIcon`, `.visually-hidden` and the deck list styles (tokens only).

### Verified

- `cargo xtask check` passes (134 Vitest tests, the Rust tests, clippy, fmt, the wasm build).
- Rust: four tests through `dispatch` (a new collection has only Default and no cards, counts roll
  up into the parent and the order is tree order, a deleted deck and its cards are left out, no
  collection open).
- Vitest, nine tests on the screen: tree and counts as a screen reader reads them, the row's
  `href`, collapse and expand with the choice remembered and restored, the empty state and its
  button, the "done for now" line, a failed load and Try again, refresh on focus, a failed refresh.
- **Desktop** (Linux, `bun run dev` with `XDG_DATA_HOME` pointed at the scratchpad, on a collection
  made by `fc fake --notes 300 --decks 6`, dark theme): a nested tree with counts that line up,
  collapse and expand by mouse and by keyboard (Tab then Space), the state survives going to
  another screen and back, a click on a row opens `#/study/<id>`, Back returns. The same screen in a
  400 px wide floating window shows the bottom bar and the same tree. The real collection was not
  touched.
- **Phone** (SM-S928B, system dark, in landscape): the debug APK installed and launched. The
  collection there has no cards, so it shows the empty state beside the navigation rail, with the
  heading clear of the status bar.

### Not verified

- **A populated list on the phone.** Nothing can add cards until 2.4, and I did not put data on the
  phone. The compact populated layout was seen only in a narrow desktop window, not on the device.
- The empty state in portrait on the phone (it was in landscape), gesture navigation, the largest
  system font size, and 200% text size on this screen.
- Pressing Enter on a focused row (the row is a plain `<a>`, and Tab focus showed the ring). No test
  presses Enter, because `happy-dom` does not turn Enter into a click.
- "You're done for now" on a real collection (the fake one has cards due). It is covered by a
  Vitest test.
- Windows, Firefox, Safari, and the web client (it has no way to hold cards yet).
- Time of `getDeckList` on the phone and in wasm. On Linux `deck_counts` was 130 ms at 50,000 cards
  (1.7b). Not measured through the UI.

### Deviations from the plan

- **Column headings are New, Learn and Review**, not New, Learning and To review as in the plan.
  The longer headings wrapped and ran together at the width of three count columns (seen on
  desktop). Screen readers still hear "learning" and "to review".
- **The plan said a Vitest would check that Enter activates the row.** I did not write it (see Not
  verified).
- **`App.test.tsx` changed:** its fake transport answers `getDeckList`, and the Decks test now expects
  the empty state, not the 2.1 placeholder text.
- **Extras not in the plan's file list:** `DeckList` and `DeckSummary` exported from `core-client`,
  `ChevronIcon`, `.visually-hidden`, and a local `log` function in `decks.rs` that turns a core
  error into the internal error (`fc-api` had no `From` for deck and study errors, and I did not
  add one).
- **Size:** about 750 lines against the plan's 500, mostly tests and generated types. No split.
- **Desktop check:** I floated and resized the app window with `i3-msg` to see the compact layout
  and closed it afterwards.
- **A screenshot slip:** my first desktop screenshot was of the whole screen and included other
  windows. I deleted it and captured only the app window after that.
- **Anthony's request during the step:** all browser and desktop checks were done in dark mode.
- **STATUS:** the stale "2.1b waiting for review" text was replaced (PR #34 was already merged).

## Build notes (step 2.3a)

Built on `step/2.3a-study-api`. First half of step 2.3 (the review screen), split in two PRs as
Anthony agreed in chat. About 830 lines of code and tests (about 330 of them tests) and 155 lines of
generated bindings, against the plan's 700. No migration, no new dependency. The review screen itself
is 2.3b.

### What was built

- **Seven `fc-api` methods** (`crates/fc-api/src/study.rs`), thin wrappers over `fc_core::study`,
  `render_card` and `media_bytes`:
  - `startStudySession(deckId)` and `endStudySession(sessionId)`, which answers `{ summary }` (the
    ADR 0009 `SessionSummary`, or null when that session was not open);
  - `nextCard(deckId)`, a union tagged `kind`: `card` (card and deck IDs, `state`, four `previews`
    for Again, Hard, Good and Easy as `{ unit: "minutes" | "days", amount }`, and the counts),
    `waiting` (`waitSeconds`, rounded up) or `done`. Counts include the card shown;
  - `renderCard(cardId)`: `front`, `back` and `media`, the core's `RenderedCard`;
  - `getMedia(name)`: the bytes as the reply attachment, plus `contentType` worked out from the
    extension (the core stores no type);
  - `answerCard(cardId, rating, durationMs)`, which answers the event ID;
  - `undoAnswer`, which answers `{ undone }` (the card ID, or null when there was nothing to undo).
- **`Collection::now_ms()`** in `fc-core`, so `nextCard` can turn the queue's "due at" into seconds.
  It has no CLI command (the reason is in `tests/coverage.rs`).
- **The card frame** (ADR 0005 amendment, text in that ADR): `frame.html` takes `theme` and
  `autoplay`, sets `color-scheme` and a `night` class on the card's `<html>`, plays the card's
  `<audio>` elements in order on `autoplay` and on a later `{ type: "play" }` from the parent, and
  tells the parent how many it found (`audio`) and when the browser refused to start sound
  (`autoplay-blocked`). `CardFrame` takes `theme`, `autoplay`, `onAudio`, `onAutoplayBlocked` and a
  `ref` with `play()`. A new theme makes a new frame.

### Verified

- `cargo xtask check` passes (62 tests in `fc-api`, 7 of them new; 138 Vitest tests, 4 of them new).
- Rust, through `dispatch`: a session from start to end (the first card is new with four previews,
  render, answer, undo gives the same card back with the same counts, a second undo says there is
  nothing, the summary leaves the undone answer out, ending twice answers null); an empty deck is
  done; a learning card due in an hour is `waiting` with 3,600 seconds; media comes back with the
  right type and a missing file is not found; bad IDs, an unknown deck and no collection are readable
  errors.
- **Web** (`bun run web:dev`, headless Brave on Linux, driven over the DevTools protocol by a scratch
  script in `target/`, not committed):
  - the frame sets `color-scheme: dark` and the `night` class for `dark`, and `light` without it for
    `light` and for a made-up theme;
  - it reports one `<audio>` element;
  - **autoplay and replay are refused on Chromium without `allow="autoplay"`**, with or without a
    real click on the page: in the real screen, `play()` in the frame failed with `NotAllowedError`
    and `navigator.userActivation.hasBeenActive` was false inside it. With `allow="autoplay"` on the
    iframe the audio played (`played` length 1, no refusal). An earlier version of this note said no
    attribute was needed. That came from a test harness whose iframe was not built like the app's, and
    it was wrong. Anthony approved the attribute in chat (ADR 0005 amendment);
  - the 54-attempt malicious card: **38 blocked, 0 SUCCEEDED, 16 other, 19 CSP violations**, no alarm,
    the core still answered and the page was not reloaded. Same as 0.6 and 1.10b. Both navigating cards
    were stopped with the message, and the Media card works.

### Not verified

- The sandbox re-run, the theme and the audio on **desktop (WebKitGTK) and the phone**. The plan put
  them here. They move to 2.3b, where the sample cards and the real screen let me check them on both,
  with the frame unchanged.
- Autoplay on WebKitGTK and the Android WebView (the web result is Chromium only), Firefox, Safari,
  Windows.
- Rendering a card with a template mistake through the API (see the deviations).
- `getMedia` with a real image or audio file from a collection in a UI (2.3b).

### Deviations from the plan

- **Desktop and phone runs of the sandbox card moved to 2.3b** (see Not verified). The web run is here.
- **`Collection::now_ms()` was added to `fc-core`** and a line to the CLI coverage table. The plan
  touched only `fc-api` and the UI.
- **`endStudySession` and `undoAnswer` answer a struct** (`{ summary }`, `{ undone }`), not
  `SessionSummary | null` and `card | null`. The bindings generator does not import the type inside an
  `Option` output, and every other method answers a struct.
- **`EventRating` also derives `Deserialize`**, so `answerCard` takes the rating the events already use.
- **The iframe gets `allow="autoplay"`**, which the plan said might be needed and asked Anthony about
  (approved). ADR 0005's "no `allow` attribute" is amended.
- **The frame sends two more messages than the plan listed** (`audio`, `autoplay-blocked`), so the
  screen can show Replay only when there is sound and say so when autoplay is refused. Both are in the
  ADR 0005 amendment.
- **"A card that is already open is not re-sent" was not built.** The frame already ignores a second
  card message (`received`), so there was nothing to add.
- **No test of a broken template through the API.** A template with a mistake cannot be saved (1.4), so
  the API cannot be given one. `renderCard`'s error mapping for it is not exercised; a missing card
  (not found) is.
- **The waiting test uses an Again answer** with a 60-minute step. A Good answer on a one-step list
  graduates the card.
- **Size:** about 830 lines of code and tests against the plan's 700, and 155 generated lines.
- **`target/scratch-2.3a/`** holds the check script and a Brave profile. It is ignored by git.

## Build notes (step 2.3b)

Built on `step/2.3b-review-screen`, stacked on 2.3a (PR #36). About 1,360 lines added and 640 deleted
(the old spike), 440 of the added lines tests, against the plan's 1,300. No migration, no new
dependency. No ADR of its own: ADR 0010 and ADR 0009 hand the screen's open points to this step, and
the one frame change (theme, autoplay, `allow="autoplay"`) is an ADR 0005 amendment made in 2.3a.

### What was built

- **`StudyScreen`** (`packages/ui/src/screens/StudyScreen.tsx`, replacing the 2.1 placeholder) and
  **`useStudy`** (`study/useStudy.ts`). The session starts when the screen opens and ends when it closes
  or the cards run out. Each card shows its question and Show answer, then Again, Hard, Good and Easy,
  each with when the card comes back (`formatInterval`: `<1m`, `10m`, `5h`, `3d`, `2.5mo`, `1.2y`).
  - The counts (New, Learn, Review) are at the top and the one for the card on screen is underlined.
  - The card is shown only through `CardFrame`, with the app's effective theme and `autoplay`. A Replay
    sound button appears when the frame reports audio, and a line says so if the browser refused to
    start it.
  - The answer buttons are in the shell's `BottomAction` slot, at least 3.5 rem tall, in a row of four.
  - **Undo** (button, `z`, Ctrl+Z) takes back the last answer given on this screen and shows that card
    again. After the last card it is offered on the end screen too, and starts a new session.
  - **Keys:** Space or Enter shows the answer, then 1 to 4 rate, and Space or Enter again means Good.
    `r` replays. Keys are ignored with Alt or Meta, in a text field, and Enter or Space on a focused
    button leaves it to the button. Key hints show only where there is a mouse (`hover: hover` and
    `pointer: fine`).
  - **End screen:** "Done for now" with the answered count, the Again count and the time studied
    (`formatSpan`); or "Nothing to show right now" with "The next card is due in 12 minutes" and Check
    again when a learning card is due later today. Back to decks is the bottom action.
  - A double tap answers once (one action at a time, in a ref), and a failure shows the core's message
    with Try again and Back to decks.
- **Leaving a half-answered card asks nothing.** Nothing is written until a card is rated, so the card
  stays due (the question ADR 0010 left for this step).
- **`debugAddSampleCards`** (`crates/fc-api/src/sample_cards.rs`, debug builds only): decks `Sample` and
  `Sample::Sound` with 13 notes (15 cards): words, a reversed pair, a cloze, and cards with the sample
  sound and image. A button on the Developer screen calls it. Calling it again adds the same notes again.
- **The 0.6 card sandbox spike is deleted** (`CardSandboxSpike`, its cards, `spikeCardMedia`), as planned.
  The sample sound and image moved to `crates/fc-api/src/sample_media/`.
- `PageHeading` takes a `className`, so the screen's heading is there for screen readers and hidden.

### Verified

- `cargo xtask check` passes (165 Vitest tests: 27 new in 2.3b, 23 for the screen and 4 for the formatting; 63 tests in `fc-api`
  with the new sample-cards test).
- **Web** (headless Brave over the DevTools protocol, screenshots read): phone width and 1100 px wide,
  light and dark. Question, answer, rating, the next card, the Sound deck with audio (the autoplay
  refusal message is gone with `allow="autoplay"`), counts, undo button, and the answer buttons at the
  page width.
- **Desktop** (Linux, `bun run dev`, WebKitGTK, dark, `XDG_DATA_HOME` pointed at a scratch folder, window
  floated at 1100 x 700 with `i3-msg`): sample cards added from the Developer screen, deck list with the
  counts, Sound deck, Space showed the answer, `3` rated Good (New 3 to New 2, Learn 1), `z` undid it and
  the same card came back with New 3. Key hints, Replay sound and Undo shown. The sandbox card ran on
  desktop before the spike was deleted: **38 blocked, 0 SUCCEEDED, 16 other, 38 CSP violations**, as in
  0.6 and 1.10b.
- **Phone** (SM-S928B, debug APK built and installed, system dark): it starts, and the Decks screen shows
  the empty state under the status bar with the bottom bar above the navigation bar. I did not tap
  anything on the phone.

### Not verified

- **Everything about studying on the phone.** The phone's collection has no cards and I may only install
  and take screenshots. **Anthony checks:** Settings, Developer tools, Add sample cards, then Decks, study
  Sample and Sample::Sound, in portrait and landscape: the heading and buttons clear the system bars, the
  buttons are easy to hit, the sound starts by itself on the Sound deck (Android's WebView may need
  something other than `allow="autoplay"`), Replay works, Undo works, and Android back leaves study.
- **The sandbox card on the phone with `allow="autoplay"` on the iframe.** It could not be run without
  tapping, and the spike is now deleted (its own commit, `2.3b: delete the 0.6 card sandbox spike`, if
  you want it back for a phone run). On web and desktop it is unchanged.
- Audio actually heard (the checks saw the element play, not the speaker), autoplay on WebKitGTK and on
  the Android WebView, the end screen and the waiting screen outside the tests, the largest system font,
  landscape on the phone, gesture navigation, Windows, Firefox, Safari.
- **Clicking inside the card moves keyboard focus into it,** and the shortcuts stop until focus leaves
  (ADR 0005). The buttons always work. Not checked by hand.
- Studying a real deck for a few days (the step's review focus): it needs cards, so after 2.4.

### Deviations from the plan

- **The sandbox card was not re-run on the phone**, and the spike was deleted (see Not verified). The plan
  said all three platforms.
- **Undo is also on the end screen** (a new session starts). The plan said Undo takes back the last answer.
- **Undo is offered only for answers given on this screen,** not for older ones the core could also undo.
- **Check again** on the waiting screen is a button, not a timer.
- **The answer row is a `fieldset` with a hidden legend,** not a `div` with `role="group"` (the lint rule).
- **Extras not in the plan's file list:** `PageHeading` `className`, type exports in `core-client`, the
  `r` replay key, `Ctrl+Z`, the underline on the count that applies.
- **`debugAddSampleCards` takes its media from the old spike's files,** which moved to `sample_media/`.
- **The bottom buttons are capped at the page width** (46 rem), found by looking at the wide layout.
- **Size:** about 1,360 lines added (440 tests) against the plan's 1,300, plus 640 deleted.
- **Desktop check:** I floated and resized the app window with `i3-msg` and closed it afterwards. My
  first `pkill -f` attempts killed my own shell and the dev server twice. Nothing was lost.
- **STATUS:** the 2.3a/2.3b "in progress" text is replaced by the final status.

## Build notes (step 2.5a)

Built on `step/2.5a-decks`, the first of two PRs for step 2.5 (2.5b is the option presets). No migration,
no new dependency. No ADR of its own: the data rules for decks are ADR 0006 (built in 1.5) and the
screens follow this ADR.

### What was built

- **`fc-api`** (`crates/fc-api/src/decks.rs`): `createDeck(name, parentId?)` answers the new ID,
  `renameDeck`, `moveDeck(deckId, parentId?)`, `deleteDeck` answers `{ decks, cards }`, the decks inside it
  and the cards that went (counted as live decks and cards before and after, so it is exact whatever the
  Default-deck rule did), `restoreDeck`. All call the `Collection` operations that 1.5 built. A name that
  is empty, has `::`, is taken, a move into itself and deleting the Default deck are `invalidInput` with
  the core's plain sentence; a deck that is gone is `notFound`; the rest is logged and answers the generic
  message. `DeckSummary` gained `isDefault`, so the screen knows which deck has no Delete.
- **Decks screen:** an Add deck button and a Manage toggle (`aria-pressed`) next to the heading. In Manage
  mode a row shows Add inside, Rename, Move and Delete (not on the Default deck) in place of the study link
  and counts. The buttons carry the deck name for screen readers.
- **`DeckDialog`** and a small shared **`Dialog`** (`components/Dialog.tsx`) on a native `dialog`: name,
  parent (`select`, "Top level" first), Move without the deck itself and the decks inside it, and a Delete
  confirmation that says how many decks go with it. A refused change shows the core's sentence in the
  window, which stays open with what was typed. After a delete a notice offers Undo (`restoreDeck`), or
  says why it could not (for example the name was taken in the meantime).
- **Back closes a window.** A `Dialog` takes one history entry while it is open, so the Android back
  button closes it and stays on the Decks screen. Without it, Back left the app (found on the phone). The
  router keeps its route object when a pop does not change the path, so a window closing does not move
  focus to the heading.

### Verified

- `cargo xtask check` passes (303 Vitest tests, 9 `fc-api` deck tests).
- **Web** (headless Brave over CDP, real key events, screenshots read) at 390 px and 1100 px: add a deck,
  add one inside it, a refused duplicate name shows the sentence, Move offers only decks outside the moved
  one, Delete says "Polish and the deck inside it", Undo brings both back, no sideways scroll.
- **Phone** (debug APK, my taps, with Anthony's permission to use the phone while it is free): the new
  buttons, Manage mode, the Move window (fits, the select is native), Android Back closes the window and
  stays in the app.

### Not verified

- Creating, renaming and deleting on the phone (I only opened the Move window and cancelled, so I changed
  none of Anthony's decks).
- The real desktop app (`bun run dev`): the screen is the same code as the web, but I did not run it.
- Delete of a very large deck: timing at 50,000 cards.

### Deviations from the plan

- **Rebased onto master** after PR #41 merged, because the branch was cut from a stale local `master`.
- **The empty state shows only when there are no cards and no deck besides Default.** The plan did not say;
  otherwise a collection with only new, empty decks hid them.
- **Extras not in the plan's file list:** the shared `Dialog` component, `isDefault` on `DeckSummary`, the
  history entry for windows, the router's same-path guard, "Add inside" on each row, a Dismiss button on the
  undo notice.
- **The New/Learn/Review header and the "done for now" line are hidden in Manage mode** (found by looking).
- **`bun run android:build` needed `NDK_HOME` and `JAVA_HOME`,** which this shell does not set. I set them
  for the commands only, and changed no shell config.
