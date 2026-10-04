# Phase 2: Study loop on desktop and Android

## Goal

Anthony can study, add cards and manage decks on desktop and phone. Every screen is designed for both from the start.

## Exit criteria

- Daily study of Polish works on the phone and on desktop (separately, no sync yet).
- Installable builds: Android APK (sideload) and Linux and Windows installers.

---

## 2.1 App shell and design direction

**Goal:** navigation, theming and layout rules shared by every later screen.

**Scope:** navigation structure for phone and desktop, light and dark themes, typography and spacing, responsive rules, basic accessibility (contrast, font scaling, screen reader labels). An ADR or short design doc for the visual direction.

**Acceptance criteria:**
- Navigation feels native on a phone (one-handed reachable) and efficient on desktop.
- Theme follows the system setting with a manual override.

**Review focus:** look and feel. This sets the tone for everything. Ask for alternatives if it feels generic.

---

## 2.2 Home and deck list

**Acceptance criteria:**
- Deck tree with new, learning and review counts.
- One tap or click to start studying a deck.
- Empty state that helps a new user get started without jargon.

---

## 2.3 Review screen

**Acceptance criteria:**
- Card renders in the sandbox, show answer, four rating buttons with next interval shown.
- Audio autoplay and replay.
- Undo.
- Keyboard shortcuts on desktop; large tap targets on mobile.
- Session end screen.

**Review focus:** study a real deck for a few days on the phone before approving.

---

## 2.4 Add note

**Acceptance criteria:**
- Pick deck and note type, fill fields, tags.
- Attach images and audio from files on desktop, and camera, gallery and files on Android.
- Comfortable to use one-handed on a phone; the keyboard never hides the active field.
- Duplicate warning.
- Basic formatting (bold, italic, lists) and cloze insertion.

---

## 2.5 Deck management

**Acceptance criteria:**
- Create, rename, nest and delete decks on both platforms.
- Edit option presets with plain-language explanations of each setting.

---

## 2.6 Settings and local backups

**Acceptance criteria:**
- Settings screen for app-level preferences.
- Automatic local backups with restore from the UI.

---

## 2.7 Dogfood builds

**Acceptance criteria:**
- Documented process to produce an Android APK (sideloaded, no Play Store) and unsigned Linux and Windows installers, all at zero cost.
- App version visible in settings.
