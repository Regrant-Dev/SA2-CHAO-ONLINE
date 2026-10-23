//! The link machine's model data (re/chao-garden-objects.md §1): `convert` turns a triangle
//! mesh (OBJ with positions, normals and UVs, as tools/link_machine.py writes it) and its
//! swatch texture (uncompressed 32-bit TGA) into SA2 chunk model pieces in a small container,
//! the files in `mod/link_machine/`; `Machine::parse` reads and checks one for drawing.
//!
//! Container (little-endian): `LMC1`, the piece count, the screen (centre x, y, front z,
//! width, height: it faces +Z), the exit point, the collision box (min, max), the texture
//! (u16 width, u16 height, RGBA8 rows from the top), then per piece its vertex list and polygon
//! list (u32 word counts, then the words). Each piece is one chunk model: one 0x29 block
//! (position and normal) of at most `MAX_VERTS` vertices at slot 0; a white diffuse material
//! with source-alpha blend, texture 0 clamped, and 0x41 strips of one triangle each (lit, no
//! specular or ambient, both sides), as tools/skinconv.py writes them.
//!
//! The source mesh is in SA2's axes (Y up, front +Z). Objects named `@collision` and `@exit` are
//! markers (their bounds are kept, not drawn); the one named `screen` is drawn and its bounds
//! are the screen.
//!
//! Also built into `machineconv` (src/bin/machineconv.rs), so it stands alone.

// The mod only reads the data; the converter is for machineconv and the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;

pub const MAGIC: &[u8; 4] = b"LMC1";
/// Vertices in one piece's block (the game's own models stay near this).
pub const MAX_VERTS: usize = 500;
/// Strips per strip chunk.
const STRIPS_PER_CHUNK: usize = 6000;
/// Material 0x11 (diffuse) with blend 0x25 (source alpha / inverse source alpha), size 2.
const MATERIAL: [u16; 4] = [0x2511, 2, 0xFFFF, 0xFFFF];
/// Texture 0, clamp U and V.
const TEXTURE: [u16; 2] = [0x3008, 0];
/// Strips with UVs (0x41): no specular (0x02), no ambient (0x04), both sides (0x10).
const STRIP_HEAD: u16 = 0x41 | 0x16 << 8;
const VERTEX_VN: u32 = 0x29;
const END: u32 = 0xFF;

/// The screen: its centre (x, y and the front z) and size; it faces +Z.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Screen {
    pub centre: [f32; 3],
    pub width: f32,
    pub height: f32,
}

/// One chunk model: its vertex and polygon lists as u32 words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Piece {
    pub vlist: Vec<u32>,
    pub plist: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub screen: Screen,
    pub exit: [f32; 3],
    pub box_min: [f32; 3],
    pub box_max: [f32; 3],
    pub texture_size: (u16, u16),
    /// RGBA8, rows from the top.
    pub texture: Vec<u8>,
    pub pieces: Vec<Piece>,
}

/// One corner of a triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Corner {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bounds {
    fn new() -> Self {
        Self {
            min: [f32::MAX; 3],
            max: [f32::MIN; 3],
        }
    }

    fn add(&mut self, p: [f32; 3]) {
        self.min = std::array::from_fn(|i| self.min[i].min(p[i]));
        self.max = std::array::from_fn(|i| self.max[i].max(p[i]));
    }

    fn empty(&self) -> bool {
        self.min[0] > self.max[0]
    }

    fn centre(&self) -> [f32; 3] {
        std::array::from_fn(|i| (self.min[i] + self.max[i]) / 2.0)
    }
}

/// The drawn triangles and the marker bounds of an OBJ.
struct Mesh {
    triangles: Vec<[Corner; 3]>,
    screen: Bounds,
    exit: Bounds,
    collision: Bounds,
}

fn floats<const N: usize>(parts: &[&str], line: usize) -> Result<[f32; N], String> {
    if parts.len() < N {
        return Err(format!("line {line}: {N} numbers expected"));
    }
    let mut out = [0.0; N];
    for (o, p) in out.iter_mut().zip(parts) {
        *o = p
            .parse()
            .map_err(|_| format!("line {line}: bad number {p:?}"))?;
    }
    Ok(out)
}

