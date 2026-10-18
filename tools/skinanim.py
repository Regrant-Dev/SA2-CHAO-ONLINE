#!/usr/bin/env python3
"""Animated skins: a model on its own skeleton, with its own animations (re/skins.md).

    tools/skinanim.py <merged.gltf> skins/master_chief --name "Master Chief" [--height 9]

The input is a glTF with a skinned mesh and its animations (tools/blender_merge_anims.py makes
one from Mixamo downloads; SpongeBob's comes that way). The output skin folder holds:
  model.sa2mdl    the skeleton as a node tree: a hidden root node that turns it to face -X,
                  stand on y = 0 and be --height tall, then one node per bone. Each vertex goes
                  with its strongest bone (small separate pieces, like a rod, whole with their
                  main one); a triangle across bones is drawn by the last of them, using the
                  others' vertices from the game's shared vertex buffer, so joints bend instead
                  of tearing (as Knuckles' body does). A bone with many vertices gets extra
                  pieces: child nodes with no transform of their own, so nothing is reduced;
  motions.sa2mot  every animation as a Ninja motion (positions and angles per node, one key per
                  frame at 30 per second; pointers as file offsets like the model);
  textures/       the texture pack; skin.ini names the animations for idle, walk, run, jump and
                  fall (picked by name, or --role role=name) and their lengths.
The mod plays them on the model where the player's character is, picking the animation from
what the character is doing.
"""
import argparse
import heapq
import math
import re
import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import charskel  # noqa: E402
import skinconv  # noqa: E402
import skinpreview  # noqa: E402

FPS = 30
MAX_NODE_VERTS = 800
# The game's chunk vertex buffer (0x25EFE48) holds 8192; we use at most this many slots.
VERTEX_SLOTS = 4096
MOTION_MAGIC = b"SA2COMOT"
BAMS = 65536 / (2 * math.pi)
# Which animation plays when, by name (first match).
ROLES = [
    ("idle", r"^(?!.*(box|carr|hold)).*(idle(?!.*fall)|breath|stand)"),
    ("walk", r"^(?!.*(box|carr|hold)).*walk"),
    ("run", r"^(?!.*(box|carr|hold)).*(run|sprint)"),
    ("jump", r"jump"),
    ("fall", r"fall"),
    # The garden: re/skins.md "Garden actions" (the mod plays them for the game's actions).
    ("pickup", r"pick"),
    ("carry", r"box idle|carry(?!.*(walk|run|jog))"),
    ("carry_walk", r"hold.*walk|carry.*walk|box walk"),
    ("carry_run", r"jog.*box|box.*(run|jog)|carry.*run"),
    ("throw", r"throw"),
    ("put_down", r"put.*down|placement"),
    ("pet", r"pet"),
    ("shake", r"pull|shak"),
    ("whistle", r"whistl|wav|beckon"),
]


def quat_matrix(q):
    x, y, z, w = q
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ])


def rot(axis, a):
    return charskel.rot(axis, a)[:3, :3]


def euler_zyx(r, near=None):
    """Angles (x, y, z) in BAMS with r = Rz(z) Ry(y) Rx(x), as the game's nodes rotate. Of the
    two equivalent answers, the one closest to `near` (unwrapped next to it)."""
    b = math.asin(max(-1.0, min(1.0, -r[2, 0])))
    if abs(math.cos(b)) > 1e-6:
        c = math.atan2(r[2, 1], r[2, 2])
        a = math.atan2(r[1, 0], r[0, 0])
    else:
        a = 0.0
        c = math.atan2(-r[1, 2], r[1, 1])
    sets = [[c, b, a], [c + math.pi, math.pi - b, a + math.pi]]
    sets = [[int(round(v * BAMS)) for v in s] for s in sets]
    if near is None:
        return [((v + 0x8000) & 0xFFFF) - 0x8000 for v in sets[0]]

    def unwrap(v, ref):
        return ref + ((v - ref + 0x8000) & 0xFFFF) - 0x8000

    sets = [[unwrap(v, n) for v, n in zip(s, near)] for s in sets]
    return min(sets, key=lambda s: sum(abs(v - n) for v, n in zip(s, near)))


