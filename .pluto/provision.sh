#!/bin/sh
# Runs once per box, on the bare Ubuntu 24.04 pluto image: install mise, then
# let mise.toml pull the toolchain. ~/.local/bin is already on the box PATH,
# so both hooks and jobs can see the mise binary.
set -eu

curl -fsSL https://mise.run | sh

mise install --yes
