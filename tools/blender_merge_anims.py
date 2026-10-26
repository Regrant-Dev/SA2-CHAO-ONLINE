"""Merges a rigged character and its animation files into one glTF with every animation.

    blender -b --python tools/blender_merge_anims.py -- <out.gltf> <rigged model> [anims...] \
        [--reduce <triangles>] [--full <mesh.obj>]

The rigged model is an FBX or glTF with its mesh (e.g. Mixamo's "With Skin" download); each
animation file (FBX or glTF, with or without skin) adds its animation, named after the file
("Running.fbx" -> "Running"). With --full, the rigged mesh (a lighter copy that Mixamo could
rig) is swapped for the full-detail one, fitted over it, with the weights carried across from
its nearest faces; the full mesh is the same model exported the same way (tools/export_obj.py),
so nothing is lost. The mesh is reduced to about <triangles> if it has more. Used by
tools/skinanim.py; needs Blender 4.4 or later.
"""
import sys
from pathlib import Path

import bpy
from mathutils import Matrix

args = sys.argv[sys.argv.index("--") + 1:]
reduce_to = None
if "--reduce" in args:
    i = args.index("--reduce")
    reduce_to = int(args[i + 1])
    del args[i:i + 2]
full = None
if "--full" in args:
    i = args.index("--full")
    full = args[i + 1]
    del args[i:i + 2]
out, model, anims = args[0], args[1], args[2:]


def load(path):
    before = set(bpy.data.objects)
    if path.lower().endswith(".fbx"):
        bpy.ops.import_scene.fbx(filepath=path, automatic_bone_orientation=False)
    elif path.lower().endswith(".obj"):
        bpy.ops.wm.obj_import(filepath=path)
    else:
        bpy.ops.import_scene.gltf(filepath=path)
    return [o for o in bpy.data.objects if o not in before]


bpy.ops.wm.read_factory_settings(use_empty=True)
objects = load(model)
armature = next(o for o in objects if o.type == "ARMATURE")
meshes = [o for o in objects if o.type == "MESH"]
for o in bpy.data.actions:
    bpy.data.actions.remove(o)



def bounds(objs):
    pts = [o.matrix_world @ v.co for o in objs for v in o.data.vertices]
    lo = [min(p[i] for p in pts) for i in range(3)]
    hi = [max(p[i] for p in pts) for i in range(3)]
    return lo, hi


if full:
    from mathutils import Matrix, Vector
    bpy.context.view_layer.update()
    detailed = [o for o in load(full) if o.type == "MESH"]
    bpy.context.view_layer.update()
    (plo, phi), (flo, fhi) = bounds(meshes), bounds(detailed)
    k = (phi[2] - plo[2]) / (fhi[2] - flo[2])
    pc = Vector(((plo[0] + phi[0]) / 2, (plo[1] + phi[1]) / 2, plo[2]))
    fc = Vector(((flo[0] + fhi[0]) / 2, (flo[1] + fhi[1]) / 2, flo[2]))
    fit = Matrix.Translation(pc) @ Matrix.Scale(k, 4) @ Matrix.Translation(-fc)
    # One proxy to carry weights from (its parts joined).
    bpy.ops.object.select_all(action="DESELECT")
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    proxy = meshes[0]
    groups = [g.name for g in proxy.vertex_groups]
    # Carried from the proxy as it rests (the full mesh is in the rest pose), not as posed.
    armature.data.pose_position = "REST"
    bpy.context.view_layer.update()
    unweighted = 0
    for o in detailed:
        o.matrix_world = fit @ o.matrix_world
        bpy.ops.object.select_all(action="DESELECT")
        o.select_set(True)
        bpy.context.view_layer.objects.active = o
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
        o.vertex_groups.clear()
        for name in groups:
            o.vertex_groups.new(name=name)
        dt = o.modifiers.new("weights", "DATA_TRANSFER")
        dt.object = proxy
        dt.use_vert_data = True
        dt.data_types_verts = {"VGROUP_WEIGHTS"}
        dt.vert_mapping = "POLYINTERP_NEAREST"
        dt.layers_vgroup_select_src = "ALL"
        dt.layers_vgroup_select_dst = "NAME"
        bpy.ops.object.datalayout_transfer(modifier="weights")
        bpy.ops.object.modifier_apply(modifier="weights")
        mw = o.matrix_world.copy()
        o.parent = armature
        o.matrix_world = mw
        o.modifiers.new("Armature", "ARMATURE").object = armature
        unweighted += sum(1 for v in o.data.vertices if not any(g.weight > 0.01 for g in v.groups))
    bpy.data.objects.remove(proxy, do_unlink=True)
    armature.data.pose_position = "POSE"
    meshes = detailed
    tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in meshes)
    print(f"merge: full mesh {tris} triangles, scale {k:.5f}, {unweighted} vertices unweighted")

