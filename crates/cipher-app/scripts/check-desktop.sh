#!/bin/sh
# Desktop behaviour, from the keyboard and the mouse: Ctrl+, opens Settings, Reading's text
# size applies at once, Esc closes, Alt+Right goes to the next Chapter, Ctrl+/ shows the
# shortcuts, the header's "Aa" opens the type popover and Ctrl+- makes the text smaller.
# Asserts through --tree. Usage: scripts/check-desktop.sh path/to/book.cipher [path/to/cipher]
set -eu
book=$1
bin=${2:-target/debug/cipher}
[ -x "$bin" ] || { echo "FAIL: no binary at $bin (cargo build -p cipher-app)"; exit 1; }
here=$(dirname "$0")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for m in start settings bigger next shortcuts type smaller book-notes; do
  "$bin" "$book" --script "$here/desktop.json" --at "mark:$m" --tree "$tmp/$m.json" 2>/dev/null
done
has() { python3 -I - "$tmp/$1.json" "$2" <<'PY'
import json, sys
els = json.load(open(sys.argv[1]))["elements"]
sys.exit(0 if any(sys.argv[2] in (e.get("key") or "") + "|" + (e.get("text") or "") for e in els) else 1)
PY
}
chapter() { python3 -I - "$tmp/$1.json" <<'PY'
import json, re, sys
for e in json.load(open(sys.argv[1]))["elements"]:
    m = re.fullmatch(r"Chapter (\d+)", e.get("text") or "")
    if m:
        print(m.group(1)); break
PY
}
has start "settings-window" && { echo "FAIL: Settings open at start"; exit 1; }
has settings "Interface size" || { echo "FAIL: Ctrl+, did not open Settings › General"; exit 1; }
has bigger "20 pt" || { echo "FAIL: the text size did not step up to 20"; exit 1; }
first=$(chapter start); next=$(chapter next)
[ -n "$first" ] && [ "$next" -eq $((first + 1)) ] || { echo "FAIL: Alt+Right went from $first to $next"; exit 1; }
has next "settings-window" && { echo "FAIL: Esc did not close Settings"; exit 1; }
has shortcuts "Keyboard shortcuts" || { echo "FAIL: Ctrl+/ did not show the shortcuts"; exit 1; }
has type "type-popover" || { echo "FAIL: Aa did not open the type popover"; exit 1; }
has type "Keyboard shortcuts" && { echo "FAIL: Esc did not close the shortcuts"; exit 1; }
python3 -I - "$tmp/smaller.json" <<'PY' || { echo "FAIL: Ctrl+- did not make the text smaller"; exit 1; }
import json, sys
els = json.load(open(sys.argv[1]))["elements"]
sys.exit(0 if any(e.get("text") == "19" for e in els) else 1)
PY
has book-notes "notes-page" || { echo "FAIL: Book notes did not open"; exit 1; }
has book-notes "Forms of Address" || { echo "FAIL: Forms of Address missing"; exit 1; }
has book-notes "Story Notes" || { echo "FAIL: Story Notes missing"; exit 1; }
echo "ok: Ctrl+, -> text size 20 -> Esc -> Alt+Right $first->$next -> Ctrl+/ -> Aa -> Ctrl+- 19 -> Book notes"
