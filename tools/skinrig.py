#!/usr/bin/env python3
"""Rigged skins: rebuilds a model on an SA2 character's own skeleton (re/skins.md "Rigged skins").

    tools/skinrig.py <scene.gltf> skins/my_skin --name "My Skin" \\
        --game "<Sonic Adventure 2>/resource/gd_PC" --character sonic,knuckles

The model's vertices are split by body part (each vertex goes with its strongest bone), each
part is turned and stretched to lie along the character's matching limb in its bind pose, and
the result is written as a model with exactly the character's node tree (same nodes, same
order, same transforms), each part in its node's space. The game's own animations then move
it, through the character's own display. The skeleton is read from the player's game files
when converting; no game data is stored here.

Sources: a glTF with a skin whose bones use Unreal-style names (pelvis, spine_01..05,
clavicle_l, upperarm_l, lowerarm_l, hand_l, thigh_l, calf_l, foot_l, ball_l, neck, head, ...),
or `--big <sonic2app.exe>`: the game's own Big the Cat cameo (re/skins.md), read from the
player's exe with its textures from e_bigtex.prs.
"""
import argparse
import json
import re
import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import charskel  # noqa: E402
import skinconv  # noqa: E402

# The 62-node humanoid tree shared by Sonic, Shadow, Amy, Metal Sonic, Knuckles, Rouge, Tails,
# Tikal and Chaos: per body part, (node its mesh goes on, node at its start joint, node at its
# end joint or None). Left is +Z, the character faces -X. The torso parts go on together,
# anchored at the hips' start joint.
HUMANOID = {
    "hips": (1, 1, 32),
    "chest": (32, 32, 34),
    "upperchest": (33, 33, 43),
    "head": (44, 43, None),
    "shoulder_l": (35, 34, 35),
    "upperarm_l": (37, 37, 38),
    "forearm_l": (38, 38, 39),
    "hand_l": (42, 39, None),
    "shoulder_r": (54, 34, 54),
    "upperarm_r": (56, 56, 57),
    "forearm_r": (57, 57, 58),
    "hand_r": (61, 58, None),
    "thigh_r": (7, 7, 8),
    "shin_r": (9, 8, 10),
    "foot_r": (13, 13, 15),
    "toe_r": (14, 14, None),
    "thigh_l": (17, 17, 18),
    "shin_l": (19, 18, 20),
    "foot_l": (23, 23, 25),
    "toe_l": (24, 24, None),
    "tail": (27, 26, 30),
}
# Eggman's 46-node tree (eggmdl.prs model 230, re/skins.md): no collarbones, so the shoulders
# go on the shoulder joints (28, 39); legs 7-14 / 16-23, arms 29-33 / 40-45, head 38.
EGGMAN = {
    "hips": (1, 1, 25),
    "chest": (25, 25, 26),
    "upperchest": (26, 26, 27),
    "head": (38, 36, None),
    "shoulder_l": (28, 27, 29),
    "upperarm_l": (29, 29, 30),
    "forearm_l": (31, 30, 32),
    "hand_l": (33, 32, None),
    "shoulder_r": (39, 27, 40),
    "upperarm_r": (40, 40, 41),
    "forearm_r": (42, 41, 43),
    "hand_r": (44, 43, None),
    "thigh_r": (7, 7, 8),
    "shin_r": (9, 8, 10),
    "foot_r": (12, 12, 14),
    "toe_r": (13, 13, None),
    "thigh_l": (16, 16, 17),
    "shin_l": (18, 17, 19),
    "foot_l": (21, 21, 23),
    "toe_l": (22, 22, None),
}
# The torso keeps its own shape as one block, anchored at the hips: the characters' torso
# segments are tiny (hips to waist is 0.1), so fitting each one would crush it.
TORSO = {"hips", "chest", "upperchest", "head"}
# Parts that keep their own length (only turned): the characters' feet are their big shoes.
NO_STRETCH = {"foot_l", "foot_r"}
# A part with no end joint keeps its parent part's turn.
PARENT_PART = {
    "head": "upperchest", "hand_l": "forearm_l", "hand_r": "forearm_r",
    "toe_l": "foot_l", "toe_r": "foot_r",
}
# Nodes a skin must leave empty on the humanoid tree: the displays swap mouth shapes into these
# (re/skins.md).
KEEP_EMPTY = {46, 47, 48, 49}

