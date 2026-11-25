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

run_gate; report "restored to baseline" "$base" "$(verdict)"
printf 'selftest: %d broken, %d skipped\n' "$bad" "$skipped"
[ "$bad" -eq 0 ]
