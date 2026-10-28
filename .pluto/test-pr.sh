#!/usr/bin/env bash
# Fast pull-request check.
#
# The PR work-item box for an untrusted pull request runs on the base image, so
# this script uses only git. It is a real check (whitespace errors and a
# readable HEAD), not an agent and not a GitHub API call. A trusted PR can
# replace this with `cargo test --locked` once the box has the Rust layer.
set -euo pipefail

cd "${PLUTO_WORKTREE:-$PWD}"

echo "pr-test: $(git rev-parse --short HEAD) on $(git rev-parse --abbrev-ref HEAD)"
git diff --check
echo "pr-test: ok"
