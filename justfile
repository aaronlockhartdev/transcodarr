# One command per verb, identical in every harness and in CI (ADR-0001). If a step works with
# `just <recipe>` and fails in CI, the recipe is wrong, not CI.

default:
    @just --list

# Run once per clone or worktree: hooks, pinned toolchains.
setup:
    git config core.hooksPath .githooks
    mise install
    rustup show

# The whole gate. The pre-commit hook and CI both call only this.
check:
    bash scripts/check.sh
