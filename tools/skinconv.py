#!/usr/bin/env python3
"""Turns a glTF model (e.g. a Sketchfab download) into an SA2 Chao Online skin folder.

    tools/skinconv.py <scene.gltf> skins/lightning_mcqueen --name "Lightning McQueen" \
        --length 16 --characters sonic --kart

Copy the skin folder into the mod's `skins/` folder (the mod puts its textures where the mod
loader finds texture packs). The skin folder holds:
  model.sa2mdl   a Ninja chunk model (re/skins.md): a hidden root node whose children are the
                 mesh split into pieces of at most MAX_VERTS vertices (the game's own karts use
                 up to 510 in one vertex block), with pointers stored as file offsets;
  textures/      an SA2 Mod Loader texture pack (index.txt + PNGs), in the model's texture order;
  skin.ini       name, texture count, which characters it rides on and how big, whether it's
                 also a kart, and the model's credit line.

The model is turned to SA2's axes (Y up, facing +Z), centred on the ground at the origin and
scaled so its longest side is --length units (a kart is about 16, Sonic about 10 tall).
Needs numpy and Pillow.
"""
import argparse
import base64
import json
import math
import re
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

MAX_VERTS = 500
MAX_TEXTURE = 512
# SA Tools' "SA2MDL" magic, file format version 3 in the top byte.
SA2MDL = 0x4C444D324153 | (3 << 56)
# Global texture indices for the pack (unique per skin: from a hash of its name).
GBIX_BASE = 0x5A000000

COMPONENTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
DTYPES = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}


def load_gltf(path):
    g = json.loads(path.read_text())
    buffers = []
    for b in g["buffers"]:
        uri = b["uri"]
        if uri.startswith("data:"):
            buffers.append(base64.b64decode(uri.split(",", 1)[1]))
        else:
            buffers.append((path.parent / uri).read_bytes())

    def accessor(i):
        a = g["accessors"][i]
        view = g["bufferViews"][a["bufferView"]]
        dtype = np.dtype(DTYPES[a["componentType"]])
        n = COMPONENTS[a["type"]]
        start = view.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = view.get("byteStride", 0) or dtype.itemsize * n
        raw = buffers[view["buffer"]]
        rows = np.ndarray((a["count"], n), dtype=dtype, buffer=raw, offset=start, strides=(stride, dtype.itemsize))
        out = rows.astype(np.float64)
        if a.get("normalized"):
            out /= float(np.iinfo(dtype).max)
        return out

    return g, accessor


def node_matrix(n):
    if "matrix" in n:
        return np.array(n["matrix"], dtype=np.float64).reshape(4, 4).T
    t = np.eye(4)
    t[:3, 3] = n.get("translation", [0, 0, 0])
    x, y, z, w = n.get("rotation", [0, 0, 0, 1])
    r = np.eye(4)
    r[:3, :3] = [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ]
    s = np.diag(list(n.get("scale", [1, 1, 1])) + [1])
    return t @ r @ s


def meshes(g, accessor):
    """Every triangle list in the scene, in world space: (material, positions, normals, uvs, tris)."""
    out = []

    def visit(i, parent):
        n = g["nodes"][i]
        m = parent @ node_matrix(n)
        if "mesh" in n:
            normal_m = np.linalg.inv(m[:3, :3]).T
            for p in g["meshes"][n["mesh"]]["primitives"]:
                if p.get("mode", 4) != 4:
                    continue
                pos = accessor(p["attributes"]["POSITION"])
                pos = (np.c_[pos, np.ones(len(pos))] @ m.T)[:, :3]
                if "NORMAL" in p["attributes"]:
                    nrm = accessor(p["attributes"]["NORMAL"]) @ normal_m.T
                    nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-9)
                else:
                    nrm = np.tile([0.0, 1.0, 0.0], (len(pos), 1))
                uv = accessor(p["attributes"]["TEXCOORD_0"]) if "TEXCOORD_0" in p["attributes"] else np.zeros((len(pos), 2))
                idx = accessor(p["indices"])[:, 0].astype(np.int64) if "indices" in p else np.arange(len(pos))
                tris = idx.reshape(-1, 3)
                if np.linalg.det(m[:3, :3]) < 0:
                    tris = tris[:, ::-1]
                out.append((p.get("material"), pos, nrm, uv, tris))
        for c in n.get("children", []):
            visit(c, m)

    scene = g["scenes"][g.get("scene", 0)]
    for root in scene["nodes"]:
        visit(root, np.eye(4))
    return out


