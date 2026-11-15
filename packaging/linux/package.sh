#!/usr/bin/env bash
# Linux packages from a release build: an AppImage and a tarball (the .deb and .rpm come from
# cargo-deb and cargo-generate-rpm, configured in crates/cipher-app/Cargo.toml).
# Usage: packaging/linux/package.sh [path/to/cipher] [out-dir]
# Downloads appimagetool once into $XDG_CACHE_HOME/cipher-packaging if it isn't on PATH.
set -euo pipefail

ID="app.cipher.Cipher"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
bin="${1:-$root/target/release/cipher}"
out="${2:-$root/target/dist}"
version="$("$root/packaging/version.sh")"
arch="$(uname -m)"
name="Cipher-$version-linux-$arch"

[[ -x "$bin" ]] || { echo "no binary at $bin (cargo build --release -p cipher-app)" >&2; exit 1; }
mkdir -p "$out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The files a desktop needs, laid out under a prefix.
share_tree() {
  local prefix="$1"
  install -Dm755 "$bin" "$prefix/bin/cipher"
  install -Dm644 "$here/$ID.desktop" "$prefix/share/applications/$ID.desktop"
  install -Dm644 "$here/$ID.mime.xml" "$prefix/share/mime/packages/$ID.xml"
  install -Dm644 "$here/$ID.metainfo.xml" "$prefix/share/metainfo/$ID.metainfo.xml"
  install -Dm644 "$here/icons/$ID.svg" "$prefix/share/icons/hicolor/scalable/apps/$ID.svg"
  install -Dm644 "$here/icons/$ID-symbolic.svg" "$prefix/share/icons/hicolor/symbolic/apps/$ID-symbolic.svg"
  for s in 16 22 24 32 48 64 128 256 512; do
    install -Dm644 "$root/packaging/icons/out/png/$s.png" "$prefix/share/icons/hicolor/${s}x${s}/apps/$ID.png"
  done
}

# Tarball: the tree plus install.sh, which puts it under ~/.local.
tar_root="$work/$name"
share_tree "$tar_root"
install -Dm755 "$here/install.sh" "$tar_root/install.sh"
tar -C "$work" -czf "$out/$name.tar.gz" "$name"
echo "wrote $out/$name.tar.gz"

# AppImage: the same tree as an AppDir.
appdir="$work/Cipher.AppDir"
share_tree "$appdir/usr"
ln -s usr/bin/cipher "$appdir/AppRun"
cp "$here/$ID.desktop" "$appdir/$ID.desktop"
cp "$root/packaging/icons/out/png/256.png" "$appdir/$ID.png"
ln -s "$ID.png" "$appdir/.DirIcon"

tool="$(command -v appimagetool || true)"
if [[ -z "$tool" ]]; then
  cache="${XDG_CACHE_HOME:-$HOME/.cache}/cipher-packaging"
  tool="$cache/appimagetool-$arch.AppImage"
  if [[ ! -x "$tool" ]]; then
    mkdir -p "$cache"
    curl -fsSL -o "$tool" \
      "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$arch.AppImage"
    chmod +x "$tool"
  fi
fi
# CI runners have no FUSE: run the tool from its extracted self.
export APPIMAGE_EXTRACT_AND_RUN=1
ARCH="$arch" VERSION="$version" "$tool" --no-appstream "$appdir" "$out/$name.AppImage"
echo "wrote $out/$name.AppImage"
