//! Optional source-authored field motions and distance-driven per-entity playback.
//! Motion sidecars are private cache data; existing PKA2 actors remain readable.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::assets::{Actor, BoneTrack, Clip, Key, MaterialTrack, VisibilityTrack};

pub const MAGIC: &[u8; 4] = b"PKL1";
pub const MAX_FILE: usize = 8 << 20;
const MAX_FRAMES: u16 = 20_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionRole {
    #[default]
    Idle,
    Walk,
    Run,
}

#[derive(Clone, Debug)]
pub struct MotionSet {
    pub species: u16,
    pub form: u16,
    pub bone_count: u16,
    pub skeleton: [u8; 32],
    pub idle: Option<Clip>,
    pub walk: Option<Clip>,
    pub run: Option<Clip>,
}

pub fn file_path(root: &Path, species: u16, form: u16) -> PathBuf {
    root.join("motions")
        .join(format!("{species:04}_{form:02}.pkl"))
}

pub fn skeleton_hash(actor: &Actor) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update((actor.bones.len() as u16).to_le_bytes());
    for bone in &actor.bones {
        hash.update(bone.parent.to_le_bytes());
        for value in bone
            .scale
            .into_iter()
            .chain(bone.rotation)
            .chain(bone.translation)
        {
            hash.update(value.to_bits().to_le_bytes());
        }
    }
    hash.finalize().into()
}

impl MotionSet {
    pub fn new(
        actor: &Actor,
        idle: Option<Clip>,
        walk: Option<Clip>,
        run: Option<Clip>,
    ) -> Result<Self, String> {
        let result = Self {
            species: actor.species,
            form: actor.form,
            bone_count: actor.bones.len() as u16,
            skeleton: skeleton_hash(actor),
            idle,
            walk,
            run,
        };
        result.validate_for(actor)?;
        Ok(result)
    }

    pub fn clip(&self, role: MotionRole) -> Option<&Clip> {
        match role {
            MotionRole::Idle => self.idle.as_ref(),
            MotionRole::Walk => self.walk.as_ref().or(self.run.as_ref()),
            MotionRole::Run => self.run.as_ref().or(self.walk.as_ref()),
        }
    }

