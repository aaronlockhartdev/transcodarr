# AGENTS.md

Before adding or changing any doc: read `docs/adr/README.md`.

1. Default to Lane B: worktree, branch, PR. Lane A applies only while the human watches every diff.
   (ADR-0001)
2. Never edit a FLAG path to get a check to pass, and never widen `deny.toml` to unblock a build.
   Never add `#[allow]`, `#[expect]`, `#[allow(clippy::all)]`, `--cap-lints` or `#[gitleaks:allow]`
   to get one green. Report the failure. FLAG list in ADR-0001; `deny.toml` in ADR-0004.
3. Pre-1.0 the schema is not frozen. Write the destructive migration. No compat shims, no asking.
   (ADR-0002)
4. Library code does not bulk-delete user intent. Tests open a fresh temporary database. (ADR-0002)
5. `contracts/openapi.json` and `apps/web/src/lib/api/**` are generated. Do not hand-edit or
   hand-merge them. Regenerate with `just gen`. (ADR-0003)
6. Do not add web-workspace dependencies. The pin set resolves as a whole (TS 6.0.3 plus
   `overrides`). (ADR-0003)
7. Brainstorm, feature and app-direction output goes to `gh issue create`. Do not commit it.

`just check` is the whole gate. Run it and quote its output. If a check you expected is missing, say
so.
