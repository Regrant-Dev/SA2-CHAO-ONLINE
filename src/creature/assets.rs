//! Private, bounded 3D actor cache and source skeletal playback. Cache data is built locally
//! by the 64-bit importer; it never enters the shared skin store or network file service.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(target_arch = "x86")]
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_FILE: usize = 32 << 20;
pub const MAX_VERTICES: usize = 100_000;
pub const MAX_BONES: usize = 512;
pub const MAX_TEXTURE_BYTES: usize = 16 << 20;
pub const MAGIC: &[u8; 4] = b"PKA2";
/// The start of a creature icon file.
pub const ICON_MAGIC: &[u8; 4] = b"PKI1";
/// `draw`'s error while the model is still being read (nothing is drawn meanwhile).
pub const LOADING: &str = "3D creature is loading";
static ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
#[cfg(target_arch = "x86")]
static RESET: AtomicBool = AtomicBool::new(false);

/// The selected private source-profile cache. It never points into the shared skins folder.
pub fn set_root(root: Option<PathBuf>) {
    let mut current = ROOT.lock().unwrap_or_else(|e| e.into_inner());
    if *current != root {
        *current = root;
        #[cfg(target_arch = "x86")]
        RESET.store(true, Ordering::Release);
    }
}

pub fn configured_root() -> Option<PathBuf> {
    ROOT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Start/poll bounded background reads. Texture creation stays on the game thread.
pub fn prepare(species: u16, form: u16, shiny: bool) -> Result<bool, String> {
    let root = configured_root().ok_or("Import a source dump before showing 3D creatures")?;
    #[cfg(target_arch = "x86")]
    return native::prepare(&root, species, form, shiny);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (root, species, form, shiny);
        Ok(false)
    }
}

/// Draw an actual locally converted 3D model. A loading/error result is explicit; no picture
/// is substituted for a missing model. Call only in the game's display pass.
pub fn draw(
    species: u16,
    form: u16,
    shiny: bool,
    position: [f32; 3],
    yaw: i32,
    seconds: f32,
    height: f32,
) -> Result<(), String> {
    let root = configured_root().ok_or("No imported 3D asset cache is selected")?;
    if !position.iter().all(|x| x.is_finite() && x.abs() < 1e6)
        || !seconds.is_finite()
        || !height.is_finite()
        || !(0.05..=100.).contains(&height)
    {
        return Err("invalid 3D creature pose".into());
    }
    #[cfg(target_arch = "x86")]
    return native::draw(
        &root, species, form, shiny, position, yaw, seconds, height, None,
    );
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (root, species, form, shiny, yaw);
        Err("3D display is available only inside the game".into())
    }
}

pub fn draw_moving(
    species: u16,
    form: u16,
    shiny: bool,
    position: [f32; 3],
    playback: &super::locomotion::Sample,
    height: f32,
) -> Result<(), String> {
    let root = configured_root().ok_or("No imported 3D asset cache is selected")?;
    if !position.iter().all(|v| v.is_finite() && v.abs() < 1e6)
        || !height.is_finite()
        || !(0.05..=100.).contains(&height)
    {
        return Err("invalid moving creature pose".into());
    }
    #[cfg(target_arch = "x86")]
    return native::draw(
        &root,
        species,
        form,
        shiny,
        position,
        playback.yaw,
        0.,
        height,
        Some(playback),
    );
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (root, species, form, shiny, playback);
        Err("3D display is available only inside the game".into())
    }
}

pub fn forget() {
    #[cfg(target_arch = "x86")]
    native::clear();
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub parent: i16,
    pub scale: [f32; 3],
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub name: String,
    pub material: u16,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub texture: u16,
    /// Scale U/V, rotation in radians, translation U/V (Maya convention).
    pub uv: [f32; 5],
    pub wrap: [u8; 2],
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub frame: u16,
    pub value: f32,
    pub slope: f32,
}

#[derive(Clone, Debug)]
pub struct BoneTrack {
    pub bone: u16,
    pub axis_angle: bool,
    pub channels: [Vec<Key>; 9],
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub frames: u16,
    pub looping: bool,
    pub tracks: Vec<BoneTrack>,
    pub visibility: Vec<VisibilityTrack>,
    pub materials: Vec<MaterialTrack>,
}

#[derive(Clone, Debug)]
pub struct VisibilityTrack {
    pub mesh: String,
    pub bits: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct MaterialTrack {
    pub material: String,
    pub channels: [Vec<Key>; 5],
}
impl Clip {
    fn frame(&self, seconds: f32) -> f32 {
        if self.looping {
            (seconds.max(0.) * 30.) % f32::from(self.frames).max(1.)
        } else {
            (seconds.max(0.) * 30.).min(f32::from(self.frames.saturating_sub(1)))
        }
    }
}

#[derive(Clone, Debug)]
pub struct Actor {
    pub species: u16,
    pub form: u16,
    pub shiny: bool,
    pub bones: Vec<Bone>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub meshes: Vec<Mesh>,
    pub idle: Option<Clip>,
    /// Import diagnostics describe visual approximations, rather than inventing source clips.
    pub notices: Vec<String>,
}

pub fn actor_path(root: &Path, species: u16, form: u16, shiny: bool) -> PathBuf {
    root.join("actors")
        .join(format!("{species:04}_{form:02}_{}.pka", u8::from(shiny)))
}

pub fn thumbnail_path(root: &Path, species: u16, form: u16, shiny: bool) -> PathBuf {
    root.join("icons")
        .join(format!("{species:04}_{form:02}_{}.rgba", u8::from(shiny)))
}

pub fn load_thumbnail(
    root: &Path,
    species: u16,
    form: u16,
    shiny: bool,
) -> Option<(u32, u32, Vec<u8>)> {
    if !(1..=807).contains(&species) || form > 255 {
        return None;
    }
    let path = thumbnail_path(root, species, form, shiny);
    if std::fs::metadata(&path).ok()?.len() > 256 * 256 * 4 + 8 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.get(..4) != Some(ICON_MAGIC) {
        return None;
    }
    let w = u32::from(u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?));
    let h = u32::from(u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?));
    if w == 0 || h == 0 || w > 256 || h > 256 || bytes.len() != 8 + w as usize * h as usize * 4 {
        return None;
    }
    Some((w, h, bytes[8..].to_vec()))
}

pub fn load_actor(root: &Path, species: u16, form: u16, shiny: bool) -> Result<Actor, String> {
    if !(1..=807).contains(&species) || form > 255 {
        return Err("invalid creature asset identity".into());
    }
    let path = actor_path(root, species, form, shiny);
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_FILE as u64 {
        return Err("creature asset cache exceeds its size limit".into());
    }
    let actor = Actor::decode(&std::fs::read(path).map_err(|e| e.to_string())?)?;
    if (actor.species, actor.form, actor.shiny) != (species, form, shiny) {
        return Err("creature asset cache identity differs from its filename".into());
    }
    Ok(actor)
}

pub fn load_venue(root: &Path) -> Result<Actor, String> {
    let path = root.join("venue/default.pka");
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_FILE as u64 {
        return Err("venue cache exceeds its file bound".into());
    }
    let actor = Actor::decode(&std::fs::read(path).map_err(|e| e.to_string())?)?;
    if actor.species != 0 {
        return Err("venue cache identity is invalid".into());
    }
    Ok(actor)
}

pub fn prepare_venue() -> Result<bool, String> {
    let root = configured_root().ok_or("No imported venue cache is selected")?;
    #[cfg(target_arch = "x86")]
    return native::prepare(&root, 0, 0, false);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = root;
        Ok(false)
    }
}

pub fn draw_venue(position: [f32; 3]) -> Result<(), String> {
    let root = configured_root().ok_or("No imported venue cache is selected")?;
    #[cfg(target_arch = "x86")]
    return native::draw(&root, 0, 0, false, position, 0, 0., 70., None);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (root, position);
        Err("Venue display is available only inside the game".into())
    }
}

struct Read<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Read<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("asset length overflow")?;
        let result = self
            .bytes
            .get(self.at..end)
            .ok_or("truncated creature asset cache")?;
        self.at = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn float(&mut self) -> Result<f32, String> {
        let value = f32::from_bits(self.u32()?);
        if !value.is_finite() || value.abs() > 1.0e6 {
            return Err("invalid asset float".into());
        }
        Ok(value)
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut values = [0.; N];
        for value in &mut values {
            *value = self.float()?;
        }
        Ok(values)
    }
    fn count(&mut self, max: usize) -> Result<usize, String> {
        let count = self.u32()? as usize;
        if count > max {
            return Err("asset array exceeds its limit".into());
        }
        Ok(count)
    }
    fn text(&mut self) -> Result<String, String> {
        let n = usize::from(self.u16()?);
        if n > 512 {
            return Err("asset name exceeds its limit".into());
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "invalid asset name".into())
    }
}

struct Write(Vec<u8>);
impl Write {
    fn byte(&mut self, n: u8) {
        self.0.push(n);
    }
    fn u16(&mut self, n: u16) {
        self.0.extend(n.to_le_bytes());
    }
    fn u32(&mut self, n: usize) {
        self.0.extend((n as u32).to_le_bytes());
    }
    fn float(&mut self, n: f32) {
        self.0.extend(n.to_le_bytes());
    }
    fn floats(&mut self, values: &[f32]) {
        for &v in values {
            self.float(v);
        }
    }
    fn text(&mut self, value: &str) {
        self.u16(value.len() as u16);
        self.0.extend(value.as_bytes());
    }
}

