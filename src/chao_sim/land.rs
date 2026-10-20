//! A garden's ground, walls and water, as the game's Chao feel them (re/garden-collision.md),
//! plus the cell grid they pick places from (re/chao-ai.md §5.1).
//!
//! The game holds these as LandTables in Data_DLL and a grid in the exe. The mod reads them
//! (`Garden::read`) and sends them to the server once (`to_bytes`); nothing from the game is
//! shipped with the server.

pub type V3 = [f32; 3];

/// COL flags that make a piece of the garden collide (`0x47D553`).
pub const COLLIDABLE: u32 = 0x2003;
/// COL flags of water (`0x47D574`). Garden water also has bit 1, so solid is `& 1 && !WATER`.
pub const WATER: u32 = 0x2002;

/// The cell grid: 32 columns by 40 rows of 10x10 units (re/chao-ai.md §5.1).
pub const GRID_COLS: usize = 32;
pub const GRID_ROWS: usize = 40;
pub const GRID_CELLS: usize = GRID_COLS * GRID_ROWS;

/// Where each garden's grid is in the exe (Neutral, Hero, Dark).
pub const GRID_ADDRESS: [u32; 3] = [0x0136_2130, 0x0136_3530, 0x0136_4930];
/// The Data_DLL exports holding each garden's LandTable (Neutral, Hero, Dark).
pub const LAND_EXPORT: [&str; 3] = ["objLandTableNeut", "objLandTableHero", "objLandTableDark"];

/// Limits for reading and for what a client may send: well above the gardens (at most 671
/// triangles) and far below anything harmful.
pub const MAX_TRIS: usize = 16_384;
const MAX_DEPTH: u32 = 32;
/// Strip length cap (`0x48B510`).
const MAX_STRIP: usize = 0x400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tri {
    pub v: [V3; 3],
    /// The COL's flags (`WATER` and others).
    pub flags: u32,
}

impl Tri {
    pub fn water(&self) -> bool {
        self.flags & WATER != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Garden {
    pub tris: Vec<Tri>,
    /// Cell values (1 land, 9 water, ...), row by row.
    pub grid: Vec<i32>,
    /// The climbable edges (re/garden-collision.md §8).
    pub ledges: Vec<Ledge>,
    /// Pillars, fences and graves (0xCC: solid, and a walking Chao turns away; §8.4).
    pub blockers: Vec<Blocker>,
    /// The garden's toys (ALW category 6) as they stand when it's read.
    pub toys: Vec<GardenToy>,
    /// Its 7 tree spots (`0x1366B30`, 0x78 a garden), in slot order.
    pub tree_spots: Vec<V3>,
}

/// Tree spot positions: `TREE_SLOTS` vectors per garden, 0x78 apart (`0x548210`).
const TREE_SPOTS: u32 = 0x0136_6B30;
const TREE_SPOTS_EVERY: u32 = 0x78;
const TREE_SLOTS: u32 = 7;

/// A garden toy as the Chao sense it (`Senses::toy`, `nearest_toy`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GardenToy {
    /// ALW `+4`: 2 TV, 3 radio, 7 horse, 8 box (and 0 others).
    pub kind: u8,
    /// `Data1+0x14` and facing `Data1+0xC` (BAMS).
    pub pos: V3,
    pub rot_y: u16,
}

/// Most toys a garden's bytes may carry.
pub const MAX_TOYS: usize = 32;

/// A solid 0xCC object's collision (re/garden-collision.md §8.4), placed and turned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blocker {
    pub shape: Shape,
    /// The shape's centre in the world.
    pub pos: V3,
    /// Its local up (capsules) and its turn about y (boxes, which turn about y only).
    pub axis: V3,
    pub rot_y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Sphere {
        radius: f32,
    },
    /// Segment `pos ± half_len·axis`, radius around it (`0x484460`).
    Capsule {
        radius: f32,
        half_len: f32,
    },
    Box {
        half: V3,
    },
}

/// The Chao's collision sphere (entry 1 of `0x13134D0`): 1.5 around 2 up from its feet.
pub const CHAO_RADIUS: f32 = 1.5;
pub const CHAO_CENTRE: f32 = 2.0;

impl Blocker {
    /// How far to move a Chao at `pos` to get its sphere out (XZ only), or None if clear.
    /// (The game's push step `0x47EB30` isn't decoded: this is the shortest way out.)
    pub fn push(&self, pos: V3) -> Option<V3> {
        let s = [pos[0], pos[1] + CHAO_CENTRE, pos[2]];
        let (closest, reach) = match self.shape {
            Shape::Sphere { radius } => (self.pos, radius),
            Shape::Capsule { radius, half_len } => {
                let d = sub(s, self.pos);
                let t = dot(d, self.axis).clamp(-half_len, half_len);
                (add(self.pos, scale3(self.axis, t)), radius)
            }
            Shape::Box { half } => {
                // Into the box's frame, clamp, and back.
                let a = f32::from(self.rot_y) * std::f32::consts::TAU / 65536.0;
                let (sin, cos) = a.sin_cos();
                let d = sub(s, self.pos);
                let (lx, lz) = (cos * d[0] - sin * d[2], sin * d[0] + cos * d[2]);
                let (cx, cy, cz) = (
                    lx.clamp(-half[0], half[0]),
                    d[1].clamp(-half[1], half[1]),
                    lz.clamp(-half[2], half[2]),
                );
                let back = [cos * cx + sin * cz, cy, -sin * cx + cos * cz];
                (add(self.pos, back), 0.0)
            }
        };
        let d = sub(s, closest);
        let dist = dot(d, d).sqrt();
        let overlap = reach + CHAO_RADIUS - dist;
        if overlap <= 0.0 {
            return None;
        }
        let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
        Some(if flat > 1e-4 {
            [d[0] / flat * overlap, 0.0, d[2] / flat * overlap]
        } else {
            [overlap, 0.0, 0.0]
        })
    }

