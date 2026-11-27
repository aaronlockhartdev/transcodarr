#!/usr/bin/env bash
# The gate's teeth, tested. End every check.sh edit with `just selftest` green. A gate exercised
# only by real failures is a gate whose exclusions and skip branches rot silently: one shipped
# exclusion regex anchored the path at the line start, `--numstat` puts it third, and it matched
# nothing while still reporting PASS. So each rule is checked in both directions: red on the
# defect, green when the defect is excluded or absent.
#
# It runs in a throwaway repository, never in this checkout. Two real defects behind that choice.
# Staging a probe in the live index races any other agent working in the same checkout. And a
# fixture helper that sets only the working directory follows an inherited GIT_DIR and commits into
# the outer repository, which is the state every agent here works in, since worktrees set GIT_DIR.
#
# Needs a GNU userland, same as scripts/check.sh. This file used `sed -i`, whose wrong-platform
# spelling fails without editing anything, so a probe that changed no text would have reported a
# green it never tested.
set -uo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel) || exit 1
unset GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE GIT_NAMESPACE GIT_OBJECT_DIRECTORY \
      GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG
export GIT_CONFIG_NOSYSTEM=1

# CI names the base of the checkout it built. Every repo the suite builds below is a scratch repo with no
# remote, so an inherited value makes the gate name a base that cannot exist -- which it did, 7 probes deep
# in CI before this line (#70). A fixture must not read its expectations from ambient CI state; the probes
# that want a named base export it themselves and unset it after.
unset GITHUB_BASE_REF
export GIT_AUTHOR_NAME=selftest GIT_AUTHOR_EMAIL=selftest@invalid
export GIT_COMMITTER_NAME=selftest GIT_COMMITTER_EMAIL=selftest@invalid

work=$(mktemp -d) || exit 1
clean() { [ -n "${work:-}" ] && rm -rf "$work"; }
trap clean EXIT
export GIT_CEILING_DIRECTORIES="$work"

# Seed a real repo so the gate sees a corpus, not an empty tree: the ADR set, both pins, the
# ignore rules, and the recipe list. An empty commit makes HEAD resolve, which is why git's own
# harness replaced hand-rolled init-db with a helper that commits.
mkdir -p "$work/scripts"
cp "$repo/scripts/check.sh" "$work/scripts/check.sh"
# A scratch recipe list rather than the real one, deliberately: the real `gen` would need the
# installed frontend, and a suite that requires an install is a suite that cannot run before CI has
# installed anything. The gate must not shell out to a toolchain that is absent here — a skip that
# is really a missing dependency is how a green starts to mean nothing.
cat > "$work/justfile" <<'JUSTFILE'
# Seeded by scripts/selftest.sh. Not the real recipe list, on purpose: no recipes at all, so the
# gate never shells out to a toolchain that is not installed in here.
JUSTFILE
cp "$repo/mise.toml" "$repo/rust-toolchain.toml" "$work/"
cp -r "$repo/docs" "$work/docs"
# The prose rules travel with their cases: the two arms read `.vale.ini`, `.vale/ADR/*.yml` and
# `lychee.toml` out of the work tree, so a scratch repo without them would test an absent tool, not
# the arm. `check.sh` passes `lychee.toml` by name, the way `.gitleaks.toml` travels (#55).
cp "$repo/.vale.ini" "$repo/lychee.toml" "$work/"
cp -r "$repo/.vale" "$work/.vale"
[ -f "$repo/.gitignore" ] && cp "$repo/.gitignore" "$work/"
# The enforced secret ruleset, tracked since #55. Without it the gate refuses to scan at all rather
# than fall back to embedded defaults, so every gitleaks probe below would be red for the wrong
# reason -- which is the point, and why it is copied here instead of made per-probe.
[ -f "$repo/.gitleaks.toml" ] && cp "$repo/.gitleaks.toml" "$work/.gitleaks.toml"
# A fixture licence policy, not the real one. The real allow list names five licences and the scratch
# graph is one crate, and since #14 `unused-allowed-license = "deny"` turns that mismatch into an error,
# so copying the real file would put an unread `FAIL deny` on every probe below that has a Cargo.lock.
# The real config's green side is CI's own `cargo deny check` over the real graph.
cat > "$work/deny.toml" <<'DENY'
# Seeded by scripts/selftest.sh, matched to the scratch graph: one crate, MIT.
[licenses]
version = 2
unused-allowed-license = "deny"
allow = ["MIT"]
DENY
( cd "$work" && git init -q -b main && git add -A && git commit -q -m seed ) || exit 1
cd "$work" || exit 1