def check_euler():
    for q in ([0.1, 0.7, 0.2, 0.6], [0.5, 0.5, 0.5, 0.5], [0, 0, 0, 1], [0.3, -0.2, 0.9, 0.1]):
        q = np.array(q, float) / np.linalg.norm(q)
        r = quat_matrix(q)
        x, y, z = euler_zyx(r)
        back = rot("z", z) @ rot("y", y) @ rot("x", x)
        assert np.allclose(back, r, atol=2e-3), (q, back, r)


def in_place(positions, up, airborne):
    """The root joint's positions with the animation's own travel taken out: the character's
    movement does that. Along the ground, the steady drift from first to last frame goes (the
    sway stays); in the air the height stays too, as the character's own jump moves it."""
    ps = np.array(positions, float)
    flat = ps - np.outer(ps @ up, up)
    drift = flat[-1] - flat[0]
    steps = np.linspace(0.0, 1.0, len(ps))
    ps = ps - np.outer(steps, drift)
    if airborne:
        ps = ps - np.outer(ps @ up - ps[0] @ up, up)
    return list(ps), drift


def rigid_islands(pos, tris, jw, ww, under, across):
    """The separate pieces of a mesh (joined by shared corners, seams included) with fewer than
    `under` triangles and no wider than `across` (a buckle, a plate; not a stocking, which has
    to bend at the knee): vertex -> the joint weighing most over the whole piece."""
    keys = {}
    parent = list(range(len(pos)))

    def find(v):
        while parent[v] != v:
            parent[v] = parent[parent[v]]
            v = parent[v]
        return v

    for v, p in enumerate(np.round(pos, 5)):
        first = keys.setdefault(tuple(p), v)
        parent[find(v)] = find(first)
    for a, b, c in tris:
        parent[find(b)] = find(a)
        parent[find(c)] = find(a)
    count = {}
    for t in tris:
        r = find(t[0])
        count[r] = count.get(r, 0) + 1
    lo, hi = {}, {}
    for v in range(len(pos)):
        r = find(v)
        lo[r] = np.minimum(lo.get(r, pos[v]), pos[v])
        hi[r] = np.maximum(hi.get(r, pos[v]), pos[v])
    weight = {}
    for v in range(len(pos)):
        r = find(v)
        if count.get(r, 0) < under and float((hi[r] - lo[r]).max()) < across:
            for j, x in zip(jw[v], ww[v]):
                weight.setdefault(r, {})
                weight[r][int(j)] = weight[r].get(int(j), 0.0) + float(x)
    main = {r: max(js, key=js.get) for r, js in weight.items()}
    return {v: main[find(v)] for v in range(len(pos)) if find(v) in main}


def decompose(m):
    """4x4 of translation, rotation and scale -> (pos, angles BAMS (x, y, z), scale)."""
    s = np.linalg.norm(m[:3, :3], axis=0)
    return m[:3, 3], euler_zyx(m[:3, :3] / s), s


def sample(times, values, t, rotation):
    if len(times) == 1 or t <= times[0]:
        return values[0]
    if t >= times[-1]:
        return values[-1]
    i = int(np.searchsorted(times, t)) - 1
    f = (t - times[i]) / (times[i + 1] - times[i])
    a, b = values[i], values[i + 1]
    if rotation:
        if np.dot(a, b) < 0:
            b = -b
        q = a * (1 - f) + b * f
        return q / np.linalg.norm(q)
    return a * (1 - f) + b * f


def ball_normals(pos, nrm, mask):
    """Anime faces: normals of the `mask` vertices pointed out from a centre behind the face, so
    light falls on it smoothly instead of picking out every dip (the original look)."""
    if not mask.any():
        return nrm
    pts = pos[mask]
    lo, hi = pts.min(axis=0), pts.max(axis=0)
    centre = (lo + hi) / 2
    # Behind the face: the face is the side the normals mostly point to.
    facing = nrm[mask].mean(axis=0)
    axis = int(np.argmax(np.abs(facing)))
    centre[axis] -= np.sign(facing[axis]) * (hi - lo)[axis] * 1.6
    out = nrm.copy()
    d = pos[mask] - centre
    out[mask] = d / np.maximum(np.linalg.norm(d, axis=1, keepdims=True), 1e-9)
    return out


