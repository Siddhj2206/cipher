#!/bin/sh
# Asserts drag and keyboard resizing, reading stability, and responsive fit through --tree.
set -eu
book=$1
bin=${2:-target/debug/cipher}
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
echo "ok: drag, keyboard, focus, reading anchor, clamp, auto-collapse and restore"
