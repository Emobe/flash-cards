# Roadmap

Each phase ends with something usable. Phases are detailed in `phases/` only up to the current horizon, because decisions in earlier phases shape later ones.

Durations are rough guesses for full-time work with Claude Code, to be revised after Phase 0.

## Phases

| # | Phase | Ends with | Rough duration |
| --- | --- | --- | --- |
| 0 | Foundations and spikes | Repo, local checks, app running on Linux and Android, Windows checked manually. Key risks tested. Architecture ADRs accepted. | 2-3 weeks |
| 1 | Core library | Tested core driven from a dev CLI: notes, cards, templates, decks, scheduling, search, media, change tracking. | 4-5 weeks |
| 2 | Study loop | Study, add cards and manage decks on desktop and Android. | 3 weeks |
| 3 | Browse, edit, capture | Full browse, filter and edit on both. Note type and template editor. Capture inbox and Android share target. | 3-4 weeks |
| 4 | Accounts and sync | Accounts, sync server, background sync, media sync, your hosted instance. | 4-5 weeks |
| 5 | Anki import | Import `.apkg` decks, ideally with review history. | 2 weeks |
| 6 | Web client | Full offline-capable web client. | 2-3 weeks |
| 7 | Hardening | Stats, backups, onboarding, performance, crash reporting. Personal v1. | 2-3 weeks |
| 8 | Fast capture expansion | Browser extension, desktop quick-add, language-learning capture helpers. | TBD |
| 9 | Shared decks | Versioned, collaborative decks and a deck library. | TBD |
| 10 | Add-ons | Sandboxed cross-platform add-on system. | TBD |
| 11 | Review debt | Catch-up modes, load smoothing, backlog tools. | TBD |
| 12 | iOS and macOS | When a Mac is available. Can move earlier. | TBD |
| 13 | Marketplace | Decks and add-ons. | TBD |

## Milestones

- **M1, local daily use (end of Phase 3):** Anthony studies Polish on the phone daily with no sync.
- **M2, synced daily driver (end of Phase 4):** Seamless use across desktop and phone.
- **M3, personal v1 (end of Phase 7):** Stable enough to rely on long term. Anki decks imported. Web works.

## Constraints

- Bun for all Node-related tooling throughout (see `PRODUCT.md`).
- Zero running cost. No paid services besides Claude until further notice (see `PRODUCT.md`).

## Ordering notes

- Mobile is built alongside desktop from Phase 2, not after.
- Anki import is before web because free Anki decks are a main content source for Polish study.
- iOS is not blocked by architecture, only by hardware.
