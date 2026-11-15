#!/bin/sh
# Prints the version every package carries: the workspace's `version` in Cargo.toml.
# A release tag must be `v` followed by it (CI checks).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' "$root/Cargo.toml"
