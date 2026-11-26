#!/usr/bin/env bash
# The whole gate (ADR-0001). The same script runs in `just check`, in the pre-commit hook, and
# in CI. A check either runs and can fail, or prints SKIP with its reason. Nothing that did not
# run counts as passing, and green with zero checks run is itself a failure.
set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "check.sh: not a git repository"; exit 1; }
cd "$root" || exit 1

# Per-run scratch. ADR-0001 rule 1 makes a worktree per unit, so two gates running at once is the
# normal shape, not the exotic one; nothing here may use a machine-wide path (#50).
run_dir=$(mktemp -d 2>/dev/null) || { echo "check.sh: cannot create a scratch dir"; exit 1; }
trap 'rm -rf "$run_dir"' EXIT

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
  # The state is the line's first word, and the guard after it is load-bearing: `proposed|accepted`
  # unanchored lets `Status: proposedly` and `proposed-legacy` read as valid states, which is the
  # defect #3 names by name, measured still passing on main and on this PR's first draft. But the state
  # may still carry a sentence, because that is how this corpus annotates the line -- all four shipped
  # ADRs read `Status: proposed. Human owner: Aaron; ...` -- and README gives that line to the human
  # alone, so the gate takes the first word rather than rewriting four people's Status lines to suit a
  # regex. The supersession value is exact, because the gate reads a number out of it.
  grep -qE '^Status: (proposed|accepted)([[:space:].]|$)|^Status: superseded by ADR-[0-9]{4}$' "$f" \
    || { echo "        $f: Status line neither begins with 'proposed'/'accepted' nor reads 'superseded by ADR-NNNN'"; adr_bad=1; }
  for h in '## Context' '## Decision' '## Rejected alternatives' '## Unenforced'; do
    grep -qx "$h" "$f" || { echo "        $f: missing section '$h'"; adr_bad=1; }
  done
  # Supersession, read off the Status line and nowhere else: the old record says `superseded by
  # ADR-NNNN`, that record exists, and it points back with `Supersedes ADR-NNNN`. Both directions are
  # required because the failure is silent otherwise -- Equinor audited its own corpus for exactly this
  # after finding "informal/undeclared supersession", and the only CI enforcement of it found anywhere
  # (plecture's check-adr-lifecycle.sh) fails the build on a one-directional pair. Anchoring to the
  # Status line is what makes prose safe: this rule used to grep the whole file for the pattern, and
  # apology for tripping on an unrelated mention by number, and a dangling target passed it.
  sup=$(sed -n 's/^Status: superseded by ADR-\([0-9][0-9][0-9][0-9]\)$/\1/p' "$f" | head -1)
  if [ -n "$sup" ]; then
    if [ "$sup" = "$num" ]; then
      echo "        $f: Status supersedes its own record"; adr_bad=1
    else
      tgt=$(find docs/adr -maxdepth 1 -name "$sup-*.md" | head -1)
      if [ -z "$tgt" ]; then
        echo "        $f: Status names ADR-$sup and no docs/adr/$sup-*.md exists"; adr_bad=1
      elif ! grep -qiE 'supersed[a-z]*[[:space:]:]+\[?ADR-'"$num" "$tgt"; then
        echo "        $f: ADR-$sup does not state 'Supersedes ADR-$num'"; adr_bad=1
      fi
    fi
  fi
done
if [ "$adr_bad" -eq 0 ]; then pass "adr-structure"; else fail "adr-structure"; fi

