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
|adversarial review|none|one reviewer, pre-PR (loop below, no ADR: `docs/adr/README.md`)|
|handoff|none|PR|
|landing|rebase, then `git merge --ff-only`, then push|CI green, human reads the diff, rebase, `--ff-only`|
|gate|the human (`--no-verify` is accepted here, by design)|CI plus branch protection|

Rule an agent can apply without judgement: *if the human did not watch this happen, it lands as a
PR.* No memory of attendance is required, because the default is the safe lane.

### Task brief: the issue body is the spec of intent

One scoped task per issue; `Task: #N` in the
commit trailers, so requirement, branch and commit are linked by data instead of by a document.
Body shape:

```text
Wanted:     behaviour, from the user's side, and not a solution
Done when:  one line per observable behaviour, in the form "when <trigger> the system shall
            <response>". Each line either names an existing test or becomes one.
Don't:      out of scope; behaviour that must not change; FLAG paths
Verify:     just check
Stop and ask if: a schema change, a new dependency, a check that will not pass, data that looks wrong
```

### Branching

`agent/<slug>` cut from `main` when the task starts and never reused for a second
task. `migrations/` is single-writer: at most one open task may touch it.

### History

`git rebase` plus `git merge --ff-only`. No squash merges. Atomic commits carrying
`Task:`/`Agent:`/`Verified:` trailers, checked for presence by the `commit-msg` hook. Trailers are
for humans and CI. They are never fed to a reviewer (see Review).

### Diff budget

50 lines soft, 200 hard, at most 10 files. All paths count except lockfiles.
`migrations/**` counts, because excluding it made the cap evadable. Touching a FLAG path forces
Lane B regardless of size.

### FLAG paths

An agent may not edit these in a feature PR: `.github/workflows/**`,
`scripts/check.sh`, `.githooks/**`, `justfile`, `deny.toml`, `.gitleaks.toml`,
`.cargo/config.toml`, `build.rs`, `.pi/**`. The last entry covers the agent's own capability
surface: reviewer definition, installed packages, MCP servers. The list includes the gate itself,
because an agent blocked by a check must not be able to edit that check and pass `--no-verify`. The
ban extends inside the code: no `#[allow]`, `#[allow(clippy::all)]`, `--cap-lints`, or
`#[gitleaks:allow]` to get a gate green, because those change the result without changing the check
file.

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

A fix round driven by judgement hands the implementer nothing it did not already have. "LLMs
struggle to self-correct their responses without external feedback, and at times, their performance
even degrades after self-correction" (Huang et al., ICLR 2024, arXiv:2310.01798), so judgement is
never a fix instruction. Style lands in `GAP` for the same reason: if `just check` is green and the
finding stands, the missing thing is the check. The fix for a contradicted comment is deletion. A
rewrite is unverified
prose inheriting the author's authority.

### Red tests

For a behavioural defect the best `REPRO` is a failing test. The reviewer writes it
in its scratch directory, runs it, and pastes command, failing output and test body. Never into the
tree: the author must start from the tree that was audited, and a reviewer-committed test would be a
second unverified authority (79–82% of generated tests compile, arXiv:2601.09695; wrong assertions
cause 15.6% of generated-test failures, arXiv:2610.32548). The implementer commits that test with
its fix so CI owns it, and may not weaken, `#[ignore]`, or delete it, because benchmark harnesses
are hardened against exactly that behaviour (arXiv:2610.24206). Disputing an assertion is a
stop-and-ask, and not a licence. A green suite afterwards proves the assertion and not correctness
(arXiv:2503.15223), which is why the test is in the diff you read. If no cheap harness can make it
red, that absence is the `GAP`, and it is worth more than the test, because it closes the class
instead of the instance.

What reaches you is at most 3 lines in the PR body: `Review: N found, M fixed, K unresolved` plus
the `HUNCH`es. Never the review itself. Most posted bot comments are rejected or left unactioned,
and the one feature that predicts a comment being acted on is a concrete inline fix
(arXiv:2607.21997).

### The loop, and who runs it

Lane B is closed-loop, and not one-shot: the PR should arrive already
revised. The orchestrator is the session that dispatched the task, and never the implementer. Every
transition is a commit or a file, and never a memory.

1. Implement: brief, code, `just check` green, commit. A red build is never reviewed, because the
   reviewer's competence starts where the deterministic check stops.
2. Review: fresh `killer` session. It is handed diff, brief and gate output, and fetches the rest of
   the repo itself. An exploring reviewer beats a fixed-context one on this exact task (agentic
   review versus single-turn review, arXiv:2607.06065).
3. Fix: exactly one round, in a **fresh implementer session** handed only the evidence-backed
   findings. A long session has decayed by then, and a fresh one has no commitment to the plan
   under review.