fn read_obj(text: &str) -> Result<Mesh, String> {
    let (mut v, mut vt, mut vn) = (Vec::new(), Vec::new(), Vec::new());
    let mut mesh = Mesh {
        triangles: Vec::new(),
        screen: Bounds::new(),
        exit: Bounds::new(),
        collision: Bounds::new(),
    };
    let mut object = String::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.first().copied() {
            Some("o") => object = parts.get(1).copied().unwrap_or("").to_string(),
            Some("v") => v.push(floats::<3>(&parts[1..], n)?),
            Some("vt") => vt.push(floats::<2>(&parts[1..], n)?),
            Some("vn") => vn.push(floats::<3>(&parts[1..], n)?),
            Some("f") => {
                let corners = parts[1..]
                    .iter()
                    .map(|c| corner(c, &v, &vt, &vn, n))
                    .collect::<Result<Vec<_>, _>>()?;
                if corners.len() < 3 {
                    return Err(format!("line {n}: a face needs 3 corners"));
                }
                let marker = match object.as_str() {
                    "@collision" => Some(&mut mesh.collision),
                    "@exit" => Some(&mut mesh.exit),
                    _ => None,
                };
                if let Some(b) = marker {
                    corners.iter().for_each(|c| b.add(c.pos));
                    continue;
                }
                if object == "screen" {
                    corners.iter().for_each(|c| mesh.screen.add(c.pos));
                }
                for k in 1..corners.len() - 1 {
                    mesh.triangles
                        .push([corners[0], corners[k], corners[k + 1]]);
                }
            }
            _ => {}
        }
    }
    Ok(mesh)
}

/// One `v/vt/vn` face corner (1-based indices).
fn corner(
    text: &str,
    v: &[[f32; 3]],
    vt: &[[f32; 2]],
    vn: &[[f32; 3]],
    line: usize,
) -> Result<Corner, String> {
    let mut it = text.split('/');
    let mut index = |len: usize| -> Result<usize, String> {
        let s = it.next().unwrap_or("");
        let i: usize = s
            .parse()
            .map_err(|_| format!("line {line}: corner {text:?} needs v/vt/vn"))?;
        (1..=len)
            .contains(&i)
            .then_some(i - 1)
            .ok_or_else(|| format!("line {line}: index {i} out of range"))
    };
    let (p, t, n) = (index(v.len())?, index(vt.len())?, index(vn.len())?);
    Ok(Corner {
        pos: v[p],
        normal: vn[n],
        uv: vt[t],
    })
}

/// An uncompressed 32-bit TGA as RGBA8 rows from the top.
pub fn read_tga(bytes: &[u8]) -> Result<(u16, u16, Vec<u8>), String> {
    if bytes.len() < 18 {
        return Err("TGA too short".into());
    }
    let id_len = usize::from(bytes[0]);
    let (kind, depth, descriptor) = (bytes[2], bytes[16], bytes[17]);
    if bytes[1] != 0 || kind != 2 || depth != 32 {
        return Err("only uncompressed 32-bit TGA".into());
    }
    let w = u16::from_le_bytes([bytes[12], bytes[13]]);
    let h = u16::from_le_bytes([bytes[14], bytes[15]]);
    if w == 0 || h == 0 || !w.is_multiple_of(4) || !h.is_multiple_of(4) {
        return Err(format!("texture {w}x{h}: sides must be multiples of 4"));
    }
    let (w_, h_) = (usize::from(w), usize::from(h));
    let data = bytes
        .get(18 + id_len..18 + id_len + w_ * h_ * 4)
        .ok_or("TGA pixels missing")?;
    let top_first = descriptor & 0x20 != 0;
    let mut rgba = vec![0u8; w_ * h_ * 4];
    for y in 0..h_ {
        let src_row = if top_first { y } else { h_ - 1 - y };
        for x in 0..w_ {
            let s = (src_row * w_ + x) * 4;
            let d = (y * w_ + x) * 4;
            rgba[d..d + 4].copy_from_slice(&[data[s + 2], data[s + 1], data[s], data[s + 3]]);
        }
    }
    Ok((w, h, rgba))
}

