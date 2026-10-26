//! Bounded GF/PICA asset conversion for the native 64-bit helper. Format reference:
//! gdkchan/SPICA (Unlicense), revision 82acbd63695e4ef8b95a92ce884f25ec1044788f;
//! see tools/source-model-notices.txt. No emulator, Python or Blender is needed by players.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use super::archive::{Garc, decompress_lz11};
use super::assets::{
    Actor, Bone, BoneTrack, Clip, Key, Material, MaterialTrack, Mesh, Texture, Vertex,
    VisibilityTrack,
};
use super::source::{GameSource, ImportError, Result};

const LIMIT: usize = 64 << 20;
const MODEL: u32 = 0x15122117;
const TEX: u32 = 0x15041213;
const MOTION: u32 = 0x00060000;

fn bad(message: &str) -> ImportError {
    ImportError::new(format!("3D assets: {message}"))
}

/// Writes `bytes` as a new file at `path`, never replacing an existing one. The data is
/// complete (and with `durable`, on disk) before the name appears: a hidden part file next to
/// it is hard-linked into place. File systems without hard links (exFAT, FAT32, some network
/// shares) instead reserve the name with an empty new file and rename the part file over it.
pub fn write_new_file(path: &Path, bytes: &[u8], durable: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| ImportError::new("output filename is missing"))?;
    let part = parent.join(format!(
        ".{}-{}.part",
        name.to_string_lossy(),
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        if durable {
            file.sync_all()?;
        }
        drop(file);
        match std::fs::hard_link(&part, path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e.into()),
            Err(_) => rename_into_reserved(&part, path),
        }
    })();
    let _ = std::fs::remove_file(&part);
    result
}

/// The fallback publication: `path` must not exist yet; the empty reservation is ours alone,
/// so the rename replaces nothing else.
fn rename_into_reserved(part: &Path, path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(e) = std::fs::rename(part, path) {
        let _ = std::fs::remove_file(path);
        return Err(e.into());
    }
    Ok(())
}

struct R<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> R<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() > LIMIT {
            return Err(bad("input exceeds 64 MiB"));
        }
        Ok(Self { data, at: 0 })
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| bad("offset overflow"))?;
        let b = self
            .data
            .get(self.at..end)
            .ok_or_else(|| bad("truncated asset"))?;
        self.at = end;
        Ok(b)
    }
    fn seek(&mut self, at: usize) -> Result<()> {
        if at > self.data.len() {
            return Err(bad("offset outside asset"));
        }
        self.at = at;
        Ok(())
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    fn align(&mut self, n: usize) -> Result<()> {
        self.seek((self.at + n - 1) & !(n - 1))
    }
    fn b(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f(&mut self) -> Result<f32> {
        let v = f32::from_bits(self.u32()?);
        if !v.is_finite() || v.abs() > 1e6 {
            return Err(bad("invalid float"));
        }
        Ok(v)
    }
    fn fs<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut a = [0.; N];
        for v in &mut a {
            *v = self.f()?;
        }
        Ok(a)
    }
    fn text(&mut self, n: usize) -> Result<String> {
        if n > 4096 {
            return Err(bad("name too long"));
        }
        let b = self.take(n)?;
        String::from_utf8(b.split(|&b| b == 0).next().unwrap_or_default().to_vec())
            .map_err(|_| bad("invalid UTF-8 asset name"))
    }
    fn short(&mut self) -> Result<String> {
        let n = usize::from(self.b()?);
        self.text(n)
    }
    fn hashname(&mut self) -> Result<String> {
        self.skip(4)?;
        self.short()
    }
    fn section(&mut self, name: &str) -> Result<usize> {
        let got = self.text(8)?;
        let n = self.u32()? as usize;
        self.skip(4)?;
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| bad("section overflow"))?;
        if got != name || end > self.data.len() {
            return Err(bad(&format!(
                "invalid GF section at {:#x}: expected {name}, got {got:?}, end {end:#x}, length {:#x}",
                self.at - 16,
                self.data.len()
            )));
        }
        Ok(end)
    }
    fn count(&mut self, max: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(bad("array count exceeds its limit"));
        }
        Ok(n)
    }
}

fn magic(data: &[u8]) -> u32 {
    data.get(..4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}
fn is_texture(data: &[u8]) -> bool {
    magic(data) == TEX && data.get(8..15) == Some(b"texture")
}
pub fn unpack(data: Vec<u8>) -> Result<Vec<u8>> {
    if data.first() == Some(&0x11) {
        decompress_lz11(&data, LIMIT)
    } else {
        Ok(data)
    }
}

pub fn package(data: &[u8]) -> Result<Vec<&[u8]>> {
    if data
        .get(..2)
        .is_none_or(|s| !s.iter().all(u8::is_ascii_uppercase))
    {
        return Ok(vec![data]);
    }
    let mut r = R::new(data)?;
    r.skip(2)?;
    let n = usize::from(r.u16()?);
    if n == 0 || n > 4096 {
        return Err(bad("package entry count"));
    }
    let mut offsets = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        offsets.push(r.u32()? as usize);
    }
    if offsets[0] < r.at || offsets[n] > data.len() || offsets.windows(2).any(|w| w[0] > w[1]) {
        return Err(bad("package offset table"));
    }
    Ok(offsets.windows(2).map(|w| &data[w[0]..w[1]]).collect())
}