CHARACTERS = {
    # name: (model file, body model number, body slots the display draws (the same tree), the
    # CharObj2 field of its texture list (its display), its part map)
    "sonic": ("sonicmdl", 0, [0, 31, 48], 0x394, HUMANOID),
    "shadow": ("teriosmdl", 65, [65, 104, 123], 0x394, HUMANOID),
    "amy": ("amymdl", 395, [395], 0x394, HUMANOID),
    "metalsonic": ("metalsonicmdl", 418, [418], 0x394, HUMANOID),
    "knuckles": ("knuckmdl", 142, [142, 169], 0x3F0, HUMANOID),
    "rouge": ("rougemdl", 171, [171, 206], 0x3F0, HUMANOID),
    "tails": ("milesmdl", 208, [208, 528], 0x3B0, HUMANOID),
    "eggman": ("eggmdl", 230, [230, 527], 0x360, EGGMAN),
}

# Unreal-style bone names -> body part (first match wins; _l/_r kept).
BONE_PARTS = [
    (r"^(pelvis|root|attach)", "hips"),
    (r"^spine_0[12]", "chest"),
    (r"^(spine_0[345]|neck)", "upperchest"),
    (r"^head", "head"),
    (r"^clavicle_(l|r)", "shoulder_{}"),
    (r"^upperarm.*_(l|r)", "upperarm_{}"),
    (r"^lowerarm.*_(l|r)", "forearm_{}"),
    (r"^(hand|index|middle|ring|pinky|thumb|weapon)\w*_(l|r)", "hand_{}"),
    (r"^thigh.*_(l|r)", "thigh_{}"),
    (r"^calf.*_(l|r)", "shin_{}"),
    (r"^foot_(l|r)", "foot_{}"),
    (r"^ball_(l|r)", "foot_{}"),
]
# Where each part's joints are in the source skeleton: (start bone, end bone).
PART_BONES = {
    "hips": ("pelvis", "spine_01"), "chest": ("spine_01", "spine_03"),
    "upperchest": ("spine_03", "head"), "head": ("head", None),
    "shoulder_l": ("clavicle_l", "upperarm_l"), "upperarm_l": ("upperarm_l", "lowerarm_l"),
    "forearm_l": ("lowerarm_l", "hand_l"), "hand_l": ("hand_l", None),
    "shoulder_r": ("clavicle_r", "upperarm_r"), "upperarm_r": ("upperarm_r", "lowerarm_r"),
    "forearm_r": ("lowerarm_r", "hand_r"), "hand_r": ("hand_r", None),
    "thigh_l": ("thigh_l", "calf_l"), "shin_l": ("calf_l", "foot_l"),
    "foot_l": ("foot_l", "ball_l"), "toe_l": ("ball_l", None),
    "thigh_r": ("thigh_r", "calf_r"), "shin_r": ("calf_r", "foot_r"),
    "foot_r": ("foot_r", "ball_r"), "toe_r": ("ball_r", None),
}
MAX_NODE_VERTS = 800


def part_of(name):
    # As named, then without an exporter's "_NN" suffix (Sketchfab: "spine_01_04").
    for base in (name.lower(), clean(name)):
        for pattern, part in BONE_PARTS:
            m = re.match(pattern, base)
            if m:
                side = m.groups()[-1] if m.groups() and m.groups()[-1] in ("l", "r") else ""
                return part.format(side)
    return None


def clean(name):
    return re.sub(r"_\d+$", "", name.lower())


def rotation_between(a, b):
    a = a / np.linalg.norm(a)
    b = b / np.linalg.norm(b)
    v = np.cross(a, b)
    c = float(np.dot(a, b))
    if c < -0.999999:
        axis = np.cross(a, [1, 0, 0])
        if np.linalg.norm(axis) < 1e-6:
            axis = np.cross(a, [0, 1, 0])
        axis /= np.linalg.norm(axis)
        return 2 * np.outer(axis, axis) - np.eye(3)
    k = np.array([[0, -v[2], v[1]], [v[2], 0, -v[0]], [-v[1], v[0], 0]])
    return np.eye(3) + k + k @ k / (1 + c)