/// The triangles in pieces of at most `MAX_VERTS` distinct vertices (position and normal).
fn split(triangles: &[[Corner; 3]]) -> Vec<Vec<[Corner; 3]>> {
    let key = |c: &Corner| (c.pos.map(f32::to_bits), c.normal.map(f32::to_bits));
    let mut pieces = Vec::new();
    let mut piece: Vec<[Corner; 3]> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in triangles {
        let new = t.iter().filter(|c| !seen.contains(&key(c))).count();
        if seen.len() + new > MAX_VERTS {
            pieces.push(std::mem::take(&mut piece));
            seen.clear();
        }
        seen.extend(t.iter().map(key));
        piece.push(*t);
    }
    if !piece.is_empty() {
        pieces.push(piece);
    }
    pieces
}

/// A UV (1.0 = the texture's size, OBJ's v from the bottom) as the strips store it (256 = 1.0,
/// v from the top).
fn uv16(uv: [f32; 2]) -> [u16; 2] {
    let q = |x: f32| ((x * 256.0).round().clamp(-32768.0, 32767.0) as i16) as u16;
    [q(uv[0]), q(1.0 - uv[1])]
}

fn piece(triangles: &[[Corner; 3]]) -> Piece {
    let mut slots: HashMap<([u32; 3], [u32; 3]), u16> = HashMap::new();
    let mut verts: Vec<&Corner> = Vec::new();
    let mut strips: Vec<[[u16; 3]; 3]> = Vec::new();
    for t in triangles {
        strips.push(t.each_ref().map(|c| {
            let k = (c.pos.map(f32::to_bits), c.normal.map(f32::to_bits));
            let slot = *slots.entry(k).or_insert_with(|| {
                verts.push(c);
                (verts.len() - 1) as u16
            });
            let [u, v] = uv16(c.uv);
            [slot, u, v]
        }));
    }
    let n = verts.len() as u32;
    let mut vlist = vec![VERTEX_VN | (1 + 6 * n) << 16, n << 16];
    for c in verts {
        vlist.extend(c.pos.iter().chain(&c.normal).map(|f| f.to_bits()));
    }
    vlist.push(END);
    let mut p16: Vec<u16> = MATERIAL.iter().chain(&TEXTURE).copied().collect();
    for chunk in strips.chunks(STRIPS_PER_CHUNK) {
        let words = 1 + 10 * chunk.len();
        let pad = words % 2;
        p16.extend([STRIP_HEAD, (words + pad) as u16, chunk.len() as u16]);
        for s in chunk {
            p16.push(3);
            p16.extend(s.iter().flatten());
        }
        if pad == 1 {
            p16.push(0);
        }
    }
    p16.push(END as u16);
    if p16.len() % 2 == 1 {
        p16.push(0);
    }
    let plist = p16
        .chunks(2)
        .map(|c| u32::from(c[0]) | u32::from(c[1]) << 16)
        .collect();
    Piece { vlist, plist }
}

/// Turns an OBJ (`obj`) and its swatch texture (`tga`) into the container's bytes.
pub fn convert(obj: &str, tga: &[u8]) -> Result<Vec<u8>, String> {
    let mesh = read_obj(obj)?;
    let (w, h, texture) = read_tga(tga)?;
    if mesh.triangles.is_empty() {
        return Err("nothing to draw".into());
    }
    for (b, name) in [
        (&mesh.screen, "screen"),
        (&mesh.exit, "@exit"),
        (&mesh.collision, "@collision"),
    ] {
        if b.empty() {
            return Err(format!("no {name} object"));
        }
    }
    let s = &mesh.screen;
    let c = s.centre();
    let machine = Machine {
        screen: Screen {
            centre: [c[0], c[1], s.max[2]],
            width: s.max[0] - s.min[0],
            height: s.max[1] - s.min[1],
        },
        exit: mesh.exit.centre(),
        box_min: mesh.collision.min,
        box_max: mesh.collision.max,
        texture_size: (w, h),
        texture,
        pieces: split(&mesh.triangles).iter().map(|t| piece(t)).collect(),
    };
    Ok(machine.to_bytes())
}

