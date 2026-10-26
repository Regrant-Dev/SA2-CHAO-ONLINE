//! Locally converted 3D creatures in the owned battle scene. Their source skeletal idle
//! runs in `creature::assets`; this module adds explicit procedural battle movement for cues
//! without claiming that a source clip represents a particular authored move.

// The display object only exists in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use super::event::Slot;
use super::visuals::motion;

/// A hop's frames and height.
const HOP_FRAMES: u16 = 16;
const HOP_HEIGHT: f32 = 0.9;
/// A shake's frames, size and speed (frames per swing).
const SHAKE_FRAMES: u16 = 18;
const SHAKE_SIZE: f32 = 0.35;
const SHAKE_SWING: u16 = 3;
/// A faint's frames and depth (the picture's height: gone below the floor).
const FAINT_FRAMES: u16 = 24;
const FAINT_DEPTH: f32 = 6.2;

/// Procedural movement applied to a source 3D actor.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Anim {
    Still,
    Hop(u16),
    Shake(u16),
    /// Frames into its fall; it stays down at the end.
    Faint(u16),
}

/// One creature's position and battle movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Figure {
    pub species: u16,
    pub form: u8,
    pub shiny: bool,
    /// Where it stands (the director's place, with its lift).
    pub at: [f32; 3],
    anim: Anim,
}

impl Figure {
    pub fn new(species: u16, shiny: bool, at: [f32; 3]) -> Figure {
        Figure {
            species,
            form: 0,
            shiny,
            at,
            anim: Anim::Still,
        }
    }

    /// A director cue mapped to procedural movement beside the real source idle.
    pub fn motion(&mut self, id: u16) {
        if matches!(self.anim, Anim::Faint(_)) {
            return;
        }
        self.anim = if id == motion::DOWN || motion::LOST.contains(&id) {
            Anim::Faint(0)
        } else if motion::HIT.contains(&id) || id == motion::SHUDDER || id == motion::STUMBLE {
            Anim::Shake(0)
        } else if id == motion::STANCE || id == motion::SLEEP || id == motion::DRAW {
            // Resting poses leave whatever move it's in to finish.
            self.anim
        } else {
            Anim::Hop(0)
        };
    }

    /// One frame on.
    pub fn tick(&mut self) {
        self.anim = match self.anim {
            Anim::Hop(f) if f + 1 >= HOP_FRAMES => Anim::Still,
            Anim::Hop(f) => Anim::Hop(f + 1),
            Anim::Shake(f) if f + 1 >= SHAKE_FRAMES => Anim::Still,
            Anim::Shake(f) => Anim::Shake(f + 1),
            Anim::Faint(f) => Anim::Faint((f + 1).min(FAINT_FRAMES)),
            Anim::Still => Anim::Still,
        };
    }

    /// Fallen all the way (not drawn).
    pub fn gone(&self) -> bool {
        self.anim == Anim::Faint(FAINT_FRAMES)
    }

    /// Its world position with procedural movement; feet stay exactly on the scene floor.
    pub fn draw_at(&self) -> [f32; 3] {
        let [x, y, z] = self.at;
        let (dx, dy) = match self.anim {
            Anim::Still => (0.0, 0.0),
            Anim::Hop(f) => {
                let t = f32::from(f) / f32::from(HOP_FRAMES);
                (0.0, HOP_HEIGHT * 4.0 * t * (1.0 - t))
            }
            Anim::Shake(f) => {
                let swing = if (f / SHAKE_SWING).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                let fade = 1.0 - f32::from(f) / f32::from(SHAKE_FRAMES);
                (swing * SHAKE_SIZE * fade, 0.0)
            }
            Anim::Faint(f) => (0.0, -FAINT_DEPTH * f32::from(f) / f32::from(FAINT_FRAMES)),
        };
        [x + dx, y + dy, z]
    }
}

#[cfg(target_arch = "x86")]
pub use game::{clear, motion as play, place, ready, show, show_form};

