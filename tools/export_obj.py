#!/usr/bin/env python3
"""Exports a skin source as an OBJ with its textures (e.g. to upload to Mixamo for animations).

    tools/export_obj.py --big "<game>/sonic2app.exe" --game "<game>/resource/gd_PC" <out dir>
    tools/export_obj.py --gltf skins-src/master_chief_small/scene.gltf <out dir>

The model is turned to face +Z with Y up (what Mixamo expects) and scaled to about 1.7 units
tall. Reads the player's own game files; the result is for their own use.
"""
import argparse
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import skinconv  # noqa: E402
import skinrig  # noqa: E402


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", type=Path)
    ap.add_argument("--big", type=Path, metavar="EXE")
    ap.add_argument("--game")
    ap.add_argument("--gltf", type=Path)
    args = ap.parse_args()
    if args.gltf:
        g, accessor = skinconv.load_gltf(args.gltf)
        if g.get("skins"):
            src = skinrig.gltf_source(args.gltf)
        else:
            # Not rigged: every mesh as placed in the scene.
            src = skinrig.Source()
            textures = {}
            for material, pos, nrm, uv, tris in skinconv.meshes(g, accessor):
                if material not in textures:
                    textures[material] = len(src.images)
                    src.images.append(skinconv.texture_for(g, args.gltf, material))
                for tri in tris:
                    src.tris.append(("", textures[material],
                                     [(pos[v], nrm[v], (uv[v][0], uv[v][1])) for v in tri]))
        turn = np.eye(3)  # glTF already faces +Z, Y up
    elif args.big and args.game:
        src = skinrig.big_source(args.big, args.game, t_pose=True)
        # -X forward -> +Z forward (turn 90 degrees about Y), Y up.
        turn = np.array([[0, 0, -1], [0, 1, 0], [1, 0, 0]], dtype=float).T
    else:
        sys.exit("give --gltf, or --big and --game")
    pos = np.array([v[0] for _, _, tri in src.tris for v in tri]) @ turn.T
    lo, hi = pos.min(axis=0), pos.max(axis=0)
    k = 1.7 / (hi[1] - lo[1])
    offset = np.array([-(lo[0] + hi[0]) / 2, -lo[1], -(lo[2] + hi[2]) / 2])
    out = args.out
    out.mkdir(parents=True, exist_ok=True)
    for i, img in enumerate(src.images):
        img.save(out / f"tex{i:02d}.png")
    mtl = [f"newmtl m{i}\nKd 1 1 1\nmap_Kd tex{i:02d}.png\n" for i in range(len(src.images))]
    (out / "model.mtl").write_text("\n".join(mtl))
    lines = ["mtllib model.mtl"]
    v_at = vt_at = vn_at = 1
    by_tex = {}
    for _, tex, tri in src.tris:
        by_tex.setdefault(tex, []).append(tri)
    for tex, tris in sorted(by_tex.items()):
        lines.append(f"usemtl m{tex}")
        for tri in tris:
            face = []
            for p, n, uv in tri:
                q = (np.asarray(p) @ turn.T + offset) * k
                nn = np.asarray(n) @ turn.T
                lines.append(f"v {q[0]:.5f} {q[1]:.5f} {q[2]:.5f}")
                lines.append(f"vt {uv[0]:.5f} {1 - uv[1]:.5f}")
                lines.append(f"vn {nn[0]:.4f} {nn[1]:.4f} {nn[2]:.4f}")
                face.append(f"{v_at}/{vt_at}/{vn_at}")
                v_at += 1
                vt_at += 1
                vn_at += 1
            lines.append("f " + " ".join(face))
    (out / "model.obj").write_text("\n".join(lines) + "\n")
    print(f"wrote {out / 'model.obj'}: {len(src.tris)} triangles, {len(src.images)} textures")


if __name__ == "__main__":
    main()