def load_source(path):
    """The skinned mesh: (bone names, bone parents, bone bind positions (mesh space),
    [(material, positions, normals, uvs, triangles, strongest bone per vertex)])."""
    g, accessor = skinconv.load_gltf(path)
    skin = g["skins"][0]
    joints = skin["joints"]
    names = [g["nodes"][j].get("name", f"bone{i}") for i, j in enumerate(joints)]
    index_of = {j: i for i, j in enumerate(joints)}
    parents = [-1] * len(joints)
    for n, node in enumerate(g["nodes"]):
        for c in node.get("children", []):
            if c in index_of and n in index_of:
                parents[index_of[c]] = index_of[n]
    ibm = accessor(skin["inverseBindMatrices"]).reshape(-1, 4, 4).transpose(0, 2, 1)
    bind = np.array([np.linalg.inv(m)[:3, 3] for m in ibm])
    parts = []
    for node in g["nodes"]:
        if "mesh" not in node or "skin" not in node:
            continue
        for p in g["meshes"][node["mesh"]]["primitives"]:
            a = p["attributes"]
            pos = accessor(a["POSITION"])
            nrm = accessor(a["NORMAL"]) if "NORMAL" in a else np.tile([0.0, 1.0, 0.0], (len(pos), 1))
            uv = accessor(a["TEXCOORD_0"]) if "TEXCOORD_0" in a else np.zeros((len(pos), 2))
            j = accessor(a["JOINTS_0"]).astype(int)
            w = accessor(a["WEIGHTS_0"])
            strongest = j[np.arange(len(j)), np.argmax(w, axis=1)]
            tris = accessor(p["indices"])[:, 0].astype(int).reshape(-1, 3)
            parts.append((p.get("material"), pos, nrm, uv, tris, strongest))
    return g, names, parents, bind, parts


class Source:
    """A model split into body parts: per part its joints (start, end or None) in the
    model's own space, its triangles ((part, texture, [(position, normal, uv)] x 3)), its
    textures (images), the axis it faces, and a credit line."""

    def __init__(self):
        self.joints = {}
        self.tris = []
        self.images = []
        self.forward = "+z"
        self.credit = ""


def gltf_source(path):
    g, names, parents, bind, src_parts = load_source(path)
    # Bone -> body part; a bone that isn't one (armour, helpers) goes with the nearest one that is.
    named = [part_of(n) for n in names]
    known = [i for i, p in enumerate(named) if p]
    part = list(named)
    for i, p in enumerate(named):
        if p is None:
            nearest = min(known, key=lambda j: np.linalg.norm(bind[j] - bind[i]))
            part[i] = named[nearest]
    by_name = {}
    for i, n in enumerate(names):
        by_name.setdefault(n.lower(), i)
        by_name.setdefault(clean(n), i)
    src = Source()
    for name, (sb, eb) in PART_BONES.items():
        if sb in by_name:
            end = bind[by_name[eb]] if eb and eb in by_name else None
            src.joints[name] = (bind[by_name[sb]], end)
    textures = {}
    for material, pos, nrm, uv, tris, strongest in src_parts:
        if material not in textures:
            textures[material] = len(src.images)
            src.images.append(skinconv.texture_for(g, path, material))
        for tri in tris:
            names3 = [part[strongest[v]] for v in tri]
            src.tris.append((max(set(names3), key=names3.count), textures[material],
                             [(pos[v], nrm[v], (uv[v][0], uv[v][1])) for v in tri]))
    lic = path.parent / "license.txt"
    if lic.exists():
        m = re.search(r"This work is based on .*", lic.read_text())
        src.credit = m.group(0).strip() if m else ""
    return src


