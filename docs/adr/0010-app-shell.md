# 0010: App shell and visual direction

Status: Proposed
Date: 2026-10-07

## Context

Step 2.1 asks for navigation, theming and layout rules shared by every later screen, with these
acceptance criteria:

- Navigation feels native on a phone (one-handed reachable) and efficient on desktop.
- The theme follows the system setting with a manual override.
- Content is never hidden behind the status bar, navigation bar, display cutout or on-screen
  keyboard. Edge-to-edge drawing and safe areas are handled once in the app shell, not per screen.
- Works with gesture and three-button navigation, in portrait and landscape.
- Checked on the real phone.

What exists:

- One React UI in `packages/ui`, mounted by `apps/native` (Tauri, desktop and Android) and
  `apps/web`. It is a single placeholder screen with the spikes (`DivideForm`, `SchedulingSpike`,
  `CardSandboxSpike`) on it, and each app has its own throwaway `styles.css`.
- `MainActivity.kt` already calls `enableEdgeToEdge()`. At Android target API 37 the app draws
  edge to edge, so content runs under the status bar (found in 0.2).
- No router, no component library, no icon set, no web fonts. Dependencies need a reason (CLAUDE.md).
- The screens that come next, from `docs/phases/`: decks (2.2), review (2.3), add note (2.4), deck
  management (2.5), settings and backups (2.6), then browse, edit, inbox and stats (Phase 3), sync
  status (4.6).
- PRODUCT.md: "easy to use, a new user can study within minutes", "real parity between mobile and
  desktop", "mobile layouts are designed for one-handed phone use, not shrunk desktop screens",
  "desktop gets keyboard shortcuts, mobile gets large tap targets".

## Decision 1: visual direction (Anthony picks)

Three directions, each with a light and a dark palette. Contrast ratios are computed (WCAG), not
estimated. All text pairs are at least 4.5:1 and the interface chrome (borders, focus rings) at
least 3:1.

### A. Paper (recommended)

A study notebook. Warm paper and ink, one terracotta accent, a serif for card text and headings,
system sans for the controls. It fits language learning, where the card text is the product, and it
does not look like a default Material or Bootstrap app.

| Role | Light | Dark |
|---|---|---|
| Background | `#FAF6EE` | `#1C1A17` |
| Text | `#2A2622` (13.9:1) | `#EDE6D8` (14.0:1) |
| Muted text | `#675F54` (5.8:1) | `#A89F8F` (6.6:1) |
| Accent | `#A93F24` (5.7:1) | `#E8906F` (7.2:1) |
| Text on accent | `#FFFFFF` (6.1:1) | `#1C1A17` (7.2:1) |

Type: headings and card text `"Iowan Old Style", "Palatino Linotype", Palatino, Charter, Georgia,
serif`; controls `system-ui, sans-serif`. No download, so the look differs a little per platform
(the serif is whichever of those the system has). Corners 10 px, flat surfaces with a 1 px border
instead of shadows.

### B. Calm blue

Cool neutrals, one blue accent, system fonts, soft rounded surfaces. Clean and familiar. It is the
safest option and the one most likely to read as generic.

| Role | Light | Dark |
|---|---|---|
| Background | `#F5F7FA` | `#111827` |
| Text | `#1B2430` (14.6:1) | `#E6EBF2` (14.8:1) |
| Muted text | `#556070` (5.9:1) | `#9AA6B8` (7.2:1) |
| Accent | `#2057D6` (5.8:1) | `#7AA7FF` (7.4:1) |

### C. High contrast

Black and white with a bold accent (blue in light, yellow in dark) and larger type. Fastest to scan
and the strongest on accessibility, and the most austere.

| Role | Light | Dark |
|---|---|---|
| Background | `#FFFFFF` | `#000000` |
| Text | `#000000` (21:1) | `#FFFFFF` (21:1) |
| Muted text | `#404040` (10.4:1) | `#C4C4C4` (12.0:1) |
| Accent | `#0039B3` (9.4:1) | `#FFD43B` (14.7:1) |

The rest of this ADR holds for any of the three: the direction only changes the token values
(`tokens.css`), nothing structural. Changing direction later is a one-file change.

## Decision 2: navigation

Top-level destinations: **Decks**, **Add**, **Browse**, **Settings**. Study is not a destination: it
is a full-screen route opened from a deck, with no navigation chrome and a back arrow. (Stats,
sync status and the inbox arrive in later steps; the shell has room for them without changing the
pattern, see Consequences.)

Two layouts of one component, chosen by the width of the window (a CSS container query on the shell,
not a platform check). A narrow desktop window gets the phone layout and a wide tablet gets the
desktop one.

