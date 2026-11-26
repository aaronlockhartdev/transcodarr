# ADR-0001: Development process: lanes, branching, and diff budget

Status: proposed. Human owner: Aaron; the merge that lands it is stamped `accepted (date)`.
Agents may edit a proposed ADR on its own branch. After merge, decisions here are not re-opened by
agents: propose `ADR-00NN supersedes ADR-0001` instead.

## Context

One maintainer, code written mostly by AI agents. Generation is free and unreliable; verification
is expensive and is the bottleneck. The observed failure mode is "agent said it was done and it
wasn't". Process exists to protect one scarce resource: the maintainer's reading time.

## Decision

### Two lanes, chosen by whether a human watched each diff

Default is Lane B.

|aspect|Lane A (attended)|Lane B (unattended)|
|---|---|---|
|trigger|human drove it, reading each diff|dispatched and walked away|
|isolation|main checkout acceptable|own `git worktree`, always|
|diff budget|~50 changed lines|~200 changed lines, else split|
|adversarial review|none|one reviewer, pre-PR (the loop below)|
|landing|rebase, then `git merge --ff-only`, then push|CI green, human reads the diff, rebase, `--ff-only`|
|gate|the human (`--no-verify` is accepted here, by design)|CI plus branch protection|

Rule an agent can apply without judgement: *if the human did not watch this happen, it lands as a
PR.* No memory of attendance is required, because the default is the safe lane.

### Task brief: the issue body is the spec of intent

One scoped task per issue, `Task: #N` in the commit trailers, so requirement, branch and commit are
linked by data and not by a document. Body shape:

```text
Wanted:     behaviour, from the user's side, and not a solution
Done when:  one line per observable behaviour, in the form "when <trigger> the system shall
            <response>". Each line either names an existing test or becomes one.
Don't:      out of scope; behaviour that must not change; FLAG paths
Verify:     just check
Stop and ask if: a schema change, a new dependency, a check that will not pass, data that looks wrong
```

### Branching and history

`agent/<slug>`, cut from `main` at task start, never reused for a second task. `migrations/` is
single-writer: at most one open task may touch it.

`git rebase` plus `git merge --ff-only`, no squash merges. Atomic commits carrying
`Task:`/`Agent:`/`Verified:` trailers, checked by the `commit-msg` hook and again by `just check` over
the commits above the base, so a clone where `just setup` never ran cannot land untrailered work. The
gate reads the trailer block with `git interpret-trailers`, so naming the keys in prose is not
provenance, which makes it the stricter of the two readers. Trailers are for humans and CI and are
never fed to a reviewer (see Review).

### Diff budget

50 lines soft, 200 hard, at most 10 files. All paths count except lockfiles.
`migrations/**` counts, because excluding it made the cap evadable. Touching a FLAG path forces
Lane B regardless of size.

### FLAG paths

