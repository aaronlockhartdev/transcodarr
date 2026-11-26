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

