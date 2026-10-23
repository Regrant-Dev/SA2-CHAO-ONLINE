//! A trainer's creatures in the arena: each stands where a Chao would, drawn as its ROM picture
//! (`gba_link::billboard`) on the far side of the ring. The director's motions for it become
//! simple moves of the picture: a hop for an action, a shake when hit, a slide down into the
//! floor when it faints.
//!
//! `Figure` is the animation (no game access); the game side keeps one per slot and draws them
//! from a display object of its own in the karate area while a trainer battle is on.

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
/// The picture's quad stands this far below the position it's given.
const BILLBOARD_DROP: f32 = 1.0;

/// What a creature's picture is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Anim {
    Still,
    Hop(u16),
    Shake(u16),
    /// Frames into its fall; it stays down at the end.
    Faint(u16),
}

/// One creature's picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Figure {
    pub species: u16,
    /// Where it stands (the director's place, with its lift).
    pub at: [f32; 3],
    anim: Anim,
}

impl Figure {
    pub fn new(species: u16, at: [f32; 3]) -> Figure {
        Figure {
            species,
            at,
            anim: Anim::Still,
        }
    }

    /// A Chao motion the director asked for, as a move of the picture.
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

    /// Where its picture is drawn now (the billboard's own position, which draws the quad's
    /// foot a little lower).
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
        [x + dx, y + dy + BILLBOARD_DROP, z]
    }
}

#[cfg(target_arch = "x86")]
pub use game::{clear, motion as play, place, show};

#[cfg(not(target_arch = "x86"))]
pub fn show(_slot: Slot, _species: Option<u16>, _at: [f32; 3]) {}
#[cfg(not(target_arch = "x86"))]
pub fn place(_slot: Slot, _at: [f32; 3]) {}
#[cfg(not(target_arch = "x86"))]
pub fn play(_slot: Slot, _id: u16) -> bool {
    false
}
#[cfg(not(target_arch = "x86"))]
pub fn clear(_area_gone: bool) {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;

    use super::{Figure, Slot};

    const NAME: &[u8] = b"SA2ChaoOnlineBattleFigures\0";
    const DELETE_OBJECT: usize = 0x0046_F720;
    const OBJ_MAIN: usize = 0x10;

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

    /// The creature `species` stands at `slot` (`None`: nobody, or a Chao, stands there).
    pub fn show(slot: Slot, species: Option<u16>, at: [f32; 3]) {
        let mut f = lock();
        if let Some(s) = slot_mut(&mut f, slot) {
            *s = species.map(|sp| Figure::new(sp, at));
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
            let figures: Vec<Figure> = lock().slots.iter().flatten().flatten().copied().collect();
            for fig in figures.iter().filter(|f| !f.gone()) {
                crate::gba_link::billboard::draw(fig.species, fig.draw_at(), false);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_hop_hits_shake_and_a_faint_goes_down_for_good() {
        let mut f = Figure::new(5, [5.0, 8.0, 19.0]);
        assert_eq!(f.draw_at(), [5.0, 8.0 + BILLBOARD_DROP, 19.0]);
        f.motion(motion::ATTACKS[0]);
        for _ in 0..HOP_FRAMES / 2 {
            f.tick();
        }
        assert!(f.draw_at()[1] > 8.0 + BILLBOARD_DROP + HOP_HEIGHT * 0.9);
        for _ in 0..HOP_FRAMES {
            f.tick();
        }
        assert_eq!(f.draw_at()[1], 8.0 + BILLBOARD_DROP);

        f.motion(motion::HIT[1]);
        f.tick();
        assert_ne!(f.draw_at()[0], 5.0);
        assert_eq!(f.draw_at()[1], 8.0 + BILLBOARD_DROP);
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
        assert!(f.draw_at()[1] < 8.0 + BILLBOARD_DROP - FAINT_DEPTH + 0.01);
        // Nothing brings it back but a new figure.
        f.motion(motion::ATTACKS[0]);
        assert!(f.gone());
    }
}