created=()
make() { # path content
  if [ -e "$1" ]; then return 1; fi          # pre-existing: caller must skip this probe
  mkdir -p "$(dirname "$1")" && printf '%s' "$2" > "$1" && created+=("$1")
}
stage()   { git add -- "$@" >/dev/null; }
unstage() { git reset -q HEAD -- "$@" >/dev/null; }
prunedirs() { for d in contracts apps/web/src/lib/api apps/web/src apps/web apps .sqlx; do rmdir -p "$d" 2>/dev/null; done; }
clean_probes() {
  [ ${#created[@]} -gt 0 ] && unstage "${created[@]}" && rm -f "${created[@]}"
  prunedirs
}
trap 'clean_probes; clean' EXIT

bad=0; skipped=0; probes=0
# Every probe outcome — ok, BROKE or skip — bumps `probes`, and the last block pins the total. #72 measured
# the hole this closes: 79 ok lines with per-block counters covering 52, so deleting an uncounted probe, or
# letting a fixture's precondition fail and print nothing, left every counter reading its expected number.
# Inside $work, so the path is per-run and dies with the run. A fixed path here is machine-wide: two
# suites on one box — the normal shape of this project, one worktree per unit — resolve their
# assertions against each other's gate output. Measured on main with two runs in flight: 4 and 8 fake
# BROKEs, one reading `wanted /tmp/tmp.V2mNtTrkvB/shims/cargo-deny, gate said /tmp/tmp.oyNWJj75vN`,
# which is another process's scratch tree answering this process's question. Nothing is lost for
# post-mortems: `report` below already echoes the log's FAIL and WARN lines into the BROKE output.
log="$work/selftest-gate.log"
arm_names=(adr-structure md-structure md-links adr-index adr-length toolchain-pins
  "generated-paths-tracked" gitleaks cargo deny web "web build" serving "diff budget"
  gate-proof suppressions "vendored instructions" trailer)  # one entry per arm, as its verdict line names it
arm_gap_names=""                                       # names an arm failed to print, across every run
run_gate() {                                           # every arm speaks, in every state, or this goes red
  bash scripts/check.sh > "$log" 2>&1
  local missing
  missing=$(comm -23 <(printf '%s\n' "${arm_names[@]}" | sort) \
                     <(grep -E '^  (PASS|FAIL|WARN|SKIP)  ' "$log" \
                        | sed -E 's/^  [A-Z]+ +//; s/( \(| — ).*$//' | sort -u))
  [ -n "$missing" ] && arm_gap_names="$arm_gap_names $(printf '%s' "$missing" | tr '\n' ' ')"
}
verdict() { local g=green; grep -q 'FAIL' "$log" && g=red; [ "$g" = green ] && grep -q 'WARN' "$log" && g=warn; printf '%s' "$g"; }
report() { # name want got
  probes=$((probes + 1))
  if [ "$3" = "$2" ]; then printf '  ok    %-42s %s\n' "$1" "$3"
  else printf '  BROKE %-42s wanted %s, gate said %s\n' "$1" "$2" "$3"
    grep -E 'FAIL|WARN' "$log" | sed 's/^/          /'; bad=$((bad + 1)); fi
}
expect() { run_gate; report "$2" "$1" "$(verdict)"; }
skipping() { printf '  skip  %-42s %s\n' "$2" "$1"; skipped=$((skipped + 1)); probes=$((probes + 1)); }

printf '=== gate selftest (scratch repo %s) ===\n' "$work"
# Teeth for the fix above, not a description of it: revert this to a fixed path and the run breaks
# before any probe can be resolved against a stranger's verdicts.
report "gate log lives in this run's scratch dir" "ok" \
  "$(case "$log" in "$work"/*) printf ok ;; *) printf shared ;; esac)"
run_gate
base=$(verdict)
# Not "green": the baseline is whatever this branch's own diff earns, and demanding green here
# would fail for a reason that has nothing to do with the gate. A red baseline is a red gate.
[ "$base" = red ] && report "baseline" "not red" "$base"
printf '  info  %-42s %s\n' "baseline verdict" "$base"

# Every arm speaks, and the summary counts what it printed (#71). Before the fix, `gate-proof` and
# `suppressions` sat inside the diff-scope guard, printed nothing, and were not counted skipped, so a
# root-commit run read `check: 6 ran, 7 skipped` and looked healthy.
# The invariant is names, not line counts, and `run_gate` evaluates it on every gate run, not once here. A
# line count pinned at 17 goes red on a legitimate state — `adr-length` warns once per over-cap file, and
# this corpus sits at the cap — while an arm name missing from any run is exactly the defect.
report "every arm printed its name in every gate run" "none" "${arm_gap_names:-none}"
lines=$(grep -cE '^  (PASS|FAIL|WARN|SKIP)  ' "$log")
report "the summary counts every line it printed" "$lines" \
  "$(awk -F'[ ,]+' '/^check:/{print $2 + $4 + $6 + $8}' "$log")"
# A `skip` is not a `pass`: flipping the two new skip lines to `pass` used to keep the suite green, because
# both arms were sampled only by line counts. Inline rather than `expect_check` — that helper is defined
# below this point, and calling it here failed as "command not found" without failing the run, which is the
# same class of defect as an arm that prints nothing.
for arm in gate-proof suppressions; do
  run_gate
  report "$arm skips, not passes, with no scope" "skip" \
    "$(grep -E "^  [A-Z]+  $arm" "$log" | awk '{print tolower($1)}')"
  report "$arm names why it skipped" "1" "$(grep -c "^  [A-Z]*  $arm — no scope measured" "$log")"
done

if make contracts/openapi.json '{"openapi":"3.1.0"}'; then
  expect red "generated file on disk, never committed"
  rm -f contracts/openapi.json
else skipping "path exists" "generated file on disk, never committed"; fi

if make probe.rs 'pub fn a() {}
' && stage probe.rs; then
  printf 'pub fn a() {}
#[allow(dead_code)]
fn b() {}
' > probe.rs && stage probe.rs
  expect red "allow attribute in Rust"
  unstage probe.rs; rm -f probe.rs
fi

if make probe.toml '[build]
rustflags = ["--cap-lints=warn"]
' && stage probe.toml; then
  expect red "cap-lints in TOML"
  unstage probe.toml; rm -f probe.toml
fi

if make over.py "$(python3 -c "print('\n'.join('a%d = %d' % (i, i) for i in range(300)))")" \
   && stage over.py; then
  expect red "300 lines of source"
  unstage over.py; rm -f over.py
fi

if make warn.py "$(python3 -c "print('\n'.join('b%d = %d' % (i, i) for i in range(60)))")" \
   && stage warn.py; then
  expect warn "60 lines of source, past the 50 soft target"
  unstage warn.py; rm -f warn.py
fi

# The exclusions, measured rather than trusted: 1,530 generated and lockfile lines plus one 40-line
# source file must report 40 lines across 1 file. And `contracts/` must NOT be excluded: nothing
# generates it yet, so a change there is hand-written work and pays into the budget (#75).
gen_ok=1
for p in apps/web/src/lib/api/schema.ts .sqlx/q.json; do
  make "$p" "$(python3 -c "print('\n'.join('x' * 60 for _ in range(510)))")" || gen_ok=0
done
make apps/web/package-lock.json "$(python3 -c "print('\n'.join('  \"k%d\": {}' % i for i in range(510)))")" || gen_ok=0
make contracts/openapi.json "$(python3 -c "print('\n'.join('x' * 60 for _ in range(120)))")" || gen_ok=0
if [ "$gen_ok" = 1 ] && make small.py "$(python3 -c "print('\n'.join('c%d = %d' % (i, i) for i in range(40)))")"; then
  stage apps/web .sqlx small.py
  run_gate
  got=$(sed -n 's/.*diff budget (\([0-9]*\) lines, \([0-9]*\) files.*/\1 \2/p' "$log")
  report "generated and lockfile output excluded" "40 1" "$got"
  # The same staged state plus the contract on top: the numbers must move by its 120 lines and one
  # file, which is what an exclusion would otherwise hide.
  stage contracts
  run_gate
  got=$(sed -n 's/.*diff budget (\([0-9]*\) lines, \([0-9]*\) files.*/\1 \2/p' "$log")
  report "the authored contract pays into the budget" "160 2" "$got"
  unstage contracts apps/web .sqlx small.py
  rm -f contracts/openapi.json apps/web/src/lib/api/schema.ts apps/web/package-lock.json \
        .sqlx/q.json small.py
  prunedirs
else
  skipping "path exists" "generated and lockfile output excluded"
fi


# The .md gates, both directions. Each line below is a live misfire found by auditing the corpus
# checks against a scratch tree: a mistyped filename was invisible to every ADR check, and an index
# row that named no file made the README-to-file direction check nothing. The supersession
# lifecycle and the `Status:` vocabulary are pinned here too, in both directions. The required
# headings live in `.vale/ADR/` as rules carrying their own cases, and `vale test --coverage` (below)
# fails on a rule no case fires. The vocabulary stays here because a Vale `conditional` pairs its two
# patterns inside one block, and a block is one line, so the rule came out vacuous rather than strict
# (measured, and it looked green); the lifecycle stays because a two-file join is the one thing
# neither prose tool can read.
adr_body() { printf 'Status: %s\n\n# T\n\n## Context\n\nb\n\n## Decision\n\nd\n\n## Rejected alternatives\n\nr\n\n## Unenforced\n\nz\n' "$1"; }
expect_check() { run_gate; # check want name
  local line got
  line=$(grep -E "^  (PASS|FAIL|SKIP|WARN)  $1" "$log" | head -1)
  case "$line" in
    *FAIL*) got=red ;; *WARN*) got=warn ;; *SKIP*) got=skip ;; *PASS*) got=green ;; *) got=absent ;;
  esac
  probes=$((probes + 1))
  # 4th arg, optional: a string the gate must have printed. A verdict alone can be satisfied by any
  # unrelated failure inside the same check, which is how a probe passes while proving nothing.
  if [ "$got" = "$2" ] && { [ -z "${4:-}" ] || grep -qF "$4" "$log"; }; then printf '  ok    %-42s %s\n' "$3" "$got"
  else printf '  BROKE %-42s wanted %s, gate said %s\n' "$3" "$2" "$got"; bad=$((bad + 1)); fi
}

if make docs/adr/000-misnumbered.md "$(adr_body proposed)"; then
  expect_check adr-structure red "ADR filename no glob can see"
  rm -f docs/adr/000-misnumbered.md
fi

# Same body, H1 and one required section both gone. Today the shell's heading loop catches it; the point
# of pinning it now is that `conditional` rules pair against a `# ` line, so the section checks cannot
# move to Vale without this precondition already in place.
if make docs/adr/9996-no-h1.md "$(adr_body proposed | grep -v '^# T$' | grep -v '^## Decision$')"; then
  expect_check adr-structure red "a record with no level-1 heading" "no level-1 heading"
  rm -f docs/adr/9996-no-h1.md
fi
if make docs/adr/9997-unlisted.md "$(adr_body proposed)"; then
  expect_check adr-index red "ADR the index does not name"
  rm -f docs/adr/9997-unlisted.md
fi

# The length cap, both directions, pinned to the documented number. The fixture has to land at 201
# lines as `wc -l` counts them, and command substitution eats the final newline, hence head -202. With
# it that way, widening the cap past 200 breaks this probe and narrowing it breaks the other one,
# because the real corpus's longest file now sits at the cap (ADR-0001, 200 lines). A fixture 32 lines over the bar pins nothing.
if make docs/adr/9995-long.md "$({ adr_body proposed; awk 'BEGIN{for(i=0;i<250;i++)print "filler " i}'; } | head -202)"; then
  expect_check adr-length warn "an ADR past the 200-line cap"
  rm -f docs/adr/9995-long.md