    /// Whether a Chao at `pos` touches it (its sphere overlaps the shape).
    pub fn touches(&self, pos: V3) -> bool {
        self.push(pos).is_some()
    }
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scale3(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// SET types with 0xCC collision, per garden (type index in its list, shapes in the object's
/// frame: offset from its position, then the shape; re/garden-collision.md §8.4).
pub fn blocker_shapes(garden: usize, kind: u16) -> &'static [(V3, Shape)] {
    const HASIRA: &[(V3, Shape)] = &[(
        [0.0, 20.0, 0.0],
        Shape::Capsule {
            radius: 5.0,
            half_len: 20.0,
        },
    )];
    const HASIRAKAKE: &[(V3, Shape)] = &[
        (
            [0.0, 14.0, 0.0],
            Shape::Capsule {
                radius: 5.0,
                half_len: 10.0,
            },
        ),
        ([0.0, 3.0, 0.0], Shape::Sphere { radius: 5.0 }),
        ([0.0, 10.0, 0.0], Shape::Sphere { radius: 5.0 }),
        ([0.0, 15.0, 0.0], Shape::Sphere { radius: 5.0 }),
        ([0.0, 20.0, 0.0], Shape::Sphere { radius: 5.0 }),
    ];
    const FENCE_L: &[(V3, Shape)] = &[(
        [0.0, 10.0, 0.0],
        Shape::Box {
            half: [14.2, 10.0, 0.8],
        },
    )];
    const FENCE_M: &[(V3, Shape)] = &[(
        [0.0, 5.0, 0.0],
        Shape::Box {
            half: [5.3, 15.0, 0.8],
        },
    )];
    const FENCE_S: &[(V3, Shape)] = &[(
        [0.0, 7.0, 0.0],
        Shape::Box {
            half: [8.0, 7.0, 0.3],
        },
    )];
    const HAKA_A: &[(V3, Shape)] = &[(
        [0.0, 5.8, 0.0],
        Shape::Box {
            half: [4.5, 5.8, 1.5],
        },
    )];
    const HAKA_B: &[(V3, Shape)] = &[(
        [0.0, 7.0, 0.0],
        Shape::Box {
            half: [4.0, 7.0, 1.5],
        },
    )];
    match (garden, kind) {
        (1, 0) => HASIRA,
        (1, 1) => HASIRAKAKE,
        (2, 0) => FENCE_L,
        (2, 1) => FENCE_M,
        (2, 2) => FENCE_S,
        (2, 3) => HAKA_A,
        (2, 4) => HAKA_B,
        _ => &[],
    }
}

/// The 0xCC shapes of a loaded SET buffer, placed by each entry's position and rotation
/// (capsules and spheres follow X, Y and Z; boxes turn about y only).
pub fn read_blockers(
    mem: &impl Mem,
    entries: u32,
    count: u32,
    garden: usize,
) -> Option<Vec<Blocker>> {
    let mut out = Vec::new();
    for k in 0..count.min(4096) {
        let e = entries.checked_add(k * 0x20)?;
        let shapes = blocker_shapes(garden, mem.u16(e)? & 0x7FFF);
        if shapes.is_empty() {
            continue;
        }
        let rot = [
            mem.u16(e + 2)? as i16,
            mem.u16(e + 4)? as i16,
            mem.u16(e + 6)? as i16,
        ];
        let pos = mem.v3(e + 8)?;
        let mut m = Matrix::IDENTITY;
        m.translate(pos);
        m.rotate(2, i32::from(rot[2]));
        m.rotate(1, i32::from(rot[1]));
        m.rotate(0, i32::from(rot[0]));
        let origin = m.apply([0.0; 3]);
        let up = sub(m.apply([0.0, 1.0, 0.0]), origin);
        for &(offset, shape) in shapes {
            out.push(Blocker {
                shape,
                pos: m.apply(offset),
                axis: up,
                rot_y: rot[1],
            });
        }
        if out.len() >= MAX_LEDGES {
            break;
        }
    }
    Some(out)
}

/// A C_CLIMB box (`0x55AB20`): a trigger of kind 0xCB a Chao can climb out at, turned about
/// y only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ledge {
    pub pos: V3,
    /// BAMS.
    pub rot_y: i16,
    /// Half-extents: the SET scale + 10, signed (`0x6E53C0`).
    pub half: V3,
}

