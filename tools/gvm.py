#!/usr/bin/env python3
"""Exports the textures of a GVM archive (GameCube textures, e.g. inside the game's .prs
texture files) as PNGs, in archive order (the order a texture list is filled in).

    tools/gvm.py <file.prs|file.gvm> <out dir>

Supports the formats SA2's GVRs use: I4, I8, IA8, RGB565, RGB5A3, ARGB8888 and CMPR (DXT1).
Needs Pillow.
"""
import struct
import sys
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import prs  # noqa: E402

BLOCK = {0: (8, 8), 1: (8, 4), 3: (4, 4), 4: (4, 4), 5: (4, 4), 6: (4, 4), 0x0E: (8, 8)}


def rgb565(v):
    return ((v >> 11 & 31) * 255 // 31, (v >> 5 & 63) * 255 // 63, (v & 31) * 255 // 31, 255)


def rgb5a3(v):
    if v & 0x8000:
        return ((v >> 10 & 31) * 255 // 31, (v >> 5 & 31) * 255 // 31, (v & 31) * 255 // 31, 255)
    return ((v >> 8 & 15) * 17, (v >> 4 & 15) * 17, (v & 15) * 17, (v >> 12 & 7) * 255 // 7)


def dxt1(block):
    c0, c1 = struct.unpack(">HH", block[:4])
    p = [rgb565(c0), rgb565(c1)]
    if c0 > c1:
        p.append(tuple((2 * a + b) // 3 for a, b in zip(p[0], p[1])))
        p.append(tuple((a + 2 * b) // 3 for a, b in zip(p[0], p[1])))
    else:
        p.append(tuple((a + b) // 2 for a, b in zip(p[0], p[1])))
        p.append((0, 0, 0, 0))
    out = []
    for row in block[4:8]:
        out.append([p[(row >> (6 - 2 * x)) & 3] for x in range(4)])
    return out


def decode(fmt, w, h, data):
    img = Image.new("RGBA", (w, h))
    px = img.load()
    bw, bh = BLOCK[fmt]
    at = 0

    def put(x, y, c):
        if x < w and y < h:
            px[x, y] = c

    for by in range(0, h, bh):
        for bx in range(0, w, bw):
            if fmt == 0x0E:
                for sub in range(4):
                    sx, sy = bx + (sub & 1) * 4, by + (sub >> 1) * 4
                    for y, row in enumerate(dxt1(data[at:at + 8])):
                        for x, c in enumerate(row):
                            put(sx + x, sy + y, c)
                    at += 8
            elif fmt == 6:
                ar, gb = data[at:at + 32], data[at + 32:at + 64]
                for i in range(16):
                    put(bx + i % 4, by + i // 4, (ar[2 * i + 1], gb[2 * i], gb[2 * i + 1], ar[2 * i]))
                at += 64
            else:
                for y in range(bh):
                    for x in range(bw):
                        if fmt == 0:
                            v = data[at + (y * bw + x) // 2]
                            v = (v >> 4 if x % 2 == 0 else v & 15) * 17
                            c = (v, v, v, 255)
                        elif fmt == 1:
                            v = data[at + y * bw + x]
                            c = (v, v, v, 255)
                        elif fmt == 3:
                            a, v = data[at + 2 * (y * bw + x)], data[at + 2 * (y * bw + x) + 1]
                            c = (v, v, v, a)
                        else:
                            v = struct.unpack_from(">H", data, at + 2 * (y * bw + x))[0]
                            c = rgb565(v) if fmt == 4 else rgb5a3(v)
                        put(bx + x, by + y, c)
                at += bw * bh // 2 if fmt == 0 else bw * bh * (1 if fmt == 1 else 2)
    return img


def textures(d):
    """[(name, gbix, image)] in archive order."""
    if d[:4] != b"GVMH":
        raise ValueError("not a GVM")
    header = struct.unpack_from("<I", d, 4)[0]
    flags, count = struct.unpack_from(">HH", d, 8)
    names = []
    # Entries: u16 id, then per flag: name[28] (8), format u16 (4), size u16 (2), gbix u32 (1).
    entry = 2 + (28 if flags & 8 else 0) + (2 if flags & 4 else 0) + (2 if flags & 2 else 0) + (4 if flags & 1 else 0)
    for i in range(count):
        e = 12 + i * entry
        name = d[e + 2:e + 30].split(b"\0")[0].decode("latin1") if flags & 8 else f"tex{i}"
        names.append(name)
    at = 8 + header
    out = []
    for name in names:
        gbix = 0
        if d[at:at + 4] in (b"GBIX", b"GCIX"):
            size = struct.unpack_from("<I", d, at + 4)[0]
            gbix = struct.unpack_from(">I", d, at + 8)[0]
            at += 8 + size
        if d[at:at + 4] != b"GVRT":
            raise ValueError(f"no GVRT at {at:#x}")
        size = struct.unpack_from("<I", d, at + 4)[0]
        fmt = d[at + 11]
        w, h = struct.unpack_from(">HH", d, at + 12)
        out.append((name, gbix, decode(fmt, w, h, d[at + 16:at + 8 + size])))
        at += 8 + size
    return out


def main():
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    d = src.read_bytes()
    if src.suffix.lower() == ".prs":
        d = prs.decompress(d)
    out.mkdir(parents=True, exist_ok=True)
    for i, (name, gbix, img) in enumerate(textures(d)):
        img.save(out / f"{i:02d}_{name}.png")
        print(f"{i:2} {name} gbix {gbix} {img.width}x{img.height}")


if __name__ == "__main__":
    main()