fi
expect_check adr-length green "the corpus inside the cap"

# The supersession lifecycle, both directions. This rule used to grep the whole file for the pattern
# and to look only for the record's own number, which got both the directions wrong: the prose
# spelling below failed the build, and a supersession whose target did not exist, or whose target never
# pointed back, passed it. The three files stay out of the index on purpose -- `expect_check` names
# adr-structure, so an unrelated adr-index red cannot stand in for a real verdict here.
sup_probes=0
expect_sup() { sup_probes=$((sup_probes + 1)); expect_check "$@"; }
sup_back() { adr_body accepted | sed 's|^## Decision|## Decision\n\nSupersedes ADR-9992|'; }
if make docs/adr/9991-new.md "$(sup_back)" \
   && make docs/adr/9992-old.md "$(adr_body 'superseded by ADR-9991')" \
   && make docs/adr/9993-prose.md "$(adr_body proposed)"; then
  expect_sup adr-structure green "supersession pair, both pointers present"
  printf '%s' "$(adr_body accepted)" > docs/adr/9991-new.md
  expect_sup adr-structure red "supersession the replacement does not return" \
    "ADR-9991 does not state 'Supersedes ADR-9992'"
  printf '%s' "$(adr_body 'superseded by ADR-9990')" > docs/adr/9992-old.md
  expect_sup adr-structure red "supersession naming a record that does not exist" \
    "no docs/adr/9990-*.md exists"
  printf '%s' "$(adr_body 'superseded by ADR-9992')" > docs/adr/9992-old.md
  expect_sup adr-structure red "a record that supersedes itself" "supersedes its own record"
  # The vocabulary is an enum, so a value that merely starts with a legal word is not legal. A regex
  # matching only the prefix lets `proposedly` and `proposed-legacy` read as states, which is the
  # defect #3 names by name; `retired` is the plain case of a word outside the enum.
  for bad_status in retired proposedly proposed-legacy; do
    printf '%s' "$(adr_body "$bad_status")" > docs/adr/9992-old.md
    expect_sup adr-structure red "Status '$bad_status' is not a state" "neither begins with"
  done
  # Green side of the same line: this corpus puts a sentence on the Status line (all four shipped ADRs
  # read `Status: proposed. Human owner: Aaron; ...`), so an over-anchored regex fails the real corpus.
  # That is how the first draft of this rule was caught, by `just check` going red on four untouched ADRs.
  printf '%s' "$(adr_body 'proposed. Human owner: Aaron; stamped accepted (date) at the merge')" \
    > docs/adr/9992-old.md
  expect_sup adr-structure green "the Status sentence the shipped corpus carries"
  # Main's rule fails this last probe: it greps the file for "superseded by ADR-9993", finds the phrase
  # inside a sentence, and calls a legal record a decision written twice.
  printf '%s' "$(sup_back)" > docs/adr/9991-new.md
  printf '%s' "$(adr_body 'superseded by ADR-9991')" > docs/adr/9992-old.md
  printf '%s' "$({ adr_body proposed; printf '\nWriting `superseded by ADR-9993` in a sentence is prose.\n'; })" \
    > docs/adr/9993-prose.md
  expect_sup adr-structure green "the pattern spelled out in prose"
  # The required-heading list is checked for the one section a future edit is most motivated to drop: the
  # admission that a rule has no machine behind it, since deleting it makes an unenforced rule look
  # enforced. The fixture is the valid pair minus that heading and nothing else, so a red can only come
  # from the heading -- built from adr_body instead it also loses the back pointer and bites nothing.
  printf '%s' "$(sup_back | grep -v '^## Unenforced$')" > docs/adr/9991-new.md
  expect_sup md-structure red "a record hiding which rules are unenforced" "ADR.Unenforced"
  rm -f docs/adr/9991-new.md docs/adr/9992-old.md docs/adr/9993-prose.md
fi
report "all supersession probes ran" "10" "$sup_probes"

# The two prose arms, both directions. The rules themselves are tested where they live, in the cases
# beside them, so these probes cover the shell: a defect reaching a verdict, the reason the arm printing
# it naming the right rule, and an empty scope being refused instead of rewarded.
md_probes=0
expect_md() { md_probes=$((md_probes + 1)); expect_check "$@"; }
# The link arm, both directions. What must break is named, not counted: a target that is not a file,
# a fragment that is not a heading, a pointer in the index to a file that is not there -- the direction
# the shell used to check with `-e` before the resolver took it over -- a scope whose links were all
# excluded, an empty scope, which prints `0 OK` and exits 0, and a config the binary will not parse.
# The red probes quote lychee's own words (`File not found`, `Cannot find fragment`), not the arm's
# label, so an arm that stops passing the tool's evidence cannot keep them green. Measured on the pin.
link_probes=0
expect_link() { link_probes=$((link_probes + 1)); expect_check "$@"; }
# A four-digit name the shell arms accept too, so the red cannot be blamed on a filename no glob
# can see: `9990-` is not in the index, which keeps adr-index out of these verdicts' way by being
# red on its own line, and `expect_check` reads one arm's line.
md_shaped() { printf '%s\n\nGoes nowhere: [nowhere](0000-nowhere.md).\n' "$(adr_body "$1")"; }
if make docs/adr/9990-unshaped.md 'Status: proposed.

# T

## Context

b
'; then
  expect_md md-structure red "an ADR with three sections missing" "ADR.Decision"
  rm -f docs/adr/9990-unshaped.md
fi
if make docs/adr/9990-blank.md "$(printf 'Status: proposed.\n\n# T\n\n## Context\n\nb\n\n## Decision\n\nd\n\n## Rejected alternatives\n\nr\n\n## Unenforced\n\n## Next\n\nn\n')"; then
  expect_md md-structure red "an empty '## Unenforced' where an admission belongs" "ADR.UnenforcedBody"
  rm -f docs/adr/9990-blank.md
fi
expect_md md-structure green "the corpus as it stands, and what it counted" "no alerts"

if make docs/adr/9990-file.md "$(md_shaped proposed)"; then
  expect_link md-links red "a link whose target is not a file" "File not found"
  rm -f docs/adr/9990-file.md
fi
if make docs/adr/9990-frag.md "$(printf '%s\n\nGone heading: [heading](0001-development-process.md#no-such-heading).\n' "$(adr_body proposed)")"; then
  expect_link md-links red "a link to a heading that is not there" "Cannot find fragment"
  rm -f docs/adr/9990-frag.md
fi
expect_link md-links green "the links as they stand, offline and fragment-checked" "fragments checked"

# The index itself pointing at nothing: the row still names a file the shell arms are happy with, so
# only a resolver can see the target is gone. `adr-index` is left holding the other direction (an ADR
# no row names), which no link check can express, so it stays green here and the red is the arm's.
if cp docs/adr/README.md readme.bak \
   && printf '\n[index row rewritten by a rename](0002-no-such-adr.md)\n' >> docs/adr/README.md; then
  expect_link md-links red "the index points at a file that is not there" "File not found"
  mv readme.bak docs/adr/README.md
fi

# Greens neither tool withholds on its own: an empty docs directory, a scope whose every link lychee
# skipped, and links it resolved as zero. Vale reports success over a directory with no Markdown and
# lychee prints `0 OK` and exits 0 over an empty or entirely excluded scope, so each arm reads its own
# evidence -- `Total` cannot be lychee's guard because it counts excluded links too (measured: two
# external links under `offline = true` print `2 Total ... 0 OK ... 2 Excluded`).
mv docs docs-empty && mkdir docs
expect_md md-structure red "a directory holding no Markdown at all" "read no Markdown files"
expect_link md-links red "no links for lychee to read" "resolved 0 links, 0 excluded"
printf 'External only: [a](https://example.com/x) and [b](https://example.org/y)\n' > docs/ext.md
expect_link md-links red "every link excluded, none resolved" "resolved 0 links, 2 excluded"
rm -r docs && mv docs-empty docs