# --- Document links: a resolver reads the corpus, the shell only reports ------------------
# `--config` names the tracked file rather than letting lychee pick up whatever sits in the working
# directory -- the lesson `.gitleaks.toml` taught in #55. `include_fragments = "full"` makes a link
# to a heading that no longer exists a failure, which is the rot a prose corpus actually produces.
# lychee exits 0 whether it read 0 links or 100, and its `Total` counts links it only excluded, so the
# guard reads the links it resolved. Measured on the pinned binary: a tree whose only links are external
# prints `2 Total ... 0 OK ... 2 Excluded` under `offline = true`, which is a green that checked none.
link_out=$(lychee --config lychee.toml --no-progress docs 2>&1); link_code=$?
link_ok=$(printf '%s\n' "$link_out" | sed -n 's/.*✅[[:space:]]*\([0-9][0-9]*\)[[:space:]]*OK.*/\1/p' | head -1)
link_ex=$(printf '%s\n' "$link_out" | sed -n 's/.*👻[[:space:]]*\([0-9][0-9]*\)[[:space:]]*Excluded.*/\1/p' | head -1)
# lychee prints no `Excluded` token when nothing was excluded (measured), so an absent count is zero.
link_ex=${link_ex:-0}
case "$link_code" in
  0) if [ -z "$link_ok" ]; then
       printf '%s\n' "$link_out" | sed 's/^/        /'
       fail "md-links (no resolved-link count in lychee's output, so nothing was proved)"
     elif [ "$link_ok" -eq 0 ]; then
       fail "md-links (lychee resolved 0 links, $link_ex excluded, so a green here would mean nothing)"
     else pass "md-links (lychee: $link_ok resolved, $link_ex excluded, offline, fragments checked)"; fi ;;
  2) printf '%s\n' "$link_out" | grep -E '^\[ERROR\]' | sed 's/^/        /'
     fail "md-links (lychee: $(printf '%s\n' "$link_out" | grep -cE '^\[ERROR\]') links do not resolve)" ;;
  *) printf '%s\n' "$link_out" | sed 's/^/        /'
     fail "md-links (lychee exited $link_code without a verdict; check lychee.toml and the pins)" ;;
esac

