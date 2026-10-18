#!/usr/bin/env python3
"""Big the Cat's cameo model, read from the game exe (re/skins.md "Big the Cat cameo").

    tools/bigmodel.py <sonic2app exe (decrypted dump)> [--dump]

The model is static little-endian data in the exe: NJS_OBJECT tree at 0x14E0EAC (chunk models),
motions in a 16-byte table at 0x1521D70 (+0 NJS_MOTION*), textures from the E_BIGTEX archive.
Prints the node tree with world bind positions and a guessed body-part label per node.
"""
import math
import struct
import sys

ROOT = 0x14E0EAC
MOTIONS = 0x1521D70


class Exe:
    def __init__(self, path):
        self.d = open(path, "rb").read()
        d = self.d
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        n = struct.unpack_from("<H", d, pe + 6)[0]
        opt = struct.unpack_from("<H", d, pe + 20)[0]
        self.base = struct.unpack_from("<I", d, pe + 24 + 28)[0]
        self.secs = []
        for i in range(n):
            o = pe + 24 + opt + 40 * i
            vs, va, rs, ro = struct.unpack_from("<IIII", d, o + 8)
            self.secs.append((va, vs, ro, rs))

    def off(self, addr):
        r = addr - self.base
        for va, vs, ro, rs in self.secs:
            if va <= r < va + max(vs, rs):
                if r - va >= rs:
                    raise ValueError(f"{addr:#x} is in uninitialised data")
                return ro + r - va
        raise ValueError(f"{addr:#x} not in the exe")

    def u32(self, a):
        return struct.unpack_from("<I", self.d, self.off(a))[0]

    def f32(self, a, n=3):
        return struct.unpack_from(f"<{n}f", self.d, self.off(a))

    def i32(self, a, n=3):
        return struct.unpack_from(f"<{n}i", self.d, self.off(a))


def rot(axis, bams):
    a = bams * 2 * math.pi / 65536
    c, s = math.cos(a), math.sin(a)
    if axis == "x":
        return [[1, 0, 0], [0, c, -s], [0, s, c]]
    if axis == "y":
        return [[c, 0, s], [0, 1, 0], [-s, 0, c]]
    return [[c, -s, 0], [s, c, 0], [0, 0, 1]]


def mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def nodes(exe, root):
    """Depth-first (the motion's node order): (index, depth, addr, flags, model, world pos)."""
    out = []

    def visit(obj, depth, wm, wp):
        while obj:
            flags, model = exe.u32(obj), exe.u32(obj + 4)
            pos = exe.f32(obj + 8)
            ang = exe.i32(obj + 0x14)
            p = [wp[i] + apply(wm, pos)[i] for i in range(3)]
            m = wm
            # Ninja order: Z, Y, X unless flag 0x20 (ZXY).
            m = mul(m, rot("z", ang[2]))
            m = mul(m, rot("y", ang[1]))
            m = mul(m, rot("x", ang[0]))
            out.append((len(out), depth, obj, flags, model, p, ang))
            child = exe.u32(obj + 0x2C)
            if child:
                visit(child, depth + 1, m, p)
            obj = exe.u32(obj + 0x30)

    visit(root, 0, [[1, 0, 0], [0, 1, 0], [0, 0, 1]], [0, 0, 0])
    return out


def main():
    exe = Exe(sys.argv[1])
    root = exe.u32(ROOT) if False else ROOT
    ns = nodes(exe, root)
    print(f"{len(ns)} nodes; meshes on {sum(1 for n in ns if n[4])}")
    for i, depth, addr, flags, model, p, ang in ns:
        print(f"{i:3} {'  ' * depth}{addr:#x} fl {flags:#04x} {'mesh' if model else '    '} "
              f"world ({p[0]:6.2f},{p[1]:6.2f},{p[2]:6.2f}) ang {[hex(a & 0xffff) for a in ang]}")
    print("motions:")
    for k in range(40):
        m = exe.u32(MOTIONS + 16 * k)
        if m < exe.base or m > exe.base + 0x2000000:
            break
        try:
            frames = exe.u32(m + 4)
            typ = struct.unpack_from("<HH", exe.d, exe.off(m + 8))
        except ValueError:
            break
        print(f"  {k}: motion {m:#x} frames {frames} type {typ[0]:#x} nodes/flags {typ[1]:#x}")


if __name__ == "__main__":
    main()
