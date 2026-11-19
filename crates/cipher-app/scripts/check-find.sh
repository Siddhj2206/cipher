#!/bin/sh
set -eu
book=$1
bin=${2:-target/debug/cipher}
here=$(dirname "$0")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for mark in open found next source refocused closed; do
  "$bin" "$book" --script "$here/find.json" --at "mark:$mark" --tree "$tmp/$mark.json" 2>/dev/null
done
python3 -I - "$tmp" <<'PY'
import json, pathlib, sys
def elements(mark):
    return json.load(open(pathlib.Path(sys.argv[1]) / (mark + '.json')))['elements']
def count(mark):
    return next(e.get('text', '') for e in elements(mark) if e.get('key') == 'find-count')
assert any(e.get('key') == 'find-bar' for e in elements('open')), 'Ctrl+F did not open find'
assert count('found').startswith('1 of '), count('found')
assert count('next').startswith('2 of '), count('next')
assert count('source') == 'No matches', count('source')
assert count('refocused').startswith('1 of '), count('refocused')
assert any(e.get('key') == 'find-input' and e.get('text') == '탑' for e in elements('refocused')), 'Ctrl+F did not select the old query'
assert not any(e.get('key') == 'find-bar' for e in elements('closed')), 'Escape did not close find'
print('ok: Ctrl+F -> query -> count -> Enter next -> Source -> Ctrl+F replaces query -> Escape')
PY