# --- ADR index: whether the index names every ADR, and only that --------------------------------
# The measured failure: 167 ADRs, an index that showed 30, and 29 live decisions invisible. That is
# the file-to-index direction, and it is the one a link resolver cannot see: lychee reads the links
# that exist, so an ADR with no link to it produces nothing to fail. Whether a listing resolves is
# md-links' job; the `-e` loop here was a hand-rolled copy of a resolver added two lines above it.
idx_bad=0
# Key on the filename, not the bare number: matching a prose mention of "ADR-0002" let an index row
# that named no file pass the direction this arm is left holding.
for f in "${adr_files[@]}"; do
  fname=${f##*/}
  grep -q -- "$fname" docs/adr/README.md || { echo "        README index does not name $fname"; idx_bad=1; }
done
if [ "$idx_bad" -eq 0 ]; then pass "adr-index"; else fail "adr-index"; fi

# --- ADR length: the README asks for one page, so one page has to be a number ------------------
# The cap is 200 lines: the bar, not today's maximum. The longest ADR was 229 when the rule was
# written, so a limit set at the maximum could never fire on the file that motivated it.
adr_len=0
for f in "${adr_files[@]}"; do
  n=$(wc -l < "$f")
  if [ "$n" -gt 200 ]; then
    echo "        $f: $n lines, past the 200-line cap; prune evidence, split only if there is a second decision"
    warn "adr-length"; adr_len=1
  fi
done
[ "$adr_len" -eq 0 ] && pass "adr-length (${#adr_files[@]} ADRs measured, all within 200 lines)"

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
want_deny=$(sed -n 's/^"cargo:cargo-deny" *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_deny=$(cargo deny --version 2>/dev/null | awk '{print $2}')
# `cargo deny` resolves from $CARGO_HOME/bin before PATH, so a plain PATH lookup would name a different
# binary than the one whose version was just read.
deny_bin="${CARGO_HOME:-$HOME/.cargo}/bin/cargo-deny"
# `[ -x ]` is true for a directory and cargo falls through one, so a plain -x can name a path that never ran.
{ [ -x "$deny_bin" ] && [ ! -d "$deny_bin" ]; } || deny_bin=$(command -v cargo-deny || echo "no cargo-deny found")
if [ -z "$want_deny" ]; then echo "        mise.toml has no \"cargo:cargo-deny\" pin to compare against"; pin_bad=1
elif [ "$want_deny" != "$got_deny" ]; then echo "        cargo-deny ${got_deny:-none}, mise.toml pins $want_deny ($deny_bin)"; pin_bad=1; fi
# The gate invokes gitleaks and just too, so a wrong version of either must not run quietly (#43):
# gitleaks decides what counts as a secret, and just is what ran this script.
want_gl=$(sed -n 's/^gitleaks *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_gl=$(gitleaks --version 2>/dev/null | awk '{print $NF}')
if [ -z "$want_gl" ]; then echo "        mise.toml has no gitleaks = \"…\" pin to compare against"; pin_bad=1
elif [ "$want_gl" != "$got_gl" ]; then echo "        gitleaks ${got_gl:-none}, mise.toml pins $want_gl ($(command -v gitleaks || echo 'not on PATH'))"; pin_bad=1; fi
want_just=$(sed -n 's/^just *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_just=$(just --version 2>/dev/null | awk '{print $NF}')
if [ -z "$want_just" ]; then echo "        mise.toml has no just = \"…\" pin to compare against"; pin_bad=1
elif [ "$want_just" != "$got_just" ]; then echo "        just ${got_just:-none}, mise.toml pins $want_just ($(command -v just || echo 'not on PATH'))"; pin_bad=1; fi
# Vale decides what a well-shaped ADR is, so a wrong version of it is a different verdict, not a
# slower one (#43's rule, one more tool). The arm that runs Vale over the corpus lands next; the pin
# is watched from here, so the toolchain never quietly gains an unwatched binary.
want_vale=$(sed -n 's/^vale *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_vale=$(vale --version 2>/dev/null | awk '{print $NF}')
if [ -z "$want_vale" ]; then echo "        mise.toml has no vale = \"…\" pin to compare against"; pin_bad=1
elif [ "$want_vale" != "$got_vale" ]; then echo "        vale ${got_vale:-none}, mise.toml pins $want_vale ($(command -v vale || echo 'not on PATH'))"; pin_bad=1; fi
want_lychee=$(sed -n 's/^lychee *= *"\([^"]*\)".*/\1/p' mise.toml | head -1)
got_lychee=$(lychee --version 2>/dev/null | awk '{print $NF}')
if [ -z "$want_lychee" ]; then echo "        mise.toml has no lychee = \"…\" pin to compare against"; pin_bad=1
elif [ "$want_lychee" != "$got_lychee" ]; then echo "        lychee ${got_lychee:-none}, mise.toml pins $want_lychee ($(command -v lychee || echo 'not on PATH'))"; pin_bad=1; fi
if [ "$pin_bad" -eq 0 ]; then pass "toolchain-pins (node $got_node, rustc $got_rust, cargo-deny $got_deny, gitleaks $got_gl, just $got_just, vale $got_vale, lychee $got_lychee)"; else fail "toolchain-pins"; fi

# --- Generated and cache files must stay tracked --------------------------------------------
# These paths have to stay tracked or every check that reads them goes quietly green: the
# generated client because it is what a drift arm will one day diff, the lockfile because it is
# what `cargo deny` and `npm ci` resolve, `.sqlx` because it is what the offline builds need.
ign_bad=0
for p in contracts/openapi.json apps/web/src/lib/api/schema.ts Cargo.lock .sqlx; do
  if git check-ignore -q "$p" 2>/dev/null; then
    echo "        $p is gitignored — an ignored path is invisible to every check"; ign_bad=1
  elif [ -e "$p" ] && ! git ls-files --error-unmatch -- "$p" >/dev/null 2>&1; then
    echo "        $p is on disk but untracked — a file no commit carries is not a contract"; ign_bad=1
  fi
done
if [ "$ign_bad" -eq 0 ]; then pass "generated-paths-tracked"; else fail "generated-paths-tracked"; fi

# --- Secrets ---------------------------------------------------------------------------------
# `--staged` is what is about to land (and the only scan possible before the first commit);
# the history scan needs a HEAD. 8.30 renamed `detect` to `git`.
# The enforced ruleset is the tracked `.gitleaks.toml`, passed with `--config`: that outranks
# $GITLEAKS_CONFIG and gitleaks' auto-load of an untracked file of the same name, which is how a
# three-line file left in a worktree came to report "no leaks found" over a real AWS key (#55).
gl_config=".gitleaks.toml"
gl_log="$run_dir/gitleaks.log"
# Two things measured on 8.30.1, both fixtured in selftest.sh: a config without
# `[extend] useDefault = true` replaces the embedded rules rather than adding to them, and a config
# that fails to parse ALSO exits 1 -- so a broken config must never be reported as a found secret.
gl_broken() { grep -qiE 'failed to load config|unable to load gitleaks config' "$gl_log"; }
gl_cmd() { local rep="$1"; shift
  gitleaks git "$@" --config "$gl_config" --report-format csv --report-path "$rep" \
    --redact --exit-code 1 >>"$gl_log" 2>&1; }
gl_reports=""
gl_found() { gl_reports="$gl_reports $1"; gl_reason="${gl_reason:+$gl_reason; }$2"; }
if ! command -v gitleaks >/dev/null 2>&1; then
  skip gitleaks "gitleaks not on PATH — run mise install"
elif [ ! -f "$gl_config" ]; then
  echo "        $gl_config is missing — the enforced ruleset has to be a tracked file (ADR-0001)"
  fail gitleaks
else
  : >"$gl_log"
  gl_reason=""
  gl_cmd "$run_dir/staged.csv" --staged \
    || { gl_broken || gl_found "$run_dir/staged.csv" "staged changes contain a secret"; }
  if git rev-parse -q --verify HEAD >/dev/null 2>&1; then
    gl_cmd "$run_dir/history.csv" \
      || { gl_broken || gl_found "$run_dir/history.csv" "history contains a secret"; }
  fi
  if gl_broken; then
    echo "        $gl_config does not load, so no verdict about secrets is possible"
    grep -iE 'failed to load|unable to load' "$gl_log" | head -2 | sed 's/^/        /'
    fail gitleaks
  elif [ -n "$gl_reason" ]; then
    echo "        $gl_reason"
    # 8.30.1's own output carries no finding detail: a red run prints the banner, "N commits
    # scanned." and "WRN leaks found: 1". The CSV report is the only thing naming rule, file and
    # commit (#51). The columns holding the secret are not printed, and --redact blanks them anyway.
    for rep in $gl_reports; do
      if [ -s "$rep" ]; then
        sed -n '2,13p' "$rep" | cut -d, -f1-3 | sed 's/^/        finding: /'
      else
        echo "        the report holds no rows, so the scan failed for a reason other than a leak"
      fi
    done
    fail gitleaks
  else
    pass "gitleaks ($gl_config: staged + history)"
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
elif ! cargo build --workspace; then
  echo "        cargo build --workspace: the shipped binary did not build."
  fail cargo
elif ! cargo test --workspace --quiet; then
  echo "        cargo test --workspace failed. CI compiled this suite and never ran it until #26."
  fail cargo
else
  pass "cargo (fmt, clippy -D warnings, build, test)"
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
  # Name first, detail in parentheses, as every other check prints: a PASS line reading `cargo deny`
  # while the FAIL line reads `deny` makes the green side unfindable by check name.
  pass "deny (cargo deny: licenses, bans, advisories)"
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

# --- The build the shipped binary serves (#30) -------------------------------------------------
# `npm run check` is svelte-check plus tsc: a type check that never bundles. Nothing in the gate,
# in CI, or in any documented recipe produced apps/web/build, the directory this binary exists to
# serve. adapter-static's fallback option, prerender config and the #lib import map are all
# bundle-time, so a defect there stayed green and was visible only in a browser.
if [ ! -f apps/web/package.json ]; then
  skip "web build" "no apps/web/package.json (ADR-0003 unit 2)"
elif ! just build-web; then
  echo "        just build-web failed. The binary would serve a directory nothing built."
  fail "web build"
elif [ ! -f apps/web/build/index.html ]; then
  echo "        just build-web reported success and left no apps/web/build/index.html."
  fail "web build"
else
  pass "web build (apps/web/build/index.html)"
fi

# --- Boot the binary and ask it for what it ships (#24) ----------------------------------------
# Everything above reads source; none of it started the process. A router built and never served,
# an env var misspelled in main.rs, ServeDir's guard regressed: all green. This runs the real
# binary against the real build directory, so it fails when either half stops fitting the other.
if ! command -v curl >/dev/null 2>&1; then
  skip serving "curl not on PATH"
else
  # Where cargo puts its binaries is cargo's business. Hardcoding target/debug would make this
  # skip on any machine that shares a target directory between worktrees.
  tgt=$(cargo metadata --no-deps --format-version 1 2>/dev/null \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)
  [ -n "$tgt" ] || tgt=target        # no workspace in here: the conventional location
  if [ ! -x "$tgt/debug/transcodarr" ]; then
    skip serving "$tgt/debug/transcodarr is not built (the cargo check above did not run)"
  elif [ ! -f apps/web/build/index.html ]; then
    fail serving "apps/web/build/index.html missing: nothing for the binary to serve"
  else
    port=$((20000 + RANDOM % 20000))   # a fixed port would collide with another worktree's daemon
    TRANSCODARR_ADDR="127.0.0.1:$port" TRANSCODARR_WEB_BUILD=apps/web/build \
      "$tgt/debug/transcodarr" >"$run_dir/serving.log" 2>&1 &
    pid=$!
    root_code=""
    api_code=""
    for _ in $(seq 1 40); do
      root_code=$(curl -s -o "$run_dir/root.html" -w '%{http_code}' "http://127.0.0.1:$port/" 2>/dev/null)
      case "$root_code" in ""|000) sleep 0.1 ;; *) break ;; esac
    done
    api_code=$(curl -s -o "$run_dir/api.html" -w '%{http_code}' "http://127.0.0.1:$port/api" 2>/dev/null)
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    srv_bad=0
    [ "$root_code" = 200 ] || { echo "        GET / answered '${root_code:-nothing}', wanted 200."; srv_bad=1; }
    grep -qi '<!doctype html' "$run_dir/root.html" 2>/dev/null \
      || { echo "        GET / answered ${root_code} without the app shell."; srv_bad=1; }
    [ "$api_code" = 404 ] || { echo "        GET /api answered '${api_code:-nothing}', wanted 404."; srv_bad=1; }
    if [ "$srv_bad" -ne 0 ]; then
      echo "        --- what the binary said: ---"
      sed 's/^/        /' "$run_dir/serving.log"
      fail serving
    else
      pass "serving (booted on :$port; / is the shell, /api is a 404)"
    fi
  fi
fi


# --- Diff budget (ADR-0001: 50 lines soft, 200 hard, 10 files) --------------------------------
# ADR-0001 claimed this was machine-enforced and nothing implemented it. Lines and files are
# countable, so it is a gate now. Excluded: lockfiles and the generated client. Not excluded:
# `contracts/`, which ADR-0003 still calls generated but nothing generates — while it is
# hand-authored, changing it is work, so it counts.
scope=()
if ! git diff --quiet --cached 2>/dev/null; then
  scope=(--cached)                              # pre-commit: what is about to be committed
elif base=$(git merge-base HEAD origin/main 2>/dev/null) && [ "$base" != "$(git rev-parse HEAD)" ]; then
  scope=("$base..HEAD")                         # CI on a branch: the whole task diff
elif git rev-parse -q --verify HEAD^ >/dev/null 2>&1; then
  scope=("HEAD^..HEAD")                         # nothing of ours: numbers the commit below HEAD (#16)
else
  skip "diff budget" "root commit, nothing to compare against"
fi
if [ ${#scope[@]} -gt 0 ]; then
  # --numstat lines are `adds TAB dels TAB path`, so exclusions match the third field. Anchoring
  # `^contracts/` at the line start silently matched nothing and counted generated output as work.
  read -r lines files < <(git diff --numstat "${scope[@]}" -- \
    | awk -F'\t' '$3 !~ /(^|\/)(package-lock\.json|Cargo\.lock)$/ && $3 !~ /\.lock$/ \
              && $3 !~ /^(apps\/web\/src\/lib\/api|\.sqlx)\// {a += $1 + $2; n++} \
              END {print a + 0, n + 0}')
  if [ "$lines" -gt 200 ] || [ "$files" -gt 10 ]; then
    echo "        $lines lines across $files files [${scope[0]}]; ADR-0001 allows 200 lines and 10 files. Split it."
    fail "diff budget"
  elif [ "$lines" -gt 50 ]; then
    warn "diff budget ($lines lines, $files files: past the 50-line soft target) [${scope[0]}]"
  else
    pass "diff budget ($lines lines, $files files) [${scope[0]}]"
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

# --- Commit trailers, where no hook can run (ADR-0001, issue #49) ---------------------------------
# `commit-msg` checks this shape, but the hook only exists where `just setup` ran: a fresh clone has
# `core.hooksPath` unset and accepts a commit with no trailers at all, green. CI is the one machine no
# `--no-verify` reaches, so it re-checks over the commits being proposed. Only commits of ours: main's
# history predates the rule, so a run on the base tip audits nothing and says so rather than going red
# on commits that were legal when written. The base is the one CI names in `$GITHUB_BASE_REF` when it
# exists, because `merge-base HEAD origin/main` charges a stacked branch for its *parent branch's*
# commits and then names them as ours (#70).
# shortcut: `$GITHUB_BASE_REF` is set only on GitHub Actions, so locally this is always origin/main
# and no fixture covers the PR-base path; the next stacked PR is the test of that line.
# The block is read with `git interpret-trailers`, so a `Task:` inside a sentence and a `Task:`
# written above the block are not trailers, which is what the hook's grep cannot tell; the hook stays
# the looser of the two readers. A revert is exempt in the shape `git revert` writes (`Revert "…"`),
# not in any subject that begins with the word, and an exempt commit is not counted, so the number
# printed on green is the number of commits actually audited. The `Agent:` model cross-check needs
# $PI_MODEL, which only a committing shell has, so that stays with the hook.
base_ref=${GITHUB_BASE_REF:-main}
if base=$(git merge-base HEAD "origin/$base_ref" 2>/dev/null); then
  if [ "$base" = "$(git rev-parse HEAD)" ]; then
    skip "trailer" "no commits of ours (HEAD is origin/$base_ref's tip)"
  else
    untrailered="" checked=0 exempt=0
    while IFS= read -r sha; do
      case "$(git log -1 --format=%s "$sha")" in
        'Revert "'*) exempt=$((exempt + 1)); continue ;;
      esac
      checked=$((checked + 1))
      block=$(git log -1 --format=%B "$sha" | git interpret-trailers --parse)
      missing=""
      if printf '%s\n' "$block" | grep -q '^Task: '; then
        task=$(printf '%s\n' "$block" | sed -n 's/^Task: //p' | head -1)
        case "$task" in
          '#'[0-9]* | bootstrap) ;;
          *) missing=" Task-shape" ;;      # `Task: banana` pointed at no issue and still went green
        esac
      else
        missing=" Task"
      fi
      for t in Agent Verified; do
        printf '%s\n' "$block" | grep -q "^$t: " || missing="$missing $t"
      done
      [ -n "$missing" ] && untrailered+="$sha missing:$missing"$'\n'
    done < <(git log --format=%h --no-merges "$base..HEAD")
    if [ -n "$untrailered" ]; then
      printf '%s' "$untrailered" | sed 's/^/        /'
      echo "        ADR-0001: every commit carries Task:/Agent:/Verified: as a trailer block, Task: being '#<issue>' or 'bootstrap'."
      fail "trailer"
    else
      pass "trailer ($checked commits checked, $exempt reverts exempt, all three trailers present)"
    fi
  fi
else
  skip "trailer" "no origin/$base_ref, so which commits are ours is unknowable"
fi

printf 'check: %d ran, %d skipped, %d warned, %d failed\n' "$ran" "$skipped" "$warned" "$failed"
if [ "$failed" -gt 0 ]; then exit 1; fi
if [ "$ran" -eq 0 ]; then echo "check: nothing ran. This green means nothing."; exit 1; fi
exit 0
