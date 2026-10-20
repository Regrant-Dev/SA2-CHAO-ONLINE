#!/usr/bin/env python3
"""Renders a converted skin the way the game draws it, to check a conversion without the game.

    tools/skinpreview.py skins/my_skin out.png [--motion 1] [--frames 0,5,10]
    tools/skinpreview.py skins/my_rigged out.png --model model_eggman.sa2mdl \
        --game-motion "<gd_PC>/eggmtn.prs" --motion 2 --frames 0,10,20

Evaluates model.sa2mdl's node tree (translate, rotate Z/Y/X, scale per node, as 0x42EBDD does)
with motions.sa2mot's keys for each node at each frame (linear, like DrawMotion), and draws the
meshes as wireframes from the front and the side, one column per frame. A rigged skin's model
for one character (`--model`) can be posed by that character's own motions from the game's
files (`--game-motion`, motion number `--motion`; nothing of them is kept).
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
    ap.add_argument("--model", default="model.sa2mdl")
    ap.add_argument("--game-motion", type=Path, help="a character's motion file (e.g. sonicmtn.prs)")
    args = ap.parse_args()
    frames = [int(f) for f in args.frames.split(",")]
    if args.game_motion:
        model = Model(args.skin, None, args.model)
        model.keys = game_keys(args.game_motion, args.motion, len(model.nodes))
    else:
        model = Model(args.skin, args.motion, args.model)
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

    def __init__(self, skin, motion=None, file="model.sa2mdl"):
        self.nodes, self.keys = load(skin, motion, file)

    def triangles(self, frame, only=None, textures=None):
        """Every triangle in game coordinates at `frame`, as the game draws them: each node puts
        its vertices in the buffer, then its strips use it. `only`: just those drawn by these
        nodes (depth-first indices). `textures`: a list that gets each triangle's texture id."""
        ms = matrices(self.nodes, self.keys, frame)
        buffer, world = {}, []
        for i, nd in enumerate(self.nodes):
            for status, off, vs in nd["blocks"]:
                for k, v in enumerate(vs):
                    if status is None:
                        buffer[off + k] = (ms[i] @ np.r_[v, 1])[:3]
                        continue
                    # A weighted vertex's share: set (start) or added (middle, end).
                    p, flags = v
                    at = off + (flags & 0xFFFF)
                    share = (ms[i] @ np.r_[p, 1])[:3] * ((flags >> 16) / 255.0)
                    buffer[at] = share if status == 0 else buffer.get(at, 0) + share
            if only is None or i in only:
                world += [[buffer[v] for v in t] for t in nd["tris"]]
                if textures is not None:
                    textures += nd["texes"]
        return world


def load(skin, motion, file="model.sa2mdl"):
    d = (skin / file).read_bytes()
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
            tris, blocks, texes = [], [], []
            tex = 0
            model = u32(o + 4)
            if model:
                vl, pl = u32(model), u32(model + 4)
                # Vertex blocks, each at its offset in the shared buffer: plain (0x29) or
                # weighted shares (0x2C: position, normal, slot | weight << 16; the weight
                # status in the header's flags).
                while u32(vl) & 0xFF != 0xFF:
                    h1, h2 = u32(vl), u32(vl + 4)
                    words, n = h1 >> 16, h2 >> 16
                    if h1 & 0xFF == 0x2C:
                        blocks.append(((h1 >> 8) & 0x7F, h2 & 0xFFFF,
                                       [(struct.unpack_from("<3f", d, vl + 8 + 28 * i),
                                         u32(vl + 8 + 28 * i + 24)) for i in range(n)]))
                    else:
                        blocks.append((None, h2 & 0xFFFF,
                                       [struct.unpack_from("<3f", d, vl + 8 + 24 * i) for i in range(n)]))
                    vl += 4 + words * 4
                p = pl
                while True:
                    h = struct.unpack_from("<H", d, p)[0]
                    t = h & 0xFF
                    if t == 0xFF:
                        break
                    if t == 8:
                        tex = struct.unpack_from("<H", d, p + 2)[0] & 0x1FFF
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
                            texes.append(tex)
                    p += 4 + size * 2
            nodes.append(dict(parent=parent, flags=fl, pos=pos, ang=ang, scl=scl, tris=tris,
                              blocks=blocks, texes=texes))
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


def game_keys(path, number, count):
    """Motion `number` of a character's motion file (PRS, big-endian; re/skins.md "DrawMotion and
    the motion format"), as per-node (position keys, angle keys) for `count` nodes."""
    import prs
    d = prs.decompress(path.read_bytes())
    at, mot = 0, None
    while True:
        mid, _nodes, off = struct.unpack_from(">hHI", d, at)
        if mid == -1:
            break
        if mid == number:
            mot = off
        at += 8
    if mot is None:
        sys.exit(f"no motion {number} in {path.name}")
    mdata, _frames, kind, inp = struct.unpack_from(">IIHH", d, mot)
    n = inp & 0x3F
    present = [k for k in (1, 2, 4) if kind & k]
    keys = []
    for i in range(count):
        ptrs = struct.unpack_from(f">{n}I", d, mdata + i * n * 8)
        counts = struct.unpack_from(f">{n}I", d, mdata + i * n * 8 + n * 4)
        pk, ak = [], []
        for kind_, ptr, c in zip(present, ptrs, counts):
            if not ptr:
                continue
            if kind_ == 1:
                pk = [struct.unpack_from(">I3f", d, ptr + 16 * k) for k in range(c)]
            elif kind_ == 2:
                ak = [struct.unpack_from(">I3i", d, ptr + 16 * k) for k in range(c)]
        keys.append((pk, ak))
    return keys


def sample(keys, frame, angle):
    """The value at `frame` between keys (each (frame, x, y, z)), linear as DrawMotion: angles
    the short way round."""
    if len(keys) == 1 or frame <= keys[0][0]:
        return keys[0][1:]
    for k0, k1 in zip(keys, keys[1:]):
        if k0[0] <= frame < k1[0]:
            t = (frame - k0[0]) / (k1[0] - k0[0])
            if angle:
                return [int(a + ((b - a + 0x8000) % 0x10000 - 0x8000) * t) for a, b in zip(k0[1:], k1[1:])]
            return [a + (b - a) * t for a, b in zip(k0[1:], k1[1:])]
    return keys[-1][1:]


def matrices(nodes, keys, frame):
    out = []
    for i, nd in enumerate(nodes):
        pos, ang = nd["pos"], nd["ang"]
        if keys:
            pk, ak = keys[i]
            if pk:
                pos = sample(pk, frame, False)
            if ak:
                ang = sample(ak, frame, True)
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