# `--config` naming the tracked file is the #55 lesson from `.gitleaks.toml`, so it gets a fixture and
# not a comment: break that file and the arm's verdict has to move. A config the binary refuses to parse
# exits 3 and prints no verdict line (measured), which lands in the arm's third branch.
cp lychee.toml lychee.bak && sed -i 's/offline = true/offline = fasle/' lychee.toml
expect_link md-links red "a config lychee refuses to load" "without a verdict"
mv lychee.bak lychee.toml
mv docs docs-gone
expect_md md-structure red "a path vale cannot find, which it reads as stdin" "read no Markdown files"
expect_link md-links red "a path lychee refuses to read at all" "without a verdict"
mv docs-gone docs
report "all link probes ran" "8" "$link_probes"
report "all prose-arm probes ran" "5" "$md_probes"

# `vale test --coverage` is the teeth on the rules themselves -- a rule that never fires fails the
# run -- so it gets a probe both ways: green on the shipped style, red once a silent rule exists.
# The counter increments outside the call: `$(cov)` runs in a subshell, and a counter raised in one
# reports 0 to the probe below it, which is a green that proves nothing.
vale_probes=0
cov() { vale test --coverage .vale >"$work/vale-test.log" 2>&1; printf '%s' "$?"; }
got=$(cov); vale_probes=$((vale_probes + 1))
report "every shipped rule fires in a case" "0" "$got"
if make .vale/ADR/Silent.yml 'extends: occurrence
message: "a heading nobody writes"
level: error
scope: raw
token: (?m)^## A Heading Nobody Writes$
min: 1
'; then
  got=$(cov); vale_probes=$((vale_probes + 1))
  report "a rule no case fires fails --coverage" "1" "$got"
  rm -f .vale/ADR/Silent.yml
fi
report "all coverage probes ran" "2" "$vale_probes"

# The rule that a gate edit must ship a fixture, tested against itself.
printf '\n# probe\n' >> scripts/check.sh
stage scripts/check.sh
expect_check gate-proof red "check.sh changed with no selftest.sh change"
unstage scripts/check.sh; git checkout -q -- scripts/check.sh

# The vendored instruction tree is third-party prose an agent may follow verbatim. The gate must see it
# without forbidding it (#56), and it must say so in the states nobody thought to test (#71): the first
# version of this arm printed a line only when the pattern matched, and its green probe ran with nothing
# staged, no origin/main and no HEAD^ — so the arm never ran at all.
watch_probes=0
watch_said() { watch_probes=$((watch_probes + 1)); report "$@"; }
watch_skip() { watch_probes=$((watch_probes + 1)); skipping "$1" "$2"; }   # counted, not shed
mkdir -p .agents/selftest-fixture && printf 'skill: probe\n' > .agents/selftest-fixture/SKILL.md
printf 'w = 1\n' > watch_me.py
if stage watch_me.py; then
  run_gate
  watch_said "an ordinary staged diff: the arm passes, it does not warn" \
    "PASS  vendored instructions (.agents/** untouched)" \
    "$(grep 'vendored instructions' "$log" | sed 's/^ *//')"
else
  watch_skip "stage an ordinary file" "an ordinary staged diff: the arm passes, it does not warn"
fi
if stage .agents/selftest-fixture/SKILL.md; then
  run_gate
  watch_said "the vendored tree changed: the arm names it" "1" "$(grep -c 'WARN.*vendored instructions' "$log")"
  unstage .agents/selftest-fixture/SKILL.md
else
  watch_skip "stage a vendored file" "the vendored tree changed: the arm names it"
fi
# A move *out* of the tree, from a committed file: `git diff` finds renames by default, so a staged
# delete-plus-add reports only the destination and an anchored `^\.agents/` pattern sees nothing (#88).
mv_base=$(git rev-parse HEAD)   # the commit below is dropped again below; no later block sees it
if stage .agents/selftest-fixture/SKILL.md \
   && git commit -q -m "vendored fixture" -- .agents/selftest-fixture/SKILL.md \
   && git rm -q --cached .agents/selftest-fixture/SKILL.md \
   && mv .agents/selftest-fixture/SKILL.md moved_skill.md && git add moved_skill.md; then
  run_gate
  watch_said "a file moved out of the tree: the path it left still counts" "1" \
    "$(grep -c 'WARN.*vendored instructions' "$log")"
  git reset -q --hard "$mv_base"; rm -f moved_skill.md
  mkdir -p .agents/selftest-fixture; printf 'skill: probe\n' > .agents/selftest-fixture/SKILL.md
  stage watch_me.py
else
  watch_skip "move a vendored file" "a file moved out of the tree: the path it left still counts"
fi
# A path git would quote in its own output used to slip past the pattern (#88): the arm reads
# NUL-separated names, which git emits verbatim.
mkdir -p '.agents/q"uote' && printf 'skill: probe\n' > '.agents/q"uote/SKILL.md'
if stage '.agents/q"uote/SKILL.md'; then
  run_gate
  watch_said "a path git would quote: the arm still sees it" "1" "$(grep -c 'WARN.*vendored instructions' "$log")"
  unstage '.agents/q"uote/SKILL.md'
else
  watch_skip "stage a quoted path" "a path git would quote: the arm still sees it"
fi
rm -rf .agents/selftest-fixture '.agents/q"uote'; unstage watch_me.py; rm -f watch_me.py
# The shape a real PR runs: nothing staged, the vendored change sitting in a commit above origin/main. The
# probes above reach the arm through `--cached`, which is not what CI measures (#88).
ci_base=$(git rev-parse HEAD)
mkdir -p .agents/selftest-fixture && printf 'skill: probe\n' > .agents/selftest-fixture/SKILL.md
if git add -A && git commit -q -m "vendored: refresh a skill" \
   && git update-ref refs/remotes/origin/main "$ci_base"; then
  run_gate
  watch_said "a vendored commit above origin/main: the arm names it" "1" \
    "$(grep -c 'WARN.*vendored instructions' "$log")"
  git reset -q --hard "$ci_base"; git update-ref -d refs/remotes/origin/main
else
  watch_skip "commit a vendored file above origin/main" "a vendored commit above origin/main: the arm names it"
fi
rm -rf .agents/selftest-fixture
report "all vendored-path probes ran" "5" "$watch_probes"

# A pin line is rewritten, not deleted, and by a portable edit: the line has to actually change,
# because a no-op edit on the wrong platform would look like a passing probe.

if grep -q '^node = ' mise.toml; then
  awk 'BEGIN{done=0} /^node = /{print "# pin removed"; done=1; next} {print}' mise.toml > mise.toml.tmp \
    && mv mise.toml.tmp mise.toml
  expect red "pin line deleted from mise.toml"
  git checkout -q -- mise.toml
fi

# gitleaks decides what counts as a secret and just is what ran this suite, so a wrong version of
# either must not run quietly (#43). Not a rewritten pin: a pin naming a version that does not exist
# makes the mise shim try to install it, so the red comes from a 404 and the probe tests mise's
# network instead of the gate's comparison (measured: `just --version` inside such a repo exits 1 on
# `GitHub release casey/just@9.9.9 not found`). A shim binary on PATH is offline and deterministic,
# and is what the cargo-deny probe below already does.
pin_probes=0
pin_said() { pin_probes=$((pin_probes + 1)); report "$@"; }
if make shims/gitleaks '#!/bin/sh
echo "gitleaks 8.20.0"
' && make shims/just '#!/bin/sh
echo "just 1.20.0"
' && make shims/lychee '#!/bin/sh
echo "lychee 0.10.0"
' && make shims/vale '#!/bin/sh
echo "vale version 3.20.0"
'; then
  chmod +x shims/gitleaks shims/just shims/lychee shims/vale
  # Not expect_check: it re-runs the gate with the ambient PATH and would report the real binaries.
  PATH="$work/shims:$PATH" bash scripts/check.sh > "$log" 2>&1
  pins=$(grep -E '^  (PASS|FAIL|SKIP|WARN)  toolchain-pins' "$log" | head -1)
  case "$pins" in *FAIL*) pins=red ;; *PASS*) pins=green ;; *) pins=absent ;; esac
  pin_said "four shadowed tools, all refused" "red" "$pins"
  for pin_tool in gitleaks just lychee vale; do
    pin_said "and it names the $pin_tool that ran" "$work/shims/$pin_tool" \
      "$(grep -E "^ +$pin_tool " "$log" | sed -n 's/.*(\(.*\))$/\1/p' | head -1)"
  done
  rm -rf shims
