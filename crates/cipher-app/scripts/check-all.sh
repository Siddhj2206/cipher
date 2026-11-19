#!/bin/sh
# Runs every headless GUI check from the checked-in fixture, so it needs no Test-Books/.
# Imports tests/fixtures/the-quiet-tower into a temporary Book and runs the checks below.
# One line per check; exits non-zero
# if any fails. Usage: scripts/check-all.sh [path/to/cipher] [path/to/cipher-dev]
set -u
here=$(cd "$(dirname "$0")" && pwd)
bin=${1:-target/debug/cipher}
dev=${2:-target/debug/cipher-dev}
fixture="$here/../tests/fixtures/the-quiet-tower"
[ -x "$dev" ] || { echo "FAIL: no binary at $dev (cargo build -p cipher-dev)"; exit 1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
"$dev" import "$fixture" "$tmp/book.cipher" >/dev/null || { echo "FAIL: import of the fixture"; exit 1; }
failed=0
run() {
  name=$1; shift
  if out=$("$@" 2>&1); then echo "PASS $name: $out"; else echo "FAIL $name: $out"; failed=1; fi
}
run queue   sh "$here/check-queue.sh"   "$tmp/book.cipher" "$bin"
run fix     sh "$here/check-fix.sh"     "$tmp/book.cipher" "$bin"
run desktop sh "$here/check-desktop.sh" "$tmp/book.cipher" "$bin"
run sidebars sh "$here/check-sidebars.sh" "$tmp/book.cipher" "$bin" "$dev"
run settings sh "$here/check-settings.sh" "$tmp/book.cipher" "$bin"
run shelf   sh "$here/check-shelf.sh"   "$fixture" "$bin"
exit $failed
