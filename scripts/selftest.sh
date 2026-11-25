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
# row that named no file made the README-to-file direction check nothing. The status vocabulary and
# the required-section list are deliberately absent here, because the Vale unit lands them as config
# and carries their fixtures instead.
adr_body() { printf 'Status: %s\n\n# T\n\n## Context\n\nb\n\n## Decision\n\nd\n\n## Rejected alternatives\n\nr\n\n## Evidence\n\ne\n\n## Binding summary for AGENTS.md\n\nz\n' "$1"; }
expect_check() { run_gate; # check want name
  local line got
  line=$(grep -E "^  (PASS|FAIL|SKIP|WARN)  $1" "$log" | head -1)
  case "$line" in
    *FAIL*) got=red ;; *WARN*) got=warn ;; *SKIP*) got=skip ;; *PASS*) got=green ;; *) got=absent ;;
  esac
  if [ "$got" = "$2" ]; then printf '  ok    %-42s %s\n' "$3" "$got"
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
# because the real corpus's longest file is 198. A fixture 32 lines over the bar pins nothing.
if make docs/adr/9995-long.md "$({ adr_body proposed; awk 'BEGIN{for(i=0;i<250;i++)print "filler " i}'; } | head -202)"; then
  expect_check adr-length warn "an ADR past the 200-line cap"
  rm -f docs/adr/9995-long.md
fi
expect_check adr-length green "the corpus inside the cap"


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
# The history probe commits a second, clean commit on purpose. With the secret as the tip commit,
# "the last commit" and "all of history" are the same question: `gl_cmd --log-opts=-1` then passes a
# suite whose history scan sees nothing. Measured on a repo holding the secret two commits back: full
# history 1 finding, `--log-opts=-1` 0 findings.
#
# Each red probe also asserts the sentence the gate prints, because a verdict word alone cannot say
# which scan fired: with the fixture's own commit failing, both red probes stay satisfied by the
# staged scan and the history probe measures nothing. Measured against pinned gitleaks 8.30.1: this
# value trips `generic-api-key` (an entropy rule), not an AWS one; the AWS docs secret key is
# allowlisted upstream; and an AWS access-key-id is flagged only inside the rule's `[A-Z2-7]` base32
# class, which is why two plausible AKIA values here measure clean. The value is split across four
# quoted runs so this file is not itself a finding. No `probe.env`: `.gitignore` ignores `*.env` by
# design (ADR-0002), and an ignored path fails the two red probes loudly — that is the signal, so
# there is no guard to skip quietly instead of asserting.
gl_probes=0
expect_gl() { # check want label [the reason the gate must print]
  expect_check "$1" "$2" "$3"; gl_probes=$((gl_probes + 1))
  if [ -n "${4:-}" ]; then
    if grep -qF "$4" "$log"; then printf '  ok    %-42s %s\n' "and names it: $4" "red"
    else printf '  BROKE %-42s wanted the gate to print it\n' "and names it: $4"; bad=$((bad + 1)); fi
    gl_probes=$((gl_probes + 1))
  fi
}
gl_base=$(git rev-parse HEAD) # the reset below returns here, so no probe of mine is in anyone's baseline
gl_secret_value="hR3nQpLd""Kf8sVtYb""NwCzXmJq""AeGdUiOo"
if make probe_aws_config.toml "aws_secret_access_key = \"$gl_secret_value\""; then
  stage probe_aws_config.toml
  expect_gl gitleaks red "a secret sitting in the index" "staged changes contain a secret"
  unstage probe_aws_config.toml
  expect_gl gitleaks green "the same bytes, present but not staged"
  stage probe_aws_config.toml
  git commit -q -m "fixture: a secret that has landed"
  printf 'clean\n' > gl_after.txt && stage gl_after.txt && git commit -q -m "a clean commit after the secret"
  expect_gl gitleaks red "a secret two commits back in history" "history contains a secret"
  git reset -q --hard "$gl_base"
  expect_gl gitleaks green "the commits gone, the scan clean again"
else
  skipping "make probe_aws_config.toml" "gitleaks assertions"
fi
# Outside the guard above on purpose: a block that never runs reports 0 against 6 and breaks the run,
# which is what keeps a precondition from shedding these probes silently.
report "all gitleaks assertions ran" "6" "$gl_probes"

run_gate; report "restored to baseline" "$base" "$(verdict)"
printf 'selftest: %d broken, %d skipped\n' "$bad" "$skipped"
[ "$bad" -eq 0 ]