def texture_for(g, gltf_path, material):
    """The material's base colour image (RGBA, at most MAX_TEXTURE, power-of-two sides), or a
    1x1 of its colour factor."""
    mat = g["materials"][material] if material is not None else {}
    pbr = mat.get("pbrMetallicRoughness", {})
    factor = pbr.get("baseColorFactor", [1, 1, 1, 1])
    tex = pbr.get("baseColorTexture")
    if tex is None:
        c = tuple(int(round(max(0.0, min(1.0, v)) * 255)) for v in factor)
        return Image.new("RGBA", (8, 8), c)
    src = g["images"][g["textures"][tex["index"]]["source"]]
    img = Image.open(gltf_path.parent / src["uri"]).convert("RGBA")
    if factor[:3] != [1, 1, 1]:
        r, gg, b, a = img.split()
        r = r.point(lambda v: v * factor[0])
        gg = gg.point(lambda v: v * factor[1])
        b = b.point(lambda v: v * factor[2])
        img = Image.merge("RGBA", (r, gg, b, a))
    side = lambda v: min(MAX_TEXTURE, 1 << max(3, round(math.log2(max(v, 1)))))
    return img.resize((side(img.width), side(img.height)), Image.LANCZOS)


class Writer:
    def __init__(self):
        self.buf = bytearray(16)  # header written at the end

    def here(self):
        return len(self.buf)

    def align(self, n=4):
        while len(self.buf) % n:
            self.buf.append(0)

    def put(self, fmt, *values):
        at = self.here()
        self.buf += struct.pack("<" + fmt, *values)
        return at

    def patch(self, at, fmt, *values):
        struct.pack_into("<" + fmt, self.buf, at, *values)


def vertex_chunk(w, pos, nrm):
    w.align()
    at = w.here()
    n = len(pos)
    words = 1 + n * 6  # header 2 + data
    w.put("I", 0x29 | (words << 16))  # CnkV_VN
    w.put("I", 0 | (n << 16))  # index offset 0, count
    for p, q in zip(pos, nrm):
        w.put("6f", *p, *q)
    w.put("I", 0xFF)  # end
    return at


def has_alpha(img):
    """Whether an image has see-through pixels (drawn with alpha blending)."""
    return img.mode == "RGBA" and img.getextrema()[3][0] < 255


def poly_chunks(w, groups, alpha=()):
    """groups: [(texture id, [(i0, uv0), (i1, uv1), (i2, uv2)] per triangle)]; textures in
    `alpha` are drawn with alpha blending (strip flag 0x08; the material's blend is already
    source alpha / inverse source alpha)."""
    w.align()
    at = w.here()
    for tex, tris in groups:
        # Diffuse material (white), as the game's karts start each mesh (type 0x11, flags 0x25).
        w.put("HHI", 0x11 | (0x25 << 8), 2, 0xFFFFFFFF)
        # Tiny texture id: bilinear (0x4000), mipmap adjust 4, repeat.
        w.put("HH", 0x08 | (0x04 << 8), 0x4000 | tex)
        # Triangle strips with UVs (0x41 CnkS_UVN), one triangle per strip; flags: ignore
        # specular and ambient like the game's karts, double sided (0x10).
        for start in range(0, len(tris), 6000):
            part = tris[start:start + 6000]
            words = 1 + 10 * len(part)
            pad = words % 2
            flags = 0x16 | (0x08 if tex in alpha else 0)
            w.put("HHH", 0x41 | (flags << 8), words + pad, len(part))
            for tri in part:
                w.put("h", 3)
                for i, (u, v) in tri:
                    w.put("Hhh", i, int(round(u * 256)), int(round(v * 256)))
            if pad:
                w.put("H", 0)
    w.put("H", 0xFF)
    w.align()
    return at