def main():
    check_euler()
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("gltf", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--name", required=True)
    ap.add_argument("--height", type=float, default=9.0, help="height in game units (Sonic ~8)")
    ap.add_argument("--size", type=float, default=1.0,
                    help="drawn this much bigger on the character (movement stays the character's)")
    ap.add_argument("--texture-size", type=int, default=1024)
    ap.add_argument("--soft-face", default="", help="regex of material names to light smoothly")
    ap.add_argument("--stiff", action="append", default=[], metavar="MATERIAL=BONE",
                    help="material (regex) moved whole by one bone, e.g. a cape on the spine")
    ap.add_argument("--cape", default="", metavar="MATERIAL=BONE",
                    help="a cape (material regex) hung from BONE that swings back clear of the "
                         "legs in every animation")
    ap.add_argument("--rigid-islands", type=float, default=0.03, metavar="FRACTION",
                    help="small separate pieces (under this share of the triangles, and of the "
                         "height across) move whole with their main bone instead of bending")
    ap.add_argument("--role", action="append", default=[], help="role=animation name override")
    ap.add_argument("--only", default="", help="regex: keep only these animations")
    ap.add_argument("--credit", default="")
    ap.add_argument("--trim", action="append", default=[], metavar="NAME=START:END",
                    help="keep only frames START..END of an animation (a long one-off: the part "
                         "the game's action needs)")
    args = ap.parse_args()
    skinconv.MAX_TEXTURE = args.texture_size

    g, accessor = skinconv.load_gltf(args.gltf)
    nodes = g["nodes"]
    skin = g["skins"][0]
    joints = skin["joints"]
    jindex = {n: i for i, n in enumerate(joints)}
    parent = {}
    for n, node in enumerate(nodes):
        for c in node.get("children", []):
            parent[c] = n

    def world(n):
        m = skinconv.node_matrix(nodes[n])
        while n in parent:
            n = parent[n]
            m = skinconv.node_matrix(nodes[n]) @ m
        return m

    roots = [j for j in joints if parent.get(j) not in jindex]
    above = world(parent[roots[0]]) if roots[0] in parent else np.eye(4)
    ibm = accessor(skin["inverseBindMatrices"]).reshape(-1, 4, 4).transpose(0, 2, 1)
    soft = re.compile(args.soft_face, re.I) if args.soft_face else None
    joint_world = [world(n) for n in joints]
    stiff = []
    for item in args.stiff:
        pattern, bone = item.rsplit("=", 1)
        j = next((i for i, n in enumerate(joints) if nodes[n].get("name") == bone), None)
        if j is None:
            sys.exit(f"--stiff: no bone {bone!r}")
        stiff.append((pattern, j))

    # A cape: a bone of its own at the cape's top edge, under BONE; its angle per frame comes
    # later (`cape_angles`), swung back just enough to stay behind the legs.
    cape = None
    if args.cape:
        pattern, bone = args.cape.rsplit("=", 1)
        pj = next((i for i, n in enumerate(joints) if nodes[n].get("name") == bone), None)
        if pj is None:
            sys.exit(f"--cape: no bone {bone!r}")
        bind = joint_world[pj] @ ibm[pj]
        pts = []
        for node in nodes:
            if "mesh" not in node or "skin" not in node:
                continue
            for p in g["meshes"][node["mesh"]]["primitives"]:
                m_ = p.get("material")
                mname = g["materials"][m_].get("name", "") if m_ is not None else ""
                if re.search(pattern, mname):
                    pos = accessor(p["attributes"]["POSITION"])
                    pts.extend((bind @ np.c_[pos, np.ones(len(pos))].T).T[:, :3])
        if not pts:
            sys.exit(f"--cape: no material matches {pattern!r}")
        pts = np.array(pts)
        top = pts[:, 1].max()
        band = pts[pts[:, 1] > top - (top - pts[:, 1].min()) * 0.05]
        pivot = np.array([band[:, 0].mean(), top, band[:, 2].mean()])
        bottom = pts[pts[:, 1] < pts[:, 1].min() + (top - pts[:, 1].min()) * 0.05].mean(axis=0)
        local = np.linalg.inv(joint_world[pj]) @ np.r_[pivot, 1.0]
        pn = joints[pj]
        cn = len(nodes)
        nodes.append({"name": "cape", "translation": list(map(float, local[:3]))})
        nodes[pn].setdefault("children", []).append(cn)
        parent[cn] = pn
        joints = list(joints) + [cn]
        jindex[cn] = len(joints) - 1
        offset = np.eye(4)
        offset[:3, 3] = local[:3]
        ibm = np.concatenate([ibm, (np.linalg.inv(offset) @ ibm[pj])[None]])
        joint_world.append(world(cn))
        stiff.append((pattern, jindex[cn]))
        cape = (cn, pn, pivot, bottom)

    # The skinned mesh: each vertex on its strongest joint, in that joint's space. Triangles
    # whose corners are on different joints are drawn by the last of them (depth-first), using
    # the others' vertices from the game's shared vertex buffer, so joints bend instead of tearing.
    textures, images = {}, []
    verts = {}  # (joint, position, normal) -> vertex id
    vert_list = []  # (joint, position, normal)
    tri_list = []  # (texture, [(vertex id, uv)] x 3)
    bind_points = []
    skinned = [p for node in nodes if "mesh" in node and "skin" in node
               for p in g["meshes"][node["mesh"]]["primitives"]]
    total = sum(g["accessors"][p["indices"]]["count"] // 3 for p in skinned)
    # The mesh's height (bind pose, its own units): small pieces are a share of it.
    span = max(float(np.ptp(accessor(p["attributes"]["POSITION"]), axis=0).max()) for p in skinned)
    for p in skinned:
        if True:
            a = p["attributes"]
            pos = accessor(a["POSITION"])
            nrm = accessor(a["NORMAL"]) if "NORMAL" in a else np.tile([0.0, 1.0, 0.0], (len(pos), 1))
            uv = accessor(a["TEXCOORD_0"]) if "TEXCOORD_0" in a else np.zeros((len(pos), 2))
            jw = accessor(a["JOINTS_0"]).astype(int)
            ww = accessor(a["WEIGHTS_0"])
            strongest = jw[np.arange(len(jw)), np.argmax(ww, axis=1)]
            tris = accessor(p["indices"])[:, 0].astype(int).reshape(-1, 3)
            material = p.get("material")
            mname = g["materials"][material].get("name", "") if material is not None else ""
            if soft and soft.search(mname):
                nrm = ball_normals(pos, nrm, np.ones(len(pos), bool))
            if material not in textures:
                textures[material] = len(images)
                images.append(skinconv.texture_for(g, args.gltf, material))
            tex = textures[material]
            stiff_j = next((j for pattern, j in stiff if re.search(pattern, mname)), None)
            island_joint = rigid_islands(pos, tris, jw, ww, total * args.rigid_islands,
                                         span * args.rigid_islands * 3)
            ids = {}

            def vertex(v):
                if v not in ids:
                    j = island_joint.get(v, int(strongest[v])) if stiff_j is None else stiff_j
                    lp = (ibm[j] @ np.r_[pos[v], 1.0])[:3]
                    ln = ibm[j][:3, :3] @ nrm[v]
                    ln /= max(np.linalg.norm(ln), 1e-9)
                    key = (j, tuple(np.round(lp, 6)), tuple(np.round(ln, 4)))
                    if key not in verts:
                        verts[key] = len(vert_list)
                        vert_list.append((j, lp, ln))
                        # Where the skeleton puts it (the joint's world bind matrix).
                        bind_points.append((joint_world[j] @ np.r_[lp, 1.0])[:3])
                    ids[v] = verts[key]
                return ids[v]

            for tri in tris:
                tri_list.append((tex, [(vertex(v), (float(uv[v][0]), float(uv[v][1])))
                                       for v in tri]))

    # Our root: what's above the root joints (the armature's own transform), then face -X
    # (glTF faces +Z), scale to --height and put the feet on y = 0, all measured in the
    # skeleton's world bind pose.
    pts = np.array(bind_points)
    lo, hi = pts.min(axis=0), pts.max(axis=0)
    k = args.height / (hi[1] - lo[1])
    face = np.eye(4)
    face[:3, :3] = rot("y", -0x4000)
    place = face @ np.diag([k, k, k, 1.0])
    place[1, 3] -= (place @ np.r_[0, lo[1], 0, 1])[1]
    root_m = place @ above
    # Straight up, in the root joints' parent space (moves along it are the character's).
    up = np.linalg.inv(above[:3, :3]) @ np.array([0.0, 1.0, 0.0])
    up /= np.linalg.norm(up)

    # Joints depth-first; each triangle goes to the last joint among its corners.
    jorder = []

    def walk(n):
        jorder.append(n)
        for c in nodes[n].get("children", []):
            if c in jindex:
                walk(c)

    for r in roots:
        walk(r)
    jpos = {joints.index(n): i for i, n in enumerate(jorder)}
    drawn_by = {}  # joint index -> its triangles
    for tri in tri_list:
        j = max((vert_list[v][0] for v, _ in tri[1]), key=jpos.get)
        drawn_by.setdefault(j, []).append(tri)

    # Each joint's triangles in pieces bringing at most MAX_NODE_VERTS of its own vertices.
    pieces = {}  # joint node -> [[triangles]]
    for j, tris in drawn_by.items():
        out, seen, group = [], set(), []
        for tri in tris:
            new = {v for v, _ in tri[1] if vert_list[v][0] == j and v not in seen}
            if group and len(seen) + len(new) > MAX_NODE_VERTS:
                out.append(group)
                seen, group = set(), []
                new = {v for v, _ in tri[1] if vert_list[v][0] == j}
            seen |= new
            group.append(tri)
        out.append(group)
        pieces[joints[j]] = out

    # Node order (depth-first, as motions address nodes): our root, then each joint followed by
    # its children; a joint's extra pieces come right after it as its last children.
    order = []  # ("joint", gltf node) or ("piece", gltf node, piece index)

    def visit(n):
        order.append(("joint", n))
        for c in nodes[n].get("children", []):
            if c in jindex:
                visit(c)
        for i in range(1, len(pieces.get(n, []))):
            order.append(("piece", n, i))

    for r in roots:
        visit(r)
    at = {item: i for i, item in enumerate(order)}

    # Which node puts each vertex in the buffer (one of its joint's: the first using it, or the
    # joint itself when a node before that needs it) and the last node needing it there.
    users = {}
    for n, plist in pieces.items():
        for i, tris in enumerate(plist):
            item = ("joint", n) if i == 0 else ("piece", n, i)
            for _, corners in tris:
                for v, _ in corners:
                    users.setdefault(v, []).append(at[item])
    emit, last = {}, {}
    for v, where in users.items():
        j = joints[vert_list[v][0]]
        own = [p for p in where if order[p][1] == j]
        emit[v] = min(own) if own and min(own) == min(where) else at[("joint", j)]
        last[v] = max(where)
    # Buffer slots: a slot is free again once the last node needing its vertex is drawn.
    slot, free, live, peak = {}, list(range(VERTEX_SLOTS)), [], 0
    heapq.heapify(free)
    emitted = {}
    for v in sorted(emit, key=lambda v: (emit[v], v)):
        emitted.setdefault(emit[v], []).append(v)
    for p in range(len(order)):
        while live and live[0][0] < p:
            heapq.heappush(free, heapq.heappop(live)[1])
        for v in emitted.get(p, []):
            if not free:
                sys.exit(f"more than {VERTEX_SLOTS} vertices in use at once: fewer triangles")
            slot[v] = heapq.heappop(free)
            heapq.heappush(live, (last[v], slot[v]))
        peak = max(peak, len(live))

    w = skinconv.Writer()
    alpha = {i for i, img in enumerate(images) if skinconv.has_alpha(img)}
    models = {}
    for item in order:
        p = at[item]
        n = item[1]
        mine = sorted(emitted.get(p, []), key=slot.get)
        plist = pieces.get(n, [])
        i = 0 if item[0] == "joint" else item[2]
        tris = plist[i] if i < len(plist) else []
        if not mine and not tris:
            continue
        # The vertices in runs of consecutive slots, one block each.
        w.align()
        vl = w.here()
        runs = []
        for v in mine:
            if runs and slot[v] == slot[runs[-1][-1]] + 1:
                runs[-1].append(v)
            else:
                runs.append([v])
        for run in runs:
            w.put("I", 0x29 | ((1 + 6 * len(run)) << 16))  # CnkV_VN
            w.put("I", slot[run[0]] | (len(run) << 16))  # buffer offset, count
            for v in run:
                w.put("6f", *vert_list[v][1], *vert_list[v][2])
        w.put("I", 0xFF)
        groups = {}
        for tex, corners in tris:
            groups.setdefault(tex, []).append([(slot[v], uv) for v, uv in corners])
        pl = skinconv.poly_chunks(w, sorted(groups.items()), alpha)
        # Never left out as off screen: later nodes use its vertices.
        w.align()
        models[item] = w.put("II4f", vl, pl, 0.0, 0.0, 0.0, 1.0e4)
    w.align()
    pos0, ang0, scl0 = decompose(root_m)
    obj_root = w.put("II3f3i3fII", 0x08, 0, *pos0, *ang0, *scl0, 0, 0)  # hidden, no mesh
    obj = {}
    rest = {}
    for item in order:
        if item[0] == "joint":
            n = item[1]
            t = nodes[n].get("translation", [0, 0, 0])
            q = nodes[n].get("rotation", [0, 0, 0, 1])
            s = nodes[n].get("scale", [1, 1, 1])
            a = euler_zyx(quat_matrix(q))
            rest[n] = (np.array(t, float), a)
            model = models.get(item, 0)
            obj[item] = w.put("II3f3i3fII", 0 if model else 0x08, model, *t, *a, *s, 0, 0)
        else:
            _, n, i = item
            # No transform of its own (unit position, angles and scale): the bone's space.
            obj[item] = w.put("II3f3i3fII", 0x07, models[item], 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0)

    def kids(n):
        out = [("joint", c) for c in nodes[n].get("children", []) if c in jindex]
        out += [("piece", n, i) for i in range(1, len(pieces.get(n, [])))]
        return out

    w.patch(obj_root + 0x2C, "I", obj[("joint", roots[0])])
    top = [("joint", r) for r in roots]
    for a_, b_ in zip(top, top[1:]):
        w.patch(obj[a_] + 0x30, "I", obj[b_])
    for item in order:
        if item[0] != "joint":
            continue
        ks = kids(item[1])
        if ks:
            w.patch(obj[item] + 0x2C, "I", obj[ks[0]])
        for a_, b_ in zip(ks, ks[1:]):
            w.patch(obj[a_] + 0x30, "I", obj[b_])
    meta = w.put("II", 0x444E45, 0)
    w.patch(0, "QII", skinconv.SA2MDL, obj_root, meta)

    # Motions: per joint, an angle key per frame (and positions where it moves). Our root and
    # the extra pieces have no keys (they keep their own transform).
    m = skinconv.Writer()
    m.buf = bytearray(MOTION_MAGIC + b"\0" * 8)
    only = re.compile(args.only, re.I) if args.only else None
    trims = {}
    for item in args.trim:
        n, span = item.rsplit("=", 1)
        a, b = span.split(":")
        trims[n] = (int(a), int(b))
    def posed(n, tt, tracks, memo):
        """Node `n`'s world matrix at time `tt` of an animation (its tracks, else its rest)."""
        if n not in memo:
            node = dict(nodes[n])
            if (n, "translation") in tracks:
                node["translation"] = list(map(float, sample(*tracks[(n, "translation")], tt, False)))
            if (n, "rotation") in tracks:
                node["rotation"] = list(map(float, sample(*tracks[(n, "rotation")], tt, True)))
            m_ = skinconv.node_matrix(node)
            memo[n] = posed(parent[n], tt, tracks, memo) @ m_ if n in parent else m_
        return memo[n]

    legs = [n for n in joints if re.search(r"(up)?leg|foot|toe", nodes[n].get("name", ""), re.I)
            and re.search(r"left|right", nodes[n].get("name", ""), re.I)]

    def cape_angles(tracks, frames, first):
        """The cape's angles (BAMS, its own bone) per frame: swung back about its top edge just
        enough to stay behind the knees, feet and toes; at once when a leg comes back, settling
        slowly after (like cloth)."""
        cn, _, pivot0, bottom0 = cape
        up = np.array([0.0, 1.0, 0.0])
        length = float(np.linalg.norm(bottom0 - pivot0))
        # Forward: from the feet to the toes at rest, level; the side axis across it.
        toes = [n for n in legs if re.search(r"toe", nodes[n].get("name", ""), re.I)]
        foot = [n for n in legs if re.search(r"foot", nodes[n].get("name", ""), re.I)]
        fwd = sum(world(t)[:3, 3] for t in toes) - sum(world(f)[:3, 3] for f in foot)
        fwd = fwd - up * (fwd @ up)
        fwd /= max(np.linalg.norm(fwd), 1e-9)
        side = np.cross(up, fwd)

        def turn(t):
            # About `side` by `t` radians (Rodrigues).
            k = np.array([[0, -side[2], side[1]], [side[2], 0, -side[0]], [-side[1], side[0], 0]])
            return np.eye(3) + math.sin(t) * k + (1 - math.cos(t)) * (k @ k)

        # Which way round swings the bottom back.
        back = 1.0 if (turn(0.1) @ -up) @ fwd < 0 else -1.0
        hang = bottom0 - pivot0
        rest = math.atan2(-(hang @ fwd), -(hang @ up))
        # The hang in the cape bone's own space: where the body's lean alone would put it.
        bind = joint_world[jindex[cn]][:3, :3]
        hang_local = np.linalg.inv(bind) @ hang

        def tilt(o):
            h = o @ hang_local
            return math.atan2(-(h @ fwd), -(h @ up))

        margin = 0.08 * length
        need = []
        for f in range(frames):
            memo = {}
            tt = (first + f) / FPS
            top = posed(cn, tt, tracks, memo)[:3, 3]
            want = rest
            for n in legs:
                d = posed(n, tt, tracks, memo)[:3, 3] - top
                down = -(d @ up)
                if length * 0.05 < down < length * 1.1:
                    want = max(want, math.atan((margin - d @ fwd) / down))
            need.append(min(want, math.radians(70)))
        # Back at once, forward slowly; twice round so the loop's start matches its end.
        s_, out = need[-1], list(need)
        for _ in range(2):
            for i, v in enumerate(need):
                s_ = v if v > s_ else s_ + (v - s_) * 0.15
                out[i] = s_
        angles, near = [], None
        for f, phi in enumerate(out):
            tt = (first + f) / FPS
            o = posed(cn, tt, tracks, {})[:3, :3]
            # From where the body's lean leaves it (it hangs, it doesn't lean with the back).
            swing = phi - tilt(o)
            o = o / np.linalg.norm(o, axis=0)
            near = euler_zyx(o.T @ turn(back * swing) @ o, near)
            angles.append(near)
        return angles

    table, infos = [], []
    for anim in g.get("animations", []):
        name = anim.get("name", f"anim{len(infos)}").split("|")[-1]
        if only and not only.search(name):
            continue
        tracks = {}
        end = 0.0
        for ch in anim["channels"]:
            node, path = ch["target"].get("node"), ch["target"]["path"]
            if node not in jindex or path not in ("translation", "rotation"):
                continue
            smp = anim["samplers"][ch["sampler"]]
            times = accessor(smp["input"])[:, 0]
            tracks[(node, path)] = (times - times[0], accessor(smp["output"]))
            end = max(end, float(times[-1] - times[0]))
        frames = max(2, int(round(end * FPS)) + 1)
        # Only part of it: frames start..end.
        first = 0
        if name in trims:
            first, last_ = trims[name]
            first = min(first, frames - 2)
            frames = max(2, min(last_, frames - 1) - first + 1)
        airborne = re.search(r"jump|fall", name, re.I) is not None
        stride = 0.0
        entries = []
        swing = cape_angles(tracks, frames, first) if cape else None
        for item in order:
            if item[0] != "joint":
                entries.append(([], []))
                continue
            n = item[1]
            t0, a0 = rest[n]
            if cape and n == cape[0]:
                entries.append(([], swing))
                continue
            angles, positions = [], []
            near, moves = a0, False
            for f in range(frames):
                tt = (first + f) / FPS
                if (n, "rotation") in tracks:
                    near = euler_zyx(quat_matrix(sample(*tracks[(n, "rotation")], tt, True)), near)
                angles.append(near)
                if (n, "translation") in tracks:
                    positions.append(sample(*tracks[(n, "translation")], tt, False))
            if positions and n in roots:
                positions, drift = in_place(positions, up, airborne)
                # How far one cycle goes, in game units (the mod plays it as fast as the
                # character moves).
                stride = max(stride, float(np.linalg.norm(root_m[:3, :3] @ drift)))
            moves = any(np.linalg.norm(p - t0) > 1e-5 for p in positions)
            entries.append((positions if moves else [], angles))
        key_at = []
        for positions, angles in entries:
            m.align()
            pa = m.here() if positions else 0
            for f, p in enumerate(positions):
                m.put("I3f", f, *p)
            aa = m.here() if angles else 0
            for f, a in enumerate(angles):
                # Unwrapped, so the game's in-between frames don't turn the long way round.
                m.put("I3i", f, *a)
            key_at.append((pa, aa, len(positions), len(angles)))
        m.align()
        mdata = m.here()
        m.put("4I", 0, 0, 0, 0)  # our root node
        for pa, aa, pc, ac in key_at:
            m.put("4I", pa, aa, pc, ac)
        table.append(m.put("IIHH", mdata, frames, 3, 2))
        infos.append((name, frames, stride))
    m.align()
    table_at = m.here()
    for t in table:
        m.put("I", t)
    struct.pack_into("<II", m.buf, 8, len(table), table_at)

    out = args.out
    out.mkdir(parents=True, exist_ok=True)
    (out / "model.sa2mdl").write_bytes(bytes(w.buf))
    (out / "motions.sa2mot").write_bytes(bytes(m.buf))
    tex_dir = out / "textures"
    tex_dir.mkdir(exist_ok=True)
    for f in tex_dir.iterdir():
        f.unlink()
    gbix = skinconv.GBIX_BASE | (skinconv.hash_name(args.name) & 0xFFFF) << 8
    lines = []
    for i, img in enumerate(images):
        file_ = f"tex{i:02d}.png"
        img.save(tex_dir / file_)
        lines.append(f"{gbix + i},{file_}")
    (tex_dir / "index.txt").write_text("\n".join(lines) + "\n")
    roles = dict(r.split("=", 1) for r in args.role)
    hits = {}
    ini = [f"name={args.name}", f"textures={len(lines)}", "animated=on",
           f"nodes={len(order) + 1}", f"height={args.height}", f"character_scale={args.size}",
           "motions=" + ",".join(f"{n}:{f}" for n, f, _ in infos),
           "strides=" + ",".join(f"{st:.3f}" for _, _, st in infos)]
    for role, pattern in ROLES:
        want = roles.get(role)
        hit = next((i for i, (n, _, _) in enumerate(infos)
                    if (n == want if want else re.search(pattern, n, re.I))), None)
        if hit is not None:
            ini.append(f"{role}={hit}")
            hits[role] = hit
    if args.credit:
        ini.append(f"credit={args.credit}")
    (out / "skin.ini").write_text("\n".join(ini) + "\n")
    # Feet on the ground as it stands idle (the bind pose can stand differently).
    if "idle" in hits:
        # By the feet (a cape or a tail can hang lower); the whole model if it has none.
        feet = {i + 1 for i, item in enumerate(order)
                if re.search(r"foot|toe", nodes[item[1]].get("name", ""), re.I)}
        idle = skinpreview.Model(out, hits["idle"])
        tris = idle.triangles(0, feet or None) or idle.triangles(0)
        low = min(p[1] for t in tris for p in t)
        y = struct.unpack_from("<f", w.buf, obj_root + 0xC)[0]
        struct.pack_into("<f", w.buf, obj_root + 0xC, y - low)
        (out / "model.sa2mdl").write_bytes(bytes(w.buf))
        print(f"{args.name}: idle feet {low:+.2f} moved to the ground")
    split = sum(len(p) - 1 for p in pieces.values())
    print(f"{args.name}: {len(order) + 1} nodes ({split} extra pieces), "
          f"{len(tri_list)} triangles ({peak} buffer slots), {len(infos)} motions "
          f"{[(n, round(st, 1)) for n, _, st in infos]}, {len(lines)} textures, scale {k:.4f}")


if __name__ == "__main__":
    main()