impl Ledge {
    /// Whether a Chao at `pos` touches it: its collision sphere (r 1.5 at +2 up, entry 1 of
    /// `0x13134D0`) against the box (`0x483E90`).
    pub fn touches(&self, pos: V3) -> bool {
        const R: f32 = 1.5;
        let s = [pos[0], pos[1] + 2.0, pos[2]];
        let (dx, dz) = (s[0] - self.pos[0], s[2] - self.pos[2]);
        let a = f32::from(self.rot_y) * std::f32::consts::TAU / 65536.0;
        let (sin, cos) = a.sin_cos();
        let lx = cos * dx - sin * dz;
        let lz = sin * dx + cos * dz;
        lx.abs() < R + self.half[0]
            && lz.abs() < R + self.half[2]
            && s[1] - R < self.pos[1] + self.half[1]
            && s[1] + R > self.pos[1] - self.half[1]
    }
}

/// Most ledges a garden may have (the game's have 4, 7 and 11).
pub const MAX_LEDGES: usize = 256;
/// SET object-type lists (Neutral, Hero, Dark) and C_CLIMB's index in each (§8.3).
pub const SET_LISTS: [u32; 3] = [0x0121_C8B0, 0x0121_AE90, 0x0125_B4F0];
pub const CLIMB_TYPE: [u16; 3] = [6, 13, 13];

/// The C_CLIMB entries of a loaded SET buffer (`count` entries of 0x20 at `entries`).
pub fn read_ledges(mem: &impl Mem, entries: u32, count: u32, climb: u16) -> Option<Vec<Ledge>> {
    let mut out = Vec::new();
    for k in 0..count.min(4096) {
        let e = entries.checked_add(k * 0x20)?;
        if mem.u16(e)? & 0x7FFF != climb {
            continue;
        }
        let scale = mem.v3(e + 0x14)?;
        out.push(Ledge {
            pos: mem.v3(e + 8)?,
            rot_y: mem.u16(e + 4)? as i16,
            half: scale.map(|s| s + 10.0),
        });
        if out.len() >= MAX_LEDGES {
            break;
        }
    }
    Some(out)
}

/// Reads game data at its addresses: the game's own memory in the mod, a copy of the file in
/// tests and tools.
pub trait Mem {
    fn bytes(&self, addr: u32, len: usize) -> Option<&[u8]>;

    fn u16(&self, addr: u32) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(addr, 2)?.try_into().ok()?))
    }
    fn u32(&self, addr: u32) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(addr, 4)?.try_into().ok()?))
    }
    fn f32(&self, addr: u32) -> Option<f32> {
        self.u32(addr).map(f32::from_bits)
    }
    fn v3(&self, addr: u32) -> Option<V3> {
        Some([self.f32(addr)?, self.f32(addr + 4)?, self.f32(addr + 8)?])
    }
}

/// 3x4 row-major: `M·v + M[:,3]` (`0x426CC0`).
#[derive(Clone, Copy)]
struct Matrix([[f32; 4]; 3]);

impl Matrix {
    const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);

    fn apply(&self, v: V3) -> V3 {
        let m = &self.0;
        std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] + m[i][3])
    }

    /// `M = M·R` for a 3x3 `R`, as every matrix op does (post-multiplied).
    fn times(&mut self, r: [[f32; 3]; 3]) {
        let m = self.0;
        for (i, row) in self.0.iter_mut().enumerate() {
            for (j, out) in row.iter_mut().take(3).enumerate() {
                *out = (0..3).map(|k| m[i][k] * r[k][j]).sum();
            }
        }
    }

    fn translate(&mut self, p: V3) {
        let t = self.apply(p);
        for (row, t) in self.0.iter_mut().zip(t) {
            row[3] = t;
        }
    }

    /// Rotation about `axis` (0 x, 1 y, 2 z) by a BAMS angle; 0 skips it like the game.
    fn rotate(&mut self, axis: usize, bams: i32) {
        if bams == 0 {
            return;
        }
        let a = f64::from(bams) * std::f64::consts::TAU / 65536.0;
        let (s, c) = (a.sin() as f32, a.cos() as f32);
        self.times(match axis {
            0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
            1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
            _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
        });
    }

    fn scale(&mut self, s: V3) {
        self.times([[s[0], 0.0, 0.0], [0.0, s[1], 0.0], [0.0, 0.0, s[2]]]);
    }
}

impl Garden {
    /// Reads garden `index` (0 Neutral, 1 Hero, 2 Dark): `land` is its LandTable's address,
    /// `grid` reads the exe (often the same `Mem` as `mem`).
    pub fn read(mem: &impl Mem, land: u32, grid: &impl Mem, index: usize) -> Option<Self> {
        let tris = read_land(mem, land)?;
        let spots = TREE_SPOTS + index as u32 * TREE_SPOTS_EVERY;
        let tree_spots = (0..TREE_SLOTS)
            .map(|i| grid.v3(spots + i * 0xC))
            .collect::<Option<_>>()?;
        let cells = grid.bytes(*GRID_ADDRESS.get(index)?, GRID_CELLS * 4)?;
        let grid = cells
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect();
        Some(Self {
            tris,
            grid,
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots,
        })
    }

