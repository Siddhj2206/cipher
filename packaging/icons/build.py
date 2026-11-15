"""Renders the icon in every format the packages need, from cipher.svg (detailed) and
cipher-small.svg (16 to 48 px). Needs rsvg-convert. Run after make_icon.py; the outputs are
checked in so CI never draws icons.

out/png/<n>.png   hicolor sizes for Linux (and the AppImage)
out/cipher.ico    Windows: 16 to 256 (BMP entries, PNG at 256; needs ImageMagick's `magick`)
out/cipher.icns   macOS: ic04 to ic14
"""

import struct
import subprocess
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "out"
SMALL = 48  # this size and below draw from cipher-small.svg


def render(size: int) -> bytes:
    src = HERE / ("cipher-small.svg" if size <= SMALL else "cipher.svg")
    return subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), str(src)],
        check=True,
        capture_output=True,
    ).stdout


def ico(sizes):
    images = [render(s) for s in sizes]
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for s, data in zip(sizes, images):
        w = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        blobs += data
    return head + entries + blobs


def icns():
    kinds = [
        ("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64),
        ("ic07", 128), ("ic08", 256), ("ic13", 256), ("ic09", 512), ("ic14", 512), ("ic10", 1024),
    ]
    body = b""
    for kind, size in kinds:
        data = render(size)
        body += kind.encode() + struct.pack(">I", 8 + len(data)) + data
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def main():
    (OUT / "png").mkdir(parents=True, exist_ok=True)
    for s in (16, 22, 24, 32, 48, 64, 128, 256, 512, 1024):
        (OUT / "png" / f"{s}.png").write_bytes(render(s))
    sizes = [16, 24, 32, 48, 64, 128, 256]
    pngs = [str(OUT / "png" / f"{s}.png") for s in sizes if (OUT / "png" / f"{s}.png").exists()]
    try:
        # rc.exe and old shells read BMP entries best; magick writes PNG only at 256.
        subprocess.run(["magick", *pngs, str(OUT / "cipher.ico")], check=True)
    except (FileNotFoundError, subprocess.CalledProcessError):
        (OUT / "cipher.ico").write_bytes(ico(sizes))
    (OUT / "cipher.icns").write_bytes(icns())
    print("wrote", OUT)


if __name__ == "__main__":
    main()
