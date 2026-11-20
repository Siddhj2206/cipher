#!/bin/sh
# Both Welcome connection choices retain the shared Endpoint actions and feedback.
set -eu
book=$1
bin=${2:-target/debug/cipher}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for kind in key chatgpt; do
  printf '%s\n' "{\"events\":[{\"t\":0.1,\"type\":\"click\",\"key\":\"welcome-$kind\"},{\"t\":0.3,\"type\":\"scroll\",\"key\":\"welcome-scroll\",\"dy\":-650},{\"t\":0.8,\"type\":\"mark\",\"name\":\"ready\"},{\"t\":0.9,\"type\":\"click\",\"key\":\"ep-save\"},{\"t\":1.1,\"type\":\"mark\",\"name\":\"save\"},{\"t\":1.2,\"type\":\"click\",\"key\":\"ep-test\"},{\"t\":1.4,\"type\":\"mark\",\"name\":\"test\"},{\"t\":1.5,\"type\":\"click\",\"key\":\"ep-cancel\"},{\"t\":1.7,\"type\":\"mark\",\"name\":\"revert\"}]}" > "$tmp/setup.json"
  for mark in ready save test revert; do
    "$bin" "$book" --scene welcome --script "$tmp/setup.json" --at "mark:$mark" --fps 10 --size 1280x900 --scale 1 --tree "$tmp/$mark.json" 2>"$tmp/stderr" || { cat "$tmp/stderr"; exit 1; }
  done
  python3 -I - "$tmp" <<'PY'
import json, pathlib, sys
folder = pathlib.Path(sys.argv[1])
for mark in ['ready', 'save', 'test', 'revert']:
    tree = json.loads((folder / f'{mark}.json').read_text())
    els = {e['key']: e for e in tree['elements'] if 'key' in e}
    assert 'welcome' in els
    if mark == 'ready':
        w, h = tree['viewport']
        for key in ['ep-footer', 'ep-status', 'ep-cancel', 'ep-test', 'ep-save']:
            x, y, ew, eh = els[key]['rect']
            assert ew > 0 and 0 <= x <= x + ew <= w and 0 <= y <= y + eh <= h, (key, els[key]['rect'])
    status = els['ep-status'].get('text', '')
    if mark in ['save', 'test']:
        assert status == 'Name the default model.', (mark, status)
    else:
        assert not status, (mark, status)
PY
done
echo 'ok: Welcome API key and ChatGPT show reachable status/Revert/Test/Save; validation and Revert respond'