    /// Grid value at a world point, if it's on the grid.
    pub fn cell_at(&self, x: f32, z: f32) -> Option<i32> {
        let (col, row) = cell_of(x, z)?;
        self.grid.get(row * GRID_COLS + col).copied()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.tris.len() * 40 + self.grid.len() * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.tris.len() as u32).to_le_bytes());
        for t in &self.tris {
            for f in t.v.iter().flatten() {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&t.flags.to_le_bytes());
        }
        for c in &self.grid {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&(self.ledges.len() as u32).to_le_bytes());
        for l in &self.ledges {
            for f in l.pos.iter().chain(&l.half) {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&l.rot_y.to_le_bytes());
        }
        out.extend_from_slice(&(self.blockers.len() as u32).to_le_bytes());
        for b in &self.blockers {
            let (kind, dims) = match b.shape {
                Shape::Sphere { radius } => (0u8, [radius, 0.0, 0.0]),
                Shape::Capsule { radius, half_len } => (1, [radius, half_len, 0.0]),
                Shape::Box { half } => (2, half),
            };
            out.push(kind);
            for f in b.pos.iter().chain(&b.axis).chain(&dims) {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&b.rot_y.to_le_bytes());
        }
        out.extend_from_slice(&(self.toys.len() as u32).to_le_bytes());
        for t in &self.toys {
            out.push(t.kind);
            for f in &t.pos {
                out.extend_from_slice(&f.to_le_bytes());
            }
            out.extend_from_slice(&t.rot_y.to_le_bytes());
        }
        out.extend_from_slice(&(self.tree_spots.len() as u32).to_le_bytes());
        for f in self.tree_spots.iter().flatten() {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out
    }

    /// The other side of `to_bytes`; anything malformed or over the limits is refused.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut r = Bytes(bytes.strip_prefix(MAGIC)?);
        let count = r.u32()? as usize;
        if count > MAX_TRIS {
            return None;
        }
        let mut tris = Vec::with_capacity(count);
        for _ in 0..count {
            let v = [r.v3()?, r.v3()?, r.v3()?];
            tris.push(Tri { v, flags: r.u32()? });
        }
        let grid = (0..GRID_CELLS)
            .map(|_| r.u32().map(|c| c as i32))
            .collect::<Option<_>>()?;
        let count = r.u32()? as usize;
        if count > MAX_LEDGES {
            return None;
        }
        let mut ledges = Vec::with_capacity(count);
        for _ in 0..count {
            let (pos, half) = (r.v3()?, r.v3()?);
            ledges.push(Ledge {
                pos,
                half,
                rot_y: r.u16()? as i16,
            });
        }
        let count = r.u32()? as usize;
        if count > MAX_LEDGES {
            return None;
        }
        let mut blockers = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = r.take::<1>()?[0];
            let (pos, axis, dims) = (r.v3()?, r.v3()?, r.v3()?);
            let shape = match kind {
                0 => Shape::Sphere { radius: dims[0] },
                1 => Shape::Capsule {
                    radius: dims[0],
                    half_len: dims[1],
                },
                2 => Shape::Box { half: dims },
                _ => return None,
            };
            blockers.push(Blocker {
                shape,
                pos,
                axis,
                rot_y: r.u16()? as i16,
            });
        }
        let count = r.u32()? as usize;
        if count > MAX_TOYS {
            return None;
        }
        let mut toys = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = r.take::<1>()?[0];
            toys.push(GardenToy {
                kind,
                pos: r.v3()?,
                rot_y: r.u16()?,
            });
        }
        let count = r.u32()?;
        if count > TREE_SLOTS {
            return None;
        }
        let tree_spots = (0..count).map(|_| r.v3()).collect::<Option<_>>()?;
        r.0.is_empty().then_some(Self {
            tris,
            grid,
            ledges,
            blockers,
            toys,
            tree_spots,
        })
    }

    /// What players compare to agree on a garden (`chao::file::crc32` of `to_bytes`).
    pub fn fingerprint(&self) -> u32 {
        crate::chao::file::crc32(&self.to_bytes())
    }
}

const MAGIC: &[u8; 8] = b"SA2LAND4";

/// Reads `from_bytes`' fields in order; coordinates must be sane numbers.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn v3(&mut self) -> Option<V3> {
        let v = [0; 3].map(|_| self.take().map(f32::from_le_bytes));
        let v = [v[0]?, v[1]?, v[2]?];
        v.iter()
            .all(|f| f.is_finite() && f.abs() <= 1e5)
            .then_some(v)
    }
}

/// The cell of a world point: 10 units a cell from x = -160, z = -200, looked up only for
/// x in -150..=150 and z in -200..=150 (`0x535010`; re/chao-ai.md §5.4).
pub fn cell_of(x: f32, z: f32) -> Option<(usize, usize)> {
    ((-150.0..=150.0).contains(&x) && (-200.0..=150.0).contains(&z))
        .then(|| (((x + 160.0) / 10.0) as usize, ((z + 200.0) / 10.0) as usize))
}