# Big's cameo (tools/bigmodel.py): per part, (his nodes whose meshes go there, start joint
# node, end joint node or None). He faces -X with his left side at +Z, like the characters.
BIG_PARTS = {
    "hips": ([23, 71, 72], 1, 58),
    "chest": ([70], 1, 58),
    "head": ([61, 62, 63, 64, 65, 66, 67, 68], 59, None),
    "upperarm_r": ([37], 28, 29), "forearm_r": ([36], 29, 30), "hand_r": ([32, 33, 34, 35], 30, None),
    "upperarm_l": ([51], 39, 40), "forearm_l": ([50], 40, 41),
    "hand_l": ([49, 43, 44, 45, 46, 47, 48], 41, None),
    "thigh_r": ([10], 4, 5), "shin_r": ([9], 5, 6), "foot_r": ([7, 8], 6, 8),
    "thigh_l": ([18], 12, 13), "shin_l": ([15], 13, 14), "foot_l": ([16, 17], 14, 17),
    "tail": ([22], 20, 21),
}


# Big's shoulder joints (right, left), and his fishing rod's nodes.
BIG_SHOULDERS = {28: 0x3C00, 39: -0x3C00}
BIG_ROD = {46, 47, 48}


def big_source(exe_path, gd_pc, t_pose=False):
    """Big's cameo model. `t_pose`: arms raised out to the sides and no fishing rod (for an
    auto-rigger like Mixamo; his bind pose has the arms hanging against his legs)."""
    import bigmodel
    import gvm
    import prs
    exe = bigmodel.Exe(exe_path)
    # His tree with world bind matrices, evaluated like the characters' (charskel.nodes).
    tree, worlds = [], []

    def visit(obj, parent_world):
        while obj:
            fl, model = exe.u32(obj), exe.u32(obj + 4)
            pos, ang, scl = exe.f32(obj + 8), exe.i32(obj + 0x14), exe.f32(obj + 0x20)
            m = np.eye(4)
            if not fl & 1:
                t = np.eye(4)
                t[:3, 3] = pos
                m = m @ t
            if not fl & 2:
                order = [("y", 1), ("x", 0), ("z", 2)] if fl & 0x20 else [("z", 2), ("y", 1), ("x", 0)]
                for axis, i in order:
                    if ang[i]:
                        m = m @ charskel.rot(axis, ang[i])
            if not fl & 4:
                m = m @ np.diag(list(scl) + [1])
            w = parent_world @ m
            raise_by = BIG_SHOULDERS.get(len(tree)) if t_pose else None
            if raise_by:
                # Turn the arm about the shoulder's forward axis (X: he faces -X).
                j = w[:3, 3]
                to, back = np.eye(4), np.eye(4)
                to[:3, 3], back[:3, 3] = j, -j
                w = to @ charskel.rot("x", raise_by) @ back @ w
            tree.append({"model": model})
            worlds.append(w)
            child = exe.u32(obj + 0x2C)
            if child:
                visit(child, w)
            obj = exe.u32(obj + 0x30)

    visit(bigmodel.ROOT, np.eye(4))
    owner = {n: part for part, (nodes, _, _) in BIG_PARTS.items() for n in nodes}
    if t_pose:
        for n in BIG_ROD:
            owner.pop(n, None)
    src = Source()
    src.forward = "-x"
    src.credit = "Big the Cat: the game's own cameo model (SEGA), read from the player's game files."
    for part, (_, a, b) in BIG_PARTS.items():
        src.joints[part] = (worlds[a][:3, 3], worlds[b][:3, 3] if b is not None else None)
    # The chunk meshes, node by node, as the game draws them: each vertex block fills the shared
    # buffer at its offset (world space here), and strips index that buffer.
    buffer = {}
    for i, t in enumerate(tree):
        model = t["model"]
        if not model:
            continue
        w = worlds[i]
        vl, pl = exe.u32(model), exe.u32(model + 4)
        at = vl
        while vl:
            h1 = exe.u32(at)
            if h1 & 0xFF == 0xFF:
                break
            if h1 & 0xFF != 0x29:
                sys.exit(f"Big node {i}: vertex chunk {h1 & 0xFF:#x}")
            h2 = exe.u32(at + 4)
            off, n = h2 & 0xFFFF, h2 >> 16
            for k in range(n):
                v = exe.f32(at + 8 + 24 * k, 6)
                buffer[off + k] = ((w @ np.r_[v[:3], 1])[:3], w[:3, :3] @ np.array(v[3:]), i)
            at += 4 + (h1 >> 16) * 4
        p = pl
        tex = 0
        while pl:
            head16 = struct.unpack_from("<H", exe.d, exe.off(p))[0]
            kind = head16 & 0xFF
            if kind == 0xFF:
                break
            if kind == 0:
                p += 2
                continue
            if kind < 8:
                p += 2
                continue
            if kind < 16:
                tex = struct.unpack_from("<H", exe.d, exe.off(p + 2))[0] & 0x1FFF
                p += 4
                continue
            size = struct.unpack_from("<H", exe.d, exe.off(p + 2))[0]
            if kind in (0x40, 0x41, 0x42):
                count = struct.unpack_from("<H", exe.d, exe.off(p + 4))[0] & 0x3FFF
                q = p + 6
                uvscale = 1024.0 if kind == 0x42 else 256.0
                for _ in range(count):
                    ln = struct.unpack_from("<h", exe.d, exe.off(q))[0]
                    q += 2
                    strip = []
                    for _ in range(abs(ln)):
                        if kind == 0x40:
                            idx = struct.unpack_from("<H", exe.d, exe.off(q))[0]
                            uv = (0.0, 0.0)
                            q += 2
                        else:
                            idx, u, v = struct.unpack_from("<Hhh", exe.d, exe.off(q))
                            uv = (u / uvscale, v / uvscale)
                            q += 6
                        strip.append((idx, uv))
                    for k in range(len(strip) - 2):
                        tri = strip[k:k + 3]
                        if (k % 2 == 1) != (ln < 0):
                            tri = [tri[1], tri[0], tri[2]]
                        if any(idx not in buffer for idx, _ in tri):
                            continue
                        verts = [buffer[idx] for idx, _ in tri]
                        if t_pose and all(node in BIG_ROD for _, _, node in verts):
                            continue
                        parts3 = [owner.get(node) for _, _, node in verts]
                        parts3 = [x for x in parts3 if x] or ["chest"]
                        src.tris.append((max(set(parts3), key=parts3.count), tex,
                                         [(pos, nrm, uv) for (pos, nrm, _), (_, uv) in zip(verts, tri)]))
            p += 4 + size * 2
    # Textures in archive order (the model's texture ids).
    archive = prs.decompress((Path(gd_pc) / "e_bigtex.prs").read_bytes())
    src.images = [img.convert("RGBA") for _, _, img in gvm.textures(archive)]
    return src


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("gltf", type=Path, nargs="?", help="a glTF model (or --big)")
    ap.add_argument("out", type=Path)
    ap.add_argument("--name", required=True)
    ap.add_argument("--game", required=True, help="the game's resource/gd_PC folder")
    ap.add_argument("--character", default="sonic",
                    help=f"characters to fit it to, e.g. sonic,knuckles, or all ({','.join(CHARACTERS)})")
    ap.add_argument("--big", type=Path, metavar="EXE", help="use the game's Big the Cat (sonic2app.exe)")
    ap.add_argument("--forward", help="the source model's facing axis (+z for glTF)")
    ap.add_argument("--bulk", type=float, default=1.0, help="widen the parts (not their length)")
    ap.add_argument("--size", type=float, default=1.0, help="draw the whole character this much bigger")
    args = ap.parse_args()

    if args.big:
        src = big_source(args.big, args.game)
    elif args.gltf:
        src = gltf_source(args.gltf)
    else:
        sys.exit("give a glTF model or --big")
    forward = args.forward or src.forward

    def build(character):
        file, body, slots, texfield, parts = CHARACTERS[character]
        d, models = charskel.load(args.game, file)
        tree = charskel.nodes(d, models[body])
        world = charskel.world(tree)
        joint = [w[:3, 3] for w in world]

        # Face the character's way (-X) and scale to its height.
        turn = {"+z": np.array([[0, 0, -1], [0, 1, 0], [1, 0, 0]]),
                "-z": np.array([[0, 0, 1], [0, 1, 0], [-1, 0, 0]]),
                "+x": np.array([[-1, 0, 0], [0, 1, 0], [0, 0, -1]]),
                "-x": np.eye(3)}[forward]
        allpos = np.array([v[0] for _, _, tri in src.tris for v in tri]) @ turn.T
        height = allpos[:, 1].max() - allpos[:, 1].min()
        target_height = max(j[1] for j in joint) - min(j[1] for j in joint)
        k = target_height / height
        floor = allpos[:, 1].min()

        def src_point(p):
            return ((np.asarray(p) @ turn.T) - [0, floor, 0]) * k

        # Feet keep their own size, so they're lowered to stand on the ground (the characters'
        # ankles are high up in their big shoes) and the shins reach down to them.
        drop = {}
        for side in "lr":
            foot = f"foot_{side}"
            if foot in src.joints:
                ankle_height = joint[parts[foot][1]][1]
                drop[side] = max(0.0, ankle_height - src_point(src.joints[foot][0])[1])
        down = lambda name: np.array([0.0, drop.get(name[-1], 0.0), 0.0])

        # Per part: where it starts in each skeleton and how to turn/stretch it.
        fit = {}
        for name, (mesh_node, start_node, end_node) in parts.items():
            if name not in src.joints:
                continue
            a, b = src.joints[name]
            s0, t0 = src_point(a), joint[start_node]
            if name in TORSO and "hips" in src.joints:
                anchor = src_point(src.joints["hips"][0])
                fit[name] = [mesh_node, anchor, joint[parts["hips"][1]], np.eye(3), 1.0, None]
                continue
            if name.startswith("foot_"):
                t0 = t0 - down(name)
            if end_node is not None and b is not None:
                s1, t1 = src_point(b), joint[end_node]
                if name.startswith(("foot_", "shin_")):
                    t1 = t1 - down(name)
                r = rotation_between(s1 - s0, t1 - t0)
                stretch = np.linalg.norm(t1 - t0) / max(np.linalg.norm(s1 - s0), 1e-6)
                if name in NO_STRETCH:
                    stretch = 1.0
                axis = (t1 - t0) / max(np.linalg.norm(t1 - t0), 1e-6)
            else:
                r, stretch, axis = None, 1.0, None
            fit[name] = [mesh_node, s0, t0, r, stretch, axis]
        for name, f in fit.items():
            if f[3] is None:
                f[3] = fit.get(PARENT_PART.get(name, ""), [None] * 4)[3]
                if f[3] is None:
                    f[3] = np.eye(3)

        def place(name, p, n):
            mesh_node, s0, t0, r, stretch, axis = fit[name]
            v = r @ (src_point(p) - s0)
            if axis is not None:
                along = np.dot(v, axis)
                v = (v - axis * along) * args.bulk + axis * along * stretch
            else:
                v = v * args.bulk
            inv = np.linalg.inv(world[mesh_node])
            local = inv @ np.r_[t0 + v, 1.0]
            nn = inv[:3, :3] @ (r @ (np.asarray(n) @ turn.T))
            nn /= max(np.linalg.norm(nn), 1e-9)
            return local[:3], nn

        # Triangles to the nodes of their parts.
        nodes = {}  # mesh node -> (positions, normals, {key: local index}, {texture: triangles})
        for name, tex, tri in src.tris:
            if name not in fit:
                name = "chest" if "chest" in fit else next(iter(fit))
            node = fit[name][0]
            vp, vn, remap, groups = nodes.setdefault(node, ([], [], {}, {}))
            out = []
            for pos, nrm, uv in tri:
                key = (tuple(np.round(pos, 5)), tuple(np.round(nrm, 3)), name)
                if key not in remap:
                    lp, ln = place(name, pos, nrm)
                    remap[key] = len(vp)
                    vp.append(lp)
                    vn.append(ln)
                out.append((remap[key], uv))
            groups.setdefault(tex, []).append(out)

        for node, (vp, *_r) in nodes.items():
            if len(vp) > MAX_NODE_VERTS:
                sys.exit(f"node {node} has {len(vp)} vertices (at most {MAX_NODE_VERTS}): "
                         f"reduce the model first (tools/blender_reduce.py)")
            if parts is HUMANOID and node in KEEP_EMPTY:
                sys.exit(f"node {node} must stay empty")

        # The character's tree, node for node, with our meshes.
        w = skinconv.Writer()
        model_at = {}
        for node, (vp, vn, _, groups) in sorted(nodes.items()):
            vp, vn = np.array(vp), np.array(vn)
            vl = skinconv.vertex_chunk(w, vp, vn)
            alpha = {i for i, img in enumerate(src.images) if skinconv.has_alpha(img)}
            pl = skinconv.poly_chunks(w, sorted(groups.items()), alpha)
            center = (vp.min(axis=0) + vp.max(axis=0)) / 2
            radius = float(np.max(np.linalg.norm(vp - center, axis=1)))
            w.align()
            model_at[node] = w.put("II4f", vl, pl, *center, radius)
        w.align()
        obj_at = {}
        for index, off, parent, fl, has_mesh, _m, pos, ang in tree:
            scl = struct.unpack_from(">3f", d, off + 0x20)
            flags = fl
            model = model_at.get(index, 0)
            if model:
                flags &= ~8  # shown (node 44 is hidden in the original)
            obj_at[index] = w.put("II3f3i3fII", flags, model, *pos, *ang, *scl, 0, 0)
        children = {}
        for index, _o, parent, *_r in tree:
            children.setdefault(parent, []).append(index)
        for parent, kids in children.items():
            if parent >= 0:
                w.patch(obj_at[parent] + 0x2C, "I", obj_at[kids[0]])
            for a, b in zip(kids, kids[1:]):
                w.patch(obj_at[a] + 0x30, "I", obj_at[b])
        meta = w.put("II", 0x444E45, 0)
        w.patch(0, "QII", skinconv.SA2MDL, obj_at[0], meta)

        height = max(j[1] for j in joint) - min(j[1] for j in joint)
        return (bytes(w.buf), {n: len(v[0]) for n, v in sorted(nodes.items())}, k, slots, texfield,
                len(tree), height)

    out = args.out
    out.mkdir(parents=True, exist_ok=True)
    rigs = []
    heights = []
    wanted = list(CHARACTERS) if args.character == "all" else args.character.split(",")
    for character in wanted:
        character = character.strip()
        if character not in CHARACTERS:
            sys.exit(f"no skeleton known for '{character}' (known: {', '.join(sorted(CHARACTERS))})")
        model, used, k, slots, texfield, count, height = build(character)
        heights.append(height)
        (out / f"model_{character}.sa2mdl").write_bytes(model)
        rigs.append(f"rig_{character}={','.join(map(str, slots))};{texfield:#x};{height:.2f}")
        print(f"{args.name}: on {character}'s {count} nodes, meshes on {used}, scale {k:.4f}")
    tex_dir = out / "textures"
    tex_dir.mkdir(exist_ok=True)
    gbix = skinconv.GBIX_BASE | (skinconv.hash_name(args.name) & 0xFFFF) << 8
    lines = []
    for i, img in enumerate(src.images):
        img = img.resize(tuple(min(skinconv.MAX_TEXTURE, v) for v in img.size))
        file_ = f"tex{i:02d}.png"
        img.save(tex_dir / file_)
        lines.append(f"{gbix + i},{file_}")
    (tex_dir / "index.txt").write_text("\n".join(lines) + "\n")
    ini = [
        f"name={args.name}",
        f"textures={len(lines)}",
        "rigged=on",
        *rigs,
        f"character_scale={args.size}",
        # Its height (the tallest fit; each rig has its own), for turning over about its middle.
        f"height={max(heights):.2f}",
        f"keep_nodes={','.join(map(str, sorted(KEEP_EMPTY)))}",
    ]
    if src.credit:
        ini.append(f"credit={src.credit}")
    (out / "skin.ini").write_text("\n".join(ini) + "\n")


if __name__ == "__main__":
    main()