impl Actor {
    pub fn validate(&self) -> Result<(), String> {
        let floats_ok = |values: &[f32]| values.iter().all(|x| x.is_finite() && x.abs() <= 1.0e6);
        if self.bones.len() > MAX_BONES
            || self.materials.is_empty()
            || self.materials.len() > 64
            || self.textures.is_empty()
            || self.textures.len() > 64
            || self.meshes.is_empty()
            || self.meshes.len() > 512
        {
            return Err("invalid asset model counts".into());
        }
        for (i, bone) in self.bones.iter().enumerate() {
            if bone.parent < -1
                || bone.parent >= i as i16
                || !floats_ok(&bone.scale)
                || !floats_ok(&bone.rotation)
                || !floats_ok(&bone.translation)
            {
                return Err("invalid asset skeleton hierarchy or transform".into());
            }
        }
        let mut texture_bytes = 0;
        for texture in &self.textures {
            let (w, h) = (usize::from(texture.width), usize::from(texture.height));
            if !(8..=1024).contains(&w)
                || !(8..=1024).contains(&h)
                || !w.is_power_of_two()
                || !h.is_power_of_two()
                || texture.pixels.len() != w * h * 4
            {
                return Err("invalid cached texture".into());
            }
            texture_bytes += texture.pixels.len();
        }
        if texture_bytes > MAX_TEXTURE_BYTES {
            return Err("too many actor texture bytes".into());
        }
        for material in &self.materials {
            if usize::from(material.texture) >= self.textures.len()
                || !floats_ok(&material.uv)
                || material.wrap.iter().any(|&w| w > 3)
                || material.name.len() > 512
            {
                return Err("invalid cached material".into());
            }
        }
        let mut count = 0;
        for mesh in &self.meshes {
            count += mesh.vertices.len();
            if count > MAX_VERTICES
                || mesh.vertices.is_empty()
                || mesh.indices.is_empty()
                || mesh.indices.len() > MAX_VERTICES * 12
                || mesh.indices.len() % 3 != 0
                || usize::from(mesh.material) >= self.materials.len()
                || mesh.name.len() > 512
                || mesh
                    .indices
                    .iter()
                    .any(|&i| i as usize >= mesh.vertices.len())
            {
                return Err("invalid cached mesh".into());
            }
            for vertex in &mesh.vertices {
                if !floats_ok(&vertex.position)
                    || !floats_ok(&vertex.normal)
                    || !floats_ok(&vertex.uv)
                    || !floats_ok(&vertex.weights)
                    || vertex.weights.iter().any(|&w| w < 0.)
                    || (vertex.weights.iter().sum::<f32>() - 1.).abs() > 1.0e-4
                    || vertex
                        .joints
                        .iter()
                        .zip(vertex.weights)
                        .any(|(&j, w)| w > 0. && usize::from(j) >= self.bones.len().max(1))
                {
                    return Err("invalid cached vertex or weights".into());
                }
            }
        }
        if count == 0 {
            return Err("empty cached model".into());
        }
        if let Some(clip) = &self.idle {
            if !(1..=20_000).contains(&clip.frames) || clip.tracks.len() > self.bones.len() {
                return Err("invalid cached clip".into());
            }
            for track in &clip.tracks {
                if usize::from(track.bone) >= self.bones.len() {
                    return Err("invalid clip bone".into());
                }
                for channel in &track.channels {
                    if channel.len() > usize::from(clip.frames) + 1
                        || channel.windows(2).any(|w| w[0].frame >= w[1].frame)
                        || channel
                            .iter()
                            .any(|k| k.frame > clip.frames || !floats_ok(&[k.value, k.slope]))
                    {
                        return Err("invalid cached animation keys".into());
                    }
                }
            }
            if clip.visibility.len() > 512 || clip.materials.len() > 64 {
                return Err("too many source visual tracks".into());
            }
            for t in &clip.visibility {
                if t.mesh.len() > 512 || t.bits.len() != (usize::from(clip.frames) + 1).div_ceil(8)
                {
                    return Err("invalid source mesh visibility track".into());
                }
            }
            for t in &clip.materials {
                if t.material.len() > 512 {
                    return Err("material track name is too long".into());
                }
                for ch in &t.channels {
                    if ch.len() > usize::from(clip.frames) + 1
                        || ch.windows(2).any(|w| w[0].frame >= w[1].frame)
                        || ch
                            .iter()
                            .any(|k| k.frame > clip.frames || !floats_ok(&[k.value, k.slope]))
                    {
                        return Err("invalid source material keys".into());
                    }
                }
            }
        }
        if self.notices.len() > 64 || self.notices.iter().any(|s| s.len() > 512) {
            return Err("too many asset diagnostics".into());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut w = Write(MAGIC.to_vec());
        w.u16(self.species);
        w.u16(self.form);
        w.byte(u8::from(self.shiny));
        w.u32(self.bones.len());
        for b in &self.bones {
            w.u16(b.parent as u16);
            w.floats(&b.scale);
            w.floats(&b.rotation);
            w.floats(&b.translation);
        }
        w.u32(self.textures.len());
        for t in &self.textures {
            w.u16(t.width);
            w.u16(t.height);
            w.u32(t.pixels.len());
            w.0.extend(&t.pixels);
        }
        w.u32(self.materials.len());
        for m in &self.materials {
            w.text(&m.name);
            w.u16(m.texture);
            w.floats(&m.uv);
            w.0.extend(m.wrap);
        }
        w.u32(self.meshes.len());
        for m in &self.meshes {
            w.text(&m.name);
            w.u16(m.material);
            w.u32(m.vertices.len());
            w.u32(m.indices.len());
            for v in &m.vertices {
                w.floats(&v.position);
                w.floats(&v.normal);
                w.floats(&v.uv);
                for j in v.joints {
                    w.u16(j);
                }
                w.floats(&v.weights);
            }
            for &i in &m.indices {
                w.u32(i as usize);
            }
        }
        w.byte(u8::from(self.idle.is_some()));
        if let Some(c) = &self.idle {
            w.u16(c.frames);
            w.byte(u8::from(c.looping));
            w.u32(c.tracks.len());
            for t in &c.tracks {
                w.u16(t.bone);
                w.byte(u8::from(t.axis_angle));
                for ch in &t.channels {
                    w.u32(ch.len());
                    for k in ch {
                        w.u16(k.frame);
                        w.float(k.value);
                        w.float(k.slope);
                    }
                }
            }
            w.u32(c.visibility.len());
            for t in &c.visibility {
                w.text(&t.mesh);
                w.u32(t.bits.len());
                w.0.extend(&t.bits);
            }
            w.u32(c.materials.len());
            for t in &c.materials {
                w.text(&t.material);
                for ch in &t.channels {
                    w.u32(ch.len());
                    for k in ch {
                        w.u16(k.frame);
                        w.float(k.value);
                        w.float(k.slope);
                    }
                }
            }
        }
        w.u32(self.notices.len());
        for s in &self.notices {
            w.text(s);
        }
        if w.0.len() > MAX_FILE {
            return Err("encoded asset exceeds its file bound".into());
        }
        Ok(w.0)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FILE || bytes.get(..4) != Some(MAGIC) {
            return Err("not a bounded creature 3D asset cache".into());
        }
        let mut r = Read { bytes, at: 4 };
        let (species, form, shiny) = (r.u16()?, r.u16()?, r.byte()? != 0);
        let n = r.count(MAX_BONES)?;
        let mut bones = Vec::with_capacity(n);
        for _ in 0..n {
            bones.push(Bone {
                parent: r.u16()? as i16,
                scale: r.floats()?,
                rotation: r.floats()?,
                translation: r.floats()?,
            });
        }
        let n = r.count(64)?;
        let mut textures = Vec::with_capacity(n);
        let mut total = 0;
        for _ in 0..n {
            let (width, height) = (r.u16()?, r.u16()?);
            let n = r.count(MAX_TEXTURE_BYTES)?;
            total += n;
            if total > MAX_TEXTURE_BYTES {
                return Err("texture cache exceeds its byte budget".into());
            }
            textures.push(Texture {
                width,
                height,
                pixels: r.take(n)?.to_vec(),
            });
        }
        let n = r.count(64)?;
        let mut materials = Vec::with_capacity(n);
        for _ in 0..n {
            materials.push(Material {
                name: r.text()?,
                texture: r.u16()?,
                uv: r.floats()?,
                wrap: [r.byte()?, r.byte()?],
            });
        }
        let n = r.count(512)?;
        let mut meshes = Vec::with_capacity(n);
        let mut total = 0;
        for _ in 0..n {
            let (name, material) = (r.text()?, r.u16()?);
            let nv = r.count(MAX_VERTICES)?;
            let ni = r.count(MAX_VERTICES * 12)?;
            total += nv;
            if total > MAX_VERTICES {
                return Err("asset vertices exceed their byte budget".into());
            }
            let mut vertices = Vec::with_capacity(nv);
            for _ in 0..nv {
                vertices.push(Vertex {
                    position: r.floats()?,
                    normal: r.floats()?,
                    uv: r.floats()?,
                    joints: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
                    weights: r.floats()?,
                });
            }
            let mut indices = Vec::with_capacity(ni);
            for _ in 0..ni {
                indices.push(r.u32()?);
            }
            meshes.push(Mesh {
                name,
                material,
                vertices,
                indices,
            });
        }
        let idle = if r.byte()? != 0 {
            let (frames, looping) = (r.u16()?, r.byte()? != 0);
            let n = r.count(MAX_BONES)?;
            let mut tracks = Vec::with_capacity(n);
            for _ in 0..n {
                let (bone, axis_angle) = (r.u16()?, r.byte()? != 0);
                let mut channels = std::array::from_fn(|_| Vec::new());
                for ch in &mut channels {
                    let n = r.count(20_001)?;
                    for _ in 0..n {
                        ch.push(Key {
                            frame: r.u16()?,
                            value: r.float()?,
                            slope: r.float()?,
                        });
                    }
                }
                tracks.push(BoneTrack {
                    bone,
                    axis_angle,
                    channels,
                });
            }
            let n = r.count(512)?;
            let mut visibility = Vec::with_capacity(n);
            for _ in 0..n {
                let mesh = r.text()?;
                let n = r.count(2501)?;
                visibility.push(VisibilityTrack {
                    mesh,
                    bits: r.take(n)?.to_vec(),
                });
            }
            let n = r.count(64)?;
            let mut materials = Vec::with_capacity(n);
            for _ in 0..n {
                let material = r.text()?;
                let mut channels = std::array::from_fn(|_| Vec::new());
                for ch in &mut channels {
                    let n = r.count(20001)?;
                    for _ in 0..n {
                        ch.push(Key {
                            frame: r.u16()?,
                            value: r.float()?,
                            slope: r.float()?,
                        });
                    }
                }
                materials.push(MaterialTrack { material, channels });
            }
            Some(Clip {
                frames,
                looping,
                tracks,
                visibility,
                materials,
            })
        } else {
            None
        };
        let n = r.count(64)?;
        let mut notices = Vec::with_capacity(n);
        for _ in 0..n {
            notices.push(r.text()?);
        }
        if r.at != bytes.len() {
            return Err("trailing bytes in asset cache".into());
        }
        let actor = Self {
            species,
            form,
            shiny,
            bones,
            materials,
            textures,
            meshes,
            idle,
            notices,
        };
        actor.validate()?;
        Ok(actor)
    }
}

/// Column-vector affine matrix. Source rotations are Z*Y*X; source bones use segment-scale
/// compensation, including animated scale (unlike the old position/angle-only skin clips).
#[derive(Clone, Copy, Debug)]
pub struct Mat(pub [[f32; 4]; 4]);
impl Mat {
    pub const ID: Self = Self([
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]);
    pub fn compose(self, b: Self) -> Self {
        Self(std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..4).map(|i| self.0[r][i] * b.0[i][c]).sum())
        }))
    }
    pub fn scale(v: [f32; 3]) -> Self {
        let mut m = Self::ID;
        for (i, x) in v.into_iter().enumerate() {
            m.0[i][i] = x;
        }
        m
    }
    pub fn translate(v: [f32; 3]) -> Self {
        let mut m = Self::ID;
        for (i, x) in v.into_iter().enumerate() {
            m.0[i][3] = x;
        }
        m
    }
    pub fn euler(v: [f32; 3]) -> Self {
        let mut m = Self::ID;
        for (axis, value) in v.into_iter().enumerate() {
            let (s, c) = value.sin_cos();
            let mut r = Self::ID;
            let (a, b) = match axis {
                0 => (1, 2),
                1 => (2, 0),
                _ => (0, 1),
            };
            r.0[a][a] = c;
            r.0[a][b] = -s;
            r.0[b][a] = s;
            r.0[b][b] = c;
            m = r.compose(m);
        }
        m
    }
    pub fn axis(v: [f32; 3]) -> Self {
        let a = (v.iter().map(|x| x * x).sum::<f32>()).sqrt();
        if a < 1e-8 {
            return Self::ID;
        }
        let (x, y, z) = (v[0] / a, v[1] / a, v[2] / a);
        let (s, c) = (a * 2.).sin_cos();
        let d = 1. - c;
        Self([
            [c + x * x * d, x * y * d - z * s, x * z * d + y * s, 0.],
            [y * x * d + z * s, c + y * y * d, y * z * d - x * s, 0.],
            [z * x * d - y * s, z * y * d + x * s, c + z * z * d, 0.],
            [0., 0., 0., 1.],
        ])
    }
    pub fn point(self, v: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|r| self.0[r][3] + (0..3).map(|i| self.0[r][i] * v[i]).sum::<f32>())
    }
    pub fn vector(self, v: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|r| (0..3).map(|i| self.0[r][i] * v[i]).sum())
    }
    pub fn inverse(self) -> Self {
        let mut a = self.0;
        let mut inv = Self::ID.0;
        for col in 0..4 {
            let pivot = (col..4)
                .max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))
                .unwrap();
            if a[pivot][col].abs() < 1e-12 {
                return Self::ID;
            }
            a.swap(col, pivot);
            inv.swap(col, pivot);
            let d = a[col][col];
            for j in 0..4 {
                a[col][j] /= d;
                inv[col][j] /= d;
            }
            for i in 0..4 {
                if i != col {
                    let k = a[i][col];
                    for j in 0..4 {
                        a[i][j] -= k * a[col][j];
                        inv[i][j] -= k * inv[col][j];
                    }
                }
            }
        }
        Self(inv)
    }
    pub fn transpose(self) -> Self {
        Self(std::array::from_fn(|i| {
            std::array::from_fn(|j| self.0[j][i])
        }))
    }
}

