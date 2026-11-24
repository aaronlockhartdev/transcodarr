#!/usr/bin/env bash
# The whole gate (ADR-0001). The same script runs in `just check`, in the pre-commit hook, and
# in CI. A check either runs and can fail, or prints SKIP with its reason. Nothing that did not
# run counts as passing, and green with zero checks run is itself a failure.
set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "check.sh: not a git repository"; exit 1; }
cd "$root" || exit 1

ran=0 skipped=0 failed=0
pass() { printf '  PASS  %s\n' "$1"; ran=$((ran + 1)); }
skip() { printf '  SKIP  %s — %s\n' "$1" "$2"; skipped=$((skipped + 1)); }
fail() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }

printf 'check.sh  %s\n' "$(git rev-parse --short HEAD 2>/dev/null || echo 'no commits')"

# --- ADR corpus: shape, status, and the one-decision-written-twice rule ----------------------
adr_bad=0
for f in docs/adr/[0-9][0-9][0-9][0-9]-*.md; do
  [ -e "$f" ] || continue
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
for f in docs/adr/[0-9][0-9][0-9][0-9]-*.md; do
  [ -e "$f" ] || continue
  num=${f##*/}
  grep -q "${num%%-*}" docs/adr/README.md || { echo "        README index does not list $num"; idx_bad=1; }
done
for ref in $(grep -oE '[0-9]{4}-[a-z0-9-]+\.md' docs/adr/README.md | sort -u); do
  [ -e "docs/adr/$ref" ] || { echo "        README index points at docs/adr/$ref, which does not exist"; idx_bad=1; }
done
if [ "$idx_bad" -eq 0 ]; then pass "adr-index"; else fail "adr-index"; fi

# --- Toolchain pins: a shadowed binary must not look like success ----------------------------
pin_bad=0
want_node=$(sed -n 's/^node *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_node=$(node --version 2>/dev/null | tr -d v)
[ -n "$want_node" ] && [ "$want_node" != "$got_node" ] && { echo "        node $got_node, mise.toml pins $want_node ($(command -v node))"; pin_bad=1; }
want_rust=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' rust-toolchain.toml | head -1)
got_rust=$(rustc --version 2>/dev/null | awk '{print $2}')
[ -n "$want_rust" ] && [ "$want_rust" != "$got_rust" ] && { echo "        rustc $got_rust, rust-toolchain.toml pins $want_rust ($(command -v rustc))"; pin_bad=1; }
if [ "$pin_bad" -eq 0 ]; then pass "toolchain-pins (node $got_node, rustc $got_rust)"; else fail "toolchain-pins"; fi

# --- Generated and cache files must stay tracked --------------------------------------------
# If one goes untracked, the drift gate can never fire again and reports empty diffs forever.
ign_bad=0
for p in contracts/openapi.json apps/web/src/lib/api/schema.ts Cargo.lock .sqlx; do
  if git check-ignore -q "$p" 2>/dev/null; then
    echo "        $p is gitignored — the drift gate and sqlx offline checks cannot function"; ign_bad=1
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
elif [ ! -d apps/web/node_modules ]; then
  skip web "apps/web/node_modules missing — run npm ci in apps/web"
elif ! npm --prefix apps/web run check; then
  fail web
else
  pass "web (svelte-check + tsc)"
fi

# --- Contract drift (ADR-0003) ---------------------------------------------------------------
if ! grep -qxq 'gen' <(just --list 2>/dev/null | sed 's/ *#.*//'); then
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

printf 'check: %d ran, %d skipped, %d failed\n' "$ran" "$skipped" "$failed"
if [ "$failed" -gt 0 ]; then exit 1; fi
if [ "$ran" -eq 0 ]; then echo "check: nothing ran. This green means nothing."; exit 1; fi
exit 0
