#!/bin/sh
# The fixing-while-reading flow on a copy of a real Book (headless runs never touch the Book):
# select a word → type → send → Accept all → Undo. Asserts through --tree that the paragraph
# changed, then reverted. It edits paragraph b10, which is chapter 1 of
# tests/fixtures/the-quiet-tower (see check-all.sh). Usage: scripts/check-fix.sh path/to/book.cipher [path/to/cipher]
set -eu
book=$1
bin=${2:-target/debug/cipher}
[ -x "$bin" ] || { echo "FAIL: no binary at $bin (cargo build -p cipher-app)"; exit 1; }
here=$(dirname "$0")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tree() { "$bin" "$book" --script "$here/fix.json" --at "mark:$1" --tree "$tmp/$1.json" 2>/dev/null; }
text() { python3 -I - "$tmp/$1.json" <<'PY'
import json, sys
for e in json.load(open(sys.argv[1]))["elements"]:
    if e.get("key") == "b10":
        print(e["text"])
PY
}
for m in sent applied undone; do tree $m; done
sent=$(text sent); applied=$(text applied); undone=$(text undone)
case $applied in *chagrin*) ;; *) echo "FAIL: Accept all did not change the paragraph: $applied"; exit 1;; esac
[ "$undone" = "$sent" ] || { echo "FAIL: Undo did not restore it: $undone"; exit 1; }
echo "ok: \"$sent\" -> \"$applied\" -> undone"