    pub fn validate_for(&self, actor: &Actor) -> Result<(), String> {
        if self.species != actor.species
            || self.form != actor.form
            || usize::from(self.bone_count) != actor.bones.len()
            || self.skeleton != skeleton_hash(actor)
            || self.bone_count > 512
        {
            return Err("Field motions do not match the actor identity or skeleton".into());
        }
        for clip in [&self.idle, &self.walk, &self.run].into_iter().flatten() {
            validate_clip(clip, usize::from(self.bone_count))?;
            if clip
                .visibility
                .iter()
                .any(|t| !actor.meshes.iter().any(|m| m.name == t.mesh))
                || clip
                    .materials
                    .iter()
                    .any(|t| !actor.materials.iter().any(|m| m.name == t.material))
            {
                return Err("Field motions reference an unknown mesh or material".into());
            }
        }
        if [&self.walk, &self.run]
            .into_iter()
            .flatten()
            .any(|c| !c.looping || c.frames < 2)
        {
            return Err("Locomotion must use a looping source motion".into());
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        if !(1..=807).contains(&self.species) || self.form > 255 || self.bone_count > 512 {
            return Err("Invalid field motion identity".into());
        }
        let clips = [&self.idle, &self.walk, &self.run];
        let size = 47usize.saturating_add(
            clips
                .into_iter()
                .flatten()
                .map(encoded_clip_size)
                .fold(0usize, usize::saturating_add),
        );
        if size > MAX_FILE {
            return Err("Field motion sidecar exceeds its size limit".into());
        }
        let mut bytes = Vec::with_capacity(size);
        bytes.extend(MAGIC);
        bytes.extend(self.species.to_le_bytes());
        bytes.extend(self.form.to_le_bytes());
        bytes.extend(self.bone_count.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(self.skeleton);
        for clip in [&self.idle, &self.walk, &self.run] {
            bytes.push(u8::from(clip.is_some()));
            let Some(clip) = clip else {
                continue;
            };
            validate_clip(clip, usize::from(self.bone_count))?;
            bytes.extend(clip.frames.to_le_bytes());
            bytes.push(u8::from(clip.looping));
            bytes.extend((clip.tracks.len() as u16).to_le_bytes());
            for track in &clip.tracks {
                bytes.extend(track.bone.to_le_bytes());
                bytes.push(u8::from(track.axis_angle));
                for channel in &track.channels {
                    encode_keys(&mut bytes, channel);
                }
            }
            bytes.extend((clip.visibility.len() as u16).to_le_bytes());
            for track in &clip.visibility {
                encode_text(&mut bytes, &track.mesh);
                bytes.extend((track.bits.len() as u32).to_le_bytes());
                bytes.extend(&track.bits);
            }
            bytes.extend((clip.materials.len() as u16).to_le_bytes());
            for track in &clip.materials {
                encode_text(&mut bytes, &track.material);
                for channel in &track.channels {
                    encode_keys(&mut bytes, channel);
                }
            }
            if bytes.len() > MAX_FILE {
                return Err("Field motion sidecar exceeds its size limit".into());
            }
        }
        Ok(bytes)
    }

    pub fn decode_for(bytes: &[u8], actor: &Actor) -> Result<Self, String> {
        if bytes.len() > MAX_FILE {
            return Err("Field motion sidecar exceeds its size limit".into());
        }
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != MAGIC {
            return Err("Unsupported field motion cache format".into());
        }
        let species = r.u16()?;
        let form = r.u16()?;
        let bone_count = r.u16()?;
        if r.u16()? != 0 || !(1..=807).contains(&species) || form > 255 || bone_count > 512 {
            return Err("Invalid field motion cache header".into());
        }
        let skeleton = r
            .take(32)?
            .try_into()
            .map_err(|_| "Truncated field skeleton hash")?;
        let idle = r.clip(usize::from(bone_count))?;
        let walk = r.clip(usize::from(bone_count))?;
        let run = r.clip(usize::from(bone_count))?;
        if r.at != bytes.len() {
            return Err("Unexpected trailing field motion data".into());
        }
        let result = Self {
            species,
            form,
            bone_count,
            skeleton,
            idle,
            walk,
            run,
        };
        result.validate_for(actor)?;
        Ok(result)
    }

    /// Missing sidecars preserve compatibility with existing private PKA2 caches. Malformed
    /// or mismatched sidecars are an error, rather than posing the wrong actor skeleton.
    pub fn load_for(root: &Path, actor: &Actor) -> Result<Option<Self>, String> {
        if actor.species == 0 {
            return Ok(None);
        }
        let path = file_path(root, actor.species, actor.form);
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("Cannot read field motion cache: {e}")),
        };
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_FILE as u64 {
            return Err("Field motion sidecar exceeds its size limit".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Self::decode_for(&bytes, actor).map(Some)
    }
}

fn valid_keys(keys: &[Key], frames: u16) -> bool {
    keys.len() <= usize::from(frames) + 1
        && !keys.windows(2).any(|p| p[0].frame >= p[1].frame)
        && keys.iter().all(|k| {
            k.frame <= frames
                && k.value.is_finite()
                && k.value.abs() <= 1e6
                && k.slope.is_finite()
                && k.slope.abs() <= 1e6
        })
}
fn validate_clip(clip: &Clip, bones: usize) -> Result<(), String> {
    if clip.frames == 0
        || clip.frames > MAX_FRAMES
        || clip.tracks.len() > bones
        || clip.visibility.len() > 512
        || clip.materials.len() > 64
    {
        return Err("Invalid field motion counts".into());
    }
    let mut seen = std::collections::HashSet::new();
    for track in &clip.tracks {
        if usize::from(track.bone) >= bones
            || !seen.insert(track.bone)
            || track.channels.iter().any(|c| !valid_keys(c, clip.frames))
        {
            return Err("Invalid field bone track".into());
        }
    }
    let valid_name = |s: &str| !s.is_empty() && s.len() <= 512;
    let mut seen = std::collections::HashSet::new();
    for t in &clip.visibility {
        if !valid_name(&t.mesh)
            || !seen.insert(t.mesh.as_str())
            || t.bits.len() != (usize::from(clip.frames) + 1).div_ceil(8)
        {
            return Err("Invalid field visibility track".into());
        }
    }
    let mut seen = std::collections::HashSet::new();
    for t in &clip.materials {
        if !valid_name(&t.material)
            || !seen.insert(t.material.as_str())
            || t.channels.iter().any(|c| !valid_keys(c, clip.frames))
        {
            return Err("Invalid field material track".into());
        }
    }
    Ok(())
}
fn encoded_clip_size(clip: &Clip) -> usize {
    let keys = |channels: &[Vec<Key>]| {
        channels
            .iter()
            .map(|c| 2usize.saturating_add(c.len().saturating_mul(10)))
            .fold(0usize, usize::saturating_add)
    };
    let mut size = 9usize;
    for t in &clip.tracks {
        size = size.saturating_add(3).saturating_add(keys(&t.channels));
    }
    for t in &clip.visibility {
        size = size
            .saturating_add(6)
            .saturating_add(t.mesh.len())
            .saturating_add(t.bits.len());
    }
    for t in &clip.materials {
        size = size
            .saturating_add(2)
            .saturating_add(t.material.len())
            .saturating_add(keys(&t.channels));
    }
    size
}

fn encode_text(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend((text.len() as u16).to_le_bytes());
    bytes.extend(text.as_bytes());
}
fn encode_keys(bytes: &mut Vec<u8>, keys: &[Key]) {
    bytes.extend((keys.len() as u16).to_le_bytes());
    for k in keys {
        bytes.extend(k.frame.to_le_bytes());
        bytes.extend(k.value.to_le_bytes());
        bytes.extend(k.slope.to_le_bytes());
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self
            .at
            .checked_add(n)
            .ok_or("Field motion offset overflow")?;
        let out = self
            .bytes
            .get(self.at..end)
            .ok_or("Truncated field motion cache")?;
        self.at = end;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn flag(&mut self) -> Result<bool, String> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("Invalid field motion flag".into()),
        }
    }
    fn float(&mut self) -> Result<f32, String> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        if !v.is_finite() || v.abs() > 1e6 {
            return Err("Invalid field motion float".into());
        }
        Ok(v)
    }
    fn text(&mut self) -> Result<String, String> {
        let n = usize::from(self.u16()?);
        if n == 0 || n > 512 {
            return Err("Invalid field motion name length".into());
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "Invalid field motion name".into())
    }
    fn keys(&mut self, frames: u16) -> Result<Vec<Key>, String> {
        let n = usize::from(self.u16()?);
        if n > usize::from(frames) + 1 || n * 10 > self.bytes.len().saturating_sub(self.at) {
            return Err("Invalid field motion key count".into());
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(Key {
                frame: self.u16()?,
                value: self.float()?,
                slope: self.float()?,
            });
        }
        if !valid_keys(&out, frames) {
            return Err("Invalid field motion key ordering".into());
        }
        Ok(out)
    }
    fn clip(&mut self, bones: usize) -> Result<Option<Clip>, String> {
        if !self.flag()? {
            return Ok(None);
        }
        let frames = self.u16()?;
        if frames == 0 || frames > MAX_FRAMES {
            return Err("Invalid field motion duration".into());
        }
        let looping = self.flag()?;
        let n = usize::from(self.u16()?);
        if n > bones {
            return Err("Too many field bone tracks".into());
        }
        let mut tracks = Vec::with_capacity(n);
        for _ in 0..n {
            let bone = self.u16()?;
            let axis_angle = self.flag()?;
            let mut channels: [Vec<Key>; 9] = std::array::from_fn(|_| Vec::new());
            for channel in &mut channels {
                *channel = self.keys(frames)?;
            }
            tracks.push(BoneTrack {
                bone,
                axis_angle,
                channels,
            });
        }
        let n = usize::from(self.u16()?);
        if n > 512 {
            return Err("Too many field visibility tracks".into());
        }
        let mut visibility = Vec::with_capacity(n);
        for _ in 0..n {
            let mesh = self.text()?;
            let length = self.u32()? as usize;
            if length != (usize::from(frames) + 1).div_ceil(8) {
                return Err("Invalid field visibility length".into());
            }
            visibility.push(VisibilityTrack {
                mesh,
                bits: self.take(length)?.to_vec(),
            });
        }
        let n = usize::from(self.u16()?);
        if n > 64 {
            return Err("Too many field material tracks".into());
        }
        let mut materials = Vec::with_capacity(n);
        for _ in 0..n {
            let material = self.text()?;
            let mut channels: [Vec<Key>; 5] = std::array::from_fn(|_| Vec::new());
            for channel in &mut channels {
                *channel = self.keys(frames)?;
            }
            materials.push(MaterialTrack { material, channels });
        }
        let clip = Clip {
            frames,
            looping,
            tracks,
            visibility,
            materials,
        };
        validate_clip(&clip, bones)?;
        Ok(Some(clip))
    }
}

