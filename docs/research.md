# Research behind the ADRs

The references the decisions lean on. An ADR states a decision and links here; a measurement is never
restated in a decision record, because nothing re-derives a number that lives in prose. A link resolver checks
the targets here and nothing else: no length cap, no heading rule, and most citations predate the gate.

## [ADR-0001](adr/0001-development-process.md) — development process


Primary source for the diff budget: Sadowski et al., *Modern Code Review: A Case Study at Google*
(ICSE SEIP 2018, `https://storage.googleapis.com/gweb-research2023-media/pubtools/4476.pdf`), an
analysis of review logs for 9 million changes. The clause leaned on is the distribution: median 24
lines modified, 90% of changes under 10 files. 50/200 sits above that median on purpose and is still
judgement, most likely to be revised here. The study's latency numbers are not used: a queue statistic
from an org with thousands of reviewers, and this project has one human and no queue.

Also: `git interpret-trailers` validates presence and syntax and not truth, so an agent can write
`Verified: just check` without running it. Hence the human is the only approver, and hence FLAG paths.

Why the brief is complete up front instead of clarified mid-run: Laban et al. (Microsoft Research and
Salesforce Research, `arXiv:2505.06120`), 200,000 simulated conversations, one clause leaned on. Performance fell an
average of 39% across six generation tasks when a fully specified instruction was instead revealed
piece by piece, because models commit to an assumption early and do not recover from a wrong turn.

Agentic runs are that experiment with nobody in the room, so the wrong turn at turn 3 gets 37 tool
calls built on top of it. One behaviour per acceptance line is EARS (`WHEN <trigger> the system
shall <response>`; Mavin et al., RE'09, adopted by Amazon's Kiro), chosen because a single-clause
requirement is already the shape of a test. The stop condition is the field agents most often lack:
without it the failure modes are halting at the first ambiguity for a sleeping human, or continuing
well past the point where asking was right.

Cited, not quoted, for the review section: comment/code contradictions raise bug-introduction odds
about 1.5× and the effect peaks immediately after the inconsistency is introduced (arXiv:2409.10781),
which here means the next reader is the next agent. 45% of merged agent PRs needed changes,
concentrated in bug fixes, documentation and adherence to project-specific standards (arXiv:2509.14745).
50.8% of suppressions were inert -- that figure is corpus-wide across three languages and four analyzers,
not a share of the 7,357 suppressions counted in 46 Python projects, so the two are not a fraction of each
other -- and they entered largely over false positives and unreadable messages, hence "fix the lint, never
allow it" (doi:10.1145/3715729). LLM evaluators recognise and favour their own output
while human annotators judge the texts equal (arXiv:2404.13076), so the separate context window
holding only the diff is the only independence we have.

Per-class performance of LLM reviewers, SWR-Bench (1,000 PRs, 5 pipelines, preprint arXiv:2509.01494):
interface 40.8% recall and logic 54.6% recall, both at ~17% precision; textual and documentation changes
13.0% recall at F1 14.3; the "larger defects" class 18.9% recall. Precision is the failure mode, not
recall -- the best pipeline scored F1 18.7 overall, and feeding a static-analysis report in as extra
context was the worst configuration on precision (2.79). One 1,000-PR review run cost ~$1.57, so the
reviewer call is not the scarce resource; attention is.

Human review bites hardest on security and is thin elsewhere: concerns raised in 35 of 40 weakness
categories and 39-41% of those addressed (Charoenwet et al., EMSE 2024, arXiv:2311.16396, 135,560
comments), while at Microsoft only ~15% of reviewer comments indicated a possible defect and a reviewer
with no prior exposure to the area had 33% of their comments judged useful, rising to ~67% on the third
pass over the same code (Czerwonka et al., 2015). Approvals of agent-authored PRs drift upward with
repetition, 30.1% to 36.8% (+14.5pp across experience deciles) with inline comments down 22% over seven
months (arXiv:2606.22721, preprint, unreplicated), which is why the exempt classes get sampled anyway. No
study relates review round count to defect escape rate, human or agent, so the round rule rests on two
measurements instead: graded revision moved 22.9% to 38.4% over four rounds (arXiv:2607.06065), and a
second LLM validating the *findings* collapsed key-bug inclusion from 20.00 to 2.22 and 13.33 to 0.00
(arXiv:2505.17928). Action on agent comments is 0.9-19.2% of valid comments changing code against 60% for
human comments (arXiv:2508.18771), while thread resolution for comparable comments reached 72.9%
(arXiv:2607.21997): the two sources measure different things, and only the first one touches the tree.

