# Docs

Planning documents for the app (name TBD, referred to as "the app" and `<APP_NAME>` until chosen).

| File | What it is |
| --- | --- |
| `PRODUCT.md` | What we are building, for whom, and the requirements. The source of truth for "what". |
| `ROADMAP.md` | Phase order, milestones, exit criteria, rough durations. |
| `PROCESS.md` | How work moves between Anthony, the planning chat and Claude Code. Step format, ADRs, definition of done. |
| `phases/` | One file per phase, broken into PR-sized steps. Detailed only up to the current horizon. |
| `adr/` | Architecture Decision Records. Written by Claude Code, reviewed and accepted by Anthony. |
| `plans/` | Implementation plans from design sessions, one per step, for the build session. |

Division of responsibility:

- These docs define **what** and **why**: features, requirements, constraints, acceptance criteria, order of work.
- Claude Code defines **how**: architecture, data model, crate and library choices, code structure. It proposes these as ADRs before building.
- Anthony reviews and accepts every ADR and every PR.