impl Machine {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend((self.pieces.len() as u32).to_le_bytes());
        let s = &self.screen;
        let size = [s.width, s.height];
        let floats = s
            .centre
            .iter()
            .chain(&size)
            .chain(&self.exit)
            .chain(&self.box_min)
            .chain(&self.box_max);
        for f in floats {
            out.extend(f.to_le_bytes());
        }
        out.extend(self.texture_size.0.to_le_bytes());
        out.extend(self.texture_size.1.to_le_bytes());
        out.extend(&self.texture);
        for p in &self.pieces {
            out.extend((p.vlist.len() as u32).to_le_bytes());
            out.extend((p.plist.len() as u32).to_le_bytes());
            for w in p.vlist.iter().chain(&p.plist) {
                out.extend(w.to_le_bytes());
            }
        }
        out
    }

    /// Reads a container and checks every piece's chunk lists.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let s = bytes.get(at..at + n).ok_or("the model data is cut short")?;
            at += n;
            Ok(s)
        };
        if take(4)? != MAGIC {
            return Err("not link machine data".into());
        }
        let u32_at = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
        let pieces = u32_at(take(4)?) as usize;
        let mut f = [0f32; 14];
        for x in &mut f {
            *x = f32::from_bits(u32_at(take(4)?));
        }
        if f.iter().any(|x| !x.is_finite()) {
            return Err("a number isn't finite".into());
        }
        let w = u16::from_le_bytes(take(2)?.try_into().unwrap_or_default());
        let h = u16::from_le_bytes(take(2)?.try_into().unwrap_or_default());
        if w == 0 || h == 0 || !w.is_multiple_of(4) || !h.is_multiple_of(4) {
            return Err("bad texture size".into());
        }
        let texture = take(usize::from(w) * usize::from(h) * 4)?.to_vec();
        if pieces == 0 || pieces > 64 {
            return Err("bad piece count".into());
        }
        let mut list = Vec::with_capacity(pieces);
        for _ in 0..pieces {
            let vn = u32_at(take(4)?) as usize;
            let pn = u32_at(take(4)?) as usize;
            let words = |s: &[u8]| s.chunks(4).map(u32_at).collect::<Vec<u32>>();
            let vlist = words(take(vn.checked_mul(4).ok_or("too big")?)?);
            let plist = words(take(pn.checked_mul(4).ok_or("too big")?)?);
            let count = check_vlist(&vlist)?;
            check_plist(&plist, count)?;
            list.push(Piece { vlist, plist });
        }
        if at != bytes.len() {
            return Err("extra bytes after the model".into());
        }
        Ok(Self {
            screen: Screen {
                centre: [f[0], f[1], f[2]],
                width: f[3],
                height: f[4],
            },
            exit: [f[5], f[6], f[7]],
            box_min: [f[8], f[9], f[10]],
            box_max: [f[11], f[12], f[13]],
            texture_size: (w, h),
            texture,
            pieces: list,
        })
    }
}

/// Checks a vertex list (one 0x29 block at slot 0, then the end); returns its vertex count.
fn check_vlist(v: &[u32]) -> Result<usize, String> {
    let (&h1, &h2) = (
        v.first().ok_or("empty vertex list")?,
        v.get(1).ok_or("short")?,
    );
    let n = (h2 >> 16) as usize;
    if h1 & 0xFF != VERTEX_VN || (h1 >> 16) as usize != 1 + 6 * n || h2 & 0xFFFF != 0 {
        return Err("bad vertex block".into());
    }
    if n == 0 || n > MAX_VERTS || v.len() != 2 + 6 * n + 1 || v[v.len() - 1] != END {
        return Err("bad vertex count or end".into());
    }
    if v[2..2 + 6 * n]
        .iter()
        .any(|w| !f32::from_bits(*w).is_finite())
    {
        return Err("a vertex isn't finite".into());
    }
    Ok(n)
}

