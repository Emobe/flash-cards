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