else
  skipping "create shim binaries" "the three extra pin probes"
fi
# Named on the PASS line as well as compared: a check that silently stops looking at a tool is the
# failure mode #43 is about, and only the green side can see that.
pin_probes=$((pin_probes + 1))
expect_check toolchain-pins green "and the green line names the tools it checked" ", gitleaks "
report "the pin probes ran" "6" "$pin_probes"

# #50's second half, pinned as a property rather than a behaviour: two gates racing over one
# machine-wide log produce verdicts neither run earned, and the only cheap defence is to refuse the
# literal. The suite's own scratch path is asserted to resolve inside its run dir in the header.
report "the gate writes only inside its own run" "0" "$(grep -c '/tmp/' scripts/check.sh)"

# The two checks that look at the shipped artifact instead of the sources, both directions.
#
# `serving` runs against a stub binary: a real HTTP server (python's) pointed at a fixture build
# directory. What these probes pin is the check's wiring, not the binary's behaviour, which
# apps/server/tests/serving.rs pins and `cargo test` only started running in CI with #26. The green
# probe is the load-bearing one: a wrong binary path would make the check SKIP quietly forever.
make target/debug/transcodarr '#!/bin/sh
exec python3 -m http.server "${TRANSCODARR_ADDR##*:}" --bind "${TRANSCODARR_ADDR%:*}" --directory apps/web/build
' && chmod +x target/debug/transcodarr
make apps/web/build/index.html '<!doctype html><html>the app</html>
'
if [ -x target/debug/transcodarr ] && [ -f apps/web/build/index.html ]; then
  rm -f apps/web/build/index.html
  expect_check serving red "serving: no index.html for the binary to serve"
  make apps/web/build/index.html '<!doctype html><html>the app</html>
'
  printf '#!/bin/sh\nexit 1\n' > target/debug/transcodarr
  expect_check serving red "serving: the binary exits without ever listening"
  printf '#!/bin/sh\nexec python3 -m http.server "${TRANSCODARR_ADDR##*:}" --bind "${TRANSCODARR_ADDR%%:*}" --directory apps/web/build\n' \
    > target/debug/transcodarr && chmod +x target/debug/transcodarr
  expect_check serving green "serving: a live process and a real build directory"
  rm -f target/debug/transcodarr
  expect_check serving skip "serving: no binary at all, so the check cannot report a pass"
fi
rm -rf target apps/web

# `cargo test` has teeth, both directions. Before #26 the suite was compiled and never run, so a
# failing test reported green: this fixture is exactly what the gate could not see. Kept to a
# no-dependency crate so it costs seconds. Cargo.lock and target go afterwards, because an
# untracked Cargo.lock would trip generated-paths-tracked in every probe that follows.
if make Cargo.toml '[package]
name = "probe"
version = "0.0.0"
edition = "2021"
license = "MIT"
' && make src/lib.rs '#[cfg(test)]
mod probes {
    #[test]
    fn red_fixture() {
        let n = std::hint::black_box(1);
        assert_eq!(n, 2, "fixture: a failing test the gate must run and report");
    }
}
'; then
  # The gate's own `cargo build` creates a Cargo.lock, and an untracked one trips
  # generated-paths-tracked inside these very probes, which is how a green probe came to sit on a red
  # line nobody read. Make it first, stage it, and it is tracked for the whole fixture.
  cargo generate-lockfile >/dev/null 2>&1 && stage Cargo.toml Cargo.lock src/lib.rs
  expect_check cargo red "a Rust test that fails"
  printf '#[cfg(test)]\nmod probes {\n    #[test]\n    fn green_fixture() {\n        let n = std::hint::black_box(1);\n        assert_eq!(n, 1);\n    }\n}\n' > src/lib.rs
  expect_check cargo green "the same crate's test passing"
  # Not hygiene for its own sake: this is what makes the staging above fail loudly instead of silently
  # putting a red line nobody reads into the log of a probe that passed.
  expect_check generated-paths-tracked green "no unrelated red inside the cargo fixture"
  unstage Cargo.toml Cargo.lock src/lib.rs
fi
rm -rf Cargo.toml Cargo.lock src target

# `web build` gets no fixture, and says so rather than going unmentioned like gitleaks did. It
# needs apps/web with node_modules installed, and a suite that runs `npm ci` is a suite that cannot
# run before CI has installed anything. Proven by hand on the branch that added it; the measured
# commands and their output are in that PR body.
skipping "needs the installed frontend; proven by hand" "web build"

# --- The pinned cargo-deny, and the gate branch that reads its config (#12, #13, #14) -----------------
# Three things were unproven. The version doing the checking was never compared against mise.toml (#13).
# The config it reads was never loaded by that binary (#12). And the gate's own `deny` line had no
# fixture, so `elif false` there reported green (#12 again).
#
# The shipped `deny.toml` is proven two ways. The gate proves it over the real graph in CI, and since #14
# that run fails if the allow list and the graph disagree in either direction. A fixture can also load it
# directly and offline: `check licenses` reads manifests from the registry cache and never touches the
# advisory database (measured with the network cut: `check licenses` exits 0, a full `check` exits 1 on
# `failed to fetch advisory database`, warm cache included).
#
# Network, stated once because the two `deny`-branch probes cannot avoid it: the gate runs a full
# `cargo deny check`, which updates the RustSec advisory database on every run, so the suite needs
# network whenever a `Cargo.lock` is in play. `just check` runs before this in CI and has already cloned
# the database by then; each probe's update measures 0.27 s.
deny_probes=0
expect_deny() { deny_probes=$((deny_probes + 1)); expect_check "$@"; }
# Every assertion here is counted, including the ones that call the binary directly rather than the gate.
# A fixture that disappears looks exactly like a fixture that passed.
deny_said() { deny_probes=$((deny_probes + 1)); report "$@"; }
# config dir -> green|red, with the binary's own output left in $log for the reason checks below.
deny_verdict() { ( cd "$2" && cargo-deny --config "$1" check licenses ) >"$log" 2>&1 && echo green || echo red; }
expect_deny "toolchain-pins" green "the installed cargo-deny matches the pin"
# CARGO_HOME is redirected as well as PATH shadowed, because `cargo deny` prefers $CARGO_HOME/bin over
# PATH. The `bin/cargo-deny` entry is a directory on purpose: cargo falls through it, and `[ -x ]` alone
# is true for a directory, so the diagnostic used to name a file that never ran.
if make shims/cargo-deny '#!/bin/sh
echo "cargo-deny 0.15.1"
'; then
  chmod +x shims/cargo-deny
  mkdir -p nohome/bin/cargo-deny
  CARGO_HOME="$work/nohome" PATH="$work/shims:$PATH" bash scripts/check.sh > "$log" 2>&1
  # Not expect_check: it re-runs the gate with the ambient PATH and would report the real binary.
  pins=$(grep -E '^  (PASS|FAIL|SKIP|WARN)  toolchain-pins' "$log" | head -1)
  case "$pins" in *FAIL*) pins=red ;; *WARN*) pins=warn ;; *SKIP*) pins=skip ;; *PASS*) pins=green ;; *) pins=absent ;; esac
  deny_said "a cargo-deny that disagrees with mise.toml" "red" "$pins"
  # The line says which binary refused. A PATH lookup can name a different file than cargo ran, and a
  # directory is not a file, so both are asserted against the shim this run actually used.
  deny_said "and it names the binary that refused" "$work/shims/cargo-deny" \
    "$(grep -E '^ +cargo-deny ' "$log" | sed -n 's/.*(\(.*\))$/\1/p' | head -1)"
  rm -rf shims nohome
else
  skipping "create a shim binary" "a cargo-deny that disagrees with mise.toml"
fi

# The other side of a pin is a line that stopped being shipped at all, which is what deleting it does.
if grep -q '^"cargo:cargo-deny" = ' mise.toml; then
  grep -v '^"cargo:cargo-deny" = ' mise.toml > mise.toml.tmp && mv mise.toml.tmp mise.toml
  expect_deny "toolchain-pins" red "the cargo-deny pin line is gone from mise.toml"
  git checkout -q -- mise.toml
