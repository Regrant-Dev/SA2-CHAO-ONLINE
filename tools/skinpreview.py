#!/usr/bin/env python3
"""Renders a converted skin the way the game draws it, to check a conversion without the game.

    tools/skinpreview.py skins/big_anim out.png [--motion 1] [--frames 0,5,10]

Evaluates model.sa2mdl's node tree (translate, rotate Z/Y/X, scale per node, as 0x42EBDD does)
with motions.sa2mot's keys for each node at each frame (linear, like DrawMotion), and draws the
meshes as wireframes from the front and the side, one column per frame.
"""
import argparse
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).parent))
import charskel  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("skin", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--motion", type=int, default=None)
    ap.add_argument("--frames", default="0")
    args = ap.parse_args()
    frames = [int(f) for f in args.frames.split(",")]
    model = Model(args.skin, args.motion)
    W, H = 300, 420
    img = Image.new("RGB", (W * len(frames), H * 2), "white")
    dr = ImageDraw.Draw(img)
    for col, f in enumerate(frames):
        world = model.triangles(f)
        pts = np.array([p for t in world for p in t])
        lo, hi = pts.min(axis=0), pts.max(axis=0)
        s = (H - 40) / max(hi[1] - lo[1], 1e-6)
        for row, (ax, sign) in enumerate([(2, 1), (0, -1)]):  # front (z across), side (x across)
            for t in world:
                dr.polygon([(col * W + W / 2 + sign * p[ax] * s, row * H + H - 20 - (p[1] - lo[1]) * s) for p in t],
                           outline=(0, 70, 140))
            dr.text((col * W + 4, row * H + 4), f"frame {f} {'front' if row == 0 else 'side'}", fill="black")
        print(f"frame {f}: height {hi[1] - lo[1]:.2f}, feet {lo[1]:.2f}")
    img.save(args.out)


class Model:
    """A converted skin's model, posed by one of its motions (or none)."""

    def __init__(self, skin, motion=None):
        self.nodes, self.keys = load(skin, motion)

    def triangles(self, frame):
        """Every triangle in game coordinates at `frame`, as the game draws them: each node puts
        its vertices in the buffer, then its strips use it."""
        ms = matrices(self.nodes, self.keys, frame)
        buffer, world = {}, []
        for i, nd in enumerate(self.nodes):
            for off, vs in nd["blocks"]:
                for k, v in enumerate(vs):
                    buffer[off + k] = (ms[i] @ np.r_[v, 1])[:3]
            world += [[buffer[v] for v in t] for t in nd["tris"]]
        return world


def load(skin, motion):
    d = (skin / "model.sa2mdl").read_bytes()
    u32 = lambda a: struct.unpack_from("<I", d, a)[0]
    # Nodes in depth-first order, each with its mesh triangles (local coordinates).
    nodes = []

    def visit(o, parent):
        while o:
            idx = len(nodes)
            fl = u32(o)
            pos = struct.unpack_from("<3f", d, o + 8)
            ang = struct.unpack_from("<3i", d, o + 0x14)
            scl = struct.unpack_from("<3f", d, o + 0x20)
            tris, blocks = [], []
            model = u32(o + 4)
            if model:
                vl, pl = u32(model), u32(model + 4)
                # Vertex blocks, each at its offset in the shared buffer.
                while u32(vl) & 0xFF != 0xFF:
                    words, h2 = u32(vl) >> 16, u32(vl + 4)
                    n = h2 >> 16
                    blocks.append((h2 & 0xFFFF,
                                   [struct.unpack_from("<3f", d, vl + 8 + 24 * i) for i in range(n)]))
                    vl += 4 + words * 4
                p = pl
                while True:
                    h = struct.unpack_from("<H", d, p)[0]
                    t = h & 0xFF
                    if t == 0xFF:
                        break
                    if t == 8:
                        p += 4
                        continue
                    size = struct.unpack_from("<H", d, p + 2)[0]
                    if t == 0x41:
                        cnt = struct.unpack_from("<H", d, p + 4)[0] & 0x3FFF
                        q = p + 6
                        for _ in range(cnt):
                            ln = struct.unpack_from("<h", d, q)[0]
                            q += 2
                            ids = [struct.unpack_from("<H", d, q + 6 * k)[0] for k in range(abs(ln))]
                            q += 6 * abs(ln)
                            tris.append(ids)
                    p += 4 + size * 2
            nodes.append(dict(parent=parent, flags=fl, pos=pos, ang=ang, scl=scl, tris=tris,
                              blocks=blocks))
            c = u32(o + 0x2C)
            if c:
                visit(c, idx)
            o = u32(o + 0x30)

    visit(u32(8), -1)
    keys = None
    if motion is not None:
        m = (skin / "motions.sa2mot").read_bytes()
        count, table = struct.unpack_from("<II", m, 8)
        mot = struct.unpack_from("<I", m, table + 4 * motion)[0]
        mdata, nframes = struct.unpack_from("<II", m, mot)
        keys = []
        for i in range(len(nodes)):
            pa, aa, pc, ac = struct.unpack_from("<4I", m, mdata + 16 * i)
            keys.append((
                [struct.unpack_from("<I3f", m, pa + 16 * k) for k in range(pc)],
                [struct.unpack_from("<I3i", m, aa + 16 * k) for k in range(ac)],
            ))
    return nodes, keys


def matrices(nodes, keys, frame):
    out = []
    for i, nd in enumerate(nodes):
        pos, ang = nd["pos"], nd["ang"]
        if keys:
            pk, ak = keys[i]
            if pk:
                pos = pk[min(frame, len(pk) - 1)][1:]
            if ak:
                ang = ak[min(frame, len(ak) - 1)][1:]
        fl = nd["flags"]
        mm = np.eye(4)
        if not fl & 1:
            t = np.eye(4)
            t[:3, 3] = pos
            mm = mm @ t
        if not fl & 2:
            order = [("y", 1), ("x", 0), ("z", 2)] if fl & 0x20 else [("z", 2), ("y", 1), ("x", 0)]
            for axis, k in order:
                if ang[k]:
                    mm = mm @ charskel.rot(axis, ang[k])
        if not fl & 4:
            mm = mm @ np.diag(list(nd["scl"]) + [1])
        out.append(mm if nd["parent"] < 0 else out[nd["parent"]] @ mm)
    return out


if __name__ == "__main__":
    main()
