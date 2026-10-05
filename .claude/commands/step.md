Build step $ARGUMENTS. Read the step brief in docs/phases/, docs/PRODUCT.md, docs/PROCESS.md, CLAUDE.md, docs/STATUS.md and all accepted ADRs, plus any plan already on the step's branch.
1. Plan first. Reply with a short plan: files touched, how each acceptance criterion will be verified, risks. Follow any accepted ADR or existing plan. Then stop and wait for me to say go.
2. After I say go, implement on branch step/$ARGUMENTS-<short-name> (reuse it if it exists). Commit after each coherent chunk. After each commit, update docs/STATUS.md with what is done and what remains, so a new session can continue if this one is cut off by a usage limit.
3. Keep cargo xtask check passing. Verify on desktop, and on my connected phone where the step touches UI.
4. Finish with a report in the PR summary format from docs/PROCESS.md: verified, not verified, deviations from the plan, things for me to review. Mark the step done in docs/STATUS.md.
Stop and ask only if a stop condition in CLAUDE.md applies.
