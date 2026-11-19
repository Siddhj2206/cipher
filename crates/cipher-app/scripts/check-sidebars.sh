#!/bin/sh
# Asserts drag and keyboard resizing, reading stability, and responsive fit through --tree.
set -eu
book=$1
bin=${2:-target/debug/cipher}
dev=${3:-target/debug/cipher-dev}
here=$(dirname "$0")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for m in start positioned dragged stepped canceled narrow priority restored minimum; do
  "$bin" "$book" --size 1480x920 --scale 1 --fps 15 --script "$here/sidebars.json" \
    --at "mark:$m" --tree "$tmp/$m.json" 2>"$tmp/log" || { cat "$tmp/log"; exit 1; }
done
python3 -I - "$tmp" <<'PY'
import json, math, pathlib, sys

def elements(mark):
    tree = json.loads((pathlib.Path(sys.argv[1]) / f"{mark}.json").read_text())
    return {e["key"]: e for e in tree["elements"] if e.get("key") and not e.get("exiting")}

def widths(mark, expected):
    els = elements(mark)
    actual = [els[key]["rect"][2] if key in els else None for key in ("contents", "context")]
    assert all(a == b or (a is not None and b is not None and math.isclose(a, b, abs_tol=1))
               for a, b in zip(actual, expected)), (mark, actual, expected)
    return els

widths("start", (272, 320))
before = elements("positioned")
after = widths("dragged", (392, 432))
assert before["resize-contents"].get("focused") and after["resize-contents"].get("focused"), "drag stole focus"
top = before["doc"]["rect"][1]
anchor = next(key for key, e in before.items()
              if key.startswith("row") and e["rect"][1] <= top < e["rect"][1] + e["rect"][3])
assert math.isclose(before[anchor]["rect"][1], after[anchor]["rect"][1], abs_tol=1), "reading position moved"
assert widths("stepped", (408, 448))["resize-context"].get("focused"), "keyboard focus was lost on resize"
widths("canceled", (408, 448))
widths("narrow", (408, None))
widths("priority", (None, 372))
widths("restored", (408, 448))
minimum = widths("minimum", (None, None))
assert minimum["app-root"]["rect"][2] < 500, "interface was not at maximum zoom"
assert minimum["page"]["rect"][2] <= minimum["doc"]["rect"][2], "reading column clips at minimum size"
PY

# Make the first paragraph tall enough to straddle the reading viewport before reflow.
cp -R "$here/../tests/fixtures/the-quiet-tower" "$tmp/long"
python3 -I - "$tmp/long/tl/1.md" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
phrase = "Erian arrived at the Quiet Tower through the dawn fog."
assert phrase in text
path.write_text(text.replace(phrase, " ".join([phrase] * 26), 1), encoding="utf-8")
PY
"$dev" import "$tmp/long" "$tmp/long.cipher" >/dev/null
cat >"$tmp/reflow.json" <<'JSON'
{"events": [
 {"t": 0.2, "type": "drag", "key": "resize-contents", "dx": 500, "dur": 0.2},
 {"t": 0.6, "type": "scroll", "key": "doc", "dy": -955, "dur": 0.4},
 {"t": 1.6, "type": "mark", "name": "before"},
 {"t": 1.75, "type": "drag", "key": "resize-contents", "dx": -500, "dur": 0.15},
 {"t": 2.1, "type": "mark", "name": "after"}
]}
JSON
for m in before after; do
  "$bin" "$tmp/long.cipher" --size 1480x920 --scale .5 --fps 15 --script "$tmp/reflow.json" \
    --at "mark:$m" --tree "$tmp/$m.json" 2>"$tmp/log" || { cat "$tmp/log"; exit 1; }
done
python3 -I - "$tmp" <<'PY'
import json, pathlib, sys

def rects(mark):
    tree = json.loads((pathlib.Path(sys.argv[1]) / f"{mark}.json").read_text())
    return {e["key"]: e["rect"] for e in tree["elements"] if e.get("key") and not e.get("exiting")}

before, after = rects("before"), rects("after")
top = before["doc"][1]
anchor = next(key for key, (_, y, _, h) in before.items()
              if key.startswith("b") and key[1:].isdigit() and y < top < y + h)
_, y, _, height = after[anchor]
assert top - before[anchor][1] > height, "reflow did not shorten the text past the old offset"
assert y < after["doc"][1] < y + height, "widening skipped the paragraph at the top of the reader"
PY
echo "ok: drag, keyboard, focus, reading anchor, clamp, auto-collapse and restore"
