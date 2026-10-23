"""Builds the gardens' link machines in Blender and exports them for the machineconv
converter (src/bin/machineconv.rs).

    blender -b --python tools/link_machine.py -- <out dir>
    cargo run --release --target x86_64-unknown-linux-gnu --bin machineconv -- <out dir> mod/link_machine

Three variants of one original low-poly kiosk: `neutral` (plain), `hero` (white and gold, a
halo and wings) and `dark` (dark, with spikes and horns). Each is written as
`<variant>.obj` (triangles in SA2's axes: Y up, the front facing +Z, game units; positions,
normals and UVs) and `<variant>.tga` (a 32x32 swatch texture: one 8x8 colour cell per
material, which the UVs point at). Objects whose name starts with `@` are markers, not drawn:
`@collision` (a box the player stands against) and `@exit` (where creatures come out). The
object named `screen` is the face the picked creature's picture is drawn on.

Run inside Blender (also through the Blender MCP: `exec` this file with `OUT` set); it leaves
the three variants side by side in the scene to look at.
"""
import math
import os
import struct
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

VARIANTS = ("neutral", "hero", "dark")
CELLS = 4  # swatch cells per row (CELLS x CELLS materials)
CELL = 8  # pixels per cell side
# Colours by role, per variant (RGB 0..255).
PALETTES = {
    "neutral": {
        "base": (92, 104, 112),
        "body": (226, 222, 206),
        "trim": (64, 150, 152),
        "screen": (24, 44, 48),
        "dark": (40, 44, 52),
        "accent1": (236, 176, 48),
        "accent2": (96, 196, 92),
        "metal": (168, 176, 184),
    },
    "hero": {
        "base": (236, 232, 222),
        "body": (250, 248, 240),
        "trim": (222, 178, 64),
        "screen": (30, 52, 84),
        "dark": (120, 150, 196),
        "accent1": (120, 200, 248),
        "accent2": (250, 214, 110),
        "metal": (222, 178, 64),
        "wing": (244, 246, 255),
    },
    "dark": {
        "base": (44, 40, 52),
        "body": (72, 52, 92),
        "trim": (176, 36, 52),
        "screen": (30, 14, 26),
        "dark": (24, 20, 28),
        "accent1": (232, 60, 60),
        "accent2": (160, 96, 220),
        "metal": (120, 112, 132),
        "spike": (196, 190, 204),
    },
}


def material(variant, role):
    name = f"{variant}_{role}"
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    r, g, b = PALETTES[variant][role]
    m.diffuse_color = (r / 255, g / 255, b / 255, 1.0)
    return m


class Builder:
    def __init__(self, variant, offset):
        self.variant = variant
        self.offset = Vector(offset)
        coll = bpy.data.collections.get(f"link_machine_{variant}")
        if coll is None:
            coll = bpy.data.collections.new(f"link_machine_{variant}")
            bpy.context.scene.collection.children.link(coll)
        for o in list(coll.objects):
            bpy.data.objects.remove(o, do_unlink=True)
        self.coll = coll
        self.count = 0

    def add(self, name, bm, role, smooth=False, matrix=None):
        if matrix is not None:
            bmesh.ops.transform(bm, matrix=matrix, verts=bm.verts)
        bmesh.ops.translate(bm, vec=self.offset, verts=bm.verts)
        me = bpy.data.meshes.new(f"{self.variant}_{name}")
        bm.to_mesh(me)
        bm.free()
        for p in me.polygons:
            p.use_smooth = smooth
        if role is not None:
            me.materials.append(material(self.variant, role))
        self.count += 1
        o = bpy.data.objects.new(name if name.startswith(("@", "screen")) else f"{name}.{self.count}", me)
        self.coll.objects.link(o)
        if name.startswith("@"):
            o.display_type = "WIRE"
        return o

    def box(self, name, size, at, role, rot=None):
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        m = Matrix.Translation(at) @ (rot or Matrix.Identity(4)) @ Matrix.Diagonal((*size, 1.0))
        return self.add(name, bm, role, matrix=m)

    def cylinder(self, name, radius, depth, at, role, segments=8, rot=None, smooth=False, top=None):
        bm = bmesh.new()
        bmesh.ops.create_cone(
            bm, cap_ends=True, segments=segments, radius1=radius,
            radius2=radius if top is None else top, depth=depth,
        )
        m = Matrix.Translation(at) @ (rot or Matrix.Identity(4))
        return self.add(name, bm, role, smooth=smooth, matrix=m)

    def cone(self, name, radius, depth, at, role, segments=4, rot=None):
        """A cone standing on `at` (its base), pointing along the rotated +Z."""
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=segments, radius1=radius, radius2=0.0, depth=depth)
        bmesh.ops.translate(bm, vec=(0, 0, depth / 2), verts=bm.verts)
        m = Matrix.Translation(at) @ (rot or Matrix.Identity(4))
        return self.add(name, bm, role, matrix=m)

    def ball(self, name, radius, at, role):
        bm = bmesh.new()
        bmesh.ops.create_icosphere(bm, subdivisions=1, radius=radius)
        return self.add(name, bm, role, smooth=True, matrix=Matrix.Translation(at))

    def ring(self, name, major, minor, at, role, rot=None, segments=12, sides=4):
        bm = bmesh.new()
        rings = []
        for i in range(segments):
            a = 2 * math.pi * i / segments
            centre = Vector((math.cos(a) * major, math.sin(a) * major, 0))
            out = Vector((math.cos(a), math.sin(a), 0))
            rings.append([
                bm.verts.new(centre + out * math.cos(2 * math.pi * j / sides) * minor
                             + Vector((0, 0, 1)) * math.sin(2 * math.pi * j / sides) * minor)
                for j in range(sides)
            ])
        for i in range(segments):
            a, b = rings[i], rings[(i + 1) % segments]
            for j in range(sides):
                bm.faces.new((a[j], b[j], b[(j + 1) % sides], a[(j + 1) % sides]))
        m = Matrix.Translation(at) @ (rot or Matrix.Identity(4))
        return self.add(name, bm, role, smooth=True, matrix=m)

    def prism(self, name, profile, width, role, matrix=None):
        """A prism: `profile` (y, z) points across x = -width/2 .. width/2."""
        bm = bmesh.new()
        left = [bm.verts.new((-width / 2, y, z)) for y, z in profile]
        right = [bm.verts.new((width / 2, y, z)) for y, z in profile]
        n = len(profile)
        bm.faces.new(list(reversed(left)))
        bm.faces.new(right)
        for i in range(n):
            j = (i + 1) % n
            bm.faces.new((left[i], left[j], right[j], right[i]))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self.add(name, bm, role, matrix=matrix)