/// One entity's motion clock. A shared model never determines an entity's stride phase.
#[derive(Clone, Debug, Default)]
pub struct Playback {
    position: Option<[f32; 3]>,
    time: Option<f32>,
    cycles: f32,
    idle_seconds: f32,
    speed: f32,
    role: MotionRole,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub role: MotionRole,
    pub cycles: f32,
    pub idle_seconds: f32,
    pub yaw: i32,
}
impl Sample {
    pub fn seconds(self, clip: &Clip) -> f32 {
        if self.role == MotionRole::Idle {
            self.idle_seconds
        } else {
            self.cycles * f32::from(clip.frames) / 30.
        }
    }
}
impl Playback {
    pub fn update(
        &mut self,
        now: f32,
        position: [f32; 3],
        heading: i32,
        held: bool,
        height: f32,
    ) -> Sample {
        let valid = now.is_finite()
            && position.iter().all(|p| p.is_finite())
            && height.is_finite()
            && height > 0.;
        if !valid {
            *self = Self::default();
            return Sample {
                yaw: heading,
                ..Sample::default()
            };
        }
        let previous = self.position;
        let last = self.time;
        let dt = last.map_or(0., |last| now - last);
        // A second render pass must not reset the entity's gait or consume its distance.
        if last.is_some() && dt == 0. {
            if held {
                self.role = MotionRole::Idle;
                self.speed = 0.;
            }
            return Sample {
                role: self.role,
                cycles: self.cycles,
                idle_seconds: self.idle_seconds,
                yaw: heading,
            };
        }
        let delta = previous.map_or(0., |p| {
            ((position[0] - p[0]).powi(2) + (position[2] - p[2]).powi(2)).sqrt()
        });
        // Native opaque/shadow display passes can be a millisecond apart. Do not turn
        // an unchanged second pass into an idle frame or consume the next movement delta.
        if last.is_some() && dt > 0. && dt < 1. / 120. && delta < 0.0001 {
            if held {
                self.role = MotionRole::Idle;
                self.speed = 0.;
            }
            return Sample {
                role: self.role,
                cycles: self.cycles,
                idle_seconds: self.idle_seconds,
                yaw: heading,
            };
        }
        self.position = Some(position);
        self.time = Some(now);
        let discontinuity = !dt.is_finite()
            || !delta.is_finite()
            || dt <= 0.
            || dt > 0.5
            || delta > height.max(1.) * 4.;
        if discontinuity {
            self.speed = 0.;
            self.role = MotionRole::Idle;
            self.cycles = 0.;
        } else {
            self.idle_seconds = (self.idle_seconds + dt) % 3600.;
            let instantaneous = delta / dt;
            self.speed += (instantaneous - self.speed) * (dt / 0.08).min(1.);
            if held || delta < 0.0001 {
                self.speed = 0.;
                self.role = MotionRole::Idle;
            } else {
                let threshold = if self.role == MotionRole::Idle {
                    0.15
                } else {
                    0.08
                };
                self.role = if self.speed < threshold {
                    MotionRole::Idle
                } else if self.speed > height * 0.8 {
                    MotionRole::Run
                } else {
                    MotionRole::Walk
                };
                if self.role != MotionRole::Idle {
                    let stride = height
                        * if self.role == MotionRole::Run {
                            0.45
                        } else {
                            0.28
                        };
                    self.cycles = (self.cycles + delta / stride.max(0.1)) % 1.;
                }
            }
        }
        // A held actor stays in an authored idle, regardless of the carrier's speed.
        if held {
            self.role = MotionRole::Idle;
            self.speed = 0.;
        }
        Sample {
            role: self.role,
            cycles: self.cycles,
            idle_seconds: self.idle_seconds,
            yaw: heading,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn actor() -> Actor {
        Actor {
            species: 25,
            form: 0,
            shiny: false,
            bones: vec![super::super::assets::Bone {
                parent: -1,
                scale: [1.; 3],
                rotation: [0.; 3],
                translation: [0.; 3],
            }],
            materials: vec![super::super::assets::Material {
                name: "body".into(),
                texture: 0,
                uv: [1., 1., 0., 0., 0.],
                wrap: [0; 2],
            }],
            textures: Vec::new(),
            meshes: vec![super::super::assets::Mesh {
                name: "body".into(),
                material: 0,
                vertices: Vec::new(),
                indices: Vec::new(),
            }],
            idle: None,
            notices: Vec::new(),
        }
    }
    fn clip() -> Clip {
        Clip {
            frames: 30,
            looping: true,
            tracks: vec![BoneTrack {
                bone: 0,
                axis_angle: false,
                channels: std::array::from_fn(|i| {
                    if i == 6 {
                        vec![
                            Key {
                                frame: 0,
                                value: 0.,
                                slope: 0.,
                            },
                            Key {
                                frame: 30,
                                value: 1.,
                                slope: 0.,
                            },
                        ]
                    } else {
                        Vec::new()
                    }
                }),
            }],
            visibility: vec![VisibilityTrack {
                mesh: "body".into(),
                bits: vec![255; 4],
            }],
            materials: Vec::new(),
        }
    }
    #[test]
    fn sidecars_bind_to_skeleton_and_preserve_real_channels() {
        let actor = actor();
        let set = MotionSet::new(&actor, Some(clip()), Some(clip()), None).unwrap();
        let bytes = set.encode().unwrap();
        let decoded = MotionSet::decode_for(&bytes, &actor).unwrap();
        assert_eq!(decoded.walk.unwrap().tracks[0].channels[6][1].value, 1.);
        let mut other = actor.clone();
        other.bones[0].translation[1] = 2.;
        assert!(MotionSet::decode_for(&bytes, &other).is_err());
        for n in 0..bytes.len() {
            assert!(MotionSet::decode_for(&bytes[..n], &actor).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(MotionSet::decode_for(&trailing, &actor).is_err());
    }
    #[test]
    fn sidecars_reject_bad_bone_indices_and_key_values() {
        let actor = actor();
        let mut bad = clip();
        bad.tracks[0].bone = 1;
        assert!(MotionSet::new(&actor, None, Some(bad), None).is_err());
        let mut bad = clip();
        bad.tracks[0].channels[6][0].value = f32::NAN;
        assert!(MotionSet::new(&actor, None, Some(bad), None).is_err());
    }
    #[test]
    fn stride_phase_follows_distance_not_idle_time_or_frame_rate() {
        let mut a = Playback::default();
        let mut b = Playback::default();
        a.update(0., [0.; 3], 0, false, 6.);
        b.update(0., [0.; 3], 0, false, 6.);
        let mut x = Sample::default();
        let mut y = Sample::default();
        for frame in 1..=60 {
            let t = frame as f32 / 60.;
            x = a.update(t, [t * 2., 0., 0.], 0x4000, false, 6.);
        }
        for frame in 1..=30 {
            let t = frame as f32 / 30.;
            y = b.update(t, [t * 2., 0., 0.], 0x4000, false, 6.);
        }
        assert_eq!(x.role, MotionRole::Walk);
        assert!((x.cycles - y.cycles).abs() < 0.002);
        assert_eq!(x.yaw, 0x4000);
        let phase = x.cycles;
        let still = a.update(1.1, [2., 0., 0.], 0, false, 6.);
        assert_eq!(still.role, MotionRole::Idle);
        assert_eq!(still.cycles, phase);
    }
    #[test]
    fn repeated_render_passes_and_invalid_frames_do_not_poison_playback() {
        let mut p = Playback::default();
        p.update(0., [0.; 3], 0, false, 6.);
        let moving = p.update(0.1, [0.2, 0., 0.], 0x4000, false, 6.);
        let repeated = p.update(0.1, [0.2, 0., 0.], 0x4000, false, 6.);
        assert_eq!(moving, repeated);
        let invalid = p.update(f32::NAN, [0.; 3], 0, false, 6.);
        assert_eq!(invalid.role, MotionRole::Idle);
        assert!(invalid.cycles.is_finite());
        p.update(1., [0.; 3], 0, false, 6.);
        let resumed = p.update(1.1, [0.2, 0., 0.], 0x4000, false, 6.);
        assert_eq!(resumed.role, MotionRole::Walk);
        assert!(resumed.cycles.is_finite());
    }

    #[test]
    fn a_second_shadow_pass_does_not_interrupt_a_moving_stride() {
        let mut p = Playback::default();
        p.update(0., [0.; 3], 0, false, 6.);
        let moving = p.update(0.1, [0.2, 0., 0.], 0x4000, false, 6.);
        let shadow = p.update(0.101, [0.2, 0., 0.], 0x4000, false, 6.);
        assert_eq!(moving, shadow);
        let next = p.update(0.1167, [0.234, 0., 0.], 0x4000, false, 6.);
        assert_eq!(next.role, MotionRole::Walk);
        assert!(next.cycles > moving.cycles);
        assert_eq!(
            p.update(0.1334, [0.234, 0., 0.], 0x4000, false, 6.).role,
            MotionRole::Idle
        );
    }

    #[test]
    fn carrying_and_teleports_do_not_become_running_steps() {
        let mut p = Playback::default();
        p.update(0., [0.; 3], 0, false, 6.);
        assert_eq!(
            p.update(0.1, [4., 0., 0.], 123, true, 6.).role,
            MotionRole::Idle
        );
        let jump = p.update(0.2, [100., 1000., 0.], 456, false, 6.);
        assert_eq!(jump.role, MotionRole::Idle);
        assert_eq!(jump.cycles, 0.);
        assert_eq!(jump.yaw, 456);
    }
}
