#!/bin/sh
# Settings chrome and Endpoint feedback stay visible in small and zoomed windows.
set -eu
book=$1
bin=${2:-target/debug/cipher}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for scene in settings-general settings settings-chatgpt settings-about settings-shortcuts shortcuts settings-no-key settings-error settings-busy settings-chatgpt-busy settings-chatgpt-signed-in settings-models-failed settings-models-failed-zoom settings-general-zoom settings-zoom settings-chatgpt-zoom settings-error-zoom settings-about-zoom settings-shortcuts-zoom shortcuts-zoom; do
  case "$scene" in shortcuts*) pane=shortcuts-list;; *) pane=settings-pane;; esac
  printf '%s\n' "{\"events\":[{\"t\":0.1,\"type\":\"scroll\",\"key\":\"$pane\",\"dy\":-1500},{\"t\":0.5,\"type\":\"mark\",\"name\":\"bottom\"}]}" > "$tmp/scroll.json"
  "$bin" "$book" --scene "$scene" --at 0.2 --fps 10 --size 640x480 --scale 1 --tree "$tmp/start.json" 2>"$tmp/stderr" || { cat "$tmp/stderr"; exit 1; }
  "$bin" "$book" --scene "$scene" --script "$tmp/scroll.json" --at mark:bottom --fps 10 --size 640x480 --scale 1 --tree "$tmp/$scene.json" 2>"$tmp/stderr" || { cat "$tmp/stderr"; exit 1; }
  case "$scene" in settings-models-failed*)
    python3 -I - "$tmp/$scene.json" "$tmp/scroll.json" <<'PY'
import json, sys
tree = json.load(open(sys.argv[1]))
els = {e['key']: e for e in tree['elements'] if 'key' in e}
delta = (els['settings-pane']['rect'][1] + 8 - els['ep-fetch-status']['rect'][1]) * 640 / tree['viewport'][0]
script = json.load(open(sys.argv[2]))
script['events'] += [{'t': 0.6, 'type': 'scroll', 'key': 'settings-pane', 'dy': delta},
                     {'t': 1.0, 'type': 'mark', 'name': 'fetch'}]
json.dump(script, open(sys.argv[2], 'w'))
PY
    "$bin" "$book" --scene "$scene" --script "$tmp/scroll.json" --at mark:fetch --fps 10 --size 640x480 --scale 1 --tree "$tmp/$scene.json" 2>"$tmp/stderr" || { cat "$tmp/stderr"; exit 1; }
  ;; esac
  python3 -I - "$tmp/$scene.json" "$scene" "$tmp/start.json" <<'PY'
import json, sys
tree = json.load(open(sys.argv[1]))
scene = sys.argv[2]
els = {e['key']: e for e in tree['elements'] if 'key' in e}
start = {e['key']: e for e in json.load(open(sys.argv[3]))['elements'] if 'key' in e}
w, h = tree['viewport']
if scene.endswith('-zoom'):
    assert abs(w - 400) < 1 and abs(h - 300) < 1, f'{scene}: expected 160% interface zoom, got {w}x{h}'
def inside(key):
    assert key in els, f'{scene}: missing {key}'
    e = els[key]
    x, y, ew, eh = e['rect']
    assert ew > 0 and eh >= 0, f'{scene}: empty {key}: {e["rect"]}'
    assert x >= -1 and y >= -1 and x + ew <= w + 1 and y + eh <= h + 1, f'{scene}: {key} outside {w}x{h}: {e["rect"]}'
    assert e['opacity'] > 0, f'{scene}: invisible {key}'
if scene.startswith('shortcuts'):
    els['shortcuts-heading'] = next(e for e in tree['elements'] if e.get('text') == 'Keyboard shortcuts')
    inside('shortcuts')
    inside('shortcuts-heading')
    inside('shortcuts-close')
else:
    for key in ['settings-window', 'settings-nav', 'settings-heading', 'set-close']:
        inside(key)
        assert els[key]['rect'] == start[key]['rect'], f'{scene}: scrolling moved {key}'
    for section in ['general', 'reading', 'translation', 'shortcuts', 'about']:
        inside('set-' + section)
    translation = not any(part in scene for part in ['general', 'about', 'shortcuts'])
    if translation:
        description = els['settings-description']
        assert '/settings-pane/' in description['path'], f'{scene}: pinned description'
        assert description['rect'][1] < start['settings-description']['rect'][1], f'{scene}: description did not scroll'
        assert description['rect'][1] + description['rect'][3] <= els['settings-pane']['rect'][1], f'{scene}: description stayed visible after scrolling'
        for key in ['ep-footer', 'ep-status', 'ep-cancel', 'ep-test', 'ep-save']:
            inside(key)
        status, save = els['ep-status']['rect'], els['ep-save']['rect']
        assert status[1] + status[3] <= save[1] + 1, f'{scene}: status overlaps buttons'
        if 'busy' in scene:
            for key in ['ep-test', 'ep-save'] + (['ep-signin'] if 'chatgpt' in scene else []):
                assert els[key]['opacity'] < 0.6 and not els[key]['focusable'], f'{scene}: enabled {key}'
        if 'error' in scene:
            assert 'server refused' in els['ep-status'].get('text', '')
        if scene in ['settings', 'settings-zoom']:
            assert 'replied' in els['ep-status'].get('text', ''), f'{scene}: scrolling cleared feedback'
        if 'models-failed' in scene:
            pane = els['settings-pane']['rect']
            for key in ['ep-model', 'ep-small']:
                x, _, ew, _ = els[key]['rect']
                assert ew > 0 and x >= pane[0] and x + ew <= pane[0] + pane[2], f'{scene}: overflowing {key}'
            model, small = els['ep-model']['rect'], els['ep-small']['rect']
            assert model[1] + model[3] <= small[1], f'{scene}: model fields did not stack'
            for key in ['ep-fetch-status', 'ep-fetch']:
                inside(key)
                x, y, ew, eh = els[key]['rect']
                assert y >= pane[1] and y + eh <= pane[1] + pane[3] + 1, f'{scene}: unreachable {key}'
            assert 'refused the key' in els['ep-fetch-status'].get('text', '')
            assert any(e.get('text') == 'Retry' for e in tree['elements'])
            assert not els['ep-fetch']['inert'] and els['ep-fetch']['opacity'] == 1, f'{scene}: Retry is disabled'
            assert els['ep-fetch-status']['rect'][1] + els['ep-fetch-status']['rect'][3] <= els['ep-fetch']['rect'][1], f'{scene}: Retry overlaps error'
    if scene == 'settings-no-key':
        assert els['ep-key-hint']['text'] == 'No key: requests go out without one.'
PY
done
echo 'ok: Settings navigation, heading, close and Endpoint footer at 640x480 and 160% zoom; description scrolls, heading stays pinned; no-key, error, busy, signed-in and Fetch models Retry states'