def rot_x(deg):
    return Matrix.Rotation(math.radians(deg), 4, "X")


def rot_y(deg):
    return Matrix.Rotation(math.radians(deg), 4, "Y")


def rot_z(deg):
    return Matrix.Rotation(math.radians(deg), 4, "Z")


def kiosk(b, top_cap=True):
    """The parts all three share: body, screen, buttons, the hatch creatures come out of and its
    ramp. Blender axes: front is -Y, up is +Z."""
    b.box("body", (5.6, 3.6, 7.0), (0, 0.2, 4.3), "body")
    if top_cap:
        b.box("cap", (6.2, 4.2, 0.7), (0, 0.2, 8.15), "trim")
    b.box("bezel", (4.2, 0.3, 3.6), (0, -1.7, 5.6), "trim")
    b.box("screen", (3.6, 0.1, 3.0), (0, -1.9, 5.6), "screen")
    for x, role in ((-0.8, "accent1"), (0.8, "accent2")):
        b.cylinder("button", 0.35, 0.3, (x, -1.7, 3.5), role, rot=rot_x(90))
    b.box("hatch", (2.8, 0.2, 1.8), (0, -1.65, 1.9), "dark")
    b.prism("ramp", [(-1.75, 1.0), (-1.75, 0.0), (-3.1, 0.0)], 2.6, "metal")
    b.box("@exit", (0.2, 0.2, 0.2), (0, -3.3, 1.2), None)
    for z in (3.0, 3.6, 4.2):
        for x in (-2.85, 2.85):
            b.box("grille", (0.1, 2.4, 0.25), (x, 0.2, z), "dark")


def neutral(b):
    b.box("base", (7.2, 5.2, 0.8), (0, 0.2, 0.4), "base")
    kiosk(b)
    b.cylinder("antenna", 0.12, 1.6, (1.9, 0.8, 9.3), "metal", segments=6)
    b.ball("antenna_tip", 0.35, (1.9, 0.8, 10.2), "accent1")
    b.box("@collision", (7.2, 5.2, 8.5), (0, 0.2, 4.25), None)


def hero(b):
    b.cylinder("pedestal", 3.6, 0.8, (0, 0.2, 0.4), "base", segments=8, rot=rot_z(22.5))
    b.cylinder("pedestal_rim", 3.75, 0.2, (0, 0.2, 0.9), "trim", segments=8, rot=rot_z(22.5))
    kiosk(b, top_cap=False)
    # An arched top: half a cylinder across x.
    arch = [(math.cos(math.pi * i / 8) * 2.8, 7.8 + math.sin(math.pi * i / 8) * 2.0) for i in range(9)]
    b.prism("arch", [(y, z) for y, z in arch], 4.0, "body", matrix=Matrix.Translation((0, 0.2, 0)) @ rot_z(90))
    b.prism("arch_trim", [(y * 1.04, 7.8 + (z - 7.8) * 1.06) for y, z in arch], 3.0, "trim",
            matrix=Matrix.Translation((0, 0.2, 0)) @ rot_z(90))
    b.ball("gem", 0.45, (0, -1.85, 8.6), "accent1")
    b.ring("halo", 1.5, 0.14, (0, 0.2, 11.0), "trim", rot=rot_x(12))
    # Wings: three leaf-shaped feathers a side, fanned upward from the back corners.
    for side in (-1, 1):
        for length, lift in ((3.4, 55), (2.9, 32), (2.3, 10)):
            leaf = [(0.0, 0.0), (length * 0.3, 0.4), (length, 0.15), (length * 0.85, -0.2), (length * 0.3, -0.3)]
            m = (Matrix.Translation((side * 2.7, 1.2, 6.2))
                 @ rot_y(-side * lift) @ rot_z(-90 * side))
            b.prism("feather", leaf, 0.2, "wing", matrix=m)
    b.box("@collision", (6.8, 5.2, 9.8), (0, 0.2, 4.9), None)


