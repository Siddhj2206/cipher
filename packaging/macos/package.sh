#!/usr/bin/env bash
# Cipher.app and a .dmg, on macOS. Builds both architectures and joins them unless a binary
# is given.
# Usage: packaging/macos/package.sh [path/to/universal/cipher] [out-dir]
#
# Signing: with APPLE_SIGNING_IDENTITY set (a "Developer ID Application: …" identity already
# in the keychain), the app is signed with the hardened runtime; with APPLE_ID,
# APPLE_TEAM_ID and APPLE_APP_PASSWORD also set, the .dmg is notarised and stapled. Without
# them the app is signed ad hoc, which runs after the user allows it once.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
version="$("$root/packaging/version.sh")"
out="${2:-$root/target/dist}"
name="Cipher-$version-macos-universal"

bin="${1:-}"
if [[ -z "$bin" ]]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$t" >/dev/null
    (cd "$root" && cargo build --release --locked -p cipher-app --target "$t")
  done
  bin="$root/target/universal/cipher"
  mkdir -p "$(dirname "$bin")"
  lipo -create -output "$bin" \
    "$root/target/aarch64-apple-darwin/release/cipher" \
    "$root/target/x86_64-apple-darwin/release/cipher"
fi

mkdir -p "$out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/Cipher.app"
install -d "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$bin" "$app/Contents/MacOS/cipher"
install -m644 "$root/packaging/icons/out/cipher.icns" "$app/Contents/Resources/cipher.icns"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$app"
else
  codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"

# The disk image: the app beside a link to /Applications, to drag it across.
stage="$work/dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
dmg="$out/$name.dmg"
rm -f "$dmg"
hdiutil create -volname "Cipher $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"

if [[ -n "${APPLE_SIGNING_IDENTITY:-}" && -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
  codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$dmg"
fi
echo "wrote $dmg"
