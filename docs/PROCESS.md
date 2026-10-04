# Process

## Roles

- **Anthony:** product owner and reviewer. Approves ADRs and PRs. Runs the app on real devices.
- **Planning chat:** writes and maintains these docs, breaks phases into steps, writes step briefs. Does not make code-level decisions.
- **Claude Code:** proposes architecture (ADRs), plans each step, implements, tests, opens PRs.

## Flow for each step

1. Anthony gives Claude Code the step brief from the phase file.
2. Claude Code reads the relevant docs and ADRs, then replies with a short implementation plan before writing code. If the step involves a significant design choice, it writes an ADR first.
3. Anthony reviews the plan or ADR. Adjusts or approves.
4. Claude Code implements on a branch, with tests.
5. Claude Code opens a PR with a summary, how to test it manually, and anything it was unsure about.
6. Anthony reviews, tests on devices where relevant, and merges.
7. If the step changed a requirement or revealed a new one, the docs are updated in the same PR or the next.

## Step brief format

Each step in a phase file has:

- **Goal:** what exists after this step that did not before.
- **Scope:** what is in and what is explicitly out.
- **Acceptance criteria:** how we know it is done.
- **Review focus:** what Anthony should look at closely.
- **Notes:** constraints, links to requirements, open questions for Claude Code.

## PR size

- Phase 0 boilerplate can be one large PR.
- Everything else: one coherent, reviewable change per PR. Roughly what a professional team would review in one sitting. If a step is too big, Claude Code proposes a split before starting.

## ADRs

- Stored in `docs/adr/` as `NNNN-short-title.md`.
- Each covers: context, options considered, decision, consequences, and what would make us revisit it.
- Status: Proposed, Accepted, Superseded.
- Nothing that depends on a Proposed ADR gets built until it is Accepted.

## Definition of done

- Acceptance criteria met.
- Tests added for new behaviour. Core logic has thorough tests.
- Lint, format and type checks pass locally via the single check command.
- Works on Linux, Windows and Android where the step touches UI.
- Docs and ADRs updated if anything changed.

## CLAUDE.md starter rules

Suggested project rules. Keep CLAUDE.md short and point to docs rather than duplicating them.

- Read `docs/PRODUCT.md` and accepted ADRs before planning a step.
- Never read, copy or adapt code from Anki or AnkiDroid. Feature ideas come from `docs/`, not Anki's source.
- Propose a plan before writing code. Write an ADR for any significant design decision.
- Keep PRs to one coherent change. Propose a split if a step is too large.
- Do not add dependencies without stating why in the PR.
- Card content is untrusted. Never give rendered card content access to app commands or data.
- Every UI feature must work on mobile and desktop.
- Nothing may cost money besides Claude. Flag any tool, service or CI setup that could incur charges before using it.
- Use Bun for all Node-related work (installs, scripts, tests, dev servers). No npm, yarn or pnpm. If something does not work with Bun, ask before falling back.
