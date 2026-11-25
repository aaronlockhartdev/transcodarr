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
log=/tmp/selftest-gate.log
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
' && make src/lib.rs '#[cfg(test)]
mod probes {
    #[test]
    fn red_fixture() {
        let n = std::hint::black_box(1);
        assert_eq!(n, 2, "fixture: a failing test the gate must run and report");
    }
}
'; then
  expect_check cargo red "a Rust test that fails"
  printf '#[cfg(test)]\nmod probes {\n    #[test]\n    fn green_fixture() {\n        let n = std::hint::black_box(1);\n        assert_eq!(n, 1);\n    }\n}\n' > src/lib.rs
  expect_check cargo green "the same crate's test passing"
fi
rm -rf Cargo.toml Cargo.lock src target

# `web build` gets no fixture, and says so rather than going unmentioned like gitleaks did. It
# needs apps/web with node_modules installed, and a suite that runs `npm ci` is a suite that cannot
# run before CI has installed anything. Proven by hand on the branch that added it; the measured
# commands and their output are in that PR body.
skipping "needs the installed frontend; proven by hand" "web build"

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

run_gate; report "restored to baseline" "$base" "$(verdict)"
printf 'selftest: %d broken, %d skipped\n' "$bad" "$skipped"
[ "$bad" -eq 0 ]