# Each animation file: its skeleton played frame by frame, and our skeleton posed the same way
# (each bone where that one is, in the armature's space), keyed into an action of our own.
# Copying the file's own action instead goes wrong: a file without the mesh has no bind pose,
# so its bones rest differently and the same rotations bend ours elsewhere (arms in the body).
scene = bpy.context.scene
actions = []
bones = sorted(armature.pose.bones, key=lambda b: len(b.parent_recursive))
for b in bones:
    b.rotation_mode = "QUATERNION"
for path in anims:
    before_actions = set(bpy.data.actions)
    new = load(path)
    source = next((o for o in new if o.type == "ARMATURE"), None)
    action = next((a for a in bpy.data.actions if a not in before_actions), None)
    if source is None or action is None:
        print(f"merge: no animation in {path}")
        for o in new:
            bpy.data.objects.remove(o, do_unlink=True)
        continue
    source.animation_data.action = action
    # Same skeleton, maybe in other units (a rig exported from Blender vs Mixamo's): poses go
    # into our armature's space, with lengths scaled by how far the root bone rests from the
    # origin in each.
    to_ours = armature.matrix_world.inverted() @ source.matrix_world
    root = next(b for b in bones if not b.parent)
    src_root = source.data.bones.get(root.name)
    k = 1.0
    if src_root:
        theirs = (to_ours @ src_root.head_local).length
        if theirs > 1e-9:
            k = root.bone.head_local.length / theirs
    ours = bpy.data.actions.new(Path(path).stem)
    ours.use_fake_user = True
    armature.animation_data_create()
    armature.animation_data.action = ours
    first, last = (int(round(x)) for x in action.frame_range)
    for f in range(first, last + 1):
        scene.frame_set(f)
        posed = {}
        for b in bones:
            src = source.pose.bones.get(b.name)
            if src is None:
                # Not in the file: keeps its rest place under its parent.
                rest = b.bone.matrix_local
                if b.parent:
                    rel = b.parent.bone.matrix_local.inverted() @ rest
                    posed[b.name] = posed[b.parent.name] @ rel
                else:
                    posed[b.name] = rest
                continue
            m = to_ours @ src.matrix
            loc, rot, _ = m.decompose()
            posed[b.name] = Matrix.Translation(loc * k) @ rot.to_matrix().to_4x4()
            # The pose relative to its parent's, over its rest relative to its parent's.
            rest = b.bone.matrix_local
            if b.parent:
                rest = b.parent.bone.matrix_local.inverted() @ rest
                here = posed[b.parent.name].inverted() @ posed[b.name]
            else:
                here = posed[b.name]
            b.matrix_basis = rest.inverted() @ here
            b.keyframe_insert("rotation_quaternion", frame=f)
            if not b.parent:
                b.keyframe_insert("location", frame=f)
    armature.animation_data.action = None
    for o in new:
        bpy.data.objects.remove(o, do_unlink=True)
    bpy.data.actions.remove(action)
    actions.append(ours)
    print(f"merge: {ours.name} frames {(first, last)}")

# Every action as its own NLA track, so the glTF export writes them as separate animations.
armature.animation_data_create()
for action in actions:
    track = armature.animation_data.nla_tracks.new()
    track.name = action.name
    start = int(action.frame_range[0])
    strip = track.strips.new(action.name, start, action)
    strip.name = action.name
    track.mute = True
armature.animation_data.action = None

if reduce_to:
    tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in meshes)
    if tris > reduce_to:
        for o in meshes:
            mod = o.modifiers.new("reduce", "DECIMATE")
            mod.ratio = reduce_to / tris
            mod.use_collapse_triangulate = True
            bpy.context.view_layer.objects.active = o
            while o.modifiers.find("reduce") > 0:
                bpy.ops.object.modifier_move_up(modifier="reduce")
            bpy.ops.object.modifier_apply(modifier="reduce")
        print(f"merge: reduced {tris} triangles to about {reduce_to}")

bpy.ops.export_scene.gltf(
    filepath=out,
    export_format="GLTF_SEPARATE",
    export_animations=True,
    export_animation_mode="NLA_TRACKS",
    export_force_sampling=True,
    export_frame_step=1,
)
print(f"merge: wrote {out} with {len(actions)} animations")
