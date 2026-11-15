#!/bin/sh
# Adding a Chapter on a copy of a real Book (headless runs never touch the Book): paste → the
# paste sheet → Enter adds and queues it → its card offers Endpoint settings, which open.
# Asserts through --tree. Usage: scripts/check-queue.sh path/to/book.cipher [path/to/cipher]
set -eu
book=$1
bin=${2:-target/debug/cipher}
[ -x "$bin" ] || { echo "FAIL: no binary at $bin (cargo build -p cipher-app)"; exit 1; }
here=$(dirname "$0")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for m in pasted added settings; do
  "$bin" "$book" --script "$here/queue.json" --at "mark:$m" --tree "$tmp/$m.json" 2>/dev/null
done
has() { python3 -I - "$tmp/$1.json" "$2" <<'PY'
import json, sys
els = json.load(open(sys.argv[1]))["elements"]
sys.exit(0 if any(sys.argv[2] in (e.get("key") or "") + "|" + (e.get("text") or "") for e in els) else 1)
PY
}
has pasted "Chapter 321" || { echo "FAIL: no new-chapter page for Chapter 321"; exit 1; }
has added "first in queue" || { echo "FAIL: Chapter 321 was not added and queued"; exit 1; }
has added "draft-text" && { echo "FAIL: the new-chapter page stayed open"; exit 1; }
has settings "Endpoints and the models they offer" || { echo "FAIL: Endpoint settings did not open"; exit 1; }
echo "ok: pasted -> Chapter 321 queued first -> Endpoint settings"
