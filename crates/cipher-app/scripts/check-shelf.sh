#!/bin/sh
# The Shelf, all from the GUI, on an empty temporary Shelf (headless runs never touch the real
# one): New book → type a title → Enter opens it → paste a Chapter → Enter adds it → back to the
# Shelf, which lists it → Import folder… with a folder of chapters. Asserts through --tree.
# Usage: scripts/check-shelf.sh path/to/import-folder [path/to/cipher]
set -eu
folder=$(cd "$1" && pwd)
bin=${2:-target/debug/cipher}
[ -x "$bin" ] || { echo "FAIL: no binary at $bin (cargo build -p cipher-app)"; exit 1; }
here=$(dirname "$0")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sed "s|IMPORT_FOLDER|$folder|" "$here/shelf.json" > "$tmp/shelf.json"
for m in empty created added back; do
  "$bin" --script "$tmp/shelf.json" --at "mark:$m" --tree "$tmp/$m.json" 2>/dev/null
done
# The import runs on a thread in real time, so this run keeps the real clock.
"$bin" --realtime --script "$tmp/shelf.json" --at "mark:imported" --tree "$tmp/imported.json" 2>/dev/null
has() { python3 -I - "$tmp/$1.json" "$2" <<'PY'
import json, sys
els = json.load(open(sys.argv[1]))["elements"]
sys.exit(0 if any(sys.argv[2] in (e.get("key") or "") + "|" + (e.get("text") or "") for e in els) else 1)
PY
}
has empty "Your shelf is empty" || { echo "FAIL: the Shelf did not start empty"; exit 1; }
has created "NEW CHAPTER" || { echo "FAIL: the new Book did not open on its first chapter's page"; exit 1; }
has created "Omniscient Reader" || { echo "FAIL: the new Book has the wrong title"; exit 1; }
has added "first in queue" || { echo "FAIL: the pasted Chapter was not added"; exit 1; }
has back "1 to translate" || { echo "FAIL: the Shelf does not list the Book"; exit 1; }
name=$(basename "$folder" | tr -- '-_' '  ')
has imported "$name" || { echo "FAIL: the imported Book is not on the Shelf"; exit 1; }
has imported "Imported " || { echo "FAIL: no import report"; exit 1; }
echo "ok: new book -> pasted chapter queued -> listed on the Shelf -> folder imported"
