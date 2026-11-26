# Research behind the ADRs

The references the decisions lean on. An ADR states a decision and links here; a measurement is never
restated in a decision record, because nothing re-derives a number that lives in prose. Nothing checks
this file -- no length cap, no heading rule, no link resolver -- and most citations here predate the gate.

## ADR-0001 — development process


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


## ADR-0002 — database policy

- docs.rs, `sqlx::migrate` (v0.9.1): "**The only solution on stable Rust right now** is to create a
  Cargo build script … `println!("cargo:rerun-if-changed=migrations")`"; and for the checksum/line
  ending hazard, "force `.sql` files to be checked out with Line Feeds: `*.sql text eol=lf`".
  Without `build.rs` an agent adds a migration, rebuilds, sees the old schema, concludes sqlx is
  broken, and invents workarounds.
- sqlite.org, `lang_altertable`: `ALTER TABLE` covers rename table/column, add column, drop column,
  add/drop constraint, and set/drop `NOT NULL`. Everything else goes through §8 "Making Other Kinds
  Of Table Schema Changes": the 12-step rebuild using `PRAGMA writable_schema=ON`, where the docs
  warn that a syntax error in the schema change "will render the database corrupt and unreadable".

## ADR-0003 — contract pipeline

Verified on this machine 2026-11-24 with the pin set above. The control: `npm install`
without the `overrides` entry fails `ERESOLVE … peer typescript@"^5.x" from
openapi-typescript@7.13.0`; with it the install is clean. Green run: `svelte-kit sync`, then
`svelte-check --threshold error` (0 errors, 0 warnings), then `vite build` produces
`build/index.html`, typechecking runes, path params and nullable narrowing.

Two claims here are measured. Regenerating the generated types from the same spec is
byte-identical, which is what makes `git diff --exit-code` a valid drift gate. And
extension-less `#lib` specifiers are refused by `tsc --traceResolution` after the mapping
resolves, which is where the extension rule comes from.


## ADR-0004 — licence

Neighbour licences (GitHub API, 2026-11-24): Sonarr `GPL-3.0`, Radarr `GPL-3.0`, Jellyfin
`GPL-2.0`, Immich `AGPL-3.0`. Immich is the direct precedent: a self-hosted media server that
switched **from MIT to AGPLv3** in February 2024 (discussion #7023) explicitly to stop closed
hosted wrappers.

Compatibility direction we depend on: MIT / Apache-2.0 / BSD / ISC / MPL-2.0 crates are usable in an
AGPL-3.0 project (Apache-2.0 to AGPLv3 is one-way compatible). GPL-2.0-only is the exclusion, and the
pinned cargo-deny has no license ban table, so the exclusion is carried by its absence from the allow
list. The semantics are SPDX's: a dependency that offers a compatible side (`MIT OR GPL-2.0-only`) is
satisfied by that side and passes, which is correct, since you may take it. The door that would re-admit
GPL-2.0-only alone is `[[licenses.exceptions]]`, and rule 1 bans reaching for it.

Text relied on: AGPL-3.0 §13, "Remote Network Interaction; Use of Source Code", i.e. network use
counts as conveying.

