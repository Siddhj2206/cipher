"""Draws Cipher's icon: a cream book open on a dark warm tile, Hangul-like syllable blocks on
the left leaf and English lines on the right (a translation at a glance), and a red ribbon.

Writes cipher.svg (detailed, 1024 grid with macOS's 100 px margin) and cipher-small.svg (for 16 to
48 px: fewer, heavier marks). Run: python3 make_icon.py, then build.sh renders every format."""

import random


def syllable(x, y, s, rnd):
    """One Hangul-like block as stroke paths: a consonant, a vowel, sometimes a final."""
    h = s / 2
    parts = []
    kind = rnd.choice(["side", "top", "side-final", "top-final"])
    def cons(cx, cy, w, hh):
        c = rnd.choice(["g", "n", "o", "m", "s"])
        if c == "g":   # ㄱ
            return f"M{cx} {cy}H{cx+w}V{cy+hh}"
        if c == "n":   # ㄴ
            return f"M{cx} {cy}V{cy+hh}H{cx+w}"
        if c == "o":   # ㅇ
            r = min(w, hh) / 2
            return f"M{cx+w/2-r} {cy+hh/2}a{r} {r} 0 1 0 {2*r} 0a{r} {r} 0 1 0 {-2*r} 0"
        if c == "m":   # ㅁ
            return f"M{cx} {cy}H{cx+w}V{cy+hh}H{cx}Z"
        return f"M{cx+w/2} {cy}L{cx} {cy+hh}M{cx+w/2} {cy}L{cx+w} {cy+hh}"  # ㅅ
    if kind.startswith("side"):
        bottom = s if kind == "side" else s * 0.55
        parts.append(cons(x, y + 2, s * 0.5, bottom - 4))
        parts.append(f"M{x+s*0.8} {y}V{y+bottom}M{x+s*0.8} {y+bottom/2}H{x+s}")  # ㅏ
        if kind == "side-final":
            parts.append(cons(x + s * 0.15, y + s * 0.68, s * 0.7, s * 0.32))
    else:
        top = s * 0.5 if kind == "top" else s * 0.4
        parts.append(cons(x + s * 0.2, y, s * 0.6, top - 3))
        parts.append(f"M{x} {y+top+3}H{x+s}M{x+s/2} {y+top+3}V{y+top-4}")  # ㅗ
        if kind == "top-final":
            parts.append(cons(x + s * 0.15, y + s * 0.66, s * 0.7, s * 0.34))
        else:
            parts.append(f"M{x+s/2} {y+top+3}V{y+s}")
    return "".join(parts)


def leaves(detail):
    rnd = random.Random(7)
    if detail:
        s, gap, rows, cols, x0, y0, dy = 30, 9, 4, 6, 258, 418, 62
    else:
        s, gap, rows, cols, x0, y0, dy = 64, 18, 2, 3, 250, 430, 110
    left = "".join(
        syllable(x0 + c * (s + gap), y0 + r * dy, s, rnd) for r in range(rows) for c in range(cols)
    )
    if detail:
        widths = [214, 236, 168, 230, 198, 236, 124]
        right = "".join(
            f'<rect x="548" y="{424 + i * 36}" width="{w}" height="12" rx="6"/>' for i, w in enumerate(widths)
        )
        stroke = 5
    else:
        widths = [220, 236, 200, 236, 140]
        right = "".join(
            f'<rect x="548" y="{430 + i * 52}" width="{w}" height="26" rx="13"/>' for i, w in enumerate(widths)
        )
        stroke = 11
    return left, right, stroke


def svg(detail):
    left, right, stroke = leaves(detail)
    tile_shadow = '<filter id="tile" x="-10%" y="-10%" width="120%" height="125%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" flood-opacity="0.35"/></filter>' if detail else ""
    x0, size, rx = (100, 824, 186) if detail else (40, 944, 210)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3A332C"/>
      <stop offset="1" stop-color="#1B1815"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.16" r="0.8">
      <stop offset="0" stop-color="#FFF1D6" stop-opacity="0.17"/>
      <stop offset="1" stop-color="#FFF1D6" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="pageL" x1="1" y1="0" x2="0" y2="0">
      <stop offset="0" stop-color="#D6C9AE"/>
      <stop offset="0.14" stop-color="#EFE6D2"/>
      <stop offset="1" stop-color="#FBF6EA"/>
    </linearGradient>
    <linearGradient id="pageR" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#D6C9AE"/>
      <stop offset="0.14" stop-color="#EFE6D2"/>
      <stop offset="1" stop-color="#FBF6EA"/>
    </linearGradient>
    <linearGradient id="ribbon" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#A12E18"/>
      <stop offset="0.5" stop-color="#CF4A2E"/>
      <stop offset="1" stop-color="#A6321C"/>
    </linearGradient>
    <filter id="soft" x="-20%" y="-60%" width="140%" height="220%"><feGaussianBlur stdDeviation="18"/></filter>
    {tile_shadow}
    <clipPath id="sq"><rect x="{x0}" y="{x0}" width="{size}" height="{size}" rx="{rx}"/></clipPath>
  </defs>
  <rect x="{x0}" y="{x0}" width="{size}" height="{size}" rx="{rx}" fill="url(#bg)"{' filter="url(#tile)"' if detail else ''}/>
  <g clip-path="url(#sq)">
    <rect x="{x0}" y="{x0}" width="{size}" height="{size}" fill="url(#glow)"/>
    <ellipse cx="512" cy="764" rx="330" ry="40" fill="#000" opacity="0.5" filter="url(#soft)"/>
  </g>
  <rect x="{x0 + 1.5}" y="{x0 + 1.5}" width="{size - 3}" height="{size - 3}" rx="{rx - 1.5}" fill="none" stroke="#FFF" stroke-opacity="0.07" stroke-width="3"/>

  <!-- the ribbon hangs from the gutter, below the leaves -->
  <path d="M566 700 L606 700 L606 838 L586 818 L566 838 Z" fill="url(#ribbon)"/>
  <!-- boards, the page block, the open leaves -->
  <path d="M512 412 C430 368 300 360 190 382 L190 734 C300 714 430 720 512 760 C594 720 724 714 834 734 L834 382 C724 360 594 368 512 412 Z" fill="#7E2416"/>
  <path d="M512 398 C434 356 312 348 206 368 L206 716 C312 696 434 702 512 744 C590 702 712 696 818 716 L818 368 C712 348 590 356 512 398 Z" fill="#CDBF9F"/>
  <path d="M512 386 C436 344 320 336 220 356 L220 702 C320 682 436 688 512 730 Z" fill="url(#pageL)"/>
  <path d="M512 386 C588 344 704 336 804 356 L804 702 C704 682 588 688 512 730 Z" fill="url(#pageR)"/>
  <path d="M512 388 L512 730" stroke="#B5A584" stroke-width="2"/>
  <path d="M566 728 L606 728 L606 716 L566 722 Z" fill="#000" opacity="0.15"/>

  <!-- the source on the left, its translation on the right -->
  <path d="{left}" fill="none" stroke="#5E5243" stroke-opacity="0.7" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round"/>
  <g fill="#5E5243" fill-opacity="0.55">{right}</g>
</svg>
'''


if __name__ == "__main__":
    open("cipher.svg", "w").write(svg(True))
    open("cipher-small.svg", "w").write(svg(False))