```
 Narrow (under 720 px)                 Wide (720 px and over)
 +---------------------------+         +--------+--------------------------+
 | Decks                     |         | Flash  |  Decks                   |
 |                           |         | cards  |                          |
 |   content scrolls         |         |        |   content, max ~46rem    |
 |                           |         | Decks  |   centred                |
 |                           |         | Add    |                          |
 |                           |         | Browse |                          |
 |  [Decks][Add][Browse][Set]|         | ...    |                          |
 +---------------------------+         | Set.   |                          |
   bottom bar in thumb zone            +--------+--------------------------+
```

- Narrow: a bottom bar with four equal targets (icon above a label, at least 56 px tall plus the
  bottom inset). The page title sits at the top and scrolls with the content. Primary actions of a
  screen (for example "Add deck") go in a bottom-right floating action on that screen, again in
  thumb reach. Nothing important lives in the top corners.
- Wide: a left sidebar of icon and label rows, the title in the content area, and keyboard
  shortcuts: `g` then `d`, `a`, `b`, `s` to go to a destination, `?` for the list. Shortcuts are
  ignored while a text field has focus.
- Routing is a small in-app router in `packages/ui` (a path string in state, kept in sync with
  `history` so the Android back button and the browser back button work). It is not a library:
  there are about six routes and no nested data loading. See Options considered.
- Android back: pops the route. At a top-level destination it leaves the app (the system default).
  During study it asks to leave the session only if a card is half answered (2.3 decides the
  details).

## Decision 3: theme

- Colour, spacing, type scale, radius and focus styles are CSS custom properties in one file,
  `packages/ui/src/styles/tokens.css`. Components use only tokens, never literal colours.
- Setting: **System** (default), **Light**, **Dark**. System follows `prefers-color-scheme`
  live. The override sets `data-theme` on `<html>`.
- Where it is stored: `localStorage` on the device (a per-device appearance preference, like a
  window size, not collection data, so it is not in the core and does not sync). Reads and writes
  are in try/catch, and the app renders correctly if storage is unavailable.
- No flash: a three-line inline script in each app's `index.html` sets `data-theme` before first
  paint. (Tauri's CSP has `style-src 'unsafe-inline'` but no `script-src 'unsafe-inline'`, so the
  script is a small file in `public/` rather than inline. Verified in the build.)
- `color-scheme` is set so form controls and scrollbars match.
- Android system bars: transparent (edge to edge), with icon colour matching the app theme. The
  WebView follows the system theme, but a manual override that differs from the system needs the
  activity to set the bar icon colour (`WindowInsetsControllerCompat`), reached from the page
  through a small Tauri command. Whether that is needed is decided by the phone check; it is the
  one place the shell may need native code, and any change goes in the build notes.
- Card content has its own theme handling inside the sandbox frame (ADR 0005) and is not changed.

## Decision 4: safe areas and the keyboard, once

One component, `AppShell`, owns every inset. It reads the insets, writes
`--inset-top`, `--inset-right`, `--inset-bottom`, `--inset-left` and `--inset-keyboard` on its root,
and pads its own chrome (bottom bar, sidebar, header). Screens render inside a scroll region the
shell has already inset, so a screen never mentions `env()`, `safe-area` or `visualViewport`.

- Both app `index.html` files get `viewport-fit=cover` (draw under the cutout) and
  `interactive-widget=resizes-content` (the layout viewport shrinks when the keyboard opens, so a
  focused field is not covered and the bottom bar rides above the keyboard).
- Source of the numbers, in order: CSS `env(safe-area-inset-*)`, then `window.visualViewport` for
  the keyboard. Whether Tauri's Android WebView reports real values for `env()` at target API 37 is
  not documented in the sources we use and has to be measured on the phone first, before the shell
  is written. If it reports zero, `MainActivity.kt` reads the real insets
  (`ViewCompat.setOnApplyWindowInsetsListener`, which also gives the IME inset) and sets the same
  five variables on the page through the WebView. The shell does not change: only where the numbers
  come from.
- The bottom bar hides while the keyboard is open (it would eat a third of a landscape screen), and
  returns when it closes.
- Landscape: side insets (the cutout and the gesture area) are applied to the left and right of
  the sidebar or content, and the bottom bar becomes a narrower rail if it would be taller than 20%
  of the screen height.
- Gesture and three-button navigation differ only in the size of `--inset-bottom` (a thin gesture
  line against a 48 dp button bar). Nothing in the shell branches on the mode.

## Decision 5: accessibility baseline

- Contrast as above, enforced by a test that computes the ratio of every text-and-background token
  pair and fails under 4.5:1 (3:1 for borders and focus rings) in both themes. The test reads
  `tokens.css`, so a later palette change cannot silently break it.