/// Checks a polygon list: the chunks this module writes, strips' slots below `count`, texture
/// ids 0, ending with 0xFF.
fn check_plist(p: &[u32], count: usize) -> Result<(), String> {
    let p16: Vec<u16> = p
        .iter()
        .flat_map(|w| [*w as u16, (*w >> 16) as u16])
        .collect();
    let mut i = 0;
    loop {
        let head = *p16.get(i).ok_or("polygon list without an end")?;
        match head & 0xFF {
            0xFF => return Ok(()),
            0x08..=0x0F => {
                let id = *p16.get(i + 1).ok_or("short texture chunk")? & 0x1FFF;
                if id != 0 {
                    return Err("texture id beyond the list".into());
                }
                i += 2;
            }
            0x10..=0x3F => i += 2 + usize::from(*p16.get(i + 1).ok_or("short material")?),
            0x41 => {
                let size = usize::from(*p16.get(i + 1).ok_or("short strips")?);
                let body = p16.get(i + 2..i + 2 + size).ok_or("strips cut short")?;
                let strips = usize::from(*body.first().ok_or("empty strips")? & 0x3FFF);
                let mut k = 1;
                for _ in 0..strips {
                    let len =
                        usize::from((*body.get(k).ok_or("strip cut short")? as i16).unsigned_abs());
                    let verts = body.get(k + 1..k + 1 + 3 * len).ok_or("strip cut short")?;
                    if verts.chunks(3).any(|c| usize::from(c[0]) >= count) {
                        return Err("a strip uses a vertex that isn't there".into());
                    }
                    k += 1 + 3 * len;
                }
                if size - k > 1 {
                    return Err("strip size doesn't match".into());
                }
                i += 2 + size;
            }
            _ => return Err(format!("unexpected chunk {head:#06x}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube (12 triangles, a normal per face) with every UV at (0.25, 0.75), plus the
    /// three markers.
    fn cube_obj() -> String {
        let mut s = String::new();
        let mut nv = 0;
        let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
            (
                [0., 0., 1.],
                [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
            ),
            (
                [0., 0., -1.],
                [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
            ),
            (
                [1., 0., 0.],
                [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
            ),
            (
                [-1., 0., 0.],
                [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
            ),
            (
                [0., 1., 0.],
                [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
            ),
            (
                [0., -1., 0.],
                [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            ),
        ];
        let mut add =
            |s: &mut String, name: &str, faces: &[([f32; 3], [[f32; 3]; 4])], off: [f32; 3]| {
                s.push_str(&format!("o {name}\n"));
                for (n, quad) in faces {
                    let mut idx = Vec::new();
                    for p in quad {
                        s.push_str(&format!(
                            "v {} {} {}\nvn {} {} {}\nvt 0.25 0.75\n",
                            p[0] + off[0],
                            p[1] + off[1],
                            p[2] + off[2],
                            n[0],
                            n[1],
                            n[2]
                        ));
                        nv += 1;
                        idx.push(nv);
                    }
                    s.push_str(&format!(
                        "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2} {3}/{3}/{3}\n",
                        idx[0], idx[1], idx[2], idx[3]
                    ));
                }
            };
        add(&mut s, "box", &faces, [0.0; 3]);
        add(&mut s, "screen", &faces[..1], [0.0, 0.0, 0.5]);
        add(&mut s, "@exit", &faces[..1], [0.0, 0.0, 3.0]);
        add(&mut s, "@collision", &faces, [0.0; 3]);
        s
    }

    /// A 4x4 TGA, bottom row first, with its bottom-left pixel red.
    fn tga() -> Vec<u8> {
        let mut t = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 4, 0, 32, 8];
        for i in 0..16 {
            t.extend(if i == 0 {
                [0, 0, 255, 255]
            } else {
                [255, 255, 255, 255]
            });
        }
        t
    }

    fn p16(words: &[u32]) -> Vec<u16> {
        words
            .iter()
            .flat_map(|w| [*w as u16, (*w >> 16) as u16])
            .collect()
    }

    #[test]
    fn a_cube_becomes_one_chunk_model() {
        let bytes = convert(&cube_obj(), &tga()).unwrap();
        let m = Machine::parse(&bytes).unwrap();
        assert_eq!(m.pieces.len(), 1);
        let p = &m.pieces[0];
        // The box: 24 vertices (4 per face, one normal each); the screen adds 4 more.
        let n = 28u32;
        assert_eq!(p.vlist[0], 0x29 | (1 + 6 * n) << 16);
        assert_eq!(p.vlist[1], n << 16);
        assert_eq!(p.vlist.len(), 2 + 6 * n as usize + 1);
        assert_eq!(*p.vlist.last().unwrap(), 0xFF);
        let s = p16(&p.plist);
        assert_eq!(s[..6], [0x2511, 2, 0xFFFF, 0xFFFF, 0x3008, 0]);
        // 14 triangles: 1 + 140 words, odd, so one padding word.
        assert_eq!(s[6..9], [0x1641, 142, 14]);
        // The first strip: 3 corners of slot, u, v (0.25, 0.75 from the bottom = 64, 64).
        assert_eq!(s[9..13], [3, 0, 64, 64]);
        assert_eq!(s[6 + 2 + 142], 0x00FF);
        assert!(s.len().is_multiple_of(2));
        assert_eq!(m.screen.centre, [0.5, 0.5, 1.5]);
        assert_eq!((m.screen.width, m.screen.height), (1.0, 1.0));
        assert_eq!(m.exit, [0.5, 0.5, 4.0]);
        assert_eq!((m.box_min, m.box_max), ([0.0; 3], [1.0; 3]));
        // Bottom-up TGA: the red pixel is on the last row.
        assert_eq!(m.texture_size, (4, 4));
        assert_eq!(m.texture[48..52], [255, 0, 0, 255]);
        assert_eq!(m.texture[..4], [255, 255, 255, 255]);
        assert_eq!(m.to_bytes(), bytes);
    }

    #[test]
    fn big_meshes_split_into_pieces() {
        let mut obj = String::new();
        let mut nv = 0;
        // 300 separate triangles: 900 distinct vertices.
        for t in 0..300 {
            obj.push_str("o box\n");
            for k in 0..3 {
                obj.push_str(&format!("v {t} {k} 0\nvn 0 0 1\nvt 0 0\n"));
            }
            obj.push_str(&format!(
                "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}\n",
                nv + 1,
                nv + 2,
                nv + 3
            ));
            nv += 3;
        }
        let markers = cube_obj();
        let markers = &markers[markers.find("o screen").unwrap()..];
        let renumbered: String = markers
            .lines()
            .map(|l| {
                if let Some(rest) = l.strip_prefix("f ") {
                    let c: Vec<String> = rest
                        .split(' ')
                        .map(|c| {
                            let i: usize = c.split('/').next().unwrap().parse().unwrap();
                            let i = i - 24 + nv;
                            format!("{i}/{i}/{i}")
                        })
                        .collect();
                    format!("f {}\n", c.join(" "))
                } else {
                    format!("{l}\n")
                }
            })
            .collect();
        obj.push_str(&renumbered);
        let m = Machine::parse(&convert(&obj, &tga()).unwrap()).unwrap();
        assert_eq!(m.pieces.len(), 2);
        let counts: Vec<u32> = m.pieces.iter().map(|p| p.vlist[1] >> 16).collect();
        assert_eq!(counts, [498, 406]);
    }

    #[test]
    fn broken_data_is_refused() {
        let good = convert(&cube_obj(), &tga()).unwrap();
        assert!(Machine::parse(&good[..good.len() - 4]).is_err());
        let mut more = good.clone();
        more.push(0);
        assert!(Machine::parse(&more).is_err());
        // A strip slot beyond the vertices: the first strip's first slot is after the header
        // (68 + 4x4x4 texture), the two word counts and the vertex list.
        let m = Machine::parse(&good).unwrap();
        let mut bad = m.clone();
        let mut s = p16(&bad.pieces[0].plist);
        s[10] = 28;
        bad.pieces[0].plist = s
            .chunks(2)
            .map(|c| u32::from(c[0]) | u32::from(c[1]) << 16)
            .collect();
        assert!(Machine::parse(&bad.to_bytes()).is_err());
        let mut bad = m.clone();
        bad.pieces[0].vlist[0] = 0x2A | bad.pieces[0].vlist[0] & !0xFF;
        assert!(Machine::parse(&bad.to_bytes()).is_err());
        assert!(convert("o box\nv 0 0 0\n", &tga()).is_err());
        assert!(read_tga(&tga()[..20]).is_err());
    }
}
