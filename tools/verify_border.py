"""Border-weight regression check for the VoidUI example.

Reads a screen capture of examples/window.exe and measures how much stroke ink
sits on each segment of the button's outline. The old GL backend shaded fill and
stroke as two quads and let the blender combine them, which left the curved
parts of a 1px border tinted by the fill and visibly lighter than the
axis-aligned parts. Uniform ink across straight edges and arcs is the property
that regression would break.

Usage:  python tools/verify_border.py [capture.png]
Exit code is non-zero if the spread exceeds the tolerance.
"""

import math
import sys

from PIL import Image

STROKE = (20, 20, 20)
FILL = (255, 0, 0)
CLEAR = (179, 230, 26)
TOLERANCE = 0.25  # max spread, as a fraction of the mean


def near(c, t, tol=12):
    return all(abs(c[i] - t[i]) <= tol for i in range(3))


def unmix_cache():
    cache = {}

    def stroke_share(c):
        """Split a pixel into stroke / fill / background and return the stroke part."""
        if c in cache:
            return cache[c]
        best = (1e9, 0.0)
        for ai in range(101):
            a = ai / 100.0
            for bi in range(0, 101 - ai, 2):
                b = bi / 100.0
                rest = 1.0 - a - b
                err = sum((a * STROKE[k] + b * FILL[k] + rest * CLEAR[k] - c[k]) ** 2
                          for k in range(3))
                if err < best[0]:
                    best = (err, a)
        cache[c] = best[1]
        return best[1]

    return stroke_share


def main(path):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px = im.load()
    share = unmix_cache()

    xs, ys = [], []
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            if near(px[x, y], CLEAR):
                xs.append(x)
                ys.append(y)
    if not xs:
        print("FAIL: no window client area found in the capture")
        return 1

    cx0, cx1, cy0, cy1 = min(xs), max(xs), min(ys), max(ys)

    reds = [(x, y)
            for y in range(cy0, min(cy0 + 200, h))
            for x in range(cx0, min(cx0 + 300, w))
            if px[x, y][0] > 140 and px[x, y][1] < 80 and px[x, y][2] < 80]
    if not reds:
        print("FAIL: button not found")
        return 1

    rx0, rx1 = min(p[0] for p in reds), max(p[0] for p in reds)
    ry0, ry1 = min(p[1] for p in reds), max(p[1] for p in reds)
    scale = (rx1 - rx0 + 1) / 100.0

    def inside(x, y):
        return cx0 + 1 <= x <= cx1 - 1 and cy0 + 1 <= y <= cy1 - 1

    r = (ry1 - ry0 + 1) / 2.0
    ccx, ccy = rx1 - r + 0.5, ry0 + r + 0.5

    print("display scale %.2f -- a 1.0 logical stroke should read %.2f device px"
          % (scale, scale))
    print()
    print("%-30s %s" % ("segment", "ink / unit length"))
    print("-" * 54)

    x_lo, x_hi = int(rx0 + r + 4), int(rx1 - r - 4)
    total = sum(share(px[x, y])
                for x in range(x_lo, x_hi + 1)
                for y in range(int(ry1 - 4), int(ry1 + 6))
                if inside(x, y))
    straight = total / float(x_hi - x_lo + 1)
    print("%-30s %.3f" % ("bottom edge (axis-aligned)", straight))

    values = []
    # The top and left of the button touch the window edge, so those sectors
    # would sample window chrome rather than the render.
    for a0 in range(-75, 90, 15):
        a1 = a0 + 15
        total, clean = 0.0, True
        for y in range(int(ccy - r - 6), int(ccy + r + 7)):
            for x in range(int(ccx - 2), int(ccx + r + 7)):
                dx, dy = x - ccx, y - ccy
                rad = math.hypot(dx, dy)
                if not (r - 4.0 <= rad <= r + 4.0):
                    continue
                if not (a0 <= math.degrees(math.atan2(dy, dx)) < a1):
                    continue
                if not inside(x, y):
                    clean = False
                    continue
                total += share(px[x, y])
        if not clean:
            continue
        v = total / (r * math.radians(15))
        values.append(v)
        print("%-30s %.3f" % ("right arc %+d..%+d deg" % (a0, a1), v))

    if not values:
        print("FAIL: no measurable arc sectors")
        return 1

    everything = values + [straight]
    mean = sum(everything) / len(everything)
    spread = (max(everything) - min(everything)) / mean

    print()
    print("straight %.3f | arc %.3f..%.3f | spread %.0f%% of mean (tolerance %.0f%%)"
          % (straight, min(values), max(values), spread * 100, TOLERANCE * 100))

    if spread > TOLERANCE:
        print("FAIL: border weight is uneven around the outline")
        return 1

    print("PASS: border weight is uniform across straight edges and arcs")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "build/capture.png"))
