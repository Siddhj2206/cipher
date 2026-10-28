#!/usr/bin/env bash
# Run cipher's tests on the pull-request head.
#
# The daemon checks out the PR head and runs this as the `test-pr` job. It has
# no agent and makes no GitHub API calls: it only runs the crate's tests and
# exits with their status. The full output belongs to the Pluto run log.
set -euo pipefail

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
cd "${PLUTO_WORKTREE:-$PWD}"

echo "pr-test: $(git rev-parse --short HEAD) on $(git rev-parse --abbrev-ref HEAD)"
cargo test --locked