/// The collision triangles of a LandTable (`0x47D160`, `0x48BE80`, `0x48B9F0`, `0x48B610`).
pub fn read_land(mem: &impl Mem, land: u32) -> Option<Vec<Tri>> {
    let count = mem.u16(land)? as i16;
    let skip = mem.u16(land + 2)? as i16;
    let cols = mem.u32(land + 0x10)?;
    let mut tris = Vec::new();
    for i in skip.max(0)..count.max(0) {
        let col = cols.checked_add(u32::from(i as u16) * 0x20)?;
        let flags = mem.u32(col + 0x1C)?;
        if flags & COLLIDABLE == 0 {
            continue;
        }
        let object = mem.u32(col + 0x10)?;
        if object != 0 {
            walk(mem, object, Matrix::IDENTITY, flags, 0, &mut tris)?;
        }
    }
    Some(tris)
}

/// One object and its children (and their siblings), `T·Rz·Ry·Rx·S`, eval flags ignored
/// (`0x48B9F0`).
fn walk(
    mem: &impl Mem,
    object: u32,
    parent: Matrix,
    flags: u32,
    depth: u32,
    out: &mut Vec<Tri>,
) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut m = parent;
    m.translate(mem.v3(object + 8)?);
    m.rotate(2, mem.u32(object + 0x1C)? as i32);
    m.rotate(1, mem.u32(object + 0x18)? as i32);
    m.rotate(0, mem.u32(object + 0x14)? as i32);
    m.scale(mem.v3(object + 0x20)?);
    let mut child = mem.u32(object + 0x2C)?;
    let mut siblings = 0;
    while child != 0 {
        walk(mem, child, m, flags, depth + 1, out)?;
        child = mem.u32(child + 0x30)?;
        siblings += 1;
        if siblings > 4096 {
            return None;
        }
    }
    let model = mem.u32(object + 4)?;
    if model != 0 {
        triangulate(mem, model, &m, flags, out)?;
    }
    Some(())
}

/// A basic model's triangles (`0x48B610`, `0x48B510`): triangles, quads and strips. Stored
/// normals are not used; the ray test makes its own.
fn triangulate(
    mem: &impl Mem,
    model: u32,
    m: &Matrix,
    flags: u32,
    out: &mut Vec<Tri>,
) -> Option<()> {
    let points = mem.u32(model)?;
    let sets = mem.u32(model + 0xC)?;
    let set_count = mem.u16(model + 0x14)?;
    let point =
        |i: u16| -> Option<V3> { Some(m.apply(mem.v3(points.checked_add(u32::from(i) * 12)?)?)) };
    let mut push = |a: u16, b: u16, c: u16| -> Option<()> {
        if out.len() >= MAX_TRIS {
            return None;
        }
        out.push(Tri {
            v: [point(a)?, point(b)?, point(c)?],
            flags,
        });
        Some(())
    };
    for k in 0..u32::from(set_count) {
        let set = sets.checked_add(k * 0x18)?;
        let kind = mem.u16(set)? & 0xC000;
        let polys = mem.u16(set + 2)?;
        let mut at = mem.u32(set + 4)?;
        let mut next = || -> Option<u16> {
            let v = mem.u16(at)?;
            at = at.checked_add(2)?;
            Some(v)
        };
        for _ in 0..polys {
            match kind {
                0x0000 => {
                    let (a, b, c) = (next()?, next()?, next()?);
                    push(a, b, c)?;
                }
                0x4000 => {
                    let (a, b, c, d) = (next()?, next()?, next()?, next()?);
                    push(a, b, c)?;
                    push(b, d, c)?;
                }
                _ => {
                    let head = next()?;
                    let len = usize::from(head & 0x7FFF).min(MAX_STRIP);
                    let flip = usize::from(head & 0x8000 != 0);
                    let strip: Vec<u16> = (0..len).map(|_| next()).collect::<Option<_>>()?;
                    for t in 0..len.saturating_sub(2) {
                        let (a, b, c) = (strip[t], strip[t + 1], strip[t + 2]);
                        if (t + flip) & 1 == 0 {
                            push(a, b, c)?;
                        } else {
                            push(a, c, b)?;
                        }
                    }
                }
            }
        }
    }
    Some(())
}

/// What a probe found along one axis (`0x48C5E0` slot).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The coordinate along the axis where the line meets the triangle.
    pub at: f32,
    /// Unit normal from the triangle's winding ((0, 1, 0) if degenerate).
    pub normal: V3,
    pub flags: u32,
}

impl Hit {
    pub fn water(&self) -> bool {
        self.flags & WATER != 0
    }
}

/// Slots in the game's order: -X, +X, -Y (floor), +Y, -Z, +Z.
pub type Probe = [Option<Hit>; 6];

/// How far a probe looks, and the range a triangle's first vertex must be within (`0x48C5E0`).
pub const PROBE_RANGE: f32 = 220.0;