/// ModelPack tables used inside BG battle venues contain named pointers to GF models and
/// textures. Keep every nested region bounded, without copying shader/program payloads.
pub fn asset_parts(data: &[u8]) -> Result<Vec<&[u8]>> {
    fn visit<'a>(data: &'a [u8], depth: usize, out: &mut Vec<&'a [u8]>) -> Result<()> {
        if depth > 4 || out.len() > 4096 {
            return Err(bad("nested asset package exceeds its bound"));
        }
        if matches!(magic(data), MODEL | MOTION) || is_texture(data) {
            out.push(data);
            return Ok(());
        }
        if magic(data) == 0x10000 {
            let mut r = R::new(data)?;
            r.skip(4)?;
            let mut counts = [0; 5];
            for n in &mut counts {
                *n = r.count(4096)?;
            }
            let total = counts.iter().sum::<usize>();
            if total > 4096 {
                return Err(bad("model-pack pointer count"));
            }
            let mut addresses = Vec::with_capacity(total);
            for _ in 0..total {
                addresses.push(r.count(LIMIT)?);
            }
            let mut regions = Vec::new();
            for pointer in addresses {
                r.seek(pointer)?;
                r.short()?;
                let at = r.count(LIMIT)?;
                if at >= data.len() {
                    return Err(bad("model-pack payload outside member"));
                }
                regions.push(at);
            }
            let mut sorted = regions.clone();
            sorted.sort_unstable();
            sorted.dedup();
            for at in regions {
                let end = sorted
                    .iter()
                    .copied()
                    .find(|&x| x > at)
                    .unwrap_or(data.len());
                if magic(&data[at..end]) == MODEL || is_texture(&data[at..end]) {
                    visit(&data[at..end], depth + 1, out)?;
                }
            }
            return Ok(());
        }
        if data
            .get(..2)
            .is_some_and(|s| s.iter().all(u8::is_ascii_uppercase))
        {
            for part in package(data)? {
                if matches!(magic(part), MODEL | TEX | MOTION | 0x10000) {
                    visit(part, depth + 1, out)?;
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(data, 0, &mut out)?;
    Ok(out)
}

fn float24(v: u32) -> f32 {
    if v & 0x7fffff == 0 {
        return f32::from_bits((v & 0x800000) << 8);
    }
    f32::from_bits(((v & 65535) << 7) | ((((v >> 16) & 127) + 64) << 23) | ((v >> 23) << 31))
}

#[derive(Default)]
struct Gpu {
    regs: HashMap<u16, u32>,
    fixed: HashMap<usize, [f32; 4]>,
}
impl Gpu {
    fn reg(&self, id: u16) -> u32 {
        *self.regs.get(&id).unwrap_or(&0)
    }
    fn read(data: &[u8]) -> Result<Self> {
        if !data.len().is_multiple_of(8) || data.len() > 4 << 20 {
            return Err(bad("GPU command length"));
        }
        let mut r = R::new(data)?;
        let mut gpu = Self::default();
        let mut fixed = 0;
        let mut words = [0; 3];
        while r.at < data.len() {
            let value = r.u32()?;
            let header = r.u32()?;
            let reg = (header & 65535) as u16;
            let mask = (header >> 16) & 15;
            let extra = ((header >> 20) & 2047) as usize;
            let mut params = vec![value];
            for _ in 0..extra {
                params.push(r.u32()?);
            }
            r.align(8)?;
            for (i, value) in params.into_iter().enumerate() {
                let id = if header >> 31 != 0 {
                    reg.checked_add(i as u16)
                        .ok_or_else(|| bad("GPU register overflow"))?
                } else {
                    reg
                };
                let mut bits = 0;
                for b in 0..4 {
                    if mask & (1 << b) != 0 {
                        bits |= 255 << (b * 8);
                    }
                }
                let old = gpu.reg(id);
                gpu.regs.insert(id, (old & !bits) | (value & bits));
                match id {
                    0x232 => {
                        fixed = value as usize;
                        if fixed >= 12 {
                            return Err(bad("fixed GPU attribute index"));
                        }
                    }
                    0x233..=0x235 => {
                        words[usize::from(id - 0x233)] = value;
                        if id == 0x235 {
                            let [a, b, c] = words;
                            gpu.fixed.insert(
                                fixed,
                                [
                                    float24(c & 0xffffff),
                                    float24((c >> 24) | ((b & 65535) << 8)),
                                    float24((b >> 16) | ((a & 255) << 16)),
                                    float24(a >> 8),
                                ],
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(gpu)
    }
}

fn vertices(
    data: &[u8],
    count: usize,
    gpu: &Gpu,
    palette: &[u16],
    bones: usize,
) -> Result<Vec<Vertex>> {
    let formats = u64::from(gpu.reg(0x201)) | u64::from(gpu.reg(0x202)) << 32;
    let layout = u64::from(gpu.reg(0x204)) | u64::from(gpu.reg(0x205) & 65535) << 32;
    let perm = u64::from(gpu.reg(0x2bb)) | u64::from(gpu.reg(0x2bc)) << 32;
    let stride = ((gpu.reg(0x205) >> 16) & 255) as usize;
    let total = (gpu.reg(0x242) + 1) as usize;
    if count == 0
        || count > super::assets::MAX_VERTICES
        || stride == 0
        || count.checked_mul(stride).is_none_or(|n| n > data.len())
        || total > 12
    {
        return Err(bad("vertex layout or count"));
    }
    let mut out = Vec::with_capacity(count);
    let scales = [1. / 127., 1. / 255., 1. / 32767., 1.];
    for index in 0..count {
        let mut r = R::new(data)?;
        r.seek(index * stride)?;
        let mut values = HashMap::<u8, ([f32; 4], usize)>::new();
        for i in 0..total {
            if (formats >> (48 + i)) & 1 != 0 {
                let name = ((perm >> (i * 4)) & 15) as u8;
                let mut v = *gpu.fixed.get(&i).unwrap_or(&[0.; 4]);
                if matches!(name, 3 | 8) {
                    for x in &mut v {
                        *x /= 255.;
                    }
                }
                values.insert(name, (v, 4));
            } else {
                let slot = ((layout >> (i * 4)) & 15) as usize;
                let name = ((perm >> (slot * 4)) & 15) as u8;
                let fmt = ((formats >> (slot * 4)) & 15) as usize;
                let count = (fmt >> 2) + 1;
                let format = fmt & 3;
                let mut v = [0.; 4];
                if format >= 2 {
                    r.align(2)?;
                }
                for x in v.iter_mut().take(count) {
                    *x = match format {
                        0 => f32::from(r.b()? as i8),
                        1 => f32::from(r.b()?),
                        2 => f32::from(r.u16()? as i16),
                        _ => r.f()?,
                    };
                    if name != 7 {
                        *x *= scales[format];
                    }
                }
                values.insert(name, (v, count));
            }
        }
        if r.at > (index + 1) * stride {
            return Err(bad("vertex attributes exceed stride"));
        }
        let p = values
            .get(&0)
            .ok_or_else(|| bad("missing vertex position"))?
            .0;
        let n = values.get(&1).map(|v| v.0).unwrap_or([0., 1., 0., 0.]);
        let uv = values.get(&4).map(|v| v.0).unwrap_or([0.; 4]);
        let (js, nj) = values.get(&7).copied().unwrap_or(([0.; 4], 1));
        let (ws, nw) = values.get(&8).copied().unwrap_or(([1., 0., 0., 0.], 1));
        let mut joints = [0; 4];
        let mut weights = [0.; 4];
        for k in 0..if bones == 0 { 0 } else { nj.min(nw).min(4) } {
            if ws[k] == 0. {
                // Unused palette slots carry -1/255 sentinels in real source meshes.
                continue;
            }
            if !ws[k].is_finite()
                || ws[k] < 0.
                || !js[k].is_finite()
                || js[k] < 0.
                || js[k].fract() != 0.
            {
                return Err(bad("invalid vertex joints/weights"));
            }
            if ws[k] > 0. {
                joints[k] = *palette
                    .get(js[k] as usize)
                    .ok_or_else(|| bad("bone palette index"))?;
                if usize::from(joints[k]) >= bones.max(1) {
                    return Err(bad("bone outside skeleton"));
                }
                weights[k] = ws[k];
            }
        }
        let sum = weights.iter().sum::<f32>();
        if sum > 0. {
            for w in &mut weights {
                *w /= sum;
            }
        } else {
            joints[0] = palette.first().copied().unwrap_or(0);
            weights[0] = 1.;
        }
        out.push(Vertex {
            position: [p[0], p[1], p[2]],
            normal: [n[0], n[1], n[2]],
            uv: [uv[0], uv[1]],
            joints,
            weights,
        });
    }
    Ok(out)
}

struct RawMaterial {
    name: String,
    texture: String,
    uv: [f32; 5],
    wrap: [u8; 2],
}
fn material(r: &mut R<'_>) -> Result<RawMaterial> {
    let end = r.section("material")?;
    let name = r.hashname()?;
    for _ in 0..3 {
        r.hashname()?;
    }
    r.skip(168)?;
    let n = r.count(3)?;
    let mut result = RawMaterial {
        name,
        texture: String::new(),
        uv: [1., 1., 0., 0., 0.],
        wrap: [2, 2],
    };
    for _ in 0..n {
        let texture = r.hashname()?;
        let unit = r.b()?;
        r.b()?;
        let uv = r.fs()?;
        let wrap = [r.u32()? as u8, r.u32()? as u8];
        r.skip(12)?;
        if unit == 0 {
            result.texture = texture;
            result.uv = uv;
            result.wrap = wrap;
        }
    }
    if r.at > end {
        return Err(bad("material crossed section"));
    }
    r.seek(end)?;
    Ok(result)
}

fn mesh(
    r: &mut R<'_>,
    bones: usize,
    materials: &[RawMaterial],
    remaining: &mut usize,
) -> Result<Vec<Mesh>> {
    let end = r.section("mesh")?;
    r.skip(4)?;
    let name = r.text(64)?;
    r.skip(36)?;
    let count = r.count(256)?;
    let influences = r.u32()?;
    if influences > 4 {
        return Err(bad("more than four bone influences"));
    }
    r.skip(16)?;
    let mut commands = Vec::with_capacity(count * 3);
    for i in 0..count * 3 {
        let n = r.count(4 << 20)?;
        if r.u32()? as usize != i || r.u32()? as usize != count * 3 {
            return Err(bad("GPU command sequence"));
        }
        r.skip(4)?;
        commands.push(Gpu::read(r.take(n)?)?);
    }
    let mut sizes = Vec::with_capacity(count);
    for _ in 0..count {
        r.skip(4)?;
        let n = r.count(4096)?;
        let mat = r.text(n)?;
        let n = usize::from(r.b()?);
        let all = r.take(31)?;
        if n > 31 {
            return Err(bad("bone palette length"));
        }
        let pal = all[..n].iter().map(|&i| u16::from(i)).collect::<Vec<_>>();
        let nv = r.count(*remaining)?;
        *remaining -= nv;
        r.u32()?;
        let vlen = r.count(LIMIT)?;
        let ilen = r.count(LIMIT)?;
        sizes.push((mat, pal, nv, vlen, ilen));
    }
    let mut meshes = Vec::new();
    for (i, (mat, pal, nv, vlen, ilen)) in sizes.into_iter().enumerate() {
        let vb = r.take(vlen)?;
        let ib = r.take(ilen)?;
        let g = &commands[i * 3 + 2];
        let ni = g.reg(0x228) as usize;
        let wide = g.reg(0x227) >> 31 != 0;
        let mut ir = R::new(ib)?;
        if ni > super::assets::MAX_VERTICES * 12 {
            return Err(bad("index count"));
        }
        let mut idx = Vec::with_capacity(ni);
        for _ in 0..ni {
            let v = if wide {
                u32::from(ir.u16()?)
            } else {
                u32::from(ir.b()?)
            };
            if v as usize >= nv {
                return Err(bad("index outside vertex buffer"));
            }
            idx.push(v);
        }
        let mut vs = vertices(vb, nv, &commands[i * 3], &pal, bones)?;
        let mode = g.reg(0x25e) >> 8;
        if matches!(mode, 1 | 2) {
            let mut tris = Vec::new();
            for j in 2..idx.len() {
                let mut t = if mode == 1 {
                    [idx[j - 2], idx[j - 1], idx[j]]
                } else {
                    [idx[0], idx[j - 1], idx[j]]
                };
                if mode == 1 && j % 2 == 1 {
                    t.swap(0, 1);
                }
                if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                    tris.extend(t);
                }
            }
            idx = tris;
        } else if mode == 3 || (mode == 0 && mat.contains("Geom") && idx.len() == nv) {
            let mut points = Vec::new();
            let mut tris = Vec::new();
            for &j in &idx {
                let v = &vs[j as usize];
                for axis in [0, 2] {
                    let base = points.len() as u32;
                    for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
                        let mut q = v.clone();
                        q.position[axis] += x;
                        q.position[1] += y;
                        q.uv = [x + 0.5, y + 0.5];
                        points.push(q);
                    }
                    tris.extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
                }
            }
            if points.len() > super::assets::MAX_VERTICES {
                return Err(bad("point geometry fallback exceeds budget"));
            }
            vs = points;
            idx = tris;
        } else if mode != 0 {
            return Err(bad("unknown primitive mode"));
        }
        if idx.is_empty() {
            continue;
        }
        if idx.len() % 3 != 0 {
            return Err(bad(&format!(
                "incomplete triangle list ({name}, {mat}, mode {mode}, {} indices, {} vertices)",
                idx.len(),
                vs.len()
            )));
        }
        let material = materials
            .iter()
            .position(|m| m.name == mat)
            .ok_or_else(|| bad("mesh has no material"))?;
        meshes.push(Mesh {
            name: name.clone(),
            material: material as u16,
            vertices: vs,
            indices: idx,
        });
    }
    if r.at > end {
        return Err(bad("mesh crossed section"));
    }
    r.seek(end)?;
    Ok(meshes)
}

type SourceMaterial = (String, String, [f32; 5], [u8; 2]);
type ModelParts = (Vec<Bone>, Vec<Mesh>, Vec<SourceMaterial>);
pub fn model(data: &[u8]) -> Result<ModelParts> {
    let mut r = R::new(data)?;
    if r.u32()? != MODEL {
        return Err(bad("not a GF model"));
    }
    r.skip(4)?;
    r.align(16)?;
    let end = r.section("gfmodel")?;
    let mut counts = [0; 4];
    for count in &mut counts {
        *count = r.count(4096)?;
        r.skip(*count * 68)?;
    }
    r.skip(96)?;
    let length = r.count(LIMIT)?;
    let offset = r.count(LIMIT)?;
    r.skip(8)?;
    r.skip(
        length
            .checked_add(offset)
            .ok_or_else(|| bad("model offset overflow"))?,
    )?;
    let n = r.count(512)?;
    r.skip(12)?;
    let mut names = Vec::with_capacity(n);
    let mut raw = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.short()?;
        let parent = r.short()?;
        r.b()?;
        let (scale, rotation, translation) = (r.fs()?, r.fs()?, r.fs()?);
        names.push(name);
        raw.push((
            parent,
            Bone {
                parent: -1,
                scale,
                rotation,
                translation,
            },
        ));
    }
    let mut bones = Vec::with_capacity(n);
    for (parent, mut bone) in raw {
        bone.parent = if parent.is_empty() {
            -1
        } else {
            names
                .iter()
                .position(|n| n == &parent)
                .ok_or_else(|| bad("missing parent bone"))? as i16
        };
        bones.push(bone);
    }
    r.seek(end)?;
    let mut materials = Vec::with_capacity(counts[2]);
    for _ in 0..counts[2] {
        materials.push(material(&mut r)?);
    }
    let mut meshes = Vec::new();
    let mut remaining = super::assets::MAX_VERTICES;
    for _ in 0..counts[3] {
        meshes.extend(mesh(&mut r, bones.len(), &materials, &mut remaining)?);
    }
    Ok((
        bones,
        meshes,
        materials
            .into_iter()
            .map(|m| (m.name, m.texture, m.uv, m.wrap))
            .collect(),
    ))
}

const MOD: [[i32; 4]; 8] = [
    [2, 8, -2, -8],
    [5, 17, -5, -17],
    [9, 29, -9, -29],
    [13, 42, -13, -42],
    [18, 60, -18, -60],
    [24, 80, -24, -80],
    [33, 106, -33, -106],
    [47, 183, -47, -183],
];
fn etc(raw: &[u8]) -> Result<[[[u8; 3]; 4]; 4]> {
    let block = u64::from_be_bytes(raw.try_into().map_err(|_| bad("ETC block size"))?);
    let (low, high) = ((block >> 32) as u32, block as u32);
    let mut colors = [[0i32; 3]; 2];
    for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
        if high & 0x2000000 != 0 {
            let v = ((high >> (shift + 3)) & 31) as i32;
            let d = ((high >> shift) & 7) as i32;
            let other = v + if d & 4 != 0 { d - 8 } else { d };
            if !(0..=31).contains(&other) {
                return Err(bad("invalid ETC differential color"));
            }
            colors[0][channel] = (v << 3) | (v >> 2);
            colors[1][channel] = (other << 3) | (other >> 2);
        } else {
            colors[0][channel] = (((high >> (shift + 4)) & 15) * 17) as i32;
            colors[1][channel] = (((high >> shift) & 15) * 17) as i32;
        }
    }
    let tables = [((high >> 29) & 7) as usize, ((high >> 26) & 7) as usize];
    let mut out = [[[0; 3]; 4]; 4];
    for (y, row) in out.iter_mut().enumerate() {
        for (x, p) in row.iter_mut().enumerate() {
            let i = x * 4 + y;
            let select = if i < 8 {
                ((low >> (i + 24)) & 1) + (((low << 1) >> (i + 8)) & 2)
            } else {
                ((low >> (i + 8)) & 1) + (((low << 1) >> (i - 8)) & 2)
            };
            let half = usize::from(if high & 0x1000000 != 0 {
                y >= 2
            } else {
                x >= 2
            });
            for c in 0..3 {
                p[c] = (colors[half][c] + MOD[tables[half]][select as usize]).clamp(0, 255) as u8;
            }
        }
    }
    Ok(out)
}

fn decode_pixels(data: &[u8], w: usize, h: usize, format: u16) -> Result<Vec<u8>> {
    if !(8..=1024).contains(&w)
        || !(8..=1024).contains(&h)
        || !w.is_power_of_two()
        || !h.is_power_of_two()
    {
        return Err(bad("texture dimensions"));
    }
    let mut out = vec![0; w * h * 4];
    let mut r = R::new(data)?;
    if matches!(format, 0x2a | 0x2b) {
        for ty in (0..h).step_by(8) {
            for tx in (0..w).step_by(8) {
                for (ox, oy) in [(0, 0), (4, 0), (0, 4), (4, 4)] {
                    let alpha = if format == 0x2b {
                        u64::from_le_bytes(r.take(8)?.try_into().unwrap())
                    } else {
                        u64::MAX
                    };
                    let colors = etc(r.take(8)?)?;
                    for (y, row) in colors.iter().enumerate() {
                        for (x, rgb) in row.iter().enumerate() {
                            let p = ((ty + oy + y) * w + tx + ox + x) * 4;
                            out[p..p + 3].copy_from_slice(rgb);
                            out[p + 3] = (((alpha >> ((x * 4 + y) * 4)) & 15) * 17) as u8;
                        }
                    }
                }
            }
        }
        return Ok(out);
    }
    let bpp = match format {
        4 => 32,
        3 => 24,
        2 | 0x16 | 0x17 | 0x23 | 0x24 => 16,
        0x25..=0x27 => 8,
        0x28 | 0x29 => 4,
        _ => return Err(bad("texture format")),
    };
    if data.len() < w * h * bpp / 8 {
        return Err(bad("truncated texture"));
    }
    let mut pixel = 0;
    for ty in (0..h).step_by(8) {
        for tx in (0..w).step_by(8) {
            for i in 0..64 {
                let x = (i & 1) | ((i >> 1) & 2) | ((i >> 2) & 4);
                let y = ((i >> 1) & 1) | ((i >> 2) & 2) | ((i >> 3) & 4);
                let at = pixel * bpp / 8;
                let v = match bpp {
                    32 => u32::from_le_bytes(data[at..at + 4].try_into().unwrap()),
                    24 => {
                        u32::from(data[at])
                            | u32::from(data[at + 1]) << 8
                            | u32::from(data[at + 2]) << 16
                    }
                    16 => u32::from(u16::from_le_bytes(data[at..at + 2].try_into().unwrap())),
                    _ => u32::from(data[at]),
                };
                let rgba = match format {
                    4 => [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8],
                    3 => [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255],
                    2 => [
                        (((v >> 11) & 31) * 255 / 31) as u8,
                        (((v >> 5) & 63) * 255 / 63) as u8,
                        ((v & 31) * 255 / 31) as u8,
                        255,
                    ],
                    0x16 => [
                        ((v >> 12) * 17) as u8,
                        (((v >> 8) & 15) * 17) as u8,
                        (((v >> 4) & 15) * 17) as u8,
                        ((v & 15) * 17) as u8,
                    ],
                    0x17 => [
                        (((v >> 11) & 31) * 255 / 31) as u8,
                        (((v >> 6) & 31) * 255 / 31) as u8,
                        (((v >> 1) & 31) * 255 / 31) as u8,
                        ((v & 1) * 255) as u8,
                    ],
                    0x23 => [(v >> 8) as u8, (v >> 8) as u8, (v >> 8) as u8, v as u8],
                    0x24 => [(v >> 8) as u8, v as u8, 0, 255],
                    0x25 => [v as u8, v as u8, v as u8, 255],
                    0x26 => [255, 255, 255, v as u8],
                    0x27 => [
                        ((v >> 4) * 17) as u8,
                        ((v >> 4) * 17) as u8,
                        ((v >> 4) * 17) as u8,
                        ((v & 15) * 17) as u8,
                    ],
                    _ => {
                        let n = (((v >> ((pixel & 1) * 4)) & 15) * 17) as u8;
                        if format == 0x28 {
                            [n, n, n, 255]
                        } else {
                            [255, 255, 255, n]
                        }
                    }
                };
                let p = ((ty + y) * w + tx + x) * 4;
                out[p..p + 4].copy_from_slice(&rgba);
                pixel += 1;
            }
        }
    }
    Ok(out)
}

fn texture(data: &[u8]) -> Result<(String, Texture)> {
    let mut r = R::new(data)?;
    if r.u32()? != TEX {
        return Err(bad("not a GF texture"));
    }
    r.skip(4)?;
    r.section("texture")?;
    let n = r.count(LIMIT)?;
    r.skip(12)?;
    let name = r.text(64)?;
    let (w, h, format) = (r.u16()?, r.u16()?, r.u16()?);
    r.skip(18)?;
    let pixels = decode_pixels(r.take(n)?, usize::from(w), usize::from(h), format)?;
    Ok((
        name,
        Texture {
            width: w,
            height: h,
            pixels,
        },
    ))
}

fn keys(r: &mut R<'_>, kind: u32, frames: u16) -> Result<Vec<Key>> {
    match kind & 7 {
        0..=2 => Ok(Vec::new()),
        3 => Ok(vec![Key {
            frame: 0,
            value: r.f()?,
            slope: 0.,
        }]),
        4 | 5 => {
            let n = r.count(20_001)?;
            let mut times = Vec::with_capacity(n);
            for _ in 0..n {
                times.push(if frames > 255 {
                    r.u16()?
                } else {
                    u16::from(r.b()?)
                });
            }
            r.align(4)?;
            let mut out = Vec::with_capacity(n);
            if kind & 1 != 0 {
                for frame in times {
                    out.push(Key {
                        frame,
                        value: r.f()?,
                        slope: r.f()?,
                    });
                }
            } else {
                let [vs, vo, ss, so] = r.fs()?;
                for frame in times {
                    out.push(Key {
                        frame,
                        value: f32::from(r.u16()?) / 65535. * vs + vo,
                        slope: f32::from(r.u16()?) / 65535. * ss + so,
                    });
                }
            }
            Ok(out)
        }
        _ => Err(bad("motion key encoding")),
    }
}

fn motion(data: &[u8], names: &[String]) -> Result<Option<Clip>> {
    let mut r = R::new(data)?;
    if r.u32()? != MOTION {
        return Ok(None);
    }
    let n = r.count(32)?;
    if n == 0 {
        return Ok(None);
    }
    let mut sections = Vec::new();
    for _ in 0..n {
        let (kind, size, at) = (r.u32()?, r.count(LIMIT)?, r.count(LIMIT)?);
        if at.checked_add(size).is_none_or(|end| end > data.len()) {
            return Err(bad("motion section outside member"));
        }
        sections.push((kind, size, at));
    }
    r.seek(sections[0].2)?;
    let frames = r.u32()?;
    if !(1..=20000).contains(&frames) {
        return Err(bad("motion frame count"));
    }
    let frames = frames as u16;
    let looping = r.u16()? & 1 != 0;
    let mut tracks = Vec::new();
    let mut visibility = Vec::new();
    let mut materials = Vec::new();
    for (kind, _, at) in sections {
        if kind == 6 {
            r.seek(at)?;
            let n = r.count(512)?;
            let length = r.count(LIMIT)?;
            let start = r.at;
            let mut names = Vec::with_capacity(n);
            for _ in 0..n {
                names.push(r.short()?);
            }
            r.seek(start + length)?;
            for mesh in names {
                visibility.push(VisibilityTrack {
                    mesh,
                    bits: r.take((usize::from(frames) + 1).div_ceil(8))?.to_vec(),
                });
            }
            continue;
        }
        if kind == 3 {
            r.seek(at)?;
            let n = r.count(64)?;
            let length = r.count(LIMIT)?;
            let mut units = Vec::with_capacity(n);
            for _ in 0..n {
                units.push(r.count(3)?);
            }
            let start = r.at;
            let mut names = Vec::with_capacity(n);
            for _ in 0..n {
                names.push(r.short()?);
            }
            r.seek(start + length)?;
            for (material, units) in names.into_iter().zip(units) {
                for _ in 0..units {
                    let unit = r.u32()?;
                    let flags = r.u32()?;
                    let length = r.count(LIMIT)?;
                    let start = r.at;
                    let mut channels = std::array::from_fn(|_| Vec::new());
                    for (i, ch) in channels.iter_mut().enumerate() {
                        *ch = keys(&mut r, flags >> (i * 3), frames)?;
                    }
                    if length != 0 && r.at != start + length {
                        return Err(bad("UV animation block length"));
                    }
                    if unit == 0 {
                        materials.push(MaterialTrack {
                            material: material.clone(),
                            channels,
                        });
                    }
                }
            }
            continue;
        }
        if kind != 1 {
            continue;
        }
        r.seek(at)?;
        let n = r.count(512)?;
        let length = r.count(LIMIT)?;
        let start = r.at;
        let mut bn = Vec::with_capacity(n);
        for _ in 0..n {
            bn.push(r.short()?);
        }
        r.seek(start + length)?;
        for name in bn {
            let flags = r.u32()?;
            let size = r.count(LIMIT)?;
            let start = r.at;
            let mut channels = std::array::from_fn(|_| Vec::new());
            for (i, ch) in channels.iter_mut().enumerate() {
                *ch = keys(&mut r, flags >> (i * 3), frames)?;
            }
            if size != 0 && r.at != start + size {
                return Err(bad("motion bone length"));
            }
            if let Some(bone) = names.iter().position(|n| n == &name) {
                tracks.push(BoneTrack {
                    bone: bone as u16,
                    axis_angle: flags >> 31 == 0,
                    channels,
                });
            }
        }
    }
    Ok(
        (!tracks.is_empty() || !visibility.is_empty() || !materials.is_empty()).then_some(Clip {
            frames,
            looping,
            tracks,
            visibility,
            materials,
        }),
    )
}

fn model_names(data: &[u8]) -> Result<Vec<String>> {
    let mut r = R::new(data)?;
    r.skip(8)?;
    r.align(16)?;
    r.section("gfmodel")?;
    for _ in 0..4 {
        let n = r.count(4096)?;
        r.skip(n * 68)?;
    }
    r.skip(96)?;
    let n = r.count(LIMIT)?;
    let off = r.count(LIMIT)?;
    r.skip(8 + n + off)?;
    let n = r.count(512)?;
    r.skip(12)?;
    let mut names = Vec::with_capacity(n);
    for _ in 0..n {
        names.push(r.short()?);
        r.short()?;
        r.skip(37)?;
    }
    Ok(names)
}

pub fn convert(
    model_pack: &[u8],
    texture_pack: &[u8],
    motion_pack: Option<&[u8]>,
    species: u16,
    shiny: bool,
) -> Result<Actor> {
    let model_bytes = asset_parts(model_pack)?
        .into_iter()
        .find(|p| magic(p) == MODEL)
        .ok_or_else(|| bad("model package contains no GF model"))?;
    let names = model_names(model_bytes)?;
    let (bones, meshes, raw_materials) = model(model_bytes)?;
    let mut texture_names = Vec::new();
    let mut textures = Vec::new();
    for part in asset_parts(texture_pack)? {
        if magic(part) == TEX {
            let (name, tex) = texture(part)?;
            texture_names.push(name);
            textures.push(tex);
        }
    }
    let mut notices=vec!["PICA lighting is approximated by the native renderer; source-authored skeletal idle is retained".into()];
    if meshes.iter().any(|m| m.name.contains("Geom")) {
        notices.push("Source geometry-shader point effects use bounded crossed 3D quads".into());
    }
    let mut materials = Vec::new();
    for (name, texture, uv, wrap) in raw_materials {
        let texture = if let Some(i) = texture_names.iter().position(|n| n == &texture) {
            i
        } else {
            notices.push(format!(
                "material {name}: missing texture uses neutral 3D material"
            ));
            let i = textures.len();
            textures.push(Texture {
                width: 8,
                height: 8,
                pixels: [160, 160, 160, 255].repeat(64),
            });
            texture_names.push(String::new());
            i
        };
        materials.push(Material {
            name,
            texture: texture as u16,
            uv,
            wrap,
        });
    }
    let idle = if let Some(pack) = motion_pack {
        let mut clip = None;
        for part in package(pack)? {
            if magic(part) == MOTION {
                match motion(part, &names) {
                    Ok(Some(c)) => {
                        clip = Some(c);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        notices.push(format!("unsupported idle: {e}; static 3D fallback"));
                        break;
                    }
                }
            }
        }
        clip
    } else {
        None
    };
    if idle.is_none() {
        notices.push(
            "no supported source idle: static 3D model with procedural battle movement".into(),
        );
    }
    notices.truncate(64);
    let actor = Actor {
        species,
        form: 0,
        shiny,
        bones,
        materials,
        textures,
        meshes,
        idle,
        notices,
    };
    actor.validate().map_err(ImportError::new)?;
    Ok(actor)
}

pub struct AssetReport {
    pub models: usize,
    pub variants: usize,
    pub animated: usize,
    pub bytes: u64,
    pub failed: Vec<(u16, String)>,
}

fn write_icon(output: &Path, actor: &Actor) -> Result<()> {
    let path = super::assets::thumbnail_path(output, actor.species, actor.form, actor.shiny);
    if super::assets::load_thumbnail(output, actor.species, actor.form, actor.shiny).is_some() {
        return Ok(());
    }
    let rgba = actor.thumbnail(64, 64);
    let mut bytes = super::assets::ICON_MAGIC.to_vec();
    bytes.extend(64u16.to_le_bytes());
    bytes.extend(64u16.to_le_bytes());
    bytes.extend(rgba);
    write_new_file(&path, &bytes, false)
}

/// Resolve canonical source forms without confusing the extra female base model with form 1.
pub fn model_slot(
    lookup: &[u8],
    species: u16,
    form: u8,
    form_count: u8,
    female: bool,
) -> Result<usize> {
    if species == 0 || species > super::data::MAX_SPECIES || form >= form_count {
        return Err(bad("invalid canonical model form"));
    }
    let at = usize::from(species - 1) * 4;
    let row = lookup
        .get(at..at + 4)
        .ok_or_else(|| bad("model lookup is truncated"))?;
    let base = usize::from(u16::from_le_bytes(row[..2].try_into().unwrap()));
    let count = usize::from(row[2]);
    let flags = row[3];
    let forms = usize::from(form_count);
    if count == forms + 1 && flags & 2 != 0 {
        return Ok(base
            + if female && form == 0 {
                1
            } else {
                usize::from(form) + usize::from(form > 0)
            });
    }
    if count == forms {
        return Ok(base + usize::from(form));
    }
    if count == 1 && flags & 4 == 0 {
        return Ok(base);
    }
    Err(bad("unverified source form/gender model layout"))
}

/// Whether an unpacked model package holds a model with at least one mesh.
fn has_geometry(pack: &[u8]) -> bool {
    asset_parts(pack)
        .ok()
        .and_then(|parts| parts.into_iter().find(|p| magic(p) == MODEL))
        .and_then(|bytes| model(bytes).ok())
        .is_some_and(|(_, meshes, _)| !meshes.is_empty())
}

/// A form's model entry and its unpacked model package, and whether it is form 0's. A form
/// whose own slot holds no geometry (one that differs from the ordinary form only in size or
/// in battle) uses the ordinary form's mesh, rather than guessing an unrelated model.
fn form_model(
    archive: &Garc,
    lookup: &[u8],
    species: u16,
    form: u8,
    form_count: u8,
    female: bool,
) -> Result<(usize, Vec<u8>, bool)> {
    let entry = 1 + model_slot(lookup, species, form, form_count, female)? * 9;
    let pack = unpack(archive.read_entry(entry, LIMIT)?)?;
    if form == 0 || has_geometry(&pack) {
        return Ok((entry, pack, false));
    }
    let ordinary = 1 + model_slot(lookup, species, 0, form_count, false)? * 9;
    Ok((
        ordinary,
        unpack(archive.read_entry(ordinary, LIMIT)?)?,
        true,
    ))
}

pub fn import_assets(
    source: &GameSource,
    output: &Path,
    first: u16,
    last: u16,
) -> Result<AssetReport> {
    use super::data::{self, SpeciesKey};
    if first == 0 || last > data::MAX_SPECIES || first > last {
        return Err(bad("species range is outside the catalog's species"));
    }
    let archive = Garc::open(source, source.file("a/0/9/4")?)?;
    let lookup = archive.read_entry(0, 1 << 20)?;
    let personal = Garc::open(source, source.file("a/0/1/7")?)?;
    let records =
        data::parse_personal_table(&personal.read_entry(personal.entry_count() - 1, LIMIT)?)
            .map_err(ImportError::new)?;
    let mut report = AssetReport {
        models: 0,
        variants: 0,
        animated: 0,
        bytes: 0,
        failed: Vec::new(),
    };
    std::fs::create_dir_all(output.join("actors"))?;
    std::fs::create_dir_all(output.join("icons"))?;
    for species in first..=last {
        let form_count = records[usize::from(species)].form_count;
        let row = lookup
            .get(usize::from(species - 1) * 4..usize::from(species - 1) * 4 + 4)
            .ok_or_else(|| bad("model lookup is truncated"))?;
        let forms: Vec<(u8, bool)> = (0..form_count)
            .filter(|&form| data::ordinary_form(SpeciesKey { species, form }))
            .map(|f| (f, false))
            .collect();
        let mut any_animated = false;
        let mut okay = true;
        for (form, female) in forms {
            let asset_form = if female { 128 } else { u16::from(form) };
            let result = (|| -> Result<()> {
                model_slot(&lookup, species, form, form_count, female)?;
                let mut source_model = None;
                for shiny in [false, true] {
                    let path = super::assets::actor_path(output, species, asset_form, shiny);
                    if path.exists() {
                        let existing =
                            super::assets::load_actor(output, species, asset_form, shiny)
                                .map_err(ImportError::new)?;
                        write_icon(output, &existing)?;
                        any_animated |= existing.idle.is_some();
                        report.variants += 1;
                        continue;
                    }
                    if source_model.is_none() {
                        let (entry, models, ordinary) =
                            form_model(&archive, &lookup, species, form, form_count, female)?;
                        let motions = unpack(archive.read_entry(entry + 4, LIMIT)?)?;
                        source_model = Some((entry, models, motions, ordinary));
                    }
                    let (entry, models, motions, ordinary) = source_model.as_ref().unwrap();
                    let textures =
                        unpack(archive.read_entry(entry + if shiny { 2 } else { 1 }, LIMIT)?)?;
                    let mut actor = convert(models, &textures, Some(motions), species, shiny)?;
                    actor.form = asset_form;
                    if form > 0 && usize::from(row[2]) == 1 {
                        actor.notices.push(format!(
                            "Source form {form} explicitly shares this visual mesh"
                        ));
                    } else if *ordinary {
                        actor.notices.push(format!(
                            "Source form {form} has no mesh of its own and uses form 0's"
                        ));
                    }
                    write_icon(output, &actor)?;
                    any_animated |= actor.idle.is_some();
                    let bytes = actor.encode().map_err(ImportError::new)?;
                    write_new_file(&path, &bytes, false)?;
                    report.bytes += bytes.len() as u64;
                    report.variants += 1;
                }
                Ok(())
            })();
            if let Err(e) = result {
                okay = false;
                report
                    .failed
                    .push((species, format!("form {form}, female {female}: {e}")));
            }
        }
        if okay {
            report.models += 1;
        }
        if any_animated {
            report.animated += 1;
        }
        if species % 25 == 0 || species == last {
            println!(
                "3D assets: {species}/{last}, {} base species, {} variants, {} errors",
                report.models,
                report.variants,
                report.failed.len()
            );
        }
    }
    source.verify_unchanged()?;
    Ok(report)
}

pub fn import_venue(source: &GameSource, output: &Path, index: usize) -> Result<Actor> {
    let archive = Garc::open(source, source.file("a/0/8/1")?)?;
    let pack = unpack(archive.read_entry(index, LIMIT)?)?;
    let actor = convert(&pack, &pack, None, 0, false)?;
    let bytes = actor.encode().map_err(ImportError::new)?;
    write_new_file(&output.join("venue").join("default.pka"), &bytes, false)?;
    source.verify_unchanged()?;
    Ok(actor)
}

pub struct FieldMotionReport {
    pub written: usize,
    pub reused: usize,
    pub missing_walk: Vec<super::data::SpeciesKey>,
    pub failed: Vec<(super::data::SpeciesKey, String)>,
}

fn same_field_sidecar(path: &Path, bytes: &[u8], actor: &Actor) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut existing = Vec::new();
    if file
        .take(super::locomotion::MAX_FILE as u64 + 1)
        .read_to_end(&mut existing)
        .is_err()
    {
        return false;
    }
    existing == bytes && super::locomotion::MotionSet::decode_for(&existing, actor).is_ok()
}

/// The source's field package (model member +6) separates idle (0), walking (2), and
/// running (3). These slots were checked against the actual looped foot/leg tracks;
/// battle package +4 remains the source for battle presentation.
///
/// Write small optional PKL1 sidecars, preserving every existing PKA2 actor and portrait.
/// No game source or running game's installed files are changed by this helper itself.
pub fn import_field_motions(
    source: &GameSource,
    output: &Path,
    keys: &[super::data::SpeciesKey],
) -> Result<FieldMotionReport> {
    use super::data::{self, SpeciesKey};
    use super::locomotion::MotionSet;
    if keys.len() > 4096 {
        return Err(bad("too many requested field motion identities"));
    }
    source.verify_unchanged()?;
    let archive = Garc::open(source, source.file("a/0/9/4")?)?;
    let lookup = archive.read_entry(0, 1 << 20)?;
    let personal = Garc::open(source, source.file("a/0/1/7")?)?;
    let records =
        data::parse_personal_table(&personal.read_entry(personal.entry_count() - 1, LIMIT)?)
            .map_err(ImportError::new)?;
    std::fs::create_dir_all(output.join("motions"))?;
    let mut report = FieldMotionReport {
        written: 0,
        reused: 0,
        missing_walk: Vec::new(),
        failed: Vec::new(),
    };
    let mut seen = std::collections::BTreeSet::new();
    for &key in keys {
        if !seen.insert(key) {
            continue;
        }
        let result = (|| -> Result<(bool, bool)> {
            if !data::ordinary_form(key) {
                return Err(bad("field motion identity is outside the ordinary format"));
            }
            let actor = super::assets::load_actor(output, key.species, u16::from(key.form), false)
                .map_err(ImportError::new)?;
            if let Some(existing) = MotionSet::load_for(output, &actor).map_err(ImportError::new)? {
                return Ok((false, existing.walk.is_none() && existing.run.is_none()));
            }
            let form_count = records[usize::from(key.species)].form_count;
            let (entry, model_pack, _) =
                form_model(&archive, &lookup, key.species, key.form, form_count, false)?;
            let model = asset_parts(&model_pack)?
                .into_iter()
                .find(|p| magic(p) == MODEL)
                .ok_or_else(|| bad("field model package contains no geometry"))?;
            let names = model_names(model)?;
            if names.len() != actor.bones.len() {
                return Err(bad("field motion skeleton differs from the cached model"));
            }
            let field_pack = unpack(archive.read_entry(entry + 6, LIMIT)?)?;
            let parts = package(&field_pack)?;
            let read = |index: usize, locomotion: bool| -> Result<Option<Clip>> {
                let Some(bytes) = parts.get(index) else {
                    return Ok(None);
                };
                if magic(bytes) != MOTION {
                    return Ok(None);
                }
                let Some(mut clip) = motion(bytes, &names)? else {
                    return Ok(None);
                };
                if locomotion && (!clip.looping || clip.frames < 2) {
                    return Ok(None);
                }
                // Source effects for other model LODs are not applied to this actor's meshes.
                clip.visibility
                    .retain(|t| actor.meshes.iter().any(|m| m.name == t.mesh));
                clip.materials
                    .retain(|t| actor.materials.iter().any(|m| m.name == t.material));
                Ok(Some(clip))
            };
            let idle = read(0, false)?;
            let walk = read(2, true)?;
            let run = read(3, true)?;
            let no_walk = walk.is_none() && run.is_none();
            let set = MotionSet::new(&actor, idle, walk, run).map_err(ImportError::new)?;
            let bytes = set.encode().map_err(ImportError::new)?;
            let path = super::locomotion::file_path(output, key.species, u16::from(key.form));
            if path.exists() {
                if same_field_sidecar(&path, &bytes, &actor) {
                    return Ok((false, no_walk));
                }
                return Err(bad("a conflicting field motion destination already exists"));
            }
            source.verify_unchanged()?;
            let written = match write_new_file(&path, &bytes, true) {
                Ok(()) => true,
                // Another run published the same identity first.
                Err(_) if same_field_sidecar(&path, &bytes, &actor) => false,
                Err(e) => return Err(e),
            };
            Ok((written, no_walk))
        })();
        match result {
            Ok((written, no_walk)) => {
                if written {
                    report.written += 1;
                } else {
                    report.reused += 1;
                }
                if no_walk {
                    report.missing_walk.push(SpeciesKey {
                        species: key.species,
                        form: key.form,
                    });
                }
            }
            Err(error) => report.failed.push((key, error.to_string())),
        }
    }
    source.verify_unchanged()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn competing_field_publication_accepts_only_identical_valid_data() {
        let actor = Actor {
            species: 1,
            form: 0,
            shiny: false,
            bones: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            meshes: Vec::new(),
            idle: None,
            notices: Vec::new(),
        };
        let data = super::super::locomotion::MotionSet::new(&actor, None, None, None)
            .unwrap()
            .encode()
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("sa2co-field-publish-{}.pkl", std::process::id()));
        std::fs::write(&path, &data).unwrap();
        assert!(same_field_sidecar(&path, &data, &actor));
        let mut different = data.clone();
        different.push(0);
        assert!(!same_field_sidecar(&path, &different, &actor));
        std::fs::write(&path, &different).unwrap();
        assert!(!same_field_sidecar(&path, &data, &actor));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn new_files_never_replace_existing_ones_with_or_without_hard_links() {
        let directory = std::env::temp_dir().join(format!("sa2co-publish-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join("data.bin");
        write_new_file(&path, b"first", true).unwrap();
        assert!(write_new_file(&path, b"second", false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        // The fallback without hard links: a reserved name, then the part file renamed.
        let part = directory.join("part");
        let other = directory.join("other.bin");
        std::fs::write(&part, b"other").unwrap();
        rename_into_reserved(&part, &other).unwrap();
        assert_eq!(std::fs::read(&other).unwrap(), b"other");
        std::fs::write(&part, b"again").unwrap();
        assert!(rename_into_reserved(&part, &other).is_err());
        assert_eq!(std::fs::read(&other).unwrap(), b"other");
        std::fs::remove_file(&part).unwrap();
        let mut left: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        left.sort();
        assert_eq!(left, ["data.bin", "other.bin"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn canonical_forms_skip_extra_gender_models_and_tell_slots_without_geometry() {
        let mut table = vec![0u8; usize::from(super::super::data::MAX_SPECIES) * 4];
        let set = |table: &mut [u8], species: usize, base: u16, count: u8, flags: u8| {
            let at = (species - 1) * 4;
            table[at..at + 2].copy_from_slice(&base.to_le_bytes());
            table[at + 2] = count;
            table[at + 3] = flags;
        };
        // Two forms and an extra female base model.
        set(&mut table, 1, 2, 3, 7);
        assert_eq!(model_slot(&table, 1, 0, 2, false).unwrap(), 2);
        assert_eq!(model_slot(&table, 1, 0, 2, true).unwrap(), 3);
        assert_eq!(model_slot(&table, 1, 1, 2, false).unwrap(), 4);
        // One model per form.
        set(&mut table, 2, 40, 2, 3);
        assert_eq!(model_slot(&table, 2, 1, 2, false).unwrap(), 41);
        // Many forms sharing one model.
        set(&mut table, 4, 60, 1, 1);
        assert_eq!(model_slot(&table, 4, 19, 20, false).unwrap(), 60);
        set(&mut table, 5, 80, 5, 5);
        assert_eq!(model_slot(&table, 5, 2, 5, false).unwrap(), 82);
        assert_eq!(model_slot(&table, 5, 3, 5, false).unwrap(), 83);
        assert!(model_slot(&table, 5, 5, 5, false).is_err());
        assert!(model_slot(&table, super::super::data::MAX_SPECIES + 1, 0, 1, false).is_err());
        // A slot with no model, or a package without one, holds no geometry.
        assert!(!has_geometry(&[]));
        assert!(!has_geometry(b"PC\0\0"));
    }
    #[test]
    fn texture_and_shader_magic_are_not_interchangeable() {
        let mut texture = vec![0; 32];
        texture[..4].copy_from_slice(&TEX.to_le_bytes());
        texture[8..15].copy_from_slice(b"texture");
        assert!(is_texture(&texture));
        let mut shader = texture.clone();
        shader[8..16].fill(0);
        shader[16..22].copy_from_slice(b"shader");
        assert!(!is_texture(&shader));
        assert_eq!(asset_parts(&shader).unwrap().len(), 0);
    }
}
