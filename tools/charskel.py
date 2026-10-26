#!/usr/bin/env python3
"""Reads an SA2 character's node tree from the game's own model file (e.g. sonicmdl.prs).

    tools/charskel.py <game gd_PC dir> sonicmdl 0      # model number 0 of Sonic's file

Prints each node in depth-first order (object, its children, then its siblings: the order
motions use) with its parent, flags, whether it has a mesh, and its world bind position.
Node evaluation (0x42EBDD, REA/listing): unless flag 1, translate by pos; unless flag 2,
rotate (flag 0x20 clear: Z, then Y, then X; set: Y, X, Z, via 0x4277B0); unless flag 4,
scale. Matrices post-multiply (child = parent * T * R * S).
Model files are PRS-compressed and big-endian: (i32 number, u32 offset) pairs ending at -1.
"""
import math
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import prs  # noqa: E402

import numpy as np


def bams(a):
    return a * math.tau / 65536.0


def rot(axis, a):
    c, s = math.cos(bams(a)), math.sin(bams(a))
    m = np.eye(4)
    i, j = {"x": (1, 2), "y": (2, 0), "z": (0, 1)}[axis]
    m[i, i], m[i, j], m[j, i], m[j, j] = c, -s, s, c
    return m


def load(gd_pc, name):
    d = prs.decompress((Path(gd_pc) / f"{name}.prs").read_bytes())
    models = {}
    k = 0
    while True:
        i, o = struct.unpack_from(">iI", d, 8 * k)
        k += 1
        if i == -1:
            break
        models[i] = o
    return d, models


def nodes(d, root):
    """[(index, offset, parent index, flags, has mesh, local matrix, pos, ang)] depth-first."""
    out = []

    def visit(o, parent):
        while o:
            fl, model = struct.unpack_from(">II", d, o)
            pos = struct.unpack_from(">3f", d, o + 8)
            ang = struct.unpack_from(">3i", d, o + 0x14)
            scl = struct.unpack_from(">3f", d, o + 0x20)
            child, sib = struct.unpack_from(">II", d, o + 0x2C)
            m = np.eye(4)
            if not fl & 1:
                t = np.eye(4)
                t[:3, 3] = pos
                m = m @ t
            if not fl & 2:
                order = [("y", 1), ("x", 0), ("z", 2)] if fl & 0x20 else [("z", 2), ("y", 1), ("x", 0)]
                for axis, i in order:
                    if ang[i]:
                        m = m @ rot(axis, ang[i])
            if not fl & 4:
                m = m @ np.diag(list(scl) + [1])
            index = len(out)
            out.append((index, o, parent, fl, model != 0, m, pos, ang))
            if child:
                visit(child, index)
            o = sib

    visit(root, -1)
    return out


def world(tree):
    w = []
    for index, _, parent, *_rest in tree:
        local = tree[index][5]
        w.append(local if parent < 0 else w[parent] @ local)
    return w


if __name__ == "__main__":
    gd, name, number = sys.argv[1], sys.argv[2], int(sys.argv[3])
    d, models = load(gd, name)
    tree = nodes(d, models[number])
    w = world(tree)
    for (index, off, parent, fl, mesh, *_), m in zip(tree, w):
        p = m[:3, 3]
        print(f"{index:2} parent {parent:2} flags {fl:#04x} {'mesh' if mesh else '    '} "
              f"world ({p[0]:6.2f}, {p[1]:6.2f}, {p[2]:6.2f})")