fn sample(keys: &[Key], frame: f32, default: f32) -> f32 {
    if keys.is_empty() {
        return default;
    }
    if frame <= f32::from(keys[0].frame) {
        return keys[0].value;
    }
    let last = keys.last().unwrap();
    if frame >= f32::from(last.frame) {
        return last.value;
    }
    let at = keys.partition_point(|k| f32::from(k.frame) <= frame);
    let (a, b) = (keys[at - 1], keys[at]);
    let diff = frame - f32::from(a.frame);
    let w = diff / f32::from(b.frame - a.frame);
    a.value
        + (a.value - b.value) * (2. * w - 3.) * w * w
        + diff * (w - 1.) * (a.slope * (w - 1.) + b.slope * w)
}

#[derive(Default)]
struct PoseBuffers {
    bones: Vec<Bone>,
    rotations: Vec<Mat>,
    matrices: Vec<Mat>,
}

type SkinnedVertex = ([f32; 3], [f32; 3]);

/// Retained game-thread scratch space. The source mesh and clips remain immutable.
#[cfg(any(target_arch = "x86", test))]
#[derive(Default)]
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
struct SkinningBuffers {
    pose: PoseBuffers,
    skin: Vec<Mat>,
    normals: Vec<Mat>,
    meshes: Vec<Vec<SkinnedVertex>>,
    visible: Vec<bool>,
    material_uv: Vec<[f32; 5]>,
}

#[inline(always)]
fn skin_vertex(v: &Vertex, skin: &[Mat], normals: &[Mat]) -> SkinnedVertex {
    if skin.is_empty() {
        return (v.position, v.normal);
    }
    let (mut p, mut n) = ([0.; 3], [0.; 3]);
    for (&joint, &weight) in v.joints.iter().zip(&v.weights) {
        if weight <= 0. {
            continue;
        }
        let q = skin[usize::from(joint)].point(v.position);
        let norm = normals[usize::from(joint)].vector(v.normal);
        for i in 0..3 {
            p[i] += q[i] * weight;
            n[i] += norm[i] * weight;
        }
    }
    let d = n.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-8);
    (p, n.map(|x| x / d))
}

impl Actor {
    pub fn visible(&self, mesh: usize, seconds: f32) -> bool {
        self.visible_with_clip(mesh, seconds, self.idle.as_ref())
    }
    pub(crate) fn visible_with_clip(&self, mesh: usize, seconds: f32, clip: Option<&Clip>) -> bool {
        let Some(c) = clip else {
            return true;
        };
        let Some(track) = c
            .visibility
            .iter()
            .find(|t| t.mesh == self.meshes[mesh].name)
        else {
            return true;
        };
        let at = c.frame(seconds) as usize;
        track
            .bits
            .get(at / 8)
            .is_some_and(|b| b & (1 << (at & 7)) != 0)
    }
    pub fn material_uv(&self, material: usize, seconds: f32) -> [f32; 5] {
        self.material_uv_with_clip(material, seconds, self.idle.as_ref())
    }
    fn material_uv_with_clip(
        &self,
        material: usize,
        seconds: f32,
        clip: Option<&Clip>,
    ) -> [f32; 5] {
        let mut uv = self.materials[material].uv;
        if let Some(c) = clip
            && let Some(t) = c
                .materials
                .iter()
                .find(|t| t.material == self.materials[material].name)
        {
            for (i, ch) in t.channels.iter().enumerate() {
                uv[i] = sample(ch, c.frame(seconds), uv[i]);
            }
        }
        uv
    }
    pub fn bind_matrices(&self) -> Vec<Mat> {
        let mut out = Vec::<Mat>::with_capacity(self.bones.len());
        for b in &self.bones {
            let local = Mat::translate(b.translation)
                .compose(Mat::euler(b.rotation))
                .compose(Mat::scale(b.scale));
            out.push(if b.parent >= 0 {
                out[b.parent as usize].compose(local)
            } else {
                local
            });
        }
        out
    }
    pub fn pose(&self, seconds: f32) -> Vec<Mat> {
        self.pose_with_clip(seconds, self.idle.as_ref())
    }
    fn pose_with_clip(&self, seconds: f32, clip: Option<&Clip>) -> Vec<Mat> {
        let mut buffers = PoseBuffers::default();
        self.pose_into(seconds, clip, &mut buffers);
        buffers.matrices
    }
    fn pose_into(&self, seconds: f32, clip: Option<&Clip>, buffers: &mut PoseBuffers) {
        let PoseBuffers {
            bones,
            rotations,
            matrices,
        } = buffers;
        bones.clone_from(&self.bones);
        rotations.clear();
        rotations.extend(bones.iter().map(|b| Mat::euler(b.rotation)));
        if let Some(c) = clip {
            let f = c.frame(seconds);
            for t in &c.tracks {
                let b = &mut bones[usize::from(t.bone)];
                let mut v = [0.; 9];
                v[..3].copy_from_slice(&b.scale);
                v[3..6].copy_from_slice(&b.rotation);
                v[6..].copy_from_slice(&b.translation);
                for (i, ch) in t.channels.iter().enumerate() {
                    v[i] = sample(ch, f, v[i]);
                }
                b.scale.copy_from_slice(&v[..3]);
                b.translation.copy_from_slice(&v[6..]);
                let r = v[3..6].try_into().unwrap();
                rotations[usize::from(t.bone)] = if t.axis_angle {
                    Mat::axis(r)
                } else {
                    Mat::euler(r)
                };
            }
        }
        matrices.clear();
        for (i, b) in bones.iter().enumerate() {
            let mut local = Mat::translate(b.translation);
            if b.parent >= 0 {
                let p = &bones[b.parent as usize];
                local = local.compose(Mat::scale(
                    p.scale.map(|s| if s.abs() > 1e-8 { 1. / s } else { 0. }),
                ));
            }
            local = local.compose(rotations[i]).compose(Mat::scale(b.scale));
            matrices.push(if b.parent >= 0 {
                matrices[b.parent as usize].compose(local)
            } else {
                local
            });
        }
    }
    /// Position-only skinning for off-thread clip-reference analysis. No normal inverses,
    /// textures or native game state are needed while finding a motion's ground envelope.
    pub(crate) fn positions_with_clip(
        &self,
        seconds: f32,
        clip: Option<&Clip>,
        inverse_bind: &[Mat],
    ) -> Vec<Vec<[f32; 3]>> {
        let skin: Vec<Mat> = self
            .pose_with_clip(seconds, clip)
            .iter()
            .zip(inverse_bind)
            .map(|(&p, &b)| p.compose(b))
            .collect();
        self.meshes
            .iter()
            .map(|mesh| {
                mesh.vertices
                    .iter()
                    .map(|v| {
                        if skin.is_empty() {
                            return v.position;
                        }
                        let mut p = [0.; 3];
                        for (&joint, &weight) in v.joints.iter().zip(&v.weights) {
                            if weight <= 0. {
                                continue;
                            }
                            let q = skin[usize::from(joint)].point(v.position);
                            for i in 0..3 {
                                p[i] += q[i] * weight;
                            }
                        }
                        p
                    })
                    .collect()
            })
            .collect()
    }

