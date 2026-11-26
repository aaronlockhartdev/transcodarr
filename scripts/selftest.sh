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
# A scratch recipe list rather than the real one, deliberately: `gen` has to be absent so check.sh
# skips drift in here. Running the real generator would need the installed frontend, and a suite
# that requires an install is a suite that cannot run before CI has installed anything. Drift's
# behaviour is proven by hand instead: hand-edit a generated file, stage it, `just check` goes red;
# regenerate and stage, it goes green.
cat > "$work/justfile" <<'JUSTFILE'
# Seeded by scripts/selftest.sh. Not the real recipe list, on purpose: no `gen` recipe, so the drift
# check skips instead of shelling out to a toolchain that is not installed in here.
JUSTFILE
cp "$repo/mise.toml" "$repo/rust-toolchain.toml" "$work/"
cp -r "$repo/docs" "$work/docs"
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

bad=0; skipped=0
# Inside $work, so the path is per-run and dies with the run. A fixed path here is machine-wide: two
# suites on one box — the normal shape of this project, one worktree per unit — resolve their
# assertions against each other's gate output. Measured on main with two runs in flight: 4 and 8 fake
# BROKEs, one reading `wanted /tmp/tmp.V2mNtTrkvB/shims/cargo-deny, gate said /tmp/tmp.oyNWJj75vN`,
# which is another process's scratch tree answering this process's question. Nothing is lost for
# post-mortems: `report` below already echoes the log's FAIL and WARN lines into the BROKE output.
log="$work/selftest-gate.log"
run_gate() { bash scripts/check.sh > "$log" 2>&1; }
verdict() { local g=green; grep -q 'FAIL' "$log" && g=red; [ "$g" = green ] && grep -q 'WARN' "$log" && g=warn; printf '%s' "$g"; }
report() { # name want got
  if [ "$3" = "$2" ]; then printf '  ok    %-42s %s\n' "$1" "$3"
  else printf '  BROKE %-42s wanted %s, gate said %s\n' "$1" "$2" "$3"
    grep -E 'FAIL|WARN' "$log" | sed 's/^/          /'; bad=$((bad + 1)); fi
}
expect() { run_gate; report "$2" "$1" "$(verdict)"; }
skipping() { printf '  skip  %-42s %s\n' "$2" "$1"; skipped=$((skipped + 1)); }

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

# The exclusions, measured rather than trusted: 2,040 generated lines plus one 40-line source
# file must report 40 lines across 1 file.
gen_ok=1
for p in contracts/openapi.json apps/web/src/lib/api/schema.ts .sqlx/q.json; do
  make "$p" "$(python3 -c "print('\n'.join('x' * 60 for _ in range(510)))")" || gen_ok=0
done
make apps/web/package-lock.json "$(python3 -c "print('\n'.join('  \"k%d\": {}' % i for i in range(510)))")" || gen_ok=0
if [ "$gen_ok" = 1 ] && make small.py "$(python3 -c "print('\n'.join('c%d = %d' % (i, i) for i in range(40)))")"; then
  stage contracts apps/web .sqlx small.py
  run_gate
  got=$(sed -n 's/.*diff budget (\([0-9]*\) lines, \([0-9]*\) files.*/\1 \2/p' "$log")
  report "generated and lockfile output excluded" "40 1" "$got"
  unstage contracts apps/web .sqlx small.py
  rm -f contracts/openapi.json apps/web/src/lib/api/schema.ts apps/web/package-lock.json \
        .sqlx/q.json small.py
  prunedirs
else
  skipping "path exists" "generated and lockfile output excluded"
fi


