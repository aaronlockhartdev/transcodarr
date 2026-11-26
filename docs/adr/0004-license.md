# ADR-0004: License, AGPL-3.0-only

Status: proposed. Human owner: Aaron; stamped `accepted (date)` at the merge that lands it.

## Context

transcodarr is a self-hosted media transcoder that can be exposed over a network. Its ecosystem is
predominantly copyleft, and the realistic threat is a closed hosted wrapper: someone runs an
unmodified or lightly modified copy as a paid service and never publishes their changes.

## Decision

### AGPL-3.0-only

Enforced mechanically, because a license an agent doesn't know about is not
real: `deny.toml` carries the allow list, and `cargo deny check licenses` runs in `just check`
and CI, so an AGPL-incompatible dependency fails the build. It does not wait for a reviewer to notice.
Neither gate stops an agent from deciding the ban is the bug and adding an allow entry, so that
decision is banned in prose.

### Costs

AGPL is banned by some corporate contribution policies and treated as radioactive by
some enterprises (Debian ships it fine). It forecloses a later dual-license/CLA path unless
copyright assignment is collected. Contributions into permissive upstream crates stay awkward.
Acceptable trade for a single-maintainer, network-exposed tool.

## Rejected alternatives

|alternative|why rejected|
|---|---|
|MIT|a hosted-transcode-service fork of an MIT transcodarr would be entirely legal and unremarkable|
|GPL-3.0|misses the case that matters: running a modified copy as a network service is not "distribution" under GPL|
|GPL-2.0-only|the one licence that is actually hostile to us here. GPL-2.0-only dependencies are incompatible with AGPL-3.0, which is why Jellyfin is stuck on it|

## Evidence

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

## Unenforced

- **Never widen `deny.toml`.** `cargo deny` reads the worktree config, so the arm fails while an unallowed
  licence sits in the graph and becomes a PASS byte-identical to a clean run the moment that licence is
  allow-listed. Nothing inspects diff paths, so a widening leaves no trace; what does fail is an allow
  entry nobody declares (#14).
- The `AGPL-3.0-only` entry is list-wide, so an `AGPL-3.0-only` dependency passes too (#14).