- All sizes in `rem`, so the system font size (Android) and the browser setting scale the UI. The
  layout is tested at 200%.
- Every control reachable by keyboard with a visible focus ring (2 px accent, 3:1 against the
  background). Tap targets at least 48 x 48 px on narrow layouts, 32 px on wide.
- Landmarks and names: `nav` labelled "Main", `main`, `header`; every icon-only button has an
  `aria-label`; the current destination has `aria-current="page"`; a route change moves focus to the
  page heading and sets `document.title`.
- `prefers-reduced-motion`: transitions are off.
- Errors use `role="alert"`, status messages `role="status"` (as the placeholder already does).

## Decision 6: empty and error states

- `EmptyState` (icon, a plain sentence, one button) is the component every empty list uses. No
  jargon: "You have no cards yet" with "Add your first card", not "Collection has 0 notes".
- A real error screen replaces the placeholder `CollectionStatus` for the two cases step 1.1a left
  for 2.1: **a newer collection** (the file was made by a newer version of the app: "Update the app
  to open this collection. Your cards are untouched.") and **the file is in use** or cannot be
  opened (what failed, and what to try). The collection is never modified in either case. Which
  `ApiError` kinds map to which message is in `packages/ui` and tested with the fake transport.

## Decision 7: what happens to the spikes

`DivideForm`, `SchedulingSpike`, `CardSandboxSpike` and (web) `SpikePanel` and `BackupPanel` are
still needed as test rigs until their replacements land (2.3 for the card frame, 2.6 for backups).
They move to a **Developer** route reachable only from Settings (a "Developer tools" row shown in
debug builds and on the web dev server, `import.meta.env.DEV`), so they stop shaping the real
layout. They are still deleted by the steps STATUS already names.

## Options considered

### Routing

| Option | For | Against |
|---|---|---|
| **Own small router (chosen)** | No dependency; about six routes; full control of the Android back and focus behaviour | We maintain about 80 lines; no nested data loading (not needed) |
| `react-router` | Standard, nested routes, loaders | A dependency for six flat routes; its data APIs would pull the app towards a shape the core (not the router) already owns |
| Tab state only | Simplest | No back button, no deep link to a deck, breaks Android back |

### Navigation pattern

| Option | For | Against |
|---|---|---|
| **Bottom bar narrow, sidebar wide (chosen)** | Matches platform habit; the bar is in thumb reach; the sidebar scales to more destinations | Two layouts to test |
| Same bar on desktop | One layout | Wastes a wide screen, bad pointer ergonomics |
| Hamburger drawer on phone | Room for many destinations | Hidden navigation, top-left, out of one-handed reach |

### Components and icons

| Option | For | Against |
|---|---|---|
| **Own components, inline SVG icons (chosen)** | No dependency or licence; every pixel follows the chosen direction; small | We draw about eight icons (simple strokes) |
| A component library (Radix, MUI, ...) | Accessible primitives for free | Dependencies; the direction is hard to pull off on top of a library's look; larger bundle in the wasm-heavy web client |
| An icon package (lucide, ...) | Many icons | A dependency for eight icons |

## Consequences

- Every later screen is written against `AppShell`, `EmptyState` and the tokens, and never against
  insets. A screen that needs something the shell does not offer (a sticky bottom action above the
  keyboard, a dialog) asks for it as a shell feature, not a local workaround.
- Adding a destination later (Stats, Inbox, Sync status) is one row in a list. If there are more
  than about six, the narrow bar moves the extras into a "More" screen; this ADR does not decide
  that now.
- The visual direction is replaceable in one file, but the components are drawn for it (the serif
  card text in A, the stroke weight of the icons), so a late switch costs a day, not a rewrite.
- The theme preference is per device and does not sync. If Anthony wants it to follow him between
  devices, it becomes a synced setting in Phase 4.
- `packages/ui` gains CSS. The two apps stop owning styles; they only mount the shell.
- Visual quality and the inset behaviour are checked on the real phone and the desktop app, not in
  `happy-dom` (it has no layout).

## Revisit if

- The WebView's `env()` or keyboard behaviour differs between Android versions or devices in a way
  one native fallback cannot cover.
- More than six destinations are needed, or tablets and foldables become a target (a three-pane
  layout).
- Anthony wants the theme or direction to be a user setting beyond light, dark and system.
- Add-ons (Phase 10) need to add screens or navigation entries.

## Decisions for review

1. Visual direction: **A (recommended), B or C**.
2. The four destinations and the order Decks, Add, Browse, Settings. Is "Add" a tab, or a floating
   button on Decks?
3. The theme preference is per device (not synced).
4. A Developer route in debug builds and the web dev server holds the spikes.
5. No new dependencies in 2.1.
