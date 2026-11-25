# ADR policy and index

An ADR records a **decision**, not information. Agents may draft one; only the human accepts it.
`AGENTS.md` carries the binding rules; an ADR carries the why and the evidence. pi does not
auto-load `docs/adr/**`, so a binding rule must already appear in `AGENTS.md`.

`AGENTS.md` holds one line per rule that survives the loud-and-cheap test, and a new rule displaces
an old one. Adherence falls with instruction count, and the harness spends about a third of an
agent's reliable budget before this file is read (arXiv:2507.11538). Read this file before adding or
changing any doc here.

## Write one only if all three hold

1. **Expensive to reverse**: data, licence, dependency surface, an external contract. Or a choice an
   agent will plausibly re-open because the rejected alternative looks better from inside the code.
2. **The reason is not inferable** from the code or from tool output. There must be a real rejected
   alternative with a real argument. No plausible challenger, no ADR.
3. **Not already self-documenting.** If a lint, a CI check, or a failing test demonstrates the rule,
   the check is the documentation.

## Never an ADR

Mechanics internal to an already-accepted process; product scope, which belongs in `docs/specs/`;
investigation output, which is delete by default (only a decision or a failing test survives an
investigation); anything with no rejected alternative.

## Format

At most 200 lines (`adr-length` in `scripts/check.sh`), `NNNN-title.md`, sections: Context · Decision ·
Rejected alternatives (with the reason that kills each) · Evidence · Binding summary for AGENTS.md.
Every binding line names its enforcer: a `just` recipe, a lint, a hook, or a platform setting. A line
with no enforcer is advice, and advice decays. The cap warns rather than fails because file structure
has no measurable effect on compliance, and adherence falls as the amount a session has already
generated grows (arXiv:2605.10039). The fix for a long ADR is pruning evidence; a second decision means
a second ADR.

Status lifecycle: an agent writes `proposed`; the human stamps `accepted (date)` at the merge that
lands it, and that line belongs to the human alone. Before merge an ADR is a draft: edit it on its
own branch, do not supersede it. After merge it may be **shortened** (prune evidence, fix dead
links, correct wording) but never **reversed**; a change of course is two lines that name each other.
`ADR-0010` supersedes `ADR-0009` by making 0009's `Status:` line read `superseded by ADR-0010` and 0010
state `Supersedes ADR-0009` where its body explains the change. Both pointers or neither: the `Status:`
line is the only place a supersession is read, so the same phrase inside a sentence is prose, not a
state.
Evidence means a quote or a command transcript. A citation to a page nobody can reach is not
evidence. Shape to copy: `0004-license.md`.

Gate (`adr-structure` in `scripts/check.sh`): the `Status:` line begins with `proposed` or `accepted`,
or reads exactly `superseded by ADR-NNNN`, and a supersession fails unless the record it names exists
and names the file back. Naming yourself is one decision written twice; a pointer at a record that does
not exist, or at one that never mentions you, is the same claim made quietly. All four are gate failures,
not advice.

Sourcing rule: cite by id (`arXiv:NNNN.NNNNN`, `doi:…`) plus the one clause leaned on. Quote verbatim
only when the exact wording is the rule. Every blockquote is a second text to keep true, and this
corpus accumulated four in a day on rules that cite numbers and not phrasings. Quantities from
secondary blogs have been caught wrong once here already, before any gate existed to catch it.

## Index

|#|Title|Status|File|
|---|---|---|---|
|0001|Development process: lanes, branching, diff budget|proposed|0001-development-process.md|
|0002|Database policy before 1.0|proposed|0002-database-policy.md|
|0003|Typed contract between the Rust API and the Svelte frontend|proposed|0003-contract-pipeline.md|
|0004|License: AGPL-3.0-only|proposed|0004-license.md|

Deliberately absent:

- **Adversarial review**: no ADR yet. It is cheap to delete and unproven. It earns the ADR once you
  have acted on a finding in 5 separate `Review:` lines, and it is deleted once 10 such lines show
  you acted on fewer than a third. Interim mechanism: ADR-0001.
- **Toolchain provisioning (mise)**: self-documenting. `just setup` asserts the effective toolset and
  `just check` fails on a non-22 node. `mise.toml` is the only place a pin is written. Nothing here
  is silent, so `AGENTS.md` has no toolchain line.
- **Worktrees, crate layout, formatting**: mechanisms or tool-enforced, not decisions.