    pub fn skinned(&self, seconds: f32) -> Vec<Vec<([f32; 3], [f32; 3])>> {
        let inverse_bind: Vec<Mat> = self.bind_matrices().into_iter().map(Mat::inverse).collect();
        self.skinned_with_bind(seconds, self.idle.as_ref(), &inverse_bind)
    }
    fn skinned_with_bind(
        &self,
        seconds: f32,
        clip: Option<&Clip>,
        inverse_bind: &[Mat],
    ) -> Vec<Vec<SkinnedVertex>> {
        let pose = self.pose_with_clip(seconds, clip);
        let skin: Vec<Mat> = pose
            .iter()
            .zip(inverse_bind)
            .map(|(&p, &b)| p.compose(b))
            .collect();
        let normals: Vec<Mat> = skin.iter().map(|m| m.inverse().transpose()).collect();
        self.meshes
            .iter()
            .map(|mesh| {
                mesh.vertices
                    .iter()
                    .map(|v| skin_vertex(v, &skin, &normals))
                    .collect()
            })
            .collect()
    }

    #[cfg(any(target_arch = "x86", test))]
    fn skin_into(
        &self,
        seconds: f32,
        clip: Option<&Clip>,
        inverse_bind: &[Mat],
        buffers: &mut SkinningBuffers,
    ) {
        self.pose_into(seconds, clip, &mut buffers.pose);
        buffers.skin.clear();
        buffers.skin.extend(
            buffers
                .pose
                .matrices
                .iter()
                .zip(inverse_bind)
                .map(|(&pose, &bind)| pose.compose(bind)),
        );
        buffers.normals.clear();
        buffers
            .normals
            .extend(buffers.skin.iter().map(|m| m.inverse().transpose()));
        buffers.meshes.resize_with(self.meshes.len(), Vec::new);
        buffers.visible.clear();
        for (index, (mesh, output)) in self.meshes.iter().zip(&mut buffers.meshes).enumerate() {
            let visible = self.visible_with_clip(index, seconds, clip);
            buffers.visible.push(visible);
            output.clear();
            if visible {
                output.extend(
                    mesh.vertices
                        .iter()
                        .map(|v| skin_vertex(v, &buffers.skin, &buffers.normals)),
                );
            }
        }
    }
}

impl Actor {
    /// Small local UI portrait rendered from the actual 3D mesh, not a bundled sprite.
    pub fn thumbnail(&self, width: usize, height: usize) -> Vec<u8> {
        if width == 0 || height == 0 || width > 256 || height > 256 {
            return Vec::new();
        }
        let posed = self.skinned(0.);
        let view = Mat::euler([-0.12, 0.36, 0.]);
        let all: Vec<[f32; 3]> = posed
            .iter()
            .enumerate()
            .filter(|(i, _)| self.visible(*i, 0.))
            .flat_map(|(_, p)| p.iter())
            .map(|(p, _)| view.point(*p))
            .collect();
        if all.is_empty() {
            return vec![0; width * height * 4];
        }
        let lo: [f32; 3] =
            std::array::from_fn(|i| all.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min));
        let hi: [f32; 3] =
            std::array::from_fn(|i| all.iter().map(|p| p[i]).fold(f32::NEG_INFINITY, f32::max));
        let scale = ((width as f32 - 6.) / (hi[0] - lo[0]).max(1.))
            .min((height as f32 - 6.) / (hi[1] - lo[1]).max(1.));
        let cx = (lo[0] + hi[0]) * 0.5;
        let cy = (lo[1] + hi[1]) * 0.5;
        let mut pixels = vec![0u8; width * height * 4];
        let mut depth = vec![f32::NEG_INFINITY; width * height];
        for (mesh_index, (mesh, points)) in self.meshes.iter().zip(&posed).enumerate() {
            if !self.visible(mesh_index, 0.) {
                continue;
            }
            let material = &self.materials[usize::from(mesh.material)];
            let texture = &self.textures[usize::from(material.texture)];
            for tri in mesh.indices.as_chunks::<3>().0 {
                let mut p = [[0.; 3]; 3];
                let mut uv = [[0.; 2]; 3];
                for i in 0..3 {
                    let j = tri[i] as usize;
                    let q = view.point(points[j].0);
                    p[i] = [
                        (q[0] - cx) * scale + width as f32 * 0.5,
                        height as f32 * 0.5 - (q[1] - cy) * scale,
                        q[2],
                    ];
                    uv[i] = mapped_uv(
                        mesh.vertices[j].uv,
                        self.material_uv(usize::from(mesh.material), 0.),
                    );
                }
                let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                    - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
                if area.abs() < 1e-6 {
                    continue;
                }
                let minx = p
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .max(0.) as usize;
                let maxx = p
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .min(width as f32 - 1.) as usize;
                let miny = p
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .max(0.) as usize;
                let maxy = p
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .min(height as f32 - 1.) as usize;
                for y in miny..=maxy {
                    for x in minx..=maxx {
                        let (qx, qy) = (x as f32 + 0.5, y as f32 + 0.5);
                        let a = ((p[1][0] - qx) * (p[2][1] - qy) - (p[1][1] - qy) * (p[2][0] - qx))
                            / area;
                        let b = ((p[2][0] - qx) * (p[0][1] - qy) - (p[2][1] - qy) * (p[0][0] - qx))
                            / area;
                        let c = 1. - a - b;
                        if a < 0. || b < 0. || c < 0. {
                            continue;
                        }
                        let z = a * p[0][2] + b * p[1][2] + c * p[2][2];
                        let at = y * width + x;
                        if z <= depth[at] {
                            continue;
                        }
                        let coords: [f32; 2] =
                            std::array::from_fn(|i| a * uv[0][i] + b * uv[1][i] + c * uv[2][i]);
                        let u = wrap(coords[0], material.wrap[0]);
                        let v = wrap(1. - coords[1], material.wrap[1]);
                        let tx = (u * f32::from(texture.width - 1)).round() as usize;
                        let ty = (v * f32::from(texture.height - 1)).round() as usize;
                        let t = (ty * usize::from(texture.width) + tx) * 4;
                        if texture.pixels[t + 3] < 80 {
                            continue;
                        }
                        pixels[at * 4..at * 4 + 4].copy_from_slice(&texture.pixels[t..t + 4]);
                        depth[at] = z;
                    }
                }
            }
        }
        pixels
    }
}

#[derive(Clone, Copy)]
struct UvTransform {
    scale: [f32; 2],
    rotation: [f32; 2],
    translation: [f32; 2],
}
impl UvTransform {
    fn new([sx, sy, a, tx, ty]: [f32; 5]) -> Self {
        let (s, c) = a.sin_cos();
        Self {
            scale: [sx, sy],
            rotation: [s, c],
            translation: [
                sx * (0.5 * s - 0.5 * c + 0.5 - tx),
                sy * (-0.5 * s - 0.5 * c + 0.5 - ty),
            ],
        }
    }
    fn point(&self, uv: [f32; 2]) -> [f32; 2] {
        let [sx, sy] = self.scale;
        let [s, c] = self.rotation;
        [
            sx * (c * uv[0] - s * uv[1]) + self.translation[0],
            sy * (s * uv[0] + c * uv[1]) + self.translation[1],
        ]
    }
}
fn mapped_uv(uv: [f32; 2], transform: [f32; 5]) -> [f32; 2] {
    UvTransform::new(transform).point(uv)
}
fn wrap(v: f32, mode: u8) -> f32 {
    match mode {
        0 | 1 => v.clamp(0., 1.),
        3 => {
            let t = v.rem_euclid(2.);
            if t > 1. { 2. - t } else { t }
        }
        _ => v.rem_euclid(1.),
    }
}

