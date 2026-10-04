# Phase 4: Accounts and sync

## Goal

Seamless use across desktop and phone, with Anthony's hosted server and a self-hostable option.

Built according to the data model and sync ADR from step 0.7, revised if Phase 1 taught us anything.

## Exit criteria

- Milestone M2: Anthony uses desktop and phone interchangeably for a week without a single manual sync action or conflict prompt.
- Self-hosting documented and tested from scratch by following the docs.

---

## Zero cost note

Hosting, storage, TLS, monitoring and email must all have a zero-cost path (see `PRODUCT.md`). Free tiers change often, so the specific options get researched when this phase is detailed, not now. Running the server on Anthony's own machine is an acceptable starting point.

---

## 4.1 Sync server foundation and self-host packaging

**Acceptance criteria:**
- Server runs locally for development with one command.
- Self-host package (for example a container image with compose file) runs on a fresh Linux VPS by following the docs.
- Health check endpoint and structured logs.

---

## 4.2 Accounts

**Notes:** Claude Code writes an ADR comparing auth approaches (for example email and password versus email magic links) for a small self-funded service that is also self-hostable. Anthony picks. Any email sending (verification, reset, magic links) must work at zero cost, or the design must work without email for now.

**Acceptance criteria:**
- Sign up, verify email, log in, log out, password reset or equivalent.
- Sessions on multiple devices, with the ability to sign out a device.
- App works fully without an account; signing in later attaches the existing local collection.

---

## 4.3 Sync of collection data

**Acceptance criteria:**
- Incremental sync between devices per the ADR.
- First sync of a device with an existing collection merges rather than overwrites.
- Automatic background sync on open, on close, after changes and periodically.

---

## 4.4 Media sync

**Acceptance criteria:**
- Media syncs incrementally; unchanged files never re-upload.
- Large media libraries sync without blocking study.
- Storage limits per account are configurable on the server.

---

## 4.5 Sync edge case test suite

**Acceptance criteria:**
- Automated tests for: a device offline for weeks; the same card reviewed on two devices; edit on one device and delete on another; device clocks wrong by hours or days; two app versions syncing; schema migration with devices on old versions; interrupted sync; corrupted local data.
- Each case produces the behaviour documented in the ADR.

**Review focus:** read the test names and check every case you can imagine is there.

---

## 4.6 Sync UX

**Acceptance criteria:**
- Status indicator on every platform (synced, syncing, offline, error).
- Errors say exactly what happened and what to do.
- Manual "sync now" exists but is never required.

---

## 4.7 Deploy the hosted instance

**Acceptance criteria:**
- Running on a zero-cost host (own machine or a free tier) with TLS, automated backups of server data and media, restore tested.
- Basic monitoring and alerting.

---

## 4.8 Account deletion and data export

**Acceptance criteria:**
- User can export all their data and delete their account from the app.
- Deletion removes server data and media.
