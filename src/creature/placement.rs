//! Constant model-space placement references for source motions with different origins.
//! References move the authored animation as a whole; they never ground each animated frame.

use super::assets::{Actor, Clip, MAX_VERTICES, Mat};
use super::locomotion::{MotionRole, MotionSet};

const MAX_SAMPLES: usize = 128;
const MAX_VERTEX_SAMPLES: usize = 4_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub battle: [f32; 3],
    /// Idle, walk, run references. Shared/fallback clips share the same reference.
    pub field: [[f32; 3]; 3],
    pub height: f32,
    pub width: f32,
}

#[derive(Clone, Copy)]
struct Bounds {
    lo: [f32; 3],
    hi: [f32; 3],
}
impl Bounds {
    fn point(p: [f32; 3]) -> Result<Self, String> {
        if p.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
            return Err("Source placement pose exceeds safe rendering bounds".into());
        }
        Ok(Self { lo: p, hi: p })
    }
    fn include(&mut self, p: [f32; 3]) -> Result<(), String> {
        Self::point(p)?;
        for (axis, value) in p.into_iter().enumerate() {
            self.lo[axis] = self.lo[axis].min(value);
            self.hi[axis] = self.hi[axis].max(value);
        }
        Ok(())
    }
    fn reference(self) -> [f32; 3] {
        [
            (self.lo[0] + self.hi[0]) * 0.5,
            self.lo[1],
            (self.lo[2] + self.hi[2]) * 0.5,
        ]
    }
}

impl Placement {
    /// Compute once while preparing the actor, away from native texture/model creation.
    pub fn for_actor(
        actor: &Actor,
        motions: Option<&MotionSet>,
        inverse_bind: &[Mat],
    ) -> Result<Self, String> {
        actor.validate()?;
        if inverse_bind.len() != actor.bones.len() {
            return Err("Placement bind palette differs from the actor skeleton".into());
        }
        if let Some(motions) = motions {
            motions.validate_for(actor)?;
        }
        let vertices: usize = actor.meshes.iter().map(|m| m.vertices.len()).sum();
        if vertices == 0 || vertices > MAX_VERTICES {
            return Err("Source placement vertex count exceeds its limit".into());
        }
        // A submesh can share a larger vertex buffer. Unreferenced points must never pull
        // the floor below its real body, and repeated triangle indices need only one visit.
        let refs: Vec<Vec<usize>> = actor
            .meshes
            .iter()
            .map(|mesh| {
                let mut seen = vec![false; mesh.vertices.len()];
                mesh.indices
                    .iter()
                    .filter_map(|&index| {
                        let index = index as usize;
                        if seen[index] {
                            None
                        } else {
                            seen[index] = true;
                            Some(index)
                        }
                    })
                    .collect()
            })
            .collect();
        let basis = match bounds_at(actor, actor.idle.as_ref(), 0., inverse_bind, &refs)? {
            Some(bounds) => bounds,
            // A fully hidden opening frame must not panic or invent visible source meshes.
            // Bind geometry supplies only a stable reference; render visibility is unchanged.
            None => bounds_at(actor, None, 0., inverse_bind, &refs)?
                .ok_or("Source actor has no indexed placement geometry")?,
        };
        let mut battle = basis.reference();
        if actor.species == 0 {
            battle[1] = 0.;
        }
        let mut result = Self {
            battle,
            field: [battle; 3],
            height: (basis.hi[1] - basis.lo[1]).max(1.),
            width: (basis.hi[0] - basis.lo[0])
                .max(basis.hi[2] - basis.lo[2])
                .max(1.),
        };
        if actor.species == 0 {
            return Ok(result);
        }
        let roles = [MotionRole::Idle, MotionRole::Walk, MotionRole::Run];
        let clips = roles.map(|role| motions.and_then(|m| m.clip(role)));
        for (slot, clip) in clips.into_iter().enumerate() {
            let Some(clip) = clip else {
                continue;
            };
            if let Some(previous) =
                (0..slot).find(|&i| clips[i].is_some_and(|other| std::ptr::eq(other, clip)))
            {
                result.field[slot] = result.field[previous];
                continue;
            }
            let mut first_visible: Option<Bounds> = None;
            let mut floor = f32::INFINITY;
            for frame in sample_frames(clip.frames, vertices) {
                if let Some(bounds) =
                    bounds_at(actor, Some(clip), frame / 30., inverse_bind, &refs)?
                {
                    if first_visible.is_none() {
                        first_visible = Some(bounds);
                    }
                    floor = floor.min(bounds.lo[1]);
                }
            }
            if let Some(first) = first_visible {
                let mut reference = first.reference();
                reference[1] = floor;
                result.field[slot] = reference;
            }
            // A wholly invisible clip has no visible floor. Preserve the existing battle
            // reference and its authored invisibility, without indexing empty collections.
        }
        Ok(result)
    }
}