def dark(b):
    b.box("base", (7.2, 5.2, 0.8), (0, 0.2, 0.4), "base")
    kiosk(b, top_cap=False)
    # A pointed roof and spikes.
    b.cylinder("roof", 4.0, 1.6, (0, 0.2, 8.6), "dark", segments=4, rot=rot_z(45), top=0.9)
    for x in (-1, 1):
        for y in (-1, 1):
            tilt = Matrix.Rotation(math.radians(35), 4, Vector((-y, x, 0)).normalized())
            b.cone("spike", 0.45, 2.2, (x * 2.6, 0.2 + y * 1.6, 9.0), "spike", rot=tilt)
            b.cone("foot_spike", 0.4, 1.4, (x * 3.4, 0.2 + y * 2.4, 0.4), "spike",
                   rot=Matrix.Rotation(math.radians(60), 4, Vector((-y, x, 0)).normalized()))
        b.cone("horn", 0.5, 2.8, (x * 1.2, 0.2, 9.3), "spike", rot=rot_y(x * 20))
        for z in (2.2, 6.4):
            b.cone("side_spike", 0.35, 1.2, (x * 2.8, 0.2, z), "spike", rot=rot_y(x * 90))
    b.ball("eye", 0.4, (0, -1.85, 7.65), "accent1")
    b.box("@collision", (7.2, 5.2, 9.4), (0, 0.2, 4.7), None)


BUILD = {"neutral": neutral, "hero": hero, "dark": dark}


def build_all():
    for i, v in enumerate(VARIANTS):
        b = Builder(v, ((i - 1) * 12.0, 0, 0))
        BUILD[v](b)


def to_game(p):
    """Blender (Z up, front -Y) to SA2 (Y up, front +Z)."""
    return (p[0], p[2], -p[1])


def roles(variant):
    return list(PALETTES[variant].keys())


def export(variant, out, offset):
    coll = bpy.data.collections[f"link_machine_{variant}"]
    order = roles(variant)
    assert len(order) <= CELLS * CELLS
    lines = [f"# {variant} link machine (tools/link_machine.py)"]
    nv = 0
    deps = bpy.context.evaluated_depsgraph_get()
    for o in sorted(coll.objects, key=lambda o: o.name):
        ev = o.evaluated_get(deps)
        me = ev.to_mesh()
        me.calc_loop_triangles()
        world = Matrix.Translation(-Vector(offset)) @ o.matrix_world
        rot = world.to_3x3().inverted().transposed()
        name = o.name.split(".")[0]
        lines.append(f"o {name}")
        if me.materials:
            role = me.materials[0].name.split("_", 1)[1]
            i = order.index(role)
            u = (i % CELLS + 0.5) / CELLS
            v = 1.0 - (i // CELLS + 0.5) / CELLS
            lines.append(f"usemtl {role}")
        else:
            u = v = 0.0
        normals = me.corner_normals
        for tri in me.loop_triangles:
            idx = []
            for corner in tri.loops:
                p = to_game(world @ me.vertices[me.loops[corner].vertex_index].co)
                n = (rot @ normals[corner].vector).normalized()
                n = to_game(n)
                lines.append("v {:.4f} {:.4f} {:.4f}".format(*p))
                lines.append("vn {:.4f} {:.4f} {:.4f}".format(*n))
                lines.append(f"vt {u:.4f} {v:.4f}")
                nv += 1
                idx.append(nv)
            lines.append("f " + " ".join(f"{k}/{k}/{k}" for k in idx))
        ev.to_mesh_clear()
    with open(os.path.join(out, f"{variant}.obj"), "w") as f:
        f.write("\n".join(lines) + "\n")
    # The swatch: uncompressed 32-bit TGA, top-left origin.
    size = CELLS * CELL
    pixels = bytearray()
    for y in range(size):
        for x in range(size):
            i = (y // CELL) * CELLS + x // CELL
            r, g, b = PALETTES[variant][order[i]] if i < len(order) else (255, 0, 255)
            pixels += bytes((b, g, r, 255))
    header = struct.pack("<BBBHHBHHHHBB", 0, 0, 2, 0, 0, 0, 0, 0, size, size, 32, 0x28)
    with open(os.path.join(out, f"{variant}.tga"), "wb") as f:
        f.write(header + pixels)


def main(out):
    os.makedirs(out, exist_ok=True)
    build_all()
    for i, v in enumerate(VARIANTS):
        export(v, out, ((i - 1) * 12.0, 0, 0))
    print(f"link machine: wrote {', '.join(VARIANTS)} to {out}")


if __name__ == "__main__":
    if "OUT" in globals():
        main(globals()["OUT"])
    elif "--" in sys.argv:
        main(sys.argv[sys.argv.index("--") + 1])
    else:
        build_all()