/// The game's six-way ray test from `q` (`0x48C5E0`): three two-sided axis lines through `q`;
/// along each, the nearest hit below and above within `PROBE_RANGE`.
pub fn probe(tris: &[Tri], q: V3) -> Probe {
    let mut slots: Probe = [None; 6];
    let r2 = PROBE_RANGE * PROBE_RANGE;
    for t in tris {
        let [v0, v1, v2] = t.v;
        let (dx, dy, dz) = (v0[0] - q[0], v0[1] - q[1], v0[2] - q[2]);
        if dx * dx > r2 || dx * dx + dz * dz > r2 || dx * dx + dz * dz + dy * dy > r2 {
            continue;
        }
        let n = cross(sub(v1, v0), sub(v2, v0));
        for axis in 0..3 {
            if n[axis] * n[axis] < 1e-4 {
                continue;
            }
            let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
            if !inside_2d(t.v, q, b, c) {
                continue;
            }
            let at = v0[axis] - (n[b] * (q[b] - v0[b]) + n[c] * (q[c] - v0[c])) / n[axis];
            let (slot, nearer) = if at < q[axis] {
                (axis * 2, at > q[axis] - PROBE_RANGE)
            } else {
                (axis * 2 + 1, at < q[axis] + PROBE_RANGE)
            };
            let better = match slots[slot] {
                Some(h) if slot % 2 == 0 => at > h.at,
                Some(h) => at < h.at,
                None => nearer,
            };
            if better {
                slots[slot] = Some(Hit {
                    at,
                    normal: unit(n),
                    flags: t.flags,
                });
            }
        }
    }
    slots
}