fn bounds_at(
    actor: &Actor,
    clip: Option<&Clip>,
    seconds: f32,
    inverse_bind: &[Mat],
    refs: &[Vec<usize>],
) -> Result<Option<Bounds>, String> {
    let positions = actor.positions_with_clip(seconds, clip, inverse_bind);
    if positions.len() != actor.meshes.len() {
        return Err("Placement mesh count changed".into());
    }
    let mut result: Option<Bounds> = None;
    for (mesh, indices) in refs.iter().enumerate() {
        if !actor.visible_with_clip(mesh, seconds, clip) {
            continue;
        }
        for &index in indices {
            let point = *positions[mesh]
                .get(index)
                .ok_or("Placement vertex index exceeds pose")?;
            match &mut result {
                Some(bounds) => bounds.include(point)?,
                None => result = Some(Bounds::point(point)?),
            }
        }
    }
    Ok(result)
}

/// Integer source frames remain exact for normal short clips. Remaining capacity samples
/// half-frames to include interpolation dips, while long clips use evenly spaced frames.
fn sample_frames(frames: u16, vertices: usize) -> Vec<f32> {
    let frames = usize::from(frames).max(1);
    let budget = (MAX_VERTEX_SAMPLES / vertices.max(1)).clamp(1, MAX_SAMPLES);
    if frames > budget {
        if budget == 1 {
            return vec![0.];
        }
        return (0..budget)
            .map(|i| (i * (frames - 1) / (budget - 1)) as f32)
            .collect();
    }
    let mut samples: Vec<f32> = (0..frames).map(|i| i as f32).collect();
    let halves = (budget - frames).min(frames);
    for i in 0..halves {
        samples.push((i * frames / halves) as f32 + 0.5);
    }
    samples.sort_by(f32::total_cmp);
    samples.dedup();
    samples
}

