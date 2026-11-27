"""Rasterize the Rustman "Cog Disc" logo (same geometry as assets/logo/logo.svg).

Usage:
    python3 scripts/render-icon.py OUT.png SIZE GEAR_HEX        # one PNG
    python3 scripts/render-icon.py OUT.ico ico GEAR_HEX         # Windows icon, 16-256 px

GEAR_HEX is the gear colour: FFFFFF for the app/dock icon (assets/logo/icon-256.png),
1F1A17 for the dark-gear variant used for Windows and macOS (rustman.ico, icon-512.png).
Pure stdlib: supersampled point tests, PNG written with zlib, ICO entries embed PNGs.
"""
import math, struct, sys, zlib

GEAR = [tuple(map(float, p.split())) for p in (
    "58.6 11.3|59.7 3.2|68.3 3.2|69.4 11.3|79.2 13.2|83.3 6.1|91.3 9.4|89.2 17.4|97.5 22.9|104.0 17.9|"
    "110.1 24.0|105.1 30.5|110.6 38.8|118.6 36.7|121.9 44.7|114.8 48.8|116.7 58.6|124.8 59.7|124.8 68.3|"
    "116.7 69.4|114.8 79.2|121.9 83.3|118.6 91.3|110.6 89.2|105.1 97.5|110.1 104.0|104.0 110.1|97.5 105.1|"
    "89.2 110.6|91.3 118.6|83.3 121.9|79.2 114.8|69.4 116.7|68.3 124.8|59.7 124.8|58.6 116.7|48.8 114.8|"
    "44.7 121.9|36.7 118.6|38.8 110.6|30.5 105.1|24.0 110.1|17.9 104.0|22.9 97.5|17.4 89.2|9.4 91.3|"
    "6.1 83.3|13.2 79.2|11.3 69.4|3.2 68.3|3.2 59.7|11.3 58.6|13.2 48.8|6.1 44.7|9.4 36.7|17.4 38.8|"
    "22.9 30.5|17.9 24.0|24.0 17.9|30.5 22.9|38.8 17.4|36.7 9.4|44.7 6.1|48.8 13.2").split("|")]
PLANE = [(38, 66), (92, 42), (72, 94), (62, 72)]
G0, G1 = (0xFF, 0x7A, 0x3D), (0xDE, 0x4F, 0x17)
CREAM, FOLD = (0xFF, 0xF7, 0xF2), (0xDE, 0x4F, 0x17)


def inside(poly, x, y):
    c = False
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            c = not c
        j = i
    return c


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def sample(x, y, gear):
    """Colour at logo-space point (0..128), or None if transparent. Painter's order, top first."""
    r = math.hypot(x - 64, y - 64)
    if r <= 46:
        if seg_dist(x, y, 62, 72, 92, 42) <= 2:
            return FOLD
        if inside(PLANE, x, y):
            return CREAM
        t = max(0.0, min(1.0, (x + y) / 256))  # gradient along the diagonal, as x1=0 y1=0 x2=1 y2=1
        return tuple(round(a + (b - a) * t) for a, b in zip(G0, G1))
    if r <= 53 or (r <= 61.5 and inside(GEAR, x, y)):
        return gear
    return None


def render(size, gear, ss=4):
    rows = []
    k = 128 / size
    for py in range(size):
        row = bytearray([0])
        for px in range(size):
            acc = [0, 0, 0]
            hit = 0
            for sy in range(ss):
                for sx in range(ss):
                    c = sample((px + (sx + 0.5) / ss) * k, (py + (sy + 0.5) / ss) * k, gear)
                    if c:
                        hit += 1
                        for i in range(3):
                            acc[i] += c[i]
            if hit:
                row += bytes([round(acc[0] / hit), round(acc[1] / hit), round(acc[2] / hit), round(255 * hit / ss / ss)])
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def ico(gear, sizes=(16, 24, 32, 48, 64, 128, 256)):
    pngs = [render(s, gear) for s in sizes]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for s, png in zip(sizes, pngs):
        # Width/height 0 means 256.
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(png), offset)
        offset += len(png)
    return head + entries + b"".join(pngs)


if __name__ == "__main__":
    out, size, hexc = sys.argv[1], sys.argv[2], sys.argv[3].lstrip("#")
    gear = tuple(int(hexc[i:i + 2], 16) for i in (0, 2, 4))
    with open(out, "wb") as f:
        f.write(ico(gear) if size == "ico" else render(int(size), gear))
