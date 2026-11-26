# One command per verb, identical in every harness and in CI (ADR-0001). If a step works with
# `just <recipe>` and fails in CI, the recipe is wrong, not CI.

default:
    @just --list

# Run once per clone or worktree: hooks, pinned toolchains.
setup:
    git config core.hooksPath .githooks
    mise install
    rustup show

# Regenerate the tracked contract's TypeScript client. Until the Rust binary emits the
# contract (ADR-0003), contracts/openapi.json is authored here, so this is the one direction
# of the pipeline that exists. Nothing checks the client matches the contract; #75 holds that.

gen:
    npm --prefix apps/web run gen

# Build the SPA the shipped binary serves. `just check` type-checks the frontend; only this
# produces apps/web/build, so before it no documented command created the directory the binary
# exists to serve.
build-web:
    npm --prefix apps/web run build

# The whole gate. The pre-commit hook and CI both call only this.
check:
    bash scripts/check.sh

# Prove the gate still has teeth after any edit to scripts/check.sh: each rule must go red on the
# defect and stay green when the defect is excluded or absent.
selftest:
    bash scripts/selftest.sh