Dropped on audit, so it does not come back: a "79-82% of generated tests compile" figure carried
arXiv:2601.09695 and could not be relocated in that paper's abstract or snippets, and arXiv:2610.24206
documents harness hardening against reward gaming (no network, patches re-graded in a clean container),
which is an analogy for a weakened committed test and not a study of one.

## [ADR-0002](adr/0002-database-policy.md) — database policy

docs.rs, `sqlx::migrate` (v0.9.1): "**The only solution on stable Rust right now** is to create a Cargo
build script … `println!("cargo:rerun-if-changed=migrations")`"; and for the checksum and line-ending
hazard, "force `.sql` files to be checked out with Line Feeds: `*.sql text eol=lf`". Without `build.rs`
an agent adds a migration, rebuilds, sees the old schema, concludes sqlx is broken, and invents
workarounds.

sqlite.org, `lang_altertable`: `ALTER TABLE` covers rename table/column, add column, drop column,
add/drop constraint, and set/drop `NOT NULL`. Everything else goes through §8 "Making Other Kinds Of
Table Schema Changes": the 12-step rebuild using `PRAGMA writable_schema=ON`, where the docs warn that a
syntax error in the schema change "will render the database corrupt and unreadable". That rebuild is
what ADR-0002's backup-before-a-destructive-migration rule is for, and no check reads for the backup.

## [ADR-0003](adr/0003-contract-pipeline.md) — contract pipeline

Verified on this machine 2026-11-24 with the pin set ADR-0003 names. The control: `npm install` without
the `overrides` entry fails `ERESOLVE … peer typescript@"^5.x" from openapi-typescript@7.13.0`; with it
the install is clean. Green run: `svelte-kit sync`, then `svelte-check --threshold error` (0 errors, 0
warnings), then `vite build` produces `build/index.html`, typechecking runes, path params and nullable
narrowing.

Two claims are measured. Regenerating the generated types from the same spec is byte-identical, which is
what a restored drift gate would rest on (#75). And extension-less `#lib` specifiers are refused by
`tsc --traceResolution` after the mapping resolves, which is where the extension rule comes from.

## [ADR-0004](adr/0004-license.md) — licence

Neighbour licences (GitHub API, 2026-11-24): Sonarr `GPL-3.0`, Radarr `GPL-3.0`, Jellyfin `GPL-2.0`,
Immich `AGPL-3.0`. Immich is the direct precedent: a self-hosted media server that switched **from MIT
to AGPLv3** in February 2024 (discussion #7023) explicitly to stop closed hosted wrappers.

Compatibility direction the decision depends on: MIT / Apache-2.0 / BSD / ISC / MPL-2.0 crates are
usable in an AGPL-3.0 project (Apache-2.0 to AGPLv3 is one-way compatible). GPL-2.0-only is the
exclusion, and the pinned cargo-deny has no license ban table, so the exclusion is carried by its absence
from the allow list. The semantics are SPDX's: a dependency that offers a compatible side
(`MIT OR GPL-2.0-only`) is satisfied by that side and passes, which is correct, since you may take it.
Three per-crate doors change a licence verdict while the allow list stays untouched, all measured on the
pinned cargo-deny 0.22.0 (#18): `[[licenses.exceptions]]` exempts a named crate, `clarify.license` re-admits
one whose manifest declares it, and `[graph] excludes` drops it from the scan. Each prints `licenses ok`,
byte-identical to a clean run, which is why ADR-0004 bans reaching for any of them in prose and why the arm
cannot be the defence.

Text relied on: AGPL-3.0 §13, "Remote Network Interaction; Use of Source Code", i.e. network use counts
as conveying.