#[cfg(not(target_arch = "x86"))]
pub fn show(_slot: Slot, _species: Option<u16>, _shiny: bool, _at: [f32; 3]) {}
#[cfg(not(target_arch = "x86"))]
pub fn show_form(_slot: Slot, _species: Option<u16>, _form: u8, _shiny: bool, _at: [f32; 3]) {}
#[cfg(not(target_arch = "x86"))]
pub fn place(_slot: Slot, _at: [f32; 3]) {}
#[cfg(not(target_arch = "x86"))]
pub fn play(_slot: Slot, _id: u16) -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn clear(_area_gone: bool) {}
#[cfg(not(target_arch = "x86"))]
pub fn ready() -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;
    use std::time::Instant;

    use super::{Figure, Slot};

    const NAME: &[u8] = b"SA2ChaoOnlineBattleFigures\0";
    const DELETE_OBJECT: usize = 0x0046_F720;
    const OBJ_MAIN: usize = 0x10;
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

    struct Figures {
        slots: [[Option<Figure>; 2]; 2],
        object: usize,
    }

    static FIGURES: Mutex<Figures> = Mutex::new(Figures {
        slots: [[None; 2]; 2],
        object: 0,
    });

    fn lock() -> std::sync::MutexGuard<'static, Figures> {
        FIGURES.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn slot_mut(f: &mut Figures, slot: Slot) -> Option<&mut Option<Figure>> {
        f.slots.get_mut(slot.side)?.get_mut(slot.pos)
    }

    /// The creature `species` (`shiny`: in its shiny colours) stands at `slot` (`None`:
    /// nobody, or a Chao, stands there).
    pub fn show(slot: Slot, species: Option<u16>, shiny: bool, at: [f32; 3]) {
        show_form(slot, species, 0, shiny, at);
    }
    pub fn show_form(slot: Slot, species: Option<u16>, form: u8, shiny: bool, at: [f32; 3]) {
        let mut f = lock();
        if let Some(s) = slot_mut(&mut f, slot) {
            *s = species.map(|sp| {
                let mut f = Figure::new(sp, shiny, at);
                f.form = form;
                f
            });
        }
        if species.is_some() && f.object == 0 {
            f.object = crate::puppet::create_display_object(NAME, display, deleted);
        }
    }

    /// Where the creature at `slot` stands this frame; a frame of its animation.
    pub fn place(slot: Slot, at: [f32; 3]) {
        let mut f = lock();
        if let Some(Some(fig)) = slot_mut(&mut f, slot) {
            fig.at = at;
            fig.tick();
        }
    }

    /// A motion for the creature at `slot`; false when no creature stands there.
    pub fn motion(slot: Slot, id: u16) -> bool {
        let mut f = lock();
        match slot_mut(&mut f, slot) {
            Some(Some(fig)) => {
                fig.motion(id);
                true
            }
            _ => false,
        }
    }

    /// The scene can wait on real 3D asset reads, and show a concrete import error.
    pub fn ready() -> Result<bool, String> {
        let figures: Vec<Figure> = lock().slots.iter().flatten().flatten().copied().collect();
        let mut ready = true;
        for figure in figures {
            ready &= crate::creature::assets::prepare(
                figure.species,
                u16::from(figure.form),
                figure.shiny,
            )?;
        }
        Ok(ready)
    }

    /// The battle is over (`area_gone`: the area and its objects went with it).
    pub fn clear(area_gone: bool) {
        let mut f = lock();
        f.slots = [[None; 2]; 2];
        if !area_gone && f.object != 0 {
            // SAFETY: our live display object; its MainSub deletes it next frame.
            unsafe { ((f.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
        }
        f.object = 0;
    }

    extern "C" fn deleted(object: usize) {
        let _ = std::panic::catch_unwind(|| {
            let mut f = lock();
            if f.object == object {
                f.object = 0;
            }
        });
    }

    extern "C" fn display(_object: usize) {
        crate::watch::mark("battle figures");
        let _ = std::panic::catch_unwind(|| {
            let figures: Vec<(usize, Figure)> = lock()
                .slots
                .iter()
                .enumerate()
                .flat_map(|(side, slots)| slots.iter().flatten().map(move |f| (side, *f)))
                .collect();
            let seconds = START.get_or_init(Instant::now).elapsed().as_secs_f32();
            for (side, fig) in figures.iter().filter(|(_, f)| !f.gone()) {
                let at = fig.draw_at();
                let yaw = if *side == 0 { 0x4000 } else { 0xc000 };
                let _ = crate::creature::assets::draw(
                    fig.species,
                    u16::from(fig.form),
                    fig.shiny,
                    at,
                    yaw,
                    seconds,
                    6.0 * crate::creature_link::visual_size(fig.species, fig.form),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_hop_hits_shake_and_a_faint_goes_down_for_good() {
        let mut f = Figure::new(5, false, [5.0, 8.0, 19.0]);
        assert_eq!(f.draw_at(), [5.0, 8.0, 19.0]);
        f.motion(motion::ATTACKS[0]);
        for _ in 0..HOP_FRAMES / 2 {
            f.tick();
        }
        assert!(f.draw_at()[1] > 8.0 + HOP_HEIGHT * 0.9);
        for _ in 0..HOP_FRAMES {
            f.tick();
        }
        assert_eq!(f.draw_at()[1], 8.0);

        f.motion(motion::HIT[1]);
        f.tick();
        assert_ne!(f.draw_at()[0], 5.0);
        assert_eq!(f.draw_at()[1], 8.0);
        // A rest pose doesn't stop the shake.
        f.motion(motion::STANCE);
        for _ in 0..SHAKE_FRAMES {
            f.tick();
        }
        assert_eq!(f.draw_at()[0], 5.0);

        f.motion(motion::DOWN);
        assert!(!f.gone());
        for _ in 0..FAINT_FRAMES * 2 {
            f.tick();
        }
        assert!(f.gone());
        assert!(f.draw_at()[1] < 8.0 - FAINT_DEPTH + 0.01);
        // Nothing brings it back but a new figure.
        f.motion(motion::ATTACKS[0]);
        assert!(f.gone());
    }
}