fi

# The shipped config against the shipped graph, both directions. Green is what the deleted probe used to
# claim without measuring; red is #14's rule, an allow entry no crate declares, and it has to be an error
# rather than a warning, which is the only thing keeping the list equal to the graph.
deny_said "the shipped deny.toml loads under the pinned binary" "green" \
  "$(deny_verdict "$repo/deny.toml" "$repo")"
if awk '{print} /^allow = \[$/{print "    \"0BSD\",  # fixture: an entry no crate declares"}' \
     "$repo/deny.toml" > deny-shipped-plus.toml; then
  got=$(deny_verdict "$work/deny-shipped-plus.toml" "$repo")
  { [ "$got" = red ] && grep -q 'error\[license-not-encountered\]' "$log"; } || got="$got for another reason: $(grep -oE '^error\[[a-z-]+\]' "$log" | head -1)"
  deny_said "an unused allow entry is an error, shipped config" "red" "$got"
fi

# The `deny` line, both directions, with a graph of one local crate. Red is the key class behind #12:
# older cargo-deny read `copyleft`, the pinned one refuses it. Cargo.lock is staged because an untracked
# one trips generated-paths-tracked, which would bury the probe in someone else's failure.
if make Cargo.toml '[package]
name = "denyfixture"
version = "0.0.0"
edition = "2021"
license = "MIT"
' && make src/lib.rs 'pub fn f() {}
' && make Cargo.lock 'version = 3

[[package]]
name = "denyfixture"
version = "0.0.0"
'; then
  stage Cargo.toml Cargo.lock src/lib.rs
  expect_deny "deny" green "cargo deny accepts a graph the config matches"
  # Same rule, seeded config: deleting `unused-allowed-license = "deny"` from the fixture's own policy
  # would otherwise be caught by nobody, because every other probe here passes either way.
  sed 's/^allow = \["MIT"\]$/allow = ["MIT", "0BSD"]/' deny.toml > deny-seeded-plus.toml
  got=$(deny_verdict "$work/deny-seeded-plus.toml" "$work")
  { [ "$got" = red ] && grep -q 'error\[license-not-encountered\]' "$log"; } || got="$got for another reason: $(grep -oE '^error\[[a-z-]+\]' "$log" | head -1)"
  deny_said "an unused allow entry is an error, seeded config" "red" "$got"
  printf 'copyleft = "warn"\n' >> deny.toml
  expect_deny "deny" red "a key the pinned binary refuses"
  # The code alone is not enough: a removed key in `[advisories]` prints the same `error[deprecated]`,
  # so the assertion demands the key name too, which the binary echoes from the offending line.
  got=$(grep -oE 'error\[deprecated\]|copyleft' "$log" | sort -u | paste -sd' ')
  deny_said "and that red names the [licenses] key" "copyleft error[deprecated]" "$got"
  unstage Cargo.toml Cargo.lock src/lib.rs
  git checkout -q -- deny.toml
fi
report "all deny probes ran" "10" "$deny_probes"
rm -f Cargo.toml Cargo.lock src/lib.rs deny-shipped-plus.toml deny-seeded-plus.toml
rm -rf src target

# --- Which diff a budget number describes (#16) ---------------------------------------------------
# With nothing staged and HEAD at origin/main's tip the dispatch falls through to HEAD^..HEAD, so a
# local run printed the commit below HEAD as if it were the task's budget: `WARN diff budget (118
# lines, 1 files ...)` for a commit that had already merged. The gate's own `scope` array is what it
# measured, so each arm is pinned by that value *and* by the numbers it produced. A hand-written
# label survives an arm that measures nothing and cannot be read off the WARN and FAIL lines at all;
# a range and its numbers cannot. `scope_probes` is asserted because a fixture that never ran looks
# exactly like a fixture that passed.
budget_scope() {
  # The last bracket group on a budget line, and only on one: a `.*\[.*\]` here picks up
  # `[staged and history]` from the gitleaks line two lines above and reports that as the scope.
  grep -E 'diff budget|lines across' "$log" \
    | sed -n 's/.*) \[\([^]]*\)\]$/\1/p;s/.*files \[\([^]]*\)\];.*/\1/p' | head -1
}
budget_numbers() {
  # Both budget line shapes, each pattern anchored to its own punctuation and none of them greedy:
  # `.*` before `lines across` let one pattern match inside the PASS line and return "1 1" for it.
  grep -E 'diff budget|lines across' "$log" \
    | sed -n 's/.*(\([0-9][0-9]*\) lines, \([0-9][0-9]*\) files.*/\1 \2/p
              s/ *\([0-9][0-9]*\) lines across \([0-9][0-9]*\) files.*/\1 \2/p' | head -1
}
scope_probes=0
scope_report() { scope_probes=$((scope_probes + 1)); report "$3" "$1" "$2"; }   # want got name
scope_base=$(git rev-parse HEAD)   # the tail resets here, so no probe of mine is in anyone's baseline
printf 'a = 1\nb = 2\n' > scope_a.py                                        # 2 added lines
printf 'c = 1\nd = 2\ne = 3\nf = 4\ng = 5\n' > scope_b.py                   # 5 added lines
printf 'h = 1\n' > scope_c.py
if stage scope_c.py; then
  run_gate
  scope_report "--cached" "$(budget_scope)" "staged: the scope measured"
  unstage scope_c.py && rm -f scope_c.py
else
  skipping "stage a probe file" "staged: the scope measured"
