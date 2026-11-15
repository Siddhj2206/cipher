#!/usr/bin/env bash
# Per-user install of Cipher and its desktop integration under ~/.local. No sudo.
# Works from the release tarball (the binary and share/ beside this script) or from the repo
# (builds target/release/cipher if missing).
# Usage: install.sh [--uninstall]
set -euo pipefail

ID="app.cipher.Cipher"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin"

targets=(
  "$bin/cipher"
  "$data/applications/$ID.desktop"
  "$data/mime/packages/$ID.xml"
  "$data/icons/hicolor/scalable/apps/$ID.svg"
  "$data/icons/hicolor/symbolic/apps/$ID-symbolic.svg"
  "$data/metainfo/$ID.metainfo.xml"
)

refresh() {
  if command -v update-mime-database >/dev/null; then update-mime-database "$data/mime" || true; fi
  if command -v update-desktop-database >/dev/null; then update-desktop-database "$data/applications" || true; fi
  if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -f -t "$data/icons/hicolor" 2>/dev/null || true; fi
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "${targets[@]}"
  refresh
  echo "Removed Cipher desktop integration."
  exit 0
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--uninstall]" >&2
  exit 2
fi

if [[ -x "$here/bin/cipher" ]]; then
  # The release tarball: bin/ and share/ beside this script.
  cp_bin="$here/bin/cipher"
  share="$here/share"
  desktop="$share/applications/$ID.desktop"
  mime="$share/mime/packages/$ID.xml"
  svg="$share/icons/hicolor/scalable/apps/$ID.svg"
  sym="$share/icons/hicolor/symbolic/apps/$ID-symbolic.svg"
  meta="$share/metainfo/$ID.metainfo.xml"
else
  if [[ ! -x "$root/target/release/cipher" ]]; then
    (cd "$root" && cargo build --release -p cipher-app)
  fi
  cp_bin="$root/target/release/cipher"
  desktop="$here/$ID.desktop"
  mime="$here/$ID.mime.xml"
  svg="$here/icons/$ID.svg"
  sym="$here/icons/$ID-symbolic.svg"
  meta="$here/$ID.metainfo.xml"
fi

install -Dm755 "$cp_bin" "${targets[0]}"
install -Dm644 "$desktop" "${targets[1]}"
install -Dm644 "$mime" "${targets[2]}"
install -Dm644 "$svg" "${targets[3]}"
install -Dm644 "$sym" "${targets[4]}"
install -Dm644 "$meta" "${targets[5]}"
refresh
echo "Installed Cipher to $bin/cipher (make sure $bin is on PATH)."
