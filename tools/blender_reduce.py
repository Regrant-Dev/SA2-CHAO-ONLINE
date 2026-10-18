"""Reduces a glTF model's triangle count, keeping its skeleton and weights (for skins).

    blender -b --python tools/blender_reduce.py -- <in.gltf> <out.gltf> <target triangles>
"""
import sys

import bpy

args = sys.argv[sys.argv.index("--") + 1:]
src, dst, target = args[0], args[1], int(args[2])

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
total = sum(len(o.data.polygons) for o in meshes)
# Count triangles (polygons may be quads after import).
tris = 0
for o in meshes:
    tris += sum(len(p.vertices) - 2 for p in o.data.polygons)
ratio = min(1.0, target / max(tris, 1))
print(f"reduce: {tris} triangles -> ratio {ratio:.3f}")
for o in meshes:
    mod = o.modifiers.new("reduce", "DECIMATE")
    mod.ratio = ratio
    mod.use_collapse_triangulate = True
    bpy.context.view_layer.objects.active = o
    # Decimate before the armature so weights stay with the reduced vertices.
    while o.modifiers.find("reduce") > 0:
        bpy.ops.object.modifier_move_up(modifier="reduce")
    bpy.ops.object.modifier_apply(modifier="reduce")
tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in meshes)
print(f"reduce: now {tris} triangles")
bpy.ops.export_scene.gltf(filepath=dst, export_format="GLTF_SEPARATE", export_animations=False)