4. Verify with the gate and not with another model: `just check` green and the diff budget still
   holding. An unverified LLM repair loop that oscillates does not converge with more rounds; it
   corrupts more code than it repairs (`extrapolator.ai`, 2026-09-10, "If It's Not Buggy, Don't Fix
   It", unread at source, no paper id found: the weakest citation in this file).
5. Re-review only when the fix landed on a boundary path. At most two reviewer calls per PR.
6. Push, open the PR, write the `Review:` line. **The loop can revise; only the human approves.**

Escalate instead of iterating on: a gate that stays red, oscillation, a disputed assertion, a fix
that would breach the budget, or reviewer and implementer reading the brief differently.

Trigger: a FLAG or boundary path, or you asking. Review an ordinary diff now and then so the flag
list grows from findings and so you get a read on how often the gates miss. The loop is worth most when the implementer is weakest. The same
generate-review-revise loop
moved resolve rate from 27.5% to 56.9% on a 30B model and from 72.2% to 75.4% on a frontier one
(arXiv:2607.06065), so a cheap model plus this loop is a rational dispatch and a frontier model
plus it mostly is not. Measure both, and do not assume either.

### Revert-first

Anything suspected of breaking is reverted, then fixed forward. No heroic
debugging on `main`.

## Rejected alternatives

|alternative|why rejected|
|---|---|
|local-only, no GitHub|no audit trail. It makes "nobody reviewed this" literally invisible, which is the failure the loop exists to catch|
|PR for everything|Lane A already involves reading each diff; a PR adds ceremony without adding a reviewer|
|squash merge|destroys per-commit provenance and the trailers that record which agent ran which check|
|400-line cap (earlier draft)|about 8× the observed median reviewed change; the earlier draft's own review called it too loose|
|a long-lived `agent-work` branch|the named anti-pattern: a week old, touches everything, awaits "the big review"|
|justifying budgets with the EASE "61% of agent PRs unreviewed" stat|that study's population is repos with 100 stars or more; this repo has none. It argues against ceremony here, so it is not used|

## Evidence

Primary source for the diff budget, reachable and quoted verbatim
(`https://storage.googleapis.com/gweb-research2023-media/pubtools/4476.pdf`, Sadowski et al.,
*Modern Code Review: A Case Study at Google*, ICSE SEIP 2018; the analysis of review logs for
9 million reviewed changes):

> "At Google, over 35% of the changes under consideration modify only a single file and about 90%
> modify fewer than 10 files. Over 10% of changes modify only a single line of code, and the median
> number of lines modified is 24."
>
> "developers have to wait for initial feedback on their change a median time of under an hour for
> small changes and about 5 hours for very large changes. The overall (all code sizes) median
> latency for the entire review process is under 4 hours."

Caveat: that is a queue statistic from an org with thousands of reviewers. This project has one
human and no queue, so the analogy is directional, not exact. The 50/200 numbers are judgement,
anchored on that median, and are the soft spot most likely to be revised.

Also: `git interpret-trailers` validates *presence and syntax*, and not truth. An agent can write
`Verified: just check` without running it. Hence "the human is the only approver" and hence FLAG
paths.

Why the brief is complete up front instead of clarified mid-run. Laban et al. (Microsoft Research
and Salesforce Research), over 200,000 simulated conversations, a fully specified instruction
instead revealed piece by piece across turns:

> "Performance fell by an average of 39% across six generation tasks … Models commit to an
> assumption early, build a full solution on it, then keep leaning on it … when a model takes a
> wrong turn, it gets lost and does not recover."

Agentic runs are that experiment with nobody in the room, so the wrong turn at turn 3 gets 37 tool
calls built on top of it. One behaviour per acceptance line is EARS (`WHEN <trigger> the system
shall <response>`; Mavin et al., RE'09, adopted by Amazon's Kiro), chosen because a single-clause
requirement is already the shape of a test. The stop condition is the field agents most often lack:
without it the failure modes are halting at the first ambiguity for a sleeping human, or continuing
well past the point where asking was right.

Cited, and not quoted, for the review section: comment/code contradictions raise bug-introduction
odds about 1.5× and the effect peaks immediately after the inconsistency is introduced
(arXiv:2409.10781), which in this repo means the next reader is the next agent. 45% of merged agent
PRs needed changes, concentrated in "bug fixes, documentation, and adherence to project-specific
standards", reviewers hand-fixing what a lint flag would catch (arXiv:2509.14745). 50.8% of 7,357
static-analysis suppressions were inert, introduced largely over false positives and unreadable
messages, hence "fix the lint, never allow it" (doi:10.1145/3715729). And LLM evaluators recognise
and favour their own output, the bias tracking recognition ability while human annotators judged
the texts equal (arXiv:2404.13076). Author and reviewer here share a model family, so the separate
context window holding only the diff is the only independence we have.

## Binding summary for AGENTS.md

1. Default to Lane B (worktree, branch, PR); Lane A only while the human watches every diff.
   Enforcer: none is possible. No tool records attendance, so this one stays prose.
2. Never edit a FLAG path to get a check to pass; report it. Enforcer: path gate in
   `scripts/check.sh` plus CODEOWNERS once CI exists. The list is in the Decision section.

Machine-enforced, therefore not `AGENTS.md` material: the diff budget (`just check`, from
`git diff --shortstat`); worktree isolation (`just check`: `--git-dir` ≠ `--git-common-dir`);
`--ff-only`, no squash, no force-push, no push to `main` (GitHub branch protection plus the
`commit-msg` hook, which also checks trailer presence); "land same day" and at most 3 branches
are maintainer hygiene, and not agent rules.