def build(parts, textures, out, name, length, credit, extra):
    # SA2 axes: glTF is already Y up; SA2 models face +Z like glTF's front.
    allpos = np.vstack([p for _, p, _, _, _ in parts])
    lo, hi = allpos.min(axis=0), allpos.max(axis=0)
    scale = length / max(hi - lo)
    offset = np.array([-(lo[0] + hi[0]) / 2, -lo[1], -(lo[2] + hi[2]) / 2])

    # Split into nodes of at most MAX_VERTS vertices, keeping each material's triangles together.
    nodes = []  # (positions, normals, groups)
    for material, pos, nrm, uv, tris in parts:
        tex = textures[material]
        pos = (pos + offset) * scale
        remap, vp, vn, group = {}, [], [], []

        def flush():
            if group:
                nodes.append((np.array(vp), np.array(vn), [(tex, list(group))]))

        for tri in tris:
            new = [i for i in tri if i not in remap]
            if len(vp) + len(new) > MAX_VERTS:
                flush()
                remap, vp, vn, group = {}, [], [], []
            out_tri = []
            for i in tri:
                if i not in remap:
                    remap[i] = len(vp)
                    vp.append(pos[i])
                    vn.append(nrm[i])
                out_tri.append((remap[i], (uv[i][0], uv[i][1])))
            group.append(out_tri)
        flush()

    w = Writer()
    objects = []
    for vp, vn, groups in nodes:
        vl = vertex_chunk(w, vp, vn)
        pl = poly_chunks(w, groups, {i for i, img in _images.items() if has_alpha(img)})
        center = (vp.min(axis=0) + vp.max(axis=0)) / 2
        radius = float(np.max(np.linalg.norm(vp - center, axis=1)))
        w.align()
        model = w.put("II4f", vl, pl, *center, radius)
        objects.append(model)

    # Objects: the hidden root first (unit position/angle/scale, hide), its children chained as
    # siblings (unit transforms, no children).
    w.align()
    root = w.put("II3f3i3fII", 0x0F, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0)
    prev = None
    for model in objects:
        at = w.put("II3f3i3fII", 0x17, model, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0)
        if prev is None:
            w.patch(root + 0x2C, "I", at)
        else:
            w.patch(prev + 0x30, "I", at)
        prev = at
    # Metadata: just the end marker.
    meta = w.put("II", 0x444E45, 0)
    w.patch(0, "QII", SA2MDL, root, meta)

    out.mkdir(parents=True, exist_ok=True)
    (out / "model.sa2mdl").write_bytes(bytes(w.buf))

    # Texture pack: index.txt lines "global index,file".
    tex_dir = out / "textures"
    tex_dir.mkdir(exist_ok=True)
    gbix = GBIX_BASE | (hash_name(name) & 0xFFFF) << 8
    lines = []
    for i, img in enumerate(unique_images(textures)):
        file = f"tex{i:02d}.png"
        img.save(tex_dir / file)
        lines.append(f"{gbix + i},{file}")
    (tex_dir / "index.txt").write_text("\n".join(lines) + "\n")

    ini = [
        f"name={name}",
        f"textures={len(lines)}",
        *extra,
        f"triangles={sum(len(t) for _, _, _, _, t in parts)}",
        f"pieces={len(nodes)}",
        f"length={length}",
        f"height={round(float((hi[1] - lo[1]) * scale), 2)}",
    ]
    if credit:
        ini.append(f"credit={credit}")
    (out / "skin.ini").write_text("\n".join(ini) + "\n")
    print(f"{name}: {len(nodes)} pieces, {len(lines)} textures, {len(w.buf)} bytes, scale {scale:.3f}")


def hash_name(name):
    h = 0x811C9DC5
    for c in name.encode():
        h = ((h ^ c) * 0x01000193) & 0xFFFFFFFF
    return h


_images = {}


def unique_images(textures):
    return [_images[k] for k in sorted(set(textures.values()))]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("gltf", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--name", required=True)
    ap.add_argument("--length", type=float, default=16.0, help="longest side in game units")
    ap.add_argument("--characters", default="", help="characters it rides on, e.g. sonic,shadow")
    ap.add_argument("--character-scale", type=float, default=1.0, help="size on a character")
    ap.add_argument("--turn", type=float, default=-90, help="degrees to face the way the character runs")
    ap.add_argument("--kart", action="store_true", help="also our kart in kart races")
    ap.add_argument("--lean", type=float, default=1.0, help="lean into turns: 1, 0 off, -1 reversed")
    args = ap.parse_args()

    g, accessor = load_gltf(args.gltf)
    parts = meshes(g, accessor)
    if not parts:
        sys.exit("no triangles in the model")

    # One texture per material, in material order (the texture id in the model).
    textures = {}
    for material, *_ in parts:
        if material not in textures:
            i = len(_images)
            _images[i] = texture_for(g, args.gltf, material)
            textures[material] = i

    credit = ""
    lic = args.gltf.parent / "license.txt"
    if lic.exists():
        m = re.search(r'This work is based on .*', lic.read_text())
        credit = m.group(0).strip() if m else ""
    extra = [
        f"characters={args.characters}",
        f"character_scale={args.character_scale}",
        f"turn={args.turn}",
        f"kart={'on' if args.kart else 'off'}",
        f"lean={args.lean}",
    ]
    build(parts, textures, args.out, args.name, args.length, credit, extra)


if __name__ == "__main__":
    main()