fi
if stage scope_a.py && git commit -q -m scope_a -- scope_a.py \
   && stage scope_b.py && git commit -q -m scope_b -- scope_b.py; then
  run_gate
  scope_report "HEAD^..HEAD" "$(budget_scope)" "no origin: scope"
  scope_report "5 1" "$(budget_numbers)" "no origin: last commit only"
  # Issue #16's literal state: origin/main resolves and equals HEAD, so arm 2 declines it.
  if git update-ref refs/remotes/origin/main HEAD; then
    run_gate
    scope_report "HEAD^..HEAD" "$(budget_scope)" "origin/main == HEAD: scope"
    scope_report "5 1" "$(budget_numbers)" "origin/main == HEAD: numbers"
    git update-ref -d refs/remotes/origin/main
  else
    skipping "origin/main ref" "origin/main == HEAD: scope"
    skipping "origin/main ref" "origin/main == HEAD: numbers"
  fi
  # Two commits ahead: HEAD~2..HEAD is 7 lines across 2 files where HEAD^..HEAD is 5 across 1, so no
  # arm can quietly measure the other and still print its scope.
  if git update-ref refs/remotes/origin/main "$scope_base"; then
    run_gate
    scope_report "$(git merge-base HEAD refs/remotes/origin/main)..HEAD" "$(budget_scope)" \
      "branch ahead: scope"
    scope_report "7 2" "$(budget_numbers)" "branch ahead: whole diff"
    git update-ref -d refs/remotes/origin/main
  else
    skipping "origin/main ref" "branch ahead: scope"
    skipping "origin/main ref" "branch ahead: whole diff"
  fi
  # The base the PR is proposed against (#70). Here origin/main sits 2 commits down (7 lines) and
  # origin/pr-base 1 commit down (5 lines): a budget that ignores `$GITHUB_BASE_REF` reports 7 2 here
  # and cannot do it quietly, because the scope it measured is printed next to the numbers.
  if git update-ref refs/remotes/origin/pr-base HEAD~1; then
    export GITHUB_BASE_REF=pr-base
    run_gate
    scope_report "$(git merge-base HEAD refs/remotes/origin/pr-base)..HEAD" "$(budget_scope)" \
      "PR base: the scope names the base"
    scope_report "5 1" "$(budget_numbers)" "PR base: the stacked parent is not charged"
    unset GITHUB_BASE_REF
    git update-ref -d refs/remotes/origin/pr-base
  else
    skipping "pr-base ref" "PR base: the scope names the base"
    skipping "pr-base ref" "PR base: the stacked parent is not charged"
  fi
  # A base CI names and this clone lacks. Red on both arms that need a base, and not one verdict each
  # too many: a skip here is exactly what `GITHUB_BASE_REF=nosuchbranch` would buy, and the budget would
  # then shrink to the last commit and report it as the task's.
  # What this variable does to the gate is pinned here, in a repo the fixture controls. The suite scrubs it
  # at the top, so a fixture never inherits CI's value: an inherited one made the gate name a base no scratch
  # repo has — and the suite went red 7 probes deep in CI before that line (#70). Failing instead of skipping
  # was tried first, and it is also wrong: a clone with no `origin/main` is a normal local clone, not a
  # finding.
  # A base CI named that this clone cannot resolve to a commit: each arm skips with the fact, and neither
  # one quietly measures `HEAD^..HEAD`. A ref that is not a committish at all (here, a tree object) is the
  # same state, so the test the arm uses -- `git merge-base` -- is the only test the fixture needs.
  export GITHUB_BASE_REF=tree
  if git update-ref refs/remotes/origin/tree "$(git hash-object -t tree /dev/null)"; then
    run_gate
    scope_report "0" "$(grep -cE 'FAIL[[:space:]]+(diff budget|trailer)' "$log")" \
      "named base, not a commit: neither arm red"
    scope_report "2" "$(grep -cE 'SKIP[[:space:]]+(diff budget|trailer).*unknowable' "$log")" \
      "and both arms say which base they could not read"
    git update-ref -d refs/remotes/origin/tree
  else
    skipping "tree ref" "named base, not a commit: neither arm red"
    skipping "tree ref" "and both arms say which base they could not read"
  fi
  unset GITHUB_BASE_REF
  # The named base being HEAD's own tip is a third thing, and it used to be reported as the first one:
  # a branch that has been pushed and pointed at itself has an empty task diff, not a root commit.
  if git update-ref refs/remotes/origin/pr-base HEAD; then
    export GITHUB_BASE_REF=pr-base
    run_gate
    scope_report "no commits of ours (HEAD is origin/pr-base's tip)" \
      "$(grep -E 'SKIP.*diff budget' "$log" | sed 's/.*diff budget[^A-Za-z(]*//; s/[[:space:]]*$//')" \
      "base equals HEAD: the reason says so"
    unset GITHUB_BASE_REF
    git update-ref -d refs/remotes/origin/pr-base
  else
    skipping "pr-base ref" "base equals HEAD: the reason says so"
  fi
  # The WARN and FAIL lines carry it too. A reader of the PASS line alone cannot see those two, and
  # that is the fixture shape that let an unlabelled fall-through ship once already.
  python3 -c "print('\n'.join('w%d = %d' % (i, i) for i in range(60)))" > scope_w.py
  if git add scope_w.py && git commit -q -m scope_w -- scope_w.py; then
    run_gate
    scope_report "HEAD^..HEAD" "$(budget_scope)" "WARN line carries the scope"
    python3 -c "print('\n'.join('f%d = %d' % (i, i) for i in range(300)))" > scope_f.py
    if git add scope_f.py && git commit -q -m scope_f -- scope_f.py; then
      run_gate
      scope_report "HEAD^..HEAD" "$(budget_scope)" "FAIL line carries the scope"
    else
      skipping "commit 300 lines" "FAIL line carries the scope"
    fi
  else
    skipping "commit 60 lines" "WARN line carries the scope"
  fi
else
  skipping "two commits in scratch repo" "budget arm fixtures"
fi
git reset -q --hard "$scope_base" 2>/dev/null
rm -f scope_*.py
report "all budget fixtures ran" "14" "$scope_probes"

# --- Trailers on the machine no hook runs on (#49) --------------------------------------------------
# `commit-msg` enforces the shape only where `just setup` ran. Measured before-state: a fresh clone
# of main has `core.hooksPath` unset, `git commit -m wip` exited 0 with no trailers at all, and that
# branch's CI run said `check: 14 ran, 0 failed`. The gate now re-checks the shape over
# the commits above the base with `git interpret-trailers`, so the arm is pinned here in both
# directions and in both skip directions -- a skip is the only way it can go blind silently -- plus
# the two ways grepping the whole message was fooled: prose naming the keys, and a subject that only
# looks like a revert. Scope commits above carry no trailers, so a trailer FAIL inside *their* probes
# is the arm working; these probes read the `trailer` verdict by name.
trail_from=$(git rev-parse HEAD)
trail_probes=0
trail() { trail_probes=$((trail_probes + 1)); expect_check "$@"; }      # check want name [reason]
trail_git() { local n="$1"; shift; git "$@" >/dev/null 2>&1 || { skipping "$n" "$n"; return 1; }; }
trail_ok=$(printf 'fix: provenance that points somewhere\n\nTask: #49\nAgent: pi / selftest fixture\nVerified: just selftest')
trail_bad=$(printf 'fix: provenance that points nowhere\n\nTask: banana\nAgent: pi / selftest fixture\nVerified: just selftest')
trail_prose=$(printf 'fix: provenance written above the block\n\nTask: #49\nAgent: pi / selftest fixture\nVerified: just selftest\n\nThe block above is not the trailer block: git reads the last paragraph, and this sentence is the last paragraph.')
if git update-ref refs/remotes/origin/main "$trail_from"; then
  trail trailer skip "HEAD is origin/main's tip, so nothing is ours" "no commits of ours"
  trail_git "commit an untrailered message" commit -q --allow-empty -m 'wip: no provenance here' \
    && trail trailer red "an untrailered commit is refused" "missing: Task Agent Verified"
  trail_git "amend to Task: banana" commit -q --allow-empty --amend -m "$trail_bad" \
    && trail trailer red "Task: banana asserts nothing" "missing: Task-shape"
  # A trailer-shaped block that is not the last paragraph is not provenance: git reads the last
  # paragraph. Grepping the message passes on this text; reading the block does not, which is the
  # difference between the hook, which greps, and this arm.
  trail_git "amend to prose naming the keys" commit -q --allow-empty --amend -m "$trail_prose" \
    && trail trailer red "trailers above a closing paragraph are not provenance" "missing: Task Agent Verified"
  # `git revert` writes `Revert "subject"`, and that is the exempt shape. A subject that merely
  # begins with the word is an ordinary commit that still owes its three trailers.
  trail_git "amend to a revert-looking subject" commit -q --allow-empty --amend -m 'Revert to the earlier budget rule' \
    && trail trailer red "a revert-looking subject is not exempt" "missing: Task Agent Verified"
  trail_git "amend to the shape the hook accepts" commit -q --allow-empty --amend -m "$trail_ok" \
    && trail trailer green "the shape the hook accepts" "1 commits checked, 0 reverts exempt"
  # Two commits above the base now, one of them the revert shape, and the PASS must say one commit
  # was checked: counting an exempt commit overstated the coverage on a green line.
  trail_git "commit a revert in the shape GitHub writes" commit -q --allow-empty \
    -m 'Revert "fix: provenance that points somewhere"' \
    && trail trailer green "a revert is exempt and not counted" "1 commits checked, 1 reverts exempt"
  git update-ref -d refs/remotes/origin/main \
    && trail trailer skip "no origin/main: a named skip, not a silent pass" "no origin/main"
else
  skipping "origin/main ref" "trailer arm fixtures"
fi
git reset -q --hard "$trail_from" 2>/dev/null
git update-ref -d refs/remotes/origin/main 2>/dev/null
report "all trailer probes ran" "8" "$trail_probes"

