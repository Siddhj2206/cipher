#!/bin/sh
# Settings chrome and Endpoint feedback stay visible in small and zoomed windows.
set -eu
book=$1
bin=${2:-target/debug/cipher}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf '%s\n' '{"events":[{"t":0.1,"type":"scroll","x":320,"y":240,"dy":-1500},{"t":0.5,"type":"mark","name":"bottom"}]}' > "$tmp/scroll.json"
for scene in settings-general settings settings-chatgpt settings-about settings-shortcuts shortcuts settings-no-key settings-error settings-busy settings-chatgpt-busy settings-chatgpt-signed-in settings-general-zoom settings-zoom settings-chatgpt-zoom settings-error-zoom settings-about-zoom settings-shortcuts-zoom shortcuts-zoom; do
  "$bin" "$book" --scene "$scene" --script "$tmp/scroll.json" --at mark:bottom --fps 10 --size 640x480 --scale 1 --tree "$tmp/$scene.json" 2>"$tmp/stderr" || { cat "$tmp/stderr"; exit 1; }
  python3 -I - "$tmp/$scene.json" "$scene" <<'PY'
import json, sys
tree = json.load(open(sys.argv[1]))
scene = sys.argv[2]
els = {e['key']: e for e in tree['elements'] if 'key' in e}
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
    for section in ['general', 'reading', 'translation', 'shortcuts', 'about']:
        inside('set-' + section)
    translation = not any(part in scene for part in ['general', 'about', 'shortcuts'])
    if translation:
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
    if scene == 'settings-no-key':
        assert els['ep-key-hint']['text'] == 'No key: requests go out without one.'
PY
done
echo 'ok: Settings navigation, heading, close and Endpoint footer at 640x480 and 160% zoom; no-key, error, busy and signed-in states'