/// Whether `q` projected on the (b, c) plane is inside the triangle, either winding.
fn inside_2d(v: [V3; 3], q: V3, b: usize, c: usize) -> bool {
    let edge = |i: usize| {
        let (p, n) = (v[i], v[(i + 1) % 3]);
        (n[b] - p[b]) * (q[c] - p[c]) - (n[c] - p[c]) * (q[b] - p[b])
    };
    let (e0, e1, e2) = (edge(0), edge(1), edge(2));
    e0 * e1 >= 0.0 && e1 * e2 >= 0.0 && e0 * e2 >= 0.0
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: V3) -> V3 {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// A PE file's mapped image, for reading Data_DLL (and the exe) outside the game.
pub struct PeFile {
    base: u32,
    sections: Vec<(u32, u32, u32)>,
    data: Vec<u8>,
}

impl PeFile {
    pub fn parse(data: Vec<u8>) -> Option<Self> {
        let at = |o: usize, n: usize| data.get(o..o + n);
        let u16_at = |o| Some(u16::from_le_bytes(at(o, 2)?.try_into().ok()?));
        let u32_at = |o| Some(u32::from_le_bytes(at(o, 4)?.try_into().ok()?));
        let pe = u32_at(0x3C)? as usize;
        if at(pe, 4)? != b"PE\0\0" {
            return None;
        }
        let count = usize::from(u16_at(pe + 6)?);
        let opt = pe + 24;
        let base = u32_at(opt + 28)?;
        let first = opt + usize::from(u16_at(pe + 20)?);
        let mut sections = Vec::new();
        for i in 0..count {
            let s = first + i * 40;
            let (vsize, va, raw_size, raw) = (
                u32_at(s + 8)?,
                u32_at(s + 12)?,
                u32_at(s + 16)?,
                u32_at(s + 20)?,
            );
            sections.push((va, raw_size.min(vsize.max(1)), raw));
        }
        Some(Self {
            base,
            sections,
            data,
        })
    }

    /// The address of an exported symbol.
    pub fn export(&self, name: &str) -> Option<u32> {
        let pe = self.u32(self.base + 0x3C)?;
        let dir = self.u32(self.base + pe + 24 + 96)?;
        let e = self.base + dir;
        let (names, functions, ordinals) =
            (self.u32(e + 32)?, self.u32(e + 28)?, self.u32(e + 36)?);
        for i in 0..self.u32(e + 24)? {
            let at = self.base + self.u32(self.base + names + i * 4)?;
            let found = self.bytes(at, name.len() + 1)?;
            if &found[..name.len()] == name.as_bytes() && found[name.len()] == 0 {
                let ordinal = u32::from(self.u16(self.base + ordinals + i * 2)?);
                return Some(self.base + self.u32(self.base + functions + ordinal * 4)?);
            }
        }
        None
    }
}

impl Mem for PeFile {
    fn bytes(&self, addr: u32, len: usize) -> Option<&[u8]> {
        let rva = addr.checked_sub(self.base)?;
        if rva < 0x1000 {
            return self.data.get(rva as usize..rva as usize + len);
        }
        let &(va, size, raw) = self
            .sections
            .iter()
            .find(|&&(va, size, _)| rva >= va && rva - va < size)?;
        let off = (raw + rva - va) as usize;
        (rva - va + len as u32 <= size).then_some(())?;
        self.data.get(off..off + len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_alternates_winding() {
        // One strip of 4 points: two triangles, the second turned around.
        let mut mem = Fake::default();
        let points = mem.put(
            &[
                0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0,
            ]
            .map(f32::to_le_bytes)
            .concat(),
        );
        let indices = mem.put(&[4u16, 0, 1, 2, 3].map(u16::to_le_bytes).concat());
        let sets = mem.put(
            &[
                &0xC000u16.to_le_bytes()[..],
                &1u16.to_le_bytes(),
                &indices.to_le_bytes(),
                &[0; 16],
            ]
            .concat(),
        );
        let model = mem.put(
            &[
                &points.to_le_bytes()[..],
                &[0; 8],
                &sets.to_le_bytes(),
                &[0; 4],
                &1u16.to_le_bytes(),
                &[0; 18],
            ]
            .concat(),
        );
        let object = mem.put(
            &[
                &0u32.to_le_bytes()[..],
                &model.to_le_bytes(),
                &[0; 24],
                &[1.0f32, 1.0, 1.0].map(f32::to_le_bytes).concat(),
                &[0; 8],
            ]
            .concat(),
        );
        let col = mem.put(
            &[
                &[0u8; 16][..],
                &object.to_le_bytes(),
                &[0; 8],
                &1u32.to_le_bytes(),
            ]
            .concat(),
        );
        let land = mem.put(
            &[
                &1u16.to_le_bytes()[..],
                &0u16.to_le_bytes(),
                &[0; 12],
                &col.to_le_bytes(),
            ]
            .concat(),
        );
        let tris = read_land(&mem, land).unwrap();
        assert_eq!(tris.len(), 2);
        assert_eq!(
            tris[0].v,
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
        );
        assert_eq!(
            tris[1].v,
            [[1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn a_probe_finds_the_floor_below_and_the_roof_above() {
        let floor = Tri {
            v: [[-10.0, 0.0, -10.0], [10.0, 0.0, -10.0], [0.0, 0.0, 10.0]],
            flags: 1,
        };
        let roof = Tri {
            v: [[-10.0, 8.0, -10.0], [0.0, 8.0, 10.0], [10.0, 8.0, -10.0]],
            flags: 1,
        };
        let p = probe(&[floor, roof], [0.0, 3.0, 0.0]);
        assert_eq!(p[2].map(|h| h.at), Some(0.0));
        assert_eq!(p[3].map(|h| h.at), Some(8.0));
        assert!(p[0].is_none() && p[1].is_none() && p[4].is_none() && p[5].is_none());
        // Out past the edge, nothing.
        assert!(probe(&[floor], [30.0, 3.0, 0.0])[2].is_none());
    }

    #[test]
    fn a_garden_survives_the_trip_and_bad_ones_dont() {
        let garden = Garden {
            tris: vec![Tri {
                v: [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]],
                flags: WATER | 1,
            }],
            grid: (0..GRID_CELLS as i32).collect(),
            ledges: vec![Ledge {
                pos: [1.0, -2.0, 3.0],
                rot_y: 0x4000,
                half: [10.0, 5.0, 2.0],
            }],
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        };
        let bytes = garden.to_bytes();
        assert_eq!(Garden::from_bytes(&bytes), Some(garden));
        assert!(Garden::from_bytes(&bytes[..bytes.len() - 1]).is_none());
        let mut nan = bytes.clone();
        // (the first triangle's x)
        nan[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Garden::from_bytes(&nan).is_none());
    }

    #[test]
    fn a_chao_touches_a_ledge_box_turned_about_y() {
        // 20 long across x when unturned, 4 deep, from y -6 to 4.
        let ledge = Ledge {
            pos: [0.0, -1.0, 0.0],
            rot_y: 0,
            half: [10.0, 5.0, 2.0],
        };
        assert!(ledge.touches([9.0, -3.0, 0.0]));
        assert!(!ledge.touches([12.0, -3.0, 0.0]));
        // The sphere's 1.5 reaches 3.4 past a 2-deep face.
        assert!(ledge.touches([0.0, -3.0, 3.4]) && !ledge.touches([0.0, -3.0, 3.6]));
        // Above: its sphere (y + 2 ± 1.5) clears the top at 4.
        assert!(!ledge.touches([0.0, 3.6, 0.0]));
        // A quarter turn: now long across z.
        let turned = Ledge {
            rot_y: 0x4000,
            ..ledge
        };
        assert!(turned.touches([0.0, -3.0, 9.0]) && !turned.touches([9.0, -3.0, 0.0]));
    }

    #[test]
    fn cells_cover_the_grid() {
        assert_eq!(cell_of(-150.0, -200.0), Some((1, 0)));
        assert_eq!(cell_of(150.0, 150.0), Some((31, 35)));
        assert_eq!(cell_of(150.1, 0.0), None);
        assert_eq!(cell_of(0.0, 150.1), None);
    }

    /// The real gardens, when the game is installed here (skipped otherwise).
    #[test]
    fn the_installed_gardens_read_like_the_notes() {
        let Ok(data) = std::fs::read(
            "/mnt/SSD1/SteamLibrary/steamapps/common/Sonic Adventure 2/resource/gd_PC/DLL/Win32/Data_DLL_orig.dll",
        ) else {
            return;
        };
        let dll = PeFile::parse(data).unwrap();
        // Triangles and water heights per re/garden-collision.md §5.
        for (name, count, water) in [
            ("objLandTableNeut", 671, -2.0),
            ("objLandTableHero", 486, -1.5),
            ("objLandTableDark", 520, -1.0),
        ] {
            let tris = read_land(&dll, dll.export(name).unwrap()).unwrap();
            assert_eq!(tris.len(), count, "{name}");
            assert!(
                tris.iter()
                    .filter(|t| t.water())
                    .all(|t| t.v.iter().all(|v| (v[1] - water).abs() < 1e-3)),
                "{name}"
            );
        }
        // Neutral's floor at (0, 0) is at 4.68 (§5 samples).
        let neut = read_land(&dll, dll.export("objLandTableNeut").unwrap()).unwrap();
        let floor = probe(&neut, [0.0, 50.0, 0.0])[2].unwrap();
        assert!((floor.at - 4.68).abs() < 0.01, "{}", floor.at);
    }

    /// (land on ground, land, water on water, water) per garden.
    const RESULTS: [(usize, usize, usize, usize); 3] =
        [(288, 288, 45, 45), (299, 316, 55, 86), (184, 185, 59, 59)];

    /// The grids from the dumped exe, against the real gardens: a water cell has water under
    /// its middle and a land cell has solid ground (skipped without the game and the dump).
    #[test]
    fn the_grids_agree_with_the_ground() {
        let (Ok(dll), Ok(exe)) = (
            std::fs::read(
                "/mnt/SSD1/SteamLibrary/steamapps/common/Sonic Adventure 2/resource/gd_PC/DLL/Win32/Data_DLL_orig.dll",
            ),
            std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/re/bin/sonic2app.dumped.exe"
            )),
        ) else {
            return;
        };
        let (dll, exe) = (PeFile::parse(dll).unwrap(), PeFile::parse(exe).unwrap());
        let mut results = Vec::new();
        for (i, name) in LAND_EXPORT.iter().enumerate() {
            let garden = Garden::read(&dll, dll.export(name).unwrap(), &exe, i).unwrap();
            // Its 7 tree spots stand on the ground.
            assert_eq!(garden.tree_spots.len(), 7);
            for spot in &garden.tree_spots {
                let floor = probe(&garden.tris, [spot[0], spot[1] + 5.0, spot[2]])[2];
                assert!(
                    floor.is_some_and(|f| (f.at - spot[1]).abs() < 3.0),
                    "{name} {spot:?}"
                );
            }
            let mut counts = std::collections::BTreeMap::new();
            let (mut land, mut land_ok, mut water, mut water_ok) = (0, 0, 0, 0);
            for row in 0..GRID_ROWS {
                for col in 0..GRID_COLS {
                    let v = garden.grid[row * GRID_COLS + col];
                    *counts.entry(v).or_insert(0) += 1;
                    let (x, z) = (
                        (col as f32 - 16.0 + 0.5) * 10.0,
                        (row as f32 - 20.0 + 0.5) * 10.0,
                    );
                    // The highest ground under the middle (from two heights: probes see 220 up).
                    let below = [60.0, 180.0]
                        .into_iter()
                        .filter_map(|y| probe(&garden.tris, [x, y, z])[2])
                        .max_by(|a, b| a.at.total_cmp(&b.at));
                    match v {
                        1 => {
                            land += 1;
                            land_ok += usize::from(below.is_some_and(|h| !h.water()));
                        }
                        9 => {
                            water += 1;
                            water_ok += usize::from(below.is_some_and(|h| h.water()));
                        }
                        _ => {}
                    }
                }
            }
            eprintln!(
                "{name}: values {counts:?}; land {land_ok}/{land} on ground, water {water_ok}/{water} on water"
            );
            results.push((land_ok, land, water_ok, water));
        }
        // Neutral agrees cell for cell. Hero's grid marks 31 cells around (0..25, 5..45) and
        // (-35..-5, 85..115) as water over dry floor at y = 0 (those Chao walk, as in the game).
        // Hero's land cells at x -5..15, z -125..-25 and Dark's (15, -105) are open water
        // along the shore (the 10-unit grid is coarse there): a Chao sent there swims
        // (re/garden-collision.md §7).
        assert_eq!(results, RESULTS);
    }

    /// Memory made of pieces put in one after another.
    #[derive(Default)]
    struct Fake(Vec<u8>);

    impl Fake {
        const BASE: u32 = 0x1000;
        fn put(&mut self, b: &[u8]) -> u32 {
            let at = Self::BASE + self.0.len() as u32;
            self.0.extend_from_slice(b);
            at
        }
    }

    impl Mem for Fake {
        fn bytes(&self, addr: u32, len: usize) -> Option<&[u8]> {
            let o = addr.checked_sub(Self::BASE)? as usize;
            self.0.get(o..o + len)
        }
    }
}
