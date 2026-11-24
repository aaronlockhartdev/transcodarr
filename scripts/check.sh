#!/usr/bin/env bash
# The whole gate (ADR-0001). The same script runs in `just check`, in the pre-commit hook, and
# in CI. A check either runs and can fail, or prints SKIP with its reason. Nothing that did not
# run counts as passing, and green with zero checks run is itself a failure.
set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "check.sh: not a git repository"; exit 1; }
cd "$root" || exit 1

ran=0 skipped=0 failed=0 warned=0
pass() { printf '  PASS  %s\n' "$1"; ran=$((ran + 1)); }
skip() { printf '  SKIP  %s — %s\n' "$1" "$2"; skipped=$((skipped + 1)); }
fail() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }
warn() { printf '  WARN  %s\n' "$1"; warned=$((warned + 1)); }

printf 'check.sh  %s\n' "$(git rev-parse --short HEAD 2>/dev/null || echo 'no commits')"

# --- ADR corpus: shape, status, and the one-decision-written-twice rule ----------------------
# Scope is every .md in docs/adr except the index. A four-digit glob made a mistyped name
# (001-x.md, adr-0006-x.md) invisible to every ADR check, and an empty glob reported PASS.
adr_files=()
while IFS= read -r f; do adr_files+=("$f"); done \
  < <(find docs/adr -maxdepth 1 -name '*.md' ! -name 'README.md' 2>/dev/null | sort)
