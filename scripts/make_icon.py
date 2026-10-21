#!/usr/bin/env python3
"""Generate the Rusty7800 app icon: a Fuji-inspired mark on a dark tile.

The geometry (three tapered strokes: two bowed outer arms + a straight
center bar) is computed once and emitted both as an SVG master and as
Pillow-rasterized PNGs + a multi-size Windows .ico, so every format
stays pixel-consistent by construction.

Usage:
    python3 scripts/make_icon.py        # writes assets/ under crates/rusty7800-gui
                                       # and site/icon.svg for the web shell

Stdlib + Pillow only.
"""

import math
import os

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
GUI_ASSETS = os.path.normpath(os.path.join(HERE, "..", "crates", "rusty7800-gui", "assets"))
WEB_SITE = os.path.normpath(os.path.join(HERE, "..", "crates", "rusty7800-web", "site"))

SIZE = 1024
STROKE = 120.0      # base stroke width at the bottom of a stroke
TAPER = 0.38        # fraction narrower at the top end
SAMPLES = 72

# Tile: near-black vertical gradient with a soft top sheen.
TILE_TOP = (46, 46, 52)
TILE_BOTTOM = (13, 13, 16)
TILE_RADIUS = 228

# Fuji red.
FUJI = (229, 50, 45)


def qbez(p0, p1, p2, t):
    u = 1.0 - t
    return (
        u * u * p0[0] + 2 * u * t * p1[0] + t * t * p2[0],
        u * u * p0[1] + 2 * u * t * p1[1] + t * t * p2[1],
    )


def stroke_polygon(p0, p1, p2):
    """Tapered-stroke outline (with round caps) for a quadratic centerline."""
    spine = [qbez(p0, p1, p2, i / (SAMPLES - 1)) for i in range(SAMPLES)]
    left, right = [], []
    for i, (x, y) in enumerate(spine):
        t = i / (SAMPLES - 1)
        ax, ay = spine[min(i + 1, SAMPLES - 1)]
        bx, by = spine[max(i - 1, 0)]
        dx, dy = ax - bx, ay - by
        leng = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / leng, dx / leng
        half = (STROKE / 2.0) * (1.0 - TAPER * t)
        left.append((x + nx * half, y + ny * half))
        right.append((x - nx * half, y - ny * half))

    # Round caps: fan around each end point with the end's half-width.
    def cap(center, edge_pt, flip):
        cx, cy = center
        ex, ey = edge_pt
        base = math.atan2(ey - cy, ex - cx)
        pts = []
        for k in range(13):
            a = base + (math.pi * k / 12 if flip else -math.pi * k / 12)
            r = math.hypot(ex - cx, ey - cy)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
        return pts

    start_half = STROKE / 2.0
    end_half = (STROKE / 2.0) * (1.0 - TAPER)
    outline = (
        cap(spine[0], left[0], False)
        + left
        + cap(spine[-1], left[-1], True)
        + list(reversed(cap(spine[-1], right[-1], False)))
        + list(reversed(right))
        + list(reversed(cap(spine[0], right[0], True)))
    )
    # Fix cap radii (cap() used the edge distance, which already is the
    # half-width, so this is consistent) — keep as computed.
    _ = (start_half, end_half)
    return outline


def fuji_strokes():
    """Center bar + two bowed arms, in the spirit of the Atari Fuji."""
    cx = SIZE / 2
    bar = stroke_polygon((cx, 862), (cx, 520), (cx, 178))
    left = stroke_polygon((cx - 232, 862), (cx - 272, 420), (cx - 122, 178))
    right = stroke_polygon((cx + 232, 862), (cx + 272, 420), (cx + 122, 178))
    return [bar, left, right]


def svg_path(poly):
    return "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in poly) + " Z"


def write_svg(path, strokes):
    paths = "\n    ".join(
        f'<path d="{svg_path(s)}" fill="#{FUJI[0]:02x}{FUJI[1]:02x}{FUJI[2]:02x}"/>' for s in strokes
    )
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#{TILE_TOP[0]:02x}{TILE_TOP[1]:02x}{TILE_TOP[2]:02x}"/>
      <stop offset="1" stop-color="#{TILE_BOTTOM[0]:02x}{TILE_BOTTOM[1]:02x}{TILE_BOTTOM[2]:02x}"/>
    </linearGradient>
  </defs>
  <rect width="{SIZE}" height="{SIZE}" rx="{TILE_RADIUS}" fill="url(#tile)"/>
  {paths}
</svg>
"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(svg)


def render_tile():
    img = Image.new("RGB", (SIZE, SIZE))
    d = ImageDraw.Draw(img)
    for y in range(SIZE):
        t = y / (SIZE - 1)
        d.line(
            [(0, y), (SIZE, y)],
            fill=tuple(int(a + (b - a) * t) for a, b in zip(TILE_TOP, TILE_BOTTOM)),
        )
    # Rounded corners: mask the tile.
    mask = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, SIZE, SIZE], radius=TILE_RADIUS, fill=255)
    out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    out.paste(img, mask=mask)
    return out


def main():
    os.makedirs(GUI_ASSETS, exist_ok=True)
    strokes = fuji_strokes()

    svg_path_gui = os.path.join(GUI_ASSETS, "icon.svg")
    write_svg(svg_path_gui, strokes)
    print(f"wrote {svg_path_gui}")

    tile = render_tile()
    mark = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(mark)
    for s in strokes:
        d.polygon(s, fill=FUJI + (255,))
    icon = Image.alpha_composite(tile, mark)

    for px in (16, 32, 48, 256, 512):
        p = os.path.join(GUI_ASSETS, f"icon-{px}.png")
        icon.resize((px, px), Image.LANCZOS).save(p)
        print(f"wrote {p}")

    ico_path = os.path.join(GUI_ASSETS, "icon.ico")
    icon.save(ico_path, sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print(f"wrote {ico_path}")

    site_svg = os.path.join(WEB_SITE, "icon.svg")
    write_svg(site_svg, strokes)
    print(f"wrote {site_svg}")


if __name__ == "__main__":
    main()