// Native UVN chunks can contain many triangle strips. A chunk flushes the immediate
// renderer, so writing one chunk per triangle both overwhelms submission cost and needlessly
// re-applies material state. Keep this encoder portable for structural regression checks.
#[cfg(any(target_arch = "x86", test))]
struct PolygonPacket {
    words: Vec<u32>,
    uv_slots: Vec<(usize, usize)>,
}
#[cfg(any(target_arch = "x86", test))]
fn encode_polygons(
    mesh: &Mesh,
    refs: &[usize],
    tris: &[u16],
    material: &Material,
    alpha: bool,
) -> Result<PolygonPacket, String> {
    if refs.is_empty()
        || refs.len() > 500
        || tris.is_empty()
        || !tris.len().is_multiple_of(3)
        || refs.iter().any(|&i| i >= mesh.vertices.len())
        || tris.iter().any(|&i| usize::from(i) >= refs.len())
    {
        return Err("invalid native triangle references".into());
    }
    let clamp = (if material.wrap[0] <= 1 { 0x2000 } else { 0 })
        | (if material.wrap[1] <= 1 { 0x1000 } else { 0 });
    let mut words = vec![0x2511u16, 2, 0xffff, 0xffff, 0x0408 | clamp, 0x4000];
    let mut uv_slots = Vec::with_capacity(tris.len());
    let uv_transform = UvTransform::new(material.uv);
    for group in tris.chunks(18_000) {
        let strips = super::strips::strips(group, !alpha);
        let size = 1 + strips.iter().map(|s| 1 + s.len() * 3).sum::<usize>();
        let pad = size & 1;
        words.extend([
            0x1641 | if alpha { 0x0800 } else { 0 },
            (size + pad) as u16,
            strips.len() as u16,
        ]);
        for strip in &strips {
            words.push(strip.len() as u16);
            for &index in strip {
                let source = refs[usize::from(index)];
                let uv = uv_transform.point(mesh.vertices[source].uv);
                let values = [
                    uv[0] / if material.wrap[0] == 3 { 2. } else { 1. },
                    (1. - uv[1]) / if material.wrap[1] == 3 { 2. } else { 1. },
                ];
                if values.iter().any(|v| !v.is_finite()) {
                    return Err("invalid native texture coordinates".into());
                }
                uv_slots.push((words.len() + 1, source));
                words.extend([
                    index,
                    (values[0] * 256.).round().clamp(-32768., 32767.) as i16 as u16,
                    (values[1] * 256.).round().clamp(-32768., 32767.) as i16 as u16,
                ]);
            }
        }
        if pad != 0 {
            words.push(0);
        }
    }
    words.push(0xff);
    if !words.len().is_multiple_of(2) {
        words.push(0);
    }
    let words = words
        .as_chunks::<2>()
        .0
        .iter()
        .map(|w| u32::from(w[0]) | u32::from(w[1]) << 16)
        .collect();
    Ok(PolygonPacket { words, uv_slots })
}

#[cfg(any(target_arch = "x86", test))]
#[derive(Debug)]
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
struct PreparedActor {
    actor: Actor,
    motions: Option<super::locomotion::MotionSet>,
    inverse_bind: Vec<Mat>,
    placement: super::placement::Placement,
    /// The actor's (and its motions') cache file lengths.
    file_size: usize,
    #[cfg(target_arch = "x86")]
    parts: Vec<native::Part>,
    #[cfg(target_arch = "x86")]
    textures: native::PreparedTextures,
}

/// Native vertex packets belong to the loaded model, so reuse only the pose actually left
/// in those packets. Another instance of the same species can replace it between passes.
#[cfg(any(target_arch = "x86", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
struct PoseKey {
    field: Option<super::locomotion::MotionRole>,
    seconds: u32,
    height: u32,
}
// Keep active reads bounded. Completed reads for actors that left the view must not occupy
// every worker slot while a new scene waits for its own venue/model; they're kept a while in
// case the actor comes back, then dropped.
#[cfg(any(target_arch = "x86", test))]
struct PendingRead {
    receiver: std::sync::mpsc::Receiver<Result<PreparedActor, String>>,
    requested: std::time::Instant,
    /// The finished read, not claimed yet (a reader that stopped is a failed read).
    done: Option<Result<PreparedActor, String>>,
}
#[cfg(any(target_arch = "x86", test))]
impl PendingRead {
    /// How long a finished read nobody asks for is kept.
    const KEPT: std::time::Duration = std::time::Duration::from_secs(10);

    fn new(receiver: std::sync::mpsc::Receiver<Result<PreparedActor, String>>) -> Self {
        Self {
            receiver,
            requested: std::time::Instant::now(),
            done: None,
        }
    }

    /// Whether the read has finished (its result then waits in `done`).
    fn finished(&mut self) -> bool {
        if self.done.is_none() {
            self.done = match self.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("3D model reader stopped".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            };
        }
        self.done.is_some()
    }