adr_bad=0
if [ ${#adr_files[@]} -eq 0 ]; then
  echo "        docs/adr holds no ADR files: a green here would mean nothing"; adr_bad=1
fi
for f in "${adr_files[@]}"; do
  case ${f##*/} in
    [0-9][0-9][0-9][0-9]-[a-z0-9]*.md) ;;
    *) echo "        $f: name is not NNNN-title.md, so no ADR check can see it"; adr_bad=1 ;;
  esac
  num=${f##*/}
  num=${num%%-*}
  grep -q '^Status: \(proposed\|accepted\)' "$f" || { echo "        $f: no 'Status: proposed' or 'Status: accepted' line"; adr_bad=1; }
  for h in '## Context' '## Decision' '## Rejected alternatives' '## Evidence' '## Binding summary for AGENTS.md'; do
    grep -qx "$h" "$f" || { echo "        $f: missing section '$h'"; adr_bad=1; }
  done
  # README Format: adding ADR-NNNN while superseding NNNN in one diff is one decision twice.
  # shortcut: prose that spells the pattern verbatim would trip this; narrow it to the file's own
  # Status/Decision block if that ever happens.
  if grep -qi "superseded by ADR-$num\b" "$f"; then
    echo "        $f: declares itself superseded by its own number"; adr_bad=1
  fi
done
if [ "$adr_bad" -eq 0 ]; then pass "adr-structure"; else fail "adr-structure"; fi

# --- ADR index: every ADR listed, every listing real -----------------------------------------
# The measured failure: 167 ADRs, an index that showed 30, and 29 live decisions invisible.
idx_bad=0
# Key on the filename, not the bare number. The old test matched any prose mention of "ADR-0002",
# and index rows written without a filename left the README-to-file direction checking nothing.
for f in "${adr_files[@]}"; do
  fname=${f##*/}
  grep -q -- "$fname" docs/adr/README.md || { echo "        README index does not name $fname"; idx_bad=1; }
done
for ref in $(grep -oE '[0-9]{4}-[a-z0-9-]+\.md' docs/adr/README.md | sort -u); do
  [ -e "docs/adr/$ref" ] || { echo "        README index points at docs/adr/$ref, which does not exist"; idx_bad=1; }
done
if [ "$idx_bad" -eq 0 ]; then pass "adr-index"; else fail "adr-index"; fi

# --- Toolchain pins: a shadowed binary must not look like success ----------------------------
pin_bad=0
want_node=$(sed -n 's/^node *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_node=$(node --version 2>/dev/null | tr -d v)
if [ -z "$want_node" ]; then echo "        mise.toml has no node = \"…\" pin to compare against"; pin_bad=1
elif [ "$want_node" != "$got_node" ]; then echo "        node $got_node, mise.toml pins $want_node ($(command -v node))"; pin_bad=1; fi
want_rust=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -1)
got_rust=$(rustc --version 2>/dev/null | awk '{print $2}')
if [ -z "$want_rust" ]; then echo "        rust-toolchain.toml has no channel = \"…\" pin to compare against"; pin_bad=1
elif [ "$want_rust" != "$got_rust" ]; then echo "        rustc $got_rust, rust-toolchain.toml pins $want_rust ($(command -v rustc))"; pin_bad=1; fi
if [ "$pin_bad" -eq 0 ]; then pass "toolchain-pins (node $got_node, rustc $got_rust)"; else fail "toolchain-pins"; fi

# --- Generated and cache files must stay tracked --------------------------------------------
# If one goes untracked, the drift gate can never fire again and reports empty diffs forever.
ign_bad=0
for p in contracts/openapi.json apps/web/src/lib/api/schema.ts Cargo.lock .sqlx; do
  if git check-ignore -q "$p" 2>/dev/null; then
    echo "        $p is gitignored — the drift gate and sqlx offline checks cannot function"; ign_bad=1
  elif [ -e "$p" ] && ! git ls-files --error-unmatch -- "$p" >/dev/null 2>&1; then
    echo "        $p is on disk but untracked — the drift gate would compare nothing"; ign_bad=1
  fi
done
if [ "$ign_bad" -eq 0 ]; then pass "generated-paths-tracked"; else fail "generated-paths-tracked"; fi

# --- Secrets ---------------------------------------------------------------------------------
# `--staged` is what is about to land (and the only scan possible before the first commit);
# the history scan needs a HEAD. 8.30 renamed `detect` to `git`.
gl_cmd() { gitleaks git "$@" --redact --exit-code 1 >>/tmp/check-gitleaks.log 2>&1; }
if ! command -v gitleaks >/dev/null 2>&1; then
  skip gitleaks "gitleaks not on PATH — run mise install"
else
  : >/tmp/check-gitleaks.log
  gl_reason=""
  gl_cmd --staged || gl_reason="staged changes contain a secret"
  if git rev-parse -q --verify HEAD >/dev/null 2>&1; then
    gl_cmd || gl_reason="${gl_reason:+$gl_reason; }history contains a secret"
  fi
  if [ -n "$gl_reason" ]; then
    echo "        $gl_reason"
    grep -E 'Secret|Finding|Location|RuleID|Commit' /tmp/check-gitleaks.log | head -12
    fail gitleaks
  else
    pass "gitleaks (staged + history)"
  fi
fi

# --- Rust ------------------------------------------------------------------------------------
if [ ! -f Cargo.toml ]; then
  skip cargo "no Cargo.toml (lands with the workspace)"
elif ! cargo fmt --all --check; then
  echo "        cargo fmt --all --check: run cargo fmt --all"
  fail cargo
elif ! cargo clippy --workspace --all-targets -- -D warnings; then
  fail cargo
elif ! cargo check --workspace --all-targets; then
  fail cargo
else
  pass "cargo (fmt, clippy -D warnings, check)"
fi

# --- License policy (ADR-0004) ---------------------------------------------------------------
if [ ! -f Cargo.lock ]; then
  skip deny "no Cargo.lock yet"
elif ! command -v cargo-deny >/dev/null 2>&1 && ! cargo deny --version >/dev/null 2>&1; then
  skip deny "cargo-deny not on PATH — run mise install"
elif ! cargo deny check; then
  echo "        Widening deny.toml to clear this is banned (ADR-0004). Report the failure."
  fail deny
else
  pass "cargo deny (licenses, bans, advisories)"
fi

# --- Frontend --------------------------------------------------------------------------------
if [ ! -f apps/web/package.json ]; then
  skip web "no apps/web/package.json (ADR-0003 unit 2)"
# shortcut: a cached node_modules is not re-verified locally; CI has none, so CI installs
# from the lockfile and is the authority on lock/package agreement.
elif [ ! -d apps/web/node_modules ] && ! npm --prefix apps/web ci --no-audit --no-fund; then
  fail web "npm ci from apps/web/package-lock.json failed"
elif ! npm --prefix apps/web run check; then
  fail web
else
  pass "web (svelte-check + tsc)"
fi

# --- Contract drift (ADR-0003) ---------------------------------------------------------------
if ! just --summary 2>/dev/null | grep -qw gen; then
  skip drift "no just gen recipe yet"
elif ! just gen; then
  fail drift
elif ! git diff --exit-code -- contracts apps/web/src/lib/api; then
  echo "        contracts/ or apps/web/src/lib/api changed after regeneration: these are generated."
  echo "        Edit the Rust handler, then run just gen. Never hand-edit the output."
  fail drift
else
  pass "drift (regenerated, diff empty)"
fi

# --- Diff budget (ADR-0001: 50 lines soft, 200 hard, 10 files) --------------------------------
# ADR-0001 claimed this was machine-enforced and nothing implemented it. Lines and files are
# countable, so it is a gate now. Excluded: lockfiles, and the paths ADR-0003 declares generated.
scope=()
if ! git diff --quiet --cached 2>/dev/null; then
  scope=(--cached)                              # pre-commit: what is about to be committed
elif base=$(git merge-base HEAD origin/main 2>/dev/null) && [ "$base" != "$(git rev-parse HEAD)" ]; then
  scope=("$base..HEAD")                         # CI on a branch: the whole task diff
elif git rev-parse -q --verify HEAD^ >/dev/null 2>&1; then
  scope=("HEAD^..HEAD")                         # CI on main: the commit that just landed
else
  skip "diff budget" "root commit, nothing to compare against"
fi
if [ ${#scope[@]} -gt 0 ]; then
  # --numstat lines are `adds TAB dels TAB path`, so exclusions match the third field. Anchoring
  # `^contracts/` at the line start silently matched nothing and counted generated output as work.
  read -r lines files < <(git diff --numstat "${scope[@]}" -- \
    | awk -F'\t' '$3 !~ /(^|\/)(package-lock\.json|Cargo\.lock)$/ && $3 !~ /\.lock$/ \
              && $3 !~ /^(apps\/web\/src\/lib\/api|contracts|\.sqlx)\// {a += $1 + $2; n++} \
              END {print a + 0, n + 0}')
  if [ "$lines" -gt 200 ] || [ "$files" -gt 10 ]; then
    echo "        $lines lines across $files files; ADR-0001 allows 200 lines and 10 files. Split it."
    fail "diff budget"
  elif [ "$lines" -gt 50 ]; then
    warn "diff budget ($lines lines, $files files: past the 50-line soft target)"
  else
    pass "diff budget ($lines lines, $files files)"
  fi

  # --- Gate edits must ship their proof (ADR-0001) ---------------------------------------------
  # A check.sh edit with no selftest.sh change is an unproven claim about the gate, which is how a
  # broken exclusion shipped reporting PASS. Pairs with `just selftest`.
  if git diff --name-only "${scope[@]}" -- | grep -qx 'scripts/check.sh' \
     && ! git diff --name-only "${scope[@]}" -- | grep -qx 'scripts/selftest.sh'; then
    echo "        scripts/check.sh changed and scripts/selftest.sh did not: add the fixture that bites."
    fail "gate-proof"
  else
    pass "gate-proof"
  fi

  # --- In-code suppressions (AGENTS.md rule 2) -------------------------------------------------
  # A suppression edits the check instead of the code, so the added lines are themselves the
  # evidence. Rust and TOML only: this script's own patterns must not match themselves.
  supp=$(git diff "${scope[@]}" -- '*.rs' '*.toml' | grep '^+' | grep -v '^+++' \
    | grep -nE '#\[(allow|expect)|--cap-lints|gitleaks:allow' | head -5)
  if [ -n "$supp" ]; then
    printf '%s\n' "$supp" | sed 's/^/        /'
    echo "        Remove the attribute and fix the lint, or report the lint instead (AGENTS.md rule 2)."
    fail "suppressions"
  else
    pass "suppressions"
  fi
fi

printf 'check: %d ran, %d skipped, %d warned, %d failed\n' "$ran" "$skipped" "$warned" "$failed"
if [ "$failed" -gt 0 ]; then exit 1; fi
if [ "$ran" -eq 0 ]; then echo "check: nothing ran. This green means nothing."; exit 1; fi
exit 0