# --- Secrets, in both scans and both directions (#37) ---------------------------------------------
# ADR-0001 claims a both-directions fixture for every rule it enforces, and for secret scanning that
# claim was false: the branch at check.sh:112-128 ran on every probe and always said green, so a gate
# whose fail arm was never taken would have shipped as a passing suite. Both scans are pinned because
# they are different code paths — `--staged` reads the index (and prints "0 commits scanned"), the
# unguarded call walks history from HEAD.
#
# The history's shape is the point of this block, so it is asserted rather than assumed. The secret
# lands three commits under the tip and two probes check the depth and that the tip tree is clean of
# the file, because both failure modes a reviewer measured pass while looking right: `--log-opts=-N`
# for any N ≥ 2 sees nothing and every probe still passed with the secret one commit back, and when
# the secret's own commit fails (a held `index.lock` is the plausible one) the next commit lands it at
# the tip, where the label says three back and the printed reason still matches because it names the
# call, not the range. shortcut: depth 3 pins any scan reaching 4+ commits back; a window wider than
# the whole fixture is not distinguished from a full walk, and no narrower mutation is expected.
#
# The config probe exists because these scans read the repo's config and the scratch repo had none, so
# every probe ran on gitleaks' embedded defaults while `just check` runs on whatever config sits in the
# worktree — the #12 class this block's first draft cited and did not fix. An untracked `.gitleaks.toml`
# in the worktree disarms both scans for real secrets and the suite cannot see it. Measured on the
# pinned binary, same tree and same history both ways: no config `leaks found: 1`, with `[allowlist]
# paths` naming this file `no leaks found`. Nothing is tracked at any `*gitleaks*` path (#51, #55).
#
# Measured against pinned gitleaks 8.30.1: this value trips `generic-api-key` (an entropy rule), not an
# AWS one; the AWS docs secret key is allowlisted upstream; `AKIA`+16 is flagged only inside that rule's
# `[A-Z2-7]` base32 class, and wrapped in a name containing `aws_secret` even an out-of-class value comes
# back flagged by the entropy rule. The value is split across four quoted runs so this file is not
# itself a finding. No `probe.env`: `.gitignore` ignores `*.env` by design (ADR-0002), and an ignored
# path fails the red probes loudly — that is the signal, so no guard skips quietly.
gl_probes=0
expect_gl() { # check want label [the reason the gate must print]
  expect_check "$1" "$2" "$3"; gl_probes=$((gl_probes + 1))
  if [ -n "${4:-}" ]; then
    if grep -qF "$4" "$log"; then printf '  ok    %-42s %s\n' "and names it: $4" "red"
    else printf '  BROKE %-42s wanted the gate to print it\n' "and names it: $4"; bad=$((bad + 1)); fi
    gl_probes=$((gl_probes + 1)); probes=$((probes + 1))
  fi
}

# Evidence, asserted as a shape rather than a rule name: the rule that fires on this fixture is the
# entropy rule, whose id is gitleaks' to change, and #51 is about there being any finding line at all.
expect_gl_finding() {
  gl_probes=$((gl_probes + 1)); probes=$((probes + 1))
  if grep -qE "$2" "$log"; then printf '  ok    %-42s %s\n' "$1" "red"
  else printf '  BROKE %-42s the gate printed no such line\n' "$1"; bad=$((bad + 1)); fi
}
expect_shape() { gl_probes=$((gl_probes + 1)); report "$1" "$2" "$3"; }
gl_base=$(git rev-parse HEAD) # the reset below returns here, so no probe of mine is in anyone's baseline
gl_secret_value="hR3nQpLd""Kf8sVtYb""NwCzXmJq""AeGdUiOo"
if make probe_aws_config.toml "aws_secret_access_key = \"$gl_secret_value\""; then
  stage probe_aws_config.toml
  expect_gl gitleaks red "a secret sitting in the index" "staged changes contain a secret"
  expect_gl_finding "the red names the finding" "finding: "
  expect_gl_finding "and names the file it found" "finding: .*probe_aws_config.toml"
  # A config that will not parse also exits 1, so this is the case where the gate used to say
  # "contains a secret" and print nothing. It must name the config, and blame no secret.
  printf '[extend]\nuseDefault = "yes"\n' > .gitleaks.toml
  expect_gl gitleaks red "a config that will not load" "does not load, so no verdict about secrets is possible"
  gl_probes=$((gl_probes + 1)); probes=$((probes + 1))
  if grep -q 'contain a secret' "$log"; then
    printf '  BROKE %-42s it called a broken config a secret\n' "and blames no secret"; bad=$((bad + 1))
  else printf '  ok    %-42s %s\n' "and blames no secret" "clean"; fi
  git checkout -q -- .gitleaks.toml
  unstage probe_aws_config.toml
  expect_gl gitleaks green "the same bytes, present but not staged"
  stage probe_aws_config.toml
  git commit -q -m "fixture: a secret that has landed"
  for gl_n in 1 2 3; do
    printf 'clean\n' > "gl_after$gl_n.txt" && stage "gl_after$gl_n.txt" && git commit -q -m "clean commit $gl_n"
  done
  expect_shape "the secret sits 3 commits under the tip" "3" \
    "$(git rev-list --count "$(git log -1 --format=%H -- probe_aws_config.toml)"..HEAD)"
  expect_shape "and the tip tree does not hold it" "0" \
    "$(git show --name-only --format= HEAD | grep -c probe_aws_config.toml)"
  expect_gl gitleaks red "a secret three commits back in history" "history contains a secret"
  expect_gl_finding "the history red names its finding" "finding: .*probe_aws_config.toml"
  # Widening the ruleset has to move the verdict, which is what proves the gate reads this file and
  # not whatever gitleaks happens to auto-load. It is also the hazard #55 records: an allowlist entry
  # is a silent disarm, so the file is a FLAG path and this probe is its witness.
  printf "[extend]\nuseDefault = true\n\n[allowlist]\npaths = ['''probe_aws_config.toml''']\n" > .gitleaks.toml
  expect_gl gitleaks green "the same history, allowed by the config the gate reads"
  git checkout -q -- .gitleaks.toml
  # The flag, not just the file: gitleaks auto-loads a root `.gitleaks.toml` either way, so only an
  # out-of-repo config can show that `--config` is what decides. Without the flag gitleaks reads
  # $GITLEAKS_CONFIG, this file wins, and a landed secret turns the gate green.
  printf "[extend]\nuseDefault = true\n\n[allowlist]\npaths = ['''probe_aws_config.toml''']\n" > env-config.toml
  export GITLEAKS_CONFIG="$PWD/env-config.toml"
  expect_gl gitleaks red "a landed secret, with an env config that allows it" "history contains a secret"
  unset GITLEAKS_CONFIG
  rm -f env-config.toml
  # The other direction of the same wiring: a missing ruleset is a failure, not a return to defaults.
  rm -f .gitleaks.toml
  expect_gl gitleaks red "the ruleset file is missing" "is missing — the enforced ruleset has to be a tracked file"
  git checkout -q -- .gitleaks.toml
  git reset -q --hard "$gl_base"
  expect_gl gitleaks green "the commits gone, the scan clean again"
else
  skipping "make probe_aws_config.toml" "gitleaks assertions"
fi
# Outside the guard above on purpose: a block that never runs reports 0 against 9 and breaks the run,
# which is what keeps a precondition from shedding these probes silently.
report "all gitleaks assertions ran" "19" "$gl_probes"

# The suite's own attendance sheet. Every probe outcome bumps `probes`, so a probe whose fixture never ran
# makes the total short and the run red. The per-block counters cannot see that: a block that never runs
# reports exactly the number its neighbours expect (#72, mutant (i) — delete an uncounted probe, every
# counter still prints its number). Bump this when adding or removing a probe. The pin is an equality, so
# it catches probes deleted and probes moved into a subshell; it does not catch a *new* assertion that
# forgets to bump — that is free against an equality, and closing it needs a per-printer census.
# Measured: the run prints one probe line more than this number, because `restored to baseline` below runs
# after the comparison reads `probes` — an ordering artefact, not an uncounted printer.
probes=$((probes + 1))
if [ "$probes" = "128" ]; then printf '  ok    %-42s %s\n' "every probe printed a line" "$probes"
else printf '  BROKE %-42s wanted %s, got %s\n' "every probe printed a line" "128" "$probes"
  bad=$((bad + 1)); fi
# A shed probe and a legitimate skip look identical on the last line, and nothing bounded `skipped`: a
# fixture whose precondition fails used to turn `1 skipped` into `2 skipped` with the run still green.
# One probe may skip on a provisioned machine — `web build`, which no fixture can provision.
report "only the un-provisionable fixture skipped" "1" "$skipped"

run_gate; report "restored to baseline" "$base" "$(verdict)"
printf 'selftest: %d broken, %d skipped\n' "$bad" "$skipped"
[ "$bad" -eq 0 ]
