<!--
This adds no rule. It carries the ones ADR-0001 already prescribes, in the order
a human reads them, because the review line is the part that reaches you.
Worked examples of the shape: #39 (review round acted on), #38 (gate fixtures),
#35 (a 4-line gate edit). Delete these comments before submitting.
-->

Task: #<issue>

## What changed

One or two lines. The diff is the detail.

## Why this shape

The measurement, constraint or failing command that forced it. Every claim here
needs the command that produced it, because #36 and #38 both shipped claims that
measurement later contradicted.

## Verified

`just check` -> the counts it printed. Plus anything this change needed on top.

## Review

```
Review: N found, M fixed, K unresolved
```

If this diff touches a FLAG path (ADR-0001, "FLAG paths"), say so here and wait
for explicit approval - that includes `scripts/check.sh` and anything under
`.github/workflows/`. A green check is not approval: the point of those paths is
that an agent blocked by a check must not be able to edit it and pass.
