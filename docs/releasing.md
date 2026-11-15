# Releasing

How a version of Cipher reaches users. Packaging details live in `docs/design/architecture.md` › Packaging and platforms.

## Try the packages without releasing

Run the **Release** workflow by hand (Actions → Release → Run workflow, or `gh workflow run release.yml --ref <branch>`). It builds every package and attaches them to the run as artifacts (`dist-linux`, `dist-windows`, `dist-macos`); nothing is published.

Locally, on Linux: `cargo build --release -p cipher-app && packaging/linux/package.sh` writes the AppImage and tarball to `target/dist/`.

## Release

1. Set the version in the root `Cargo.toml` (`[workspace.package] version`), and run `cargo check` so `Cargo.lock` follows. A version with a hyphen (`0.2.0-beta.1`) becomes a pre-release.
2. Merge to `preview` (or the branch you release from) with CI green.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`. The tag must be `v` + the Cargo version, or the workflow stops.
4. The workflow builds Linux, Windows and macOS in parallel and creates a **draft** release with every package, `SHA256SUMS.txt`, the download table from `packaging/release-notes.md` and generated notes.
5. Download and try the packages, edit the notes, then publish the draft.

A broken tag: delete the draft release and the tag (`git push --delete origin v0.2.0`), fix, tag again.

## Signing (optional, later)

macOS signs and notarises when these repository secrets exist; without them it signs ad hoc.
- `MACOS_CERTIFICATE`: the Developer ID Application certificate as base64 `.p12`; `MACOS_CERTIFICATE_PASSWORD`.
- `APPLE_SIGNING_IDENTITY`: e.g. `Developer ID Application: Name (TEAMID)`.
- `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` (an app-specific password) for notarisation.

Windows has no signing step yet; adding one means a certificate (or Azure Trusted Signing) and a `signtool` call in `package.ps1` on `cipher.exe` and the installer.

## Icons

Change the drawing in `packaging/icons/make_icon.py`, then `python3 make_icon.py && python3 build.py` in that folder, copy `cipher.svg` to `packaging/linux/icons/app.cipher.Cipher.svg`, and commit `out/`.