    /// Finished, and not asked for in a while.
    fn obsolete(&mut self, now: std::time::Instant) -> bool {
        self.finished() && now.saturating_duration_since(self.requested) >= Self::KEPT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet_faces(packet: &PolygonPacket) -> (Vec<u16>, usize) {
        let words: Vec<u16> = packet
            .words
            .iter()
            .flat_map(|w| [*w as u16, (*w >> 16) as u16])
            .collect();
        let mut at = 6;
        let mut faces = Vec::new();
        let mut chunks = 0;
        while words[at] != 0xff {
            assert_eq!(words[at] & 255, 0x41);
            let end = at + 2 + usize::from(words[at + 1]);
            let mut vertex = at + 3;
            for _ in 0..words[at + 2] & 0x3fff {
                let count = usize::from(words[vertex]);
                assert!(count >= 3);
                vertex += 1;
                let indices: Vec<_> = (0..count).map(|i| words[vertex + i * 3]).collect();
                for (i, tri) in indices.windows(3).enumerate() {
                    faces.extend(if i.is_multiple_of(2) {
                        [tri[0], tri[1], tri[2]]
                    } else {
                        [tri[1], tri[0], tri[2]]
                    });
                }
                vertex += count * 3;
            }
            assert!(vertex == end || (vertex + 1 == end && words[vertex] == 0));
            at = end;
            chunks += 1;
        }
        (faces, chunks)
    }

    #[test]
    fn native_triangles_are_batched_without_dropping_faces_or_uv_locations() {
        let actor = tiny();
        let mesh = &actor.meshes[0];
        let material = &actor.materials[0];
        let tris: Vec<_> = (0..6001).flat_map(|_| [0, 1, 2]).collect();
        let packet = encode_polygons(mesh, &[0, 1, 2], &tris, material, false).unwrap();
        let (decoded, chunks) = packet_faces(&packet);
        assert_eq!(decoded, tris);
        assert_eq!(chunks, 2);
        assert_eq!(packet.uv_slots.len(), tris.len());
        let words: Vec<u16> = packet
            .words
            .iter()
            .flat_map(|w| [*w as u16, (*w >> 16) as u16])
            .collect();
        for &(uv, source) in &packet.uv_slots {
            assert_eq!(words[uv - 1], source as u16);
        }
        assert_eq!(words[6], 0x1641);
    }

    #[test]
    fn native_clamp_axes_and_transparency_match_the_chunk_format() {
        let actor = tiny();
        let mesh = &actor.meshes[0];
        let mut material = actor.materials[0].clone();
        material.wrap = [0, 2];
        let a = encode_polygons(mesh, &[0, 1, 2], &[0, 1, 2], &material, true).unwrap();
        assert_eq!(a.words[2] as u16, 0x2408);
        assert_eq!(a.words[3] as u16, 0x1e41);
        material.wrap = [2, 0];
        let b = encode_polygons(mesh, &[0, 1, 2], &[0, 1, 2], &material, false).unwrap();
        assert_eq!(b.words[2] as u16, 0x1408);
        assert!(encode_polygons(mesh, &[0, 1, 2], &[0, 1, 3], &material, false).is_err());
        assert!(encode_polygons(mesh, &[0, 1, 99], &[0, 1, 2], &material, false).is_err());
    }

    #[test]
    fn connected_native_packets_preserve_winding_and_uv_seams_with_fewer_vertices() {
        let mut actor = tiny();
        let mut vertex = actor.meshes[0].vertices[1].clone();
        vertex.position = [1., 1., 0.];
        vertex.uv = [0.75, 0.25];
        actor.meshes[0].vertices.push(vertex);
        let source = [0, 1, 2, 2, 1, 3];
        for alpha in [false, true] {
            let packet = encode_polygons(
                &actor.meshes[0],
                &[0, 1, 2, 3],
                &source,
                &actor.materials[0],
                alpha,
            )
            .unwrap();
            assert_eq!(packet_faces(&packet), (source.to_vec(), 1));
            assert_eq!(packet.uv_slots.len(), 4);
            let words: Vec<u16> = packet
                .words
                .iter()
                .flat_map(|w| [*w as u16, (*w >> 16) as u16])
                .collect();
            for &(slot, vertex) in &packet.uv_slots {
                assert_eq!(words[slot - 1], vertex as u16);
                let uv = actor.meshes[0].vertices[vertex].uv;
                assert_eq!(words[slot], (uv[0] * 256.).round() as u16);
                assert_eq!(words[slot + 1], ((1. - uv[1]) * 256.).round() as u16);
            }
        }
    }

    #[test]
    fn retained_skinning_preserves_weighted_scaled_poses_across_visibility_and_clip_changes() {
        let mut actor = tiny();
        actor.bones[0].scale = [2., 1., 0.5];
        actor.bones[0].rotation = [0.1, 0.4, -0.2];
        actor.bones.push(Bone {
            parent: 0,
            scale: [0.5, 3., 1.],
            rotation: [0.2, 0., 0.3],
            translation: [0., 1., 0.],
        });
        for v in &mut actor.meshes[0].vertices {
            v.joints = [0, 1, 0, 0];
            v.weights = [0.25, 0.75, 0., 0.];
        }
        let mut channels = std::array::from_fn(|_| Vec::new());
        channels[7] = vec![
            Key {
                frame: 0,
                value: 1.,
                slope: 0.,
            },
            Key {
                frame: 2,
                value: 3.,
                slope: 0.,
            },
        ];
        let clip = Clip {
            frames: 3,
            looping: true,
            tracks: vec![BoneTrack {
                bone: 1,
                axis_angle: true,
                channels,
            }],
            visibility: vec![VisibilityTrack {
                mesh: "body".into(),
                bits: vec![0b101],
            }],
            materials: vec![],
        };
        let inverse: Vec<_> = actor
            .bind_matrices()
            .into_iter()
            .map(Mat::inverse)
            .collect();
        let mut buffers = SkinningBuffers::default();
        actor.skin_into(0., Some(&clip), &inverse, &mut buffers);
        let capacity = buffers.meshes[0].capacity();
        for frame in [0., 0.5, 1., 1.5, 2., 2.5, 0., 1., 0.] {
            for selected in [Some(&clip), None] {
                let seconds = frame / 30.;
                actor.skin_into(seconds, selected, &inverse, &mut buffers);
                let visible = actor.visible_with_clip(0, seconds, selected);
                assert_eq!(buffers.visible, vec![visible]);
                if visible {
                    assert_eq!(
                        buffers.meshes,
                        actor.skinned_with_bind(seconds, selected, &inverse)
                    );
                } else {
                    assert!(buffers.meshes[0].is_empty());
                }
                assert_eq!(buffers.meshes[0].capacity(), capacity);
            }
        }
    }

    #[test]
    fn leaving_a_view_frees_completed_reads_without_losing_current_or_active_reads() {
        use std::{
            sync::mpsc,
            time::{Duration, Instant},
        };
        let now = Instant::now();
        let (old_tx, old_rx) = mpsc::channel();
        old_tx.send(Err("old view".into())).unwrap();
        let mut old = PendingRead::new(old_rx);
        old.requested = now - PendingRead::KEPT;
        assert!(old.obsolete(now));
        // A finished read that was asked for recently is kept, result and all.
        let (current_tx, current_rx) = mpsc::channel();
        current_tx.send(Err("current view".into())).unwrap();
        let mut current = PendingRead::new(current_rx);
        current.requested = now - Duration::from_secs(1);
        assert!(!current.obsolete(now));
        assert!(current.finished());
        assert_eq!(current.done.take().unwrap().unwrap_err(), "current view");
        // An active read is never dropped, however old.
        let (active_tx, active_rx) = mpsc::channel();
        let mut active = PendingRead::new(active_rx);
        active.requested = now - PendingRead::KEPT * 2;
        assert!(!active.obsolete(now));
        assert!(!active.finished());
        // A reader that stopped without a result has failed.
        drop(active_tx);
        assert!(active.finished());
        assert!(active.done.take().unwrap().is_err());
    }
    fn tiny() -> Actor {
        let vertex = |position| Vertex {
            position,
            normal: [0., 0., 1.],
            uv: [0.25, 0.25],
            joints: [0; 4],
            weights: [1., 0., 0., 0.],
        };
        Actor {
            species: 1,
            form: 0,
            shiny: false,
            bones: vec![Bone {
                parent: -1,
                scale: [1.; 3],
                rotation: [0.; 3],
                translation: [0.; 3],
            }],
            materials: vec![Material {
                name: "body".into(),
                texture: 0,
                uv: [1., 1., 0., 0., 0.],
                wrap: [2; 2],
            }],
            textures: vec![Texture {
                width: 8,
                height: 8,
                pixels: [180, 200, 100, 255].repeat(64),
            }],
            meshes: vec![Mesh {
                name: "body".into(),
                material: 0,
                vertices: vec![
                    vertex([0., 0., 0.]),
                    vertex([1., 0., 0.]),
                    vertex([0., 1., 0.]),
                ],
                indices: vec![0, 1, 2],
            }],
            idle: None,
            notices: Vec::new(),
        }
    }
    #[test]
    fn cache_round_trip_is_bounded_and_deterministic() {
        let actor = tiny();
        let bytes = actor.encode().unwrap();
        assert_eq!(bytes, actor.encode().unwrap());
        let decoded = Actor::decode(&bytes).unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        for n in [0, 3, bytes.len() - 1] {
            assert!(Actor::decode(&bytes[..n]).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(Actor::decode(&trailing).is_err());
    }
    #[test]
    fn malformed_geometry_weights_and_hierarchies_are_rejected() {
        let mut actor = tiny();
        actor.meshes[0].vertices[0].position[0] = f32::NAN;
        assert!(actor.encode().is_err());
        let mut actor = tiny();
        actor.meshes[0].vertices[0].weights[0] = -1.;
        assert!(actor.encode().is_err());
        let mut actor = tiny();
        actor.meshes[0].indices[2] = 3;
        assert!(actor.encode().is_err());
        let mut actor = tiny();
        actor.bones[0].parent = 0;
        assert!(actor.encode().is_err());
    }
    #[test]
    fn source_scale_visibility_and_material_keys_play_without_skin_format_losses() {
        let mut actor = tiny();
        let mut channels = std::array::from_fn(|_| Vec::new());
        channels[0] = vec![Key {
            frame: 0,
            value: 2.,
            slope: 0.,
        }];
        let mut uv = std::array::from_fn(|_| Vec::new());
        uv[3] = vec![Key {
            frame: 0,
            value: 0.5,
            slope: 0.,
        }];
        actor.idle = Some(Clip {
            frames: 3,
            looping: true,
            tracks: vec![BoneTrack {
                bone: 0,
                axis_angle: false,
                channels,
            }],
            visibility: vec![VisibilityTrack {
                mesh: "body".into(),
                bits: vec![1],
            }],
            materials: vec![MaterialTrack {
                material: "body".into(),
                channels: uv,
            }],
        });
        actor.validate().unwrap();
        assert_eq!(actor.skinned(0.)[0][1].0, [2., 0., 0.]);
        assert!(actor.visible(0, 0.));
        assert!(!actor.visible(0, 1. / 30.));
        assert_eq!(actor.material_uv(0, 0.)[3], 0.5);
        let copy = Actor::decode(&actor.encode().unwrap()).unwrap();
        assert!(!copy.visible(0, 1. / 30.));
    }
    #[test]
    fn segment_scale_compensation_preserves_child_scale_and_scaled_segment_length() {
        let mut actor = tiny();
        actor.bones[0].scale = [2.; 3];
        actor.bones.push(Bone {
            parent: 0,
            scale: [3.; 3],
            rotation: [0.; 3],
            translation: [0., 5., 0.],
        });
        let pose = actor.pose(0.);
        assert_eq!(pose[1].point([0.; 3]), [0., 10., 0.]);
        assert_eq!(pose[1].vector([1., 0., 0.]), [3., 0., 0.]);
    }
    #[test]
    fn generated_ui_portrait_contains_real_mesh_pixels() {
        let pixels = tiny().thumbnail(32, 32);
        assert_eq!(pixels.len(), 4096);
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0));
        assert!(tiny().thumbnail(1000, 1000).is_empty());
    }
}

#[cfg(target_arch = "x86")]
mod native {
    use super::*;
    use crate::creature_link::billboard::Textures;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::Duration;
    use std::time::Instant;
    const RESIDENT: usize = 128 << 20;
    // Thirty shared visitors, four partners, battle actors and the venue.
    // The byte budget remains authoritative even when these identities differ.
    const LOADED: usize = 40;
    /// Native texture records all loaded models may hold together (the game's pool is shared
    /// with its own scenes; billboard keeps a further reserve free).
    const TEXTURE_RECORDS: usize = 320;
    /// A model drawn this recently is never evicted for another: that one waits instead.
    const RECENT: Duration = Duration::from_secs(1);
    /// Background reads at once.
    const READERS: usize = 2;
    /// When a failed model is tried again: soon when it was short of room, else later.
    const RESOURCE_RETRY: Duration = Duration::from_secs(3);
    const FAILURE_RETRY: Duration = Duration::from_secs(30);
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    type Id = (PathBuf, u16, u16, bool);
    struct Failure {
        message: String,
        retry: Instant,
    }
    struct Cache {
        loaded: Vec<Loaded>,
        pending: HashMap<Id, PendingRead>,
        failed: HashMap<Id, Failure>,
        /// The last failure logged for each model, so a retried one logs only what's new.
        logged: HashMap<Id, String>,
        serial: u32,
        upload_tick: Option<u32>,
        timing: FrameTiming,
    }
    struct FrameTiming {
        tick: Option<u32>,
        calls: usize,
        micros: [u128; 3],
        reported: Instant,
    }
    impl FrameTiming {
        fn new() -> Self {
            Self {
                tick: None,
                calls: 0,
                micros: [0; 3],
                reported: Instant::now(),
            }
        }
        fn record(&mut self, tick: u32, load: u128, pose: u128, submit: u128) {
            if self.tick != Some(tick) {
                // Low-rate CPU diagnostics help distinguish native drawing from animation
                // or first-appearance uploads without hooks in the running game process.
                if self.micros.iter().sum::<u128>() >= 8_000
                    && self.reported.elapsed().as_secs() >= 5
                {
                    crate::log!(
                        "3D frame CPU: {} draws; prepare {:.2} ms, animate {:.2} ms, submit {:.2} ms",
                        self.calls,
                        self.micros[0] as f64 / 1000.,
                        self.micros[1] as f64 / 1000.,
                        self.micros[2] as f64 / 1000.
                    );
                    self.reported = Instant::now();
                }
                self.tick = Some(tick);
                self.calls = 0;
                self.micros = [0; 3];
            }
            self.calls += 1;
            for (total, value) in self.micros.iter_mut().zip([load, pose, submit]) {
                *total += value;
            }
        }
    }
    struct Loaded {
        id: Id,
        actor: Actor,
        motions: Option<super::super::locomotion::MotionSet>,
        inverse_bind: Vec<Mat>,
        parts: Vec<Part>,
        textures: Vec<Textures>,
        material_textures: Vec<usize>,
        used: Instant,
        size: usize,
        placement: super::super::placement::Placement,
        skinning: SkinningBuffers,
        last_pose: Option<PoseKey>,
    }
    #[derive(Debug)]
    pub(super) struct Part {
        object: Box<[u32; 13]>,
        _model: Box<[u32; 6]>,
        vertices: Box<[u32]>,
        _polygons: Box<[u32]>,
        refs: Vec<usize>,
        mesh: usize,
        material: usize,
        uv_slots: Vec<(usize, usize)>,
        uv: [f32; 5],
    }
    impl Drop for Loaded {
        fn drop(&mut self) {
            for t in &self.textures {
                t.release();
            }
        }
    }
    fn with<T>(f: impl FnOnce(&mut Cache) -> T) -> T {
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let cache = c.get_or_insert_with(|| Cache {
            loaded: Vec::new(),
            pending: HashMap::new(),
            failed: HashMap::new(),
            logged: HashMap::new(),
            serial: 0,
            upload_tick: None,
            timing: FrameTiming::new(),
        });
        // Root selection can change on the loader thread. Native texture release belongs
        // only to this game-thread path; never clear GPU objects in set_root itself.
        if RESET.swap(false, Ordering::AcqRel) {
            cache.loaded.clear();
            cache.pending.clear();
            cache.failed.clear();
            cache.logged.clear();
            cache.upload_tick = None;
        }
        f(cache)
    }
    pub fn clear() {
        with(|c| {
            c.loaded.clear();
            c.pending.clear();
            c.failed.clear();
            c.logged.clear();
            c.upload_tick = None;
        });
    }
    fn baked(texture: &Texture, material: &Material) -> (Vec<u8>, usize, usize) {
        let mu = material.wrap[0] == 3;
        let mv = material.wrap[1] == 3;
        let w = usize::from(texture.width);
        let h = usize::from(texture.height);
        let ow = w * if mu { 2 } else { 1 };
        let oh = h * if mv { 2 } else { 1 };
        let mut rgba = vec![0; ow * oh * 4];
        for y in 0..oh {
            for x in 0..ow {
                let sx = if x >= w { 2 * w - 1 - x } else { x };
                let sy = if y >= h { 2 * h - 1 - y } else { y };
                let a = (y * ow + x) * 4;
                let b = (sy * w + sx) * 4;
                rgba[a..a + 4].copy_from_slice(&texture.pixels[b..b + 4]);
            }
        }
        (rgba, ow, oh)
    }
    fn part(actor: &Actor, mesh: usize, refs: Vec<usize>, tris: Vec<u16>) -> Result<Part, String> {
        let material = usize::from(actor.meshes[mesh].material);
        let n = refs.len();
        let mut v = vec![0x29u32 | (((1 + n * 6) as u32) << 16), (n as u32) << 16];
        for _ in 0..n {
            v.extend([0., 0., 0., 0., 1., 0.].map(f32::to_bits));
        }
        v.push(0xff);
        let mut v = v.into_boxed_slice();
        let mat = &actor.materials[material];
        let alpha = actor.textures[usize::from(mat.texture)]
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] != 255);
        let packet = encode_polygons(&actor.meshes[mesh], &refs, &tris, mat, alpha)?;
        let uv_slots = packet.uv_slots;
        let mut p = packet.words.into_boxed_slice();
        let model = Box::new([
            v.as_mut_ptr() as u32,
            p.as_mut_ptr() as u32,
            0,
            0,
            0,
            0, // Match the known-good owned skin path: zero bypasses native sphere culling.
        ]);
        let mut object = Box::new([0u32; 13]);
        object[0] = 0x17;
        object[1] = model.as_ptr() as u32;
        object[8] = 1f32.to_bits();
        object[9] = 1f32.to_bits();
        object[10] = 1f32.to_bits();
        Ok(Part {
            object,
            _model: model,
            vertices: v,
            _polygons: p,
            refs,
            mesh,
            material,
            uv_slots,
            uv: mat.uv,
        })
    }
    #[derive(Debug)]
    pub(super) struct PreparedTextures {
        archives: Vec<(Vec<u8>, bool)>,
        material_textures: Vec<usize>,
        byte_size: usize,
    }
    // CPU-only work: safe on the bounded reader threads, with no game/GPU access.
    fn prepare_textures(actor: &Actor) -> Result<PreparedTextures, String> {
        let mut result = PreparedTextures {
            archives: Vec::new(),
            material_textures: Vec::new(),
            byte_size: 0,
        };
        let mut shared = HashMap::new();
        for material in &actor.materials {
            let key = (
                material.texture,
                material.wrap[0] == 3,
                material.wrap[1] == 3,
            );
            if let Some(&index) = shared.get(&key) {
                result.material_textures.push(index);
                continue;
            }
            let bytes = actor.textures[usize::from(material.texture)].pixels.len()
                * if key.1 { 2 } else { 1 }
                * if key.2 { 2 } else { 1 };
            result.byte_size += bytes;
            if result.byte_size > 32 << 20 {
                return Err("3D creature exceeds its texture memory budget".into());
            }
            let (rgba, w, h) = baked(&actor.textures[usize::from(material.texture)], material);
            let alpha = rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 255);
            let archive = crate::creature_link::billboard::gvm(&[rgba], w, h, 0);
            let index = result.archives.len();
            result.archives.push((archive, alpha));
            result.material_textures.push(index);
            shared.insert(key, index);
        }
        Ok(result)
    }
    fn prepare_parts(actor: &Actor) -> Result<Vec<Part>, String> {
        let mut parts = Vec::new();
        for (mi, mesh) in actor.meshes.iter().enumerate() {
            let mut map = HashMap::new();
            let mut refs = Vec::new();
            let mut tris = Vec::new();
            for tri in mesh.indices.as_chunks::<3>().0 {
                let additional = tri.iter().filter(|i| !map.contains_key(*i)).count();
                if refs.len() + additional > 500 {
                    parts.push(part(
                        actor,
                        mi,
                        std::mem::take(&mut refs),
                        std::mem::take(&mut tris),
                    )?);
                    map.clear();
                }
                for &j in tri {
                    let next = refs.len() as u16;
                    let local = *map.entry(j).or_insert_with(|| {
                        refs.push(j as usize);
                        next
                    });
                    tris.push(local);
                }
            }
            if !tris.is_empty() {
                parts.push(part(actor, mi, refs, tris)?);
            }
        }
        Ok(parts)
    }
    /// Resident bytes of a prepared model once built: its cache data, matrices and packets,
    /// and its native textures (counted at their RGBA size).
    fn resident_size(prepared: &PreparedActor) -> usize {
        let actor = &prepared.actor;
        prepared.file_size
            + prepared.inverse_bind.len() * std::mem::size_of::<Mat>()
            + actor.bones.len() * (std::mem::size_of::<Bone>() + 4 * std::mem::size_of::<Mat>())
            + actor
                .meshes
                .iter()
                .map(|m| m.vertices.len() * std::mem::size_of::<SkinnedVertex>() + 1)
                .sum::<usize>()
            + prepared.textures.byte_size
            + prepared
                .parts
                .iter()
                .map(|p| p.vertices.len() * 4 + p._polygons.len() * 4)
                .sum::<usize>()
    }
    /// Evicts the least recently drawn models (never one drawn within `RECENT`) until one of
    /// `size` bytes and `records` textures fits. False: it doesn't fit yet.
    fn make_room(c: &mut Cache, size: usize, records: usize) -> bool {
        let now = Instant::now();
        loop {
            let fits = c.loaded.len() < LOADED
                && c.loaded.iter().map(|l| l.size).sum::<usize>() + size <= RESIDENT
                && c.loaded.iter().map(|l| l.textures.len()).sum::<usize>() + records
                    <= TEXTURE_RECORDS;
            if fits {
                return true;
            }
            let Some(i) = (0..c.loaded.len())
                .filter(|&i| now.saturating_duration_since(c.loaded[i].used) >= RECENT)
                .min_by_key(|&i| c.loaded[i].used)
            else {
                return false;
            };
            c.loaded.swap_remove(i);
        }
    }
    /// Logs `message` for model `id` unless it was the last one logged for it.
    fn log_once(c: &mut Cache, id: &Id, message: &str, wait: Option<Duration>) {
        if c.logged.get(id).is_some_and(|m| m == message) {
            return;
        }
        match wait {
            Some(wait) => crate::log!(
                "3D cache: species {} form {}: {message} (tried again in {} s)",
                id.1,
                id.2,
                wait.as_secs()
            ),
            None => crate::log!(
                "3D cache: species {} form {}: {message} (waiting for room)",
                id.1,
                id.2
            ),
        }
        c.logged.insert(id.clone(), message.to_owned());
    }
    fn fail(c: &mut Cache, id: Id, message: String, wait: Duration) -> String {
        log_once(c, &id, &message, Some(wait));
        c.failed.insert(
            id,
            Failure {
                message: message.clone(),
                retry: Instant::now() + wait,
            },
        );
        message
    }
    fn build(prepared: PreparedActor, id: Id, serial: &mut u32) -> Result<Loaded, String> {
        let size = resident_size(&prepared);
        let PreparedActor {
            actor,
            motions,
            inverse_bind,
            placement,
            parts,
            textures,
            ..
        } = prepared;
        let PreparedTextures {
            archives,
            material_textures,
            ..
        } = textures;
        let mut textures = Vec::with_capacity(archives.len());
        for (mut archive, alpha) in archives {
            let gbix = 0x7e60_0000u32.wrapping_add(*serial & 0xfffff);
            *serial = serial.wrapping_add(1);
            // Single-entry GVMH global index. All pixel conversion is already complete.
            archive[14..18].copy_from_slice(&gbix.to_be_bytes());
            let Some(t) = Textures::from_gvm(archive, 1, alpha) else {
                for texture in textures {
                    Textures::release(&texture);
                }
                return Err("Not enough native texture space for the 3D creature".into());
            };
            textures.push(t);
        }
        Ok(Loaded {
            id,
            actor,
            motions,
            inverse_bind,
            parts,
            textures,
            material_textures,
            used: Instant::now(),
            size,
            placement,
            skinning: SkinningBuffers::default(),
            last_pose: None,
        })
    }
    /// A reader thread's work: everything about a model short of its native textures.
    fn read(path: &Path, species: u16, form: u16, shiny: bool) -> Result<PreparedActor, String> {
        let mut actor = if species == 0 {
            load_venue(path)
        } else {
            load_actor(path, species, form, shiny)
        }?;
        let motions = if species == 0 {
            None
        } else {
            match super::super::locomotion::MotionSet::load_for(path, &actor) {
                Ok(motions) => motions,
                Err(error) => {
                    crate::log!(
                        "3D motions: species {species} form {form}: {error}; using source idle"
                    );
                    None
                }
            }
        };
        let inverse_bind: Vec<Mat> = actor
            .bind_matrices()
            .into_iter()
            .map(Mat::inverse)
            .collect();
        let placement =
            super::super::placement::Placement::for_actor(&actor, motions.as_ref(), &inverse_bind)?;
        let length = |file: PathBuf| std::fs::metadata(file).map_or(0, |m| m.len() as usize);
        let file_size = if species == 0 {
            length(path.join("venue/default.pka"))
        } else {
            length(actor_path(path, species, form, shiny))
        } + if motions.is_some() {
            length(super::super::locomotion::file_path(path, species, form))
        } else {
            0
        };
        let parts = prepare_parts(&actor)?;
        let textures = prepare_textures(&actor)?;
        // The archives hold the textures now; the RGBA copies aren't needed.
        for texture in &mut actor.textures {
            texture.pixels = Vec::new();
        }
        Ok(PreparedActor {
            actor,
            motions,
            inverse_bind,
            placement,
            file_size,
            parts,
            textures,
        })
    }
    pub fn prepare(root: &Path, species: u16, form: u16, shiny: bool) -> Result<bool, String> {
        with(|c| {
            if let Some(index) = c.loaded.iter().position(|l| {
                l.id.0.as_path() == root && l.id.1 == species && l.id.2 == form && l.id.3 == shiny
            }) {
                // Native records can be reused during scene changes. Validate this actor
                // before drawing; unrelated cached actors need no per-draw scan.
                if c.loaded[index].textures.iter().all(Textures::ours) {
                    return Ok(true);
                }
                c.loaded.swap_remove(index);
            }
            let id = (root.to_owned(), species, form, shiny);
            let now = Instant::now();
            if let Some(failure) = c.failed.get(&id) {
                if now < failure.retry {
                    return Err(failure.message.clone());
                }
                c.failed.remove(&id);
            }
            if let Some(pending) = c.pending.get_mut(&id) {
                pending.requested = now;
                let tick = crate::render_tick();
                // One upload per frame; the read waits meanwhile.
                if c.upload_tick == Some(tick) || !pending.finished() {
                    return Ok(false);
                }
                let Some(mut read) = c.pending.remove(&id) else {
                    return Ok(false);
                };
                let Some(result) = read.done.take() else {
                    return Ok(false);
                };
                let prepared = match result {
                    Ok(prepared) => prepared,
                    Err(e) => return Err(fail(c, id, e, FAILURE_RETRY)),
                };
                let size = resident_size(&prepared);
                let records = prepared.textures.archives.len();
                if size > RESIDENT || records > TEXTURE_RECORDS {
                    let message = "3D model exceeds the renderer memory budget".to_string();
                    return Err(fail(c, id, message, FAILURE_RETRY));
                }
                if !make_room(c, size, records) {
                    // Every model in the cache is on screen: this one shows its picture
                    // rather than pushing one of them out. Its finished read waits for room,
                    // tried again each time it's asked for, without reading it anew.
                    let message = "3D model cache is full".to_string();
                    log_once(c, &id, &message, None);
                    read.done = Some(Ok(prepared));
                    c.pending.insert(id, read);
                    return Err(message);
                }
                c.upload_tick = Some(tick);
                return match build(prepared, id.clone(), &mut c.serial) {
                    Ok(loaded) => {
                        crate::log!(
                            "3D cache: prepared species {species} form {form}, shiny {shiny} ({} KiB)",
                            loaded.size / 1024
                        );
                        c.loaded.push(loaded);
                        c.logged.remove(&id);
                        Ok(true)
                    }
                    Err(e) => Err(fail(c, id, e, RESOURCE_RETRY)),
                };
            }
            c.pending.retain(|_, pending| !pending.obsolete(now));
            if c.pending
                .values_mut()
                .map(|p| usize::from(!p.finished()))
                .sum::<usize>()
                >= READERS
            {
                return Ok(false);
            }
            let (tx, rx) = mpsc::channel();
            let path = root.to_owned();
            let spawned = std::thread::Builder::new()
                .name("3d-model-reader".into())
                .spawn(move || {
                    let _ = tx.send(read(&path, species, form, shiny));
                });
            if let Err(error) = spawned {
                let message = format!("Cannot start a 3D model reader: {error}");
                return Err(fail(c, id, message, RESOURCE_RETRY));
            }
            c.pending.insert(id, PendingRead::new(rx));
            Ok(false)
        })
    }
    // Internal adapter adds the selected cache root to the public pose arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        root: &Path,
        species: u16,
        form: u16,
        shiny: bool,
        position: [f32; 3],
        yaw: i32,
        seconds: f32,
        height: f32,
        playback: Option<&super::super::locomotion::Sample>,
    ) -> Result<(), String> {
        let started = Instant::now();
        if !prepare(root, species, form, shiny)? {
            return Err(LOADING.into());
        }
        let ready = Instant::now();
        with(|c| {
            let loaded = c
                .loaded
                .iter_mut()
                .find(|l| {
                    l.id.0.as_path() == root
                        && l.id.1 == species
                        && l.id.2 == form
                        && l.id.3 == shiny
                })
                .ok_or("3D creature cache changed")?;
            loaded.used = Instant::now();
            let authored = playback.and_then(|sample| {
                loaded
                    .motions
                    .as_ref()
                    .and_then(|motions| motions.clip(sample.role))
            });
            let clip = authored.or(loaded.actor.idle.as_ref());
            let seconds = match playback {
                Some(sample) if authored.is_some() => sample.seconds(clip.expect("authored clip")),
                Some(sample) => sample.idle_seconds,
                None => seconds,
            };
            let key = PoseKey {
                field: playback
                    .filter(|_| authored.is_some())
                    .map(|sample| sample.role),
                seconds: if clip.is_some() { seconds.to_bits() } else { 0 },
                height: height.to_bits(),
            };
            if loaded.last_pose != Some(key) {
                loaded.last_pose = None;
                loaded
                    .actor
                    .skin_into(seconds, clip, &loaded.inverse_bind, &mut loaded.skinning);
                if loaded
                    .skinning
                    .meshes
                    .iter()
                    .flatten()
                    .any(|(p, n)| p.iter().chain(n).any(|v| !v.is_finite() || v.abs() > 1e6))
                {
                    return Err("Source animation pose exceeds safe rendering bounds".into());
                }
                let s = height
                    / if species == 0 {
                        loaded.placement.width
                    } else {
                        loaded.placement.height.max(loaded.placement.width * 0.5)
                    };
                let reference = key.field.map_or(loaded.placement.battle, |role| {
                    loaded.placement.field[role as usize]
                });
                loaded.skinning.material_uv.clear();
                for material in 0..loaded.actor.materials.len() {
                    let uv = loaded.actor.material_uv_with_clip(material, seconds, clip);
                    loaded.skinning.material_uv.push(uv);
                }
                for part in &mut loaded.parts {
                    if !loaded.skinning.visible[part.mesh] {
                        continue;
                    }
                    for (i, &j) in part.refs.iter().enumerate() {
                        let (p, n) = loaded.skinning.meshes[part.mesh][j];
                        let at = 2 + i * 6;
                        for k in 0..3 {
                            part.vertices[at + k] = ((p[k] - reference[k]) * s).to_bits();
                            part.vertices[at + 3 + k] = n[k].to_bits();
                        }
                    }
                    let uv = loaded.skinning.material_uv[part.material];
                    if part.uv != uv {
                        let uv_transform = UvTransform::new(uv);
                        let material = &loaded.actor.materials[part.material];
                        for &(at, vertex) in &part.uv_slots {
                            let uv = uv_transform
                                .point(loaded.actor.meshes[part.mesh].vertices[vertex].uv);
                            let values = [
                                uv[0] / if material.wrap[0] == 3 { 2. } else { 1. },
                                (1. - uv[1]) / if material.wrap[1] == 3 { 2. } else { 1. },
                            ];
                            for (i, v) in values.into_iter().enumerate() {
                                if !v.is_finite() {
                                    return Err(
                                        "Source material animation exceeds safe bounds".into()
                                    );
                                }
                                let at = at + i;
                                let packed =
                                    (v * 256.).round().clamp(-32768., 32767.) as i16 as u16;
                                let word = &mut part._polygons[at / 2];
                                let shift = (at & 1) * 16;
                                *word = (*word & !(65535 << shift)) | (u32::from(packed) << shift);
                            }
                        }
                        part.uv = uv;
                    }
                }
                loaded.last_pose = Some(key);
            }
            let posed = Instant::now();
            let mut drawn_parts = 0;
            for part in &loaded.parts {
                if !loaded.skinning.visible[part.mesh] {
                    continue;
                }
                // prepare validated this actor's native texture records on the same game
                // thread. Drawing its checked packets cannot replace those records.
                let texture = &loaded.textures[loaded.material_textures[part.material]];
                // SAFETY: own bounded geometry, live scene-owned textures, game display thread.
                unsafe {
                    crate::puppet::draw_independent_solid(
                        part.object.as_ptr() as usize,
                        texture.list(),
                        position,
                        [0, 0x8000i32.wrapping_sub(yaw), 0],
                        1.,
                        crate::puppet::Tilt::default(),
                        None,
                    );
                }
                drawn_parts += 1;
            }
            if drawn_parts == 0 {
                return Err("Active source motion has no visible mesh".into());
            }
            c.timing.record(
                crate::render_tick(),
                ready.duration_since(started).as_micros(),
                posed.duration_since(ready).as_micros(),
                posed.elapsed().as_micros(),
            );
            Ok(())
        })
    }
}