# The .md gates, both directions. Each line below is a live misfire found by auditing the corpus
# checks against a scratch tree: a mistyped filename was invisible to every ADR check, and an index
# row that named no file made the README-to-file direction check nothing. The status vocabulary, the
# required headings and the supersession lifecycle are pinned here too, in both directions. The plan to move
# them into a Vale config was deferred and never ran, so nothing else enforces them but these probes.
adr_body() { printf 'Status: %s\n\n# T\n\n## Context\n\nb\n\n## Decision\n\nd\n\n## Rejected alternatives\n\nr\n\n## Unenforced\n\nz\n' "$1"; }
expect_check() { run_gate; # check want name
  local line got
  line=$(grep -E "^  (PASS|FAIL|SKIP|WARN)  $1" "$log" | head -1)
  case "$line" in
    *FAIL*) got=red ;; *WARN*) got=warn ;; *SKIP*) got=skip ;; *PASS*) got=green ;; *) got=absent ;;
  esac
  # 4th arg, optional: a string the gate must have printed. A verdict alone can be satisfied by any
  # unrelated failure inside the same check, which is how a probe passes while proving nothing.
  if [ "$got" = "$2" ] && { [ -z "${4:-}" ] || grep -qF "$4" "$log"; }; then printf '  ok    %-42s %s\n' "$3" "$got"
  else printf '  BROKE %-42s wanted %s, gate said %s\n' "$3" "$2" "$got"; bad=$((bad + 1)); fi
}

if make docs/adr/000-misnumbered.md "$(adr_body proposed)"; then
  expect_check adr-structure red "ADR filename no glob can see"
  rm -f docs/adr/000-misnumbered.md
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
  # The vocabulary is an enum, so a value that merely starts with a legal word is not legal. Main
  # matched the prefix, and `proposedly` and `proposed-legacy` are the values #3 names by name.
  for bad_status in retired proposedly proposed-legacy; do
    printf '%s' "$(adr_body "$bad_status")" > docs/adr/9992-old.md
    expect_sup adr-structure red "Status '$bad_status' is not a state" "begins with 'proposed'"
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
  expect_sup adr-structure red "a record hiding which rules are unenforced" \
    "missing section '## Unenforced'"
  rm -f docs/adr/9991-new.md docs/adr/9992-old.md docs/adr/9993-prose.md
fi
report "all supersession probes ran" "10" "$sup_probes"

# The rule that a gate edit must ship a fixture, tested against itself.
printf '\n# probe\n' >> scripts/check.sh
stage scripts/check.sh
expect_check gate-proof red "check.sh changed with no selftest.sh change"
unstage scripts/check.sh; git checkout -q -- scripts/check.sh

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
'; then
  chmod +x shims/gitleaks shims/just
  # Not expect_check: it re-runs the gate with the ambient PATH and would report the real binaries.
  PATH="$work/shims:$PATH" bash scripts/check.sh > "$log" 2>&1
  pins=$(grep -E '^  (PASS|FAIL|SKIP|WARN)  toolchain-pins' "$log" | head -1)
  case "$pins" in *FAIL*) pins=red ;; *PASS*) pins=green ;; *) pins=absent ;; esac
  pin_said "two shadowed tools, both refused" "red" "$pins"
  for pin_tool in gitleaks just; do
    pin_said "and it names the $pin_tool that ran" "$work/shims/$pin_tool" \
      "$(grep -E "^ +$pin_tool " "$log" | sed -n 's/.*(\(.*\))$/\1/p' | head -1)"
  done
  rm -rf shims
else
  skipping "create shim binaries" "the two extra pin probes"
fi
# Named on the PASS line as well as compared: a check that silently stops looking at a tool is the
# failure mode #43 is about, and only the green side can see that.
pin_probes=$((pin_probes + 1))
expect_check toolchain-pins green "and the green line names the tools it checked" ", gitleaks "
report "the pin probes ran" "4" "$pin_probes"

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
report "all budget fixtures ran" "9" "$scope_probes"

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
    gl_probes=$((gl_probes + 1))
  fi
}

# Evidence, asserted as a shape rather than a rule name: the rule that fires on this fixture is the
# entropy rule, whose id is gitleaks' to change, and #51 is about there being any finding line at all.
expect_gl_finding() {
  gl_probes=$((gl_probes + 1))
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
  gl_probes=$((gl_probes + 1))
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

run_gate; report "restored to baseline" "$base" "$(verdict)"
printf 'selftest: %d broken, %d skipped\n' "$bad" "$skipped"
[ "$bad" -eq 0 ]