#[cfg(test)]
mod tests {
    use super::super::assets::{
        Bone, BoneTrack, Key, Material, Mesh, Texture, Vertex, VisibilityTrack,
    };
    use super::*;
    fn actor() -> Actor {
        let vertex = |p| Vertex {
            position: p,
            normal: [0., 1., 0.],
            uv: [0.; 2],
            joints: [0; 4],
            weights: [1., 0., 0., 0.],
        };
        Actor {
            species: 25,
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
                wrap: [0; 2],
            }],
            textures: vec![Texture {
                width: 8,
                height: 8,
                pixels: vec![255; 8 * 8 * 4],
            }],
            meshes: vec![Mesh {
                name: "body".into(),
                material: 0,
                vertices: vec![
                    vertex([-1., 0., -1.]),
                    vertex([1., 0., -1.]),
                    vertex([0., 2., 1.]),
                ],
                indices: vec![0, 1, 2],
            }],
            idle: None,
            notices: Vec::new(),
        }
    }
    fn clip(y: &[(u16, f32)]) -> Clip {
        let mut channels: [Vec<Key>; 9] = std::array::from_fn(|_| Vec::new());
        channels[7] = y
            .iter()
            .map(|&(frame, value)| Key {
                frame,
                value,
                slope: 0.,
            })
            .collect();
        Clip {
            frames: 4,
            looping: true,
            tracks: vec![BoneTrack {
                bone: 0,
                axis_angle: false,
                channels,
            }],
            visibility: Vec::new(),
            materials: Vec::new(),
        }
    }
    fn bind(actor: &Actor) -> Vec<Mat> {
        actor
            .bind_matrices()
            .into_iter()
            .map(Mat::inverse)
            .collect()
    }
    #[test]
    fn translated_field_origins_align_without_flattening_the_authored_hop() {
        let mut actor = actor();
        actor.idle = Some(clip(&[(0, 100.), (4, 100.)]));
        let moving = clip(&[(0, -40.), (1, -30.), (2, -40.), (3, -20.), (4, -40.)]);
        let motions = MotionSet::new(&actor, None, Some(moving.clone()), None).unwrap();
        let bind = bind(&actor);
        let place = Placement::for_actor(&actor, Some(&motions), &bind).unwrap();
        assert_eq!(place.battle, [0., 100., 0.]);
        assert_eq!(place.field[1], [0., -40., 0.]);
        let mut lowest = f32::INFINITY;
        let mut highest = f32::NEG_INFINITY;
        let base = actor.positions_with_clip(0., Some(&moving), &bind)[0][0][1];
        for frame in 0..4 {
            let raw = actor.positions_with_clip(frame as f32 / 30., Some(&moving), &bind)[0][0][1];
            let shifted = raw - place.field[1][1];
            assert!((shifted - (raw - base)).abs() < 1e-4);
            lowest = lowest.min(shifted);
            highest = highest.max(shifted);
        }
        assert_eq!(lowest, 0.);
        assert_eq!(highest - lowest, 20.);
    }
    #[test]
    fn unused_vertices_do_not_change_model_floor_or_scale() {
        let mut actor = actor();
        let mut unused = actor.meshes[0].vertices[0].clone();
        unused.position = [5000., -1000., 2000.];
        actor.meshes[0].vertices.push(unused);
        let place = Placement::for_actor(&actor, None, &bind(&actor)).unwrap();
        assert_eq!(place.battle, [0.; 3]);
        assert_eq!(place.height, 2.);
        assert_eq!(place.width, 2.);
    }
    #[test]
    fn venue_keeps_its_authored_vertical_origin() {
        let mut actor = actor();
        actor.species = 0;
        for vertex in &mut actor.meshes[0].vertices {
            vertex.position[1] += 50.;
        }
        let place = Placement::for_actor(&actor, None, &bind(&actor)).unwrap();
        assert_eq!(place.battle, [0.; 3]);
        assert_eq!(place.field, [place.battle; 3]);
        assert_eq!(place.height, 2.);
    }
    #[test]
    fn missing_field_motions_and_shared_run_fallback_keep_the_correct_reference() {
        let actor = actor();
        let bind = bind(&actor);
        let plain = Placement::for_actor(&actor, None, &bind).unwrap();
        assert_eq!(plain.field, [plain.battle; 3]);
        let motions =
            MotionSet::new(&actor, None, None, Some(clip(&[(0, -15.), (4, -15.)]))).unwrap();
        let place = Placement::for_actor(&actor, Some(&motions), &bind).unwrap();
        assert_eq!(place.field[0], place.battle);
        assert_eq!(place.field[1], place.field[2]);
        assert_eq!(place.field[2], [0., -15., 0.]);
    }
    #[test]
    fn wholly_hidden_clips_have_a_safe_reference_without_forcing_visibility() {
        let actor = actor();
        let bind = bind(&actor);
        let mut invisible = clip(&[(0, -10.), (4, -10.)]);
        invisible.visibility.push(VisibilityTrack {
            mesh: "body".into(),
            bits: vec![0],
        });
        let motions = MotionSet::new(&actor, None, Some(invisible.clone()), None).unwrap();
        let place = Placement::for_actor(&actor, Some(&motions), &bind).unwrap();
        assert_eq!(place.field[1], place.battle);
        assert!(!actor.visible_with_clip(0, 0., Some(&invisible)));
        let mut hidden_actor = actor.clone();
        hidden_actor.idle = Some(invisible);
        let reference = Placement::for_actor(&hidden_actor, None, &bind).unwrap();
        assert_eq!(reference.battle, [0.; 3]);
    }
    #[test]
    fn clip_sampling_is_bounded_and_short_loops_include_every_frame_and_half_frame() {
        let short = sample_frames(62, 5000);
        assert_eq!(short.len(), 124);
        for frame in 0..62 {
            assert!(short.contains(&(frame as f32)));
            assert!(short.contains(&(frame as f32 + 0.5)));
        }
        let long = sample_frames(20_000, 100_000);
        assert_eq!(long.len(), 40);
        assert_eq!(long[0], 0.);
        assert_eq!(*long.last().unwrap(), 19999.);
        assert!(long.len() * 100_000 <= MAX_VERTEX_SAMPLES);
    }
    #[test]
    fn malformed_bind_palette_is_rejected_before_sampling() {
        assert!(Placement::for_actor(&actor(), None, &[]).is_err());
    }
}