An agent may not edit these in a feature PR: `.github/workflows/**`, `scripts/check.sh`,
`.githooks/**`, `justfile`, `deny.toml`, `.gitleaks.toml`, `.cargo/config.toml`, `build.rs`, `.pi/**`
(the agent's own capability surface: reviewer definition, installed packages, MCP servers). The list
includes the gate itself: an agent blocked by a check must not be able to edit that check and pass
`--no-verify`. The ban extends inside the code: no `#[allow]`, `#[allow(clippy::all)]`, `--cap-lints`,
or `#[gitleaks:allow]`, because those change the result without changing the check file.

### Review

The reviewer starts from the frozen artifact (`git diff main...HEAD`, the task brief,
`just check` output), reads whatever else it needs itself, and never sees the implementer's
reasoning. Agents given a narrative optimise for agreement over correctness, and a review loop's
measured failure mode is amplifying speculative concerns into out-of-scope edits
(arXiv:2608.18167). Every finding is exactly one of three kinds, and the kind decides where it goes:

|kind|what it is|goes to|
|---|---|---|
|`REPRO`|a command that fails on the branch, or two quoted lines that contradict each other|implementer, one fix round|
|`GAP`|a check that does not exist, or a behaviour nothing can observe|`scripts/check.sh`, a FLAG path, so you approve it, or a new issue|
|`HUNCH`|judgement with no evidence|you, verbatim, at most 5 one-liners|

A fix round driven by judgement hands the implementer nothing it did not already have: LLMs struggle
to self-correct without external feedback and performance can degrade after self-correction (Huang et
al., ICLR 2024, arXiv:2310.01798), so judgement is never a fix instruction. Style lands in `GAP` for
the same reason: if `just check` is green and the finding stands, the missing thing is the check. The
fix for a contradicted comment is deletion, because a rewrite is unverified prose inheriting the
author's authority.

### Red tests

For a behavioural defect the best `REPRO` is a failing test. The reviewer writes it in its scratch
directory, runs it, and pastes command, failing output and test body. Never into the tree: the author
must start from the tree that was audited, and a reviewer-committed test is a second unverified
authority (79–82% of generated tests compile, arXiv:2601.09695; wrong assertions cause 15.6% of
generated-test failures, arXiv:2610.32548). The implementer commits that test with its fix so CI owns
it, and may not weaken, `#[ignore]`, or delete it, which is what benchmark harnesses harden against
(arXiv:2610.24206). Disputing an assertion is a stop-and-ask, not a licence. A green suite proves the
assertion and not correctness (arXiv:2503.15223), which is why the test is in the diff you read. If no
cheap harness can make it red, that absence is the `GAP`, and it closes the class and not the instance.

What reaches you is at most 3 lines in the PR body: `Review: N found, M fixed, K unresolved` plus the
`HUNCH`es, never the review itself. Most posted bot comments are rejected or left unactioned, and the
one feature that predicts a comment being acted on is a concrete inline fix (arXiv:2607.21997).

### The loop, and who runs it

Lane B is closed-loop, and not one-shot: the PR should arrive already revised. The orchestrator is the
session that dispatched the task, and never the implementer. Every transition is a commit or a file,
and never a memory.

1. Implement: brief, code, `just check` green, commit. A red build is never reviewed, because the
   reviewer's competence starts where the deterministic check stops.
2. Review: fresh `killer` session handed diff, brief and gate output. An exploring reviewer beats a
   fixed-context one on this task (arXiv:2607.06065).
3. Fix: exactly one round, in a **fresh implementer session** handed only the evidence-backed
   findings. A long session has decayed, and a fresh one has no commitment to the plan under review.
4. Verify with the gate and not with another model: `just check` green and the diff budget still
   holding. An unverified repair loop that oscillates does not converge with more rounds; it corrupts
   more code than it repairs (extrapolator.ai, 2026-09-10, "If It's Not Buggy, Don't Fix It", no paper
   id: the weakest cite here).
5. Re-review only when the fix landed on a boundary path. At most two reviewer calls per PR.
6. Push, open the PR, write the `Review:` line. **The loop can revise; only the human approves.**

Escalate instead of iterating on: a gate that stays red, oscillation, a disputed assertion, a fix
that would breach the budget, or reviewer and implementer reading the brief differently.

Trigger: a FLAG or boundary path, or you asking. Review an ordinary diff now and then, so the flag
list grows from findings and you get a read on how often the gates miss. The loop is worth most when
the implementer is weakest: the same generate-review-revise loop moved resolve rate from 27.5% to
56.9% on a 30B model and from 72.2% to 75.4% on a frontier one (arXiv:2607.06065). Measure both.

### Revert-first

Anything suspected of breaking is reverted, then fixed forward. No heroic debugging on `main`.

## Rejected alternatives

|alternative|why rejected|
|---|---|
|local-only, no GitHub|no audit trail. It makes "nobody reviewed this" literally invisible, which is the failure the loop exists to catch|
|PR for everything|Lane A already involves reading each diff; a PR adds ceremony without adding a reviewer|
|squash merge|destroys per-commit provenance and the trailers that record which agent ran which check|
|400-line cap (earlier draft)|about 8× the observed median reviewed change; the earlier draft's own review called it too loose|
|a long-lived `agent-work` branch|the named anti-pattern: a week old, touches everything, awaits "the big review"|
|justifying budgets with the EASE "61% of agent PRs unreviewed" stat|that study's population is repos with 100 stars or more; this repo has none. It argues against ceremony here, so it is not used|

## Unenforced

What has no machine behind it, with the issue that owes the gate. Neither tool's output is a registry: an
arm with an empty scope and a fixture whose precondition fails each print no line at all. #71 #72

- **Lane B has no attendance record.** Nothing records whether a human watched the diff, so the rule that
  picks the lane cannot be checked by anything.
- **FLAG paths have no detector.** Nothing inspects diff paths, and no tool tells a fix from a cover-up.
  The motive is gated instead: a widened check buys budget or suppressions, and both of those fail.
- **Trailers are presence, not truth.** `commit-msg` checks the three names, the `Task:` value shape, and
  an `Agent:` whose words appear in `$PI_MODEL` -- a clause it skips when `$PI_MODEL` is unset, which is
  every CI runner and every human shell. `just check` re-reads the block over the commits above the
  base, closing the untrailered landing (#61); the hook greps, so it is the looser reader. Nothing
  verifies that a review ran, that `Verified:`
  describes a real run, or that `Task: #N` names an issue (#63).
- **Platform settings are configured, not observed.** `--ff-only`, no squash, no force-push, no push to
  `main` and one required approval are GitHub settings; no check in this repo reads them, so a silent
  flip leaves nothing in the history. #67
- **The ADR length cap warns and does not block**, which is the whole of its power.
- Worktree isolation, "land same day" and at most 3 open branches are maintainer hygiene, not agent rules.

Citations behind these decisions live in `docs/research.md`, outside every ADR arm: no length cap, no
heading rule there, though `md-links` does check whether a target it names still resolves.
