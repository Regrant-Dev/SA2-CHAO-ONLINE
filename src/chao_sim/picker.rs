//! The picker (`0x562A80`): when a behaviour asks what next, every candidate scores itself
//! against the best so far and the winner installs its behaviour; nothing over 0.2 means
//! wander (re/chao-ai.md §1.5). Candidates run in the game's order, consume the game's
//! random rolls and keep its quirks.

use super::brain::{Behaviour, Chao, Link, Rng};
use super::land::{Garden, V3};
use super::record::{TYPE_CHILD, off};

/// What a Chao perceives around it. The server fills this in as the garden's objects and
/// players come in; until then a Chao is alone with its needs.
pub trait Senses {
    /// A signal of `kind` (0xA5..0xA8, 0xAB, 0xC9, 0xCA) it touches (`0x486880`).
    fn signal(&self, _kind: u8) -> Option<Signal> {
        None
    }
    /// The player it perceives, as a bond index (`D+0x7E4`, `0x53E0C0`).
    fn player_seen(&self) -> Option<usize> {
        None
    }
    /// Player 1's position (`*0x1DEA6C0` `+0x14`), seen or not: the behaviours that go to or
    /// from the player read it straight off the player object.
    fn player_pos(&self) -> Option<V3> {
        None
    }
    /// The nearest fruit (`D+0xF14`..).
    fn nearest_fruit(&self) -> Option<Fruit> {
        None
    }
    /// Whether it sees a tree (list 3, `D+0x12AC`).
    fn tree_seen(&self) -> bool {
        self.nearest_tree().is_some()
    }
    /// The nearest tree it sees (list 3, index `D+0x12BC`).
    fn nearest_tree(&self) -> Option<Tree> {
        None
    }
    /// A fruit by id, while it is in the garden: the one a Chao is linked to or holds.
    fn fruit(&self, _id: u32) -> Option<Fruit> {
        None
    }
    /// The fruit it touches (`0x486760(obj, 0x83)`).
    fn touching_fruit(&self) -> Option<Fruit> {
        None
    }
    /// The game's local player's character (`*(*0x1DE9600 + 1)`): it signs the alignment
    /// change of `AddHappiness` (`0x536320`) and picks the bond (`0x53E0C0`). None: no
    /// alignment change, bond 0.
    fn player_character(&self) -> Option<u8> {
        None
    }
    /// The nearest toy (`0x590D20`): its kind (ALW `+4`) and position.
    fn nearest_toy(&self) -> Option<(u8, V3)> {
        None
    }
    /// The garden's toy of `kind` (ALW category 6): 2 the TV (`*0x1AED288`), 3 the radio
    /// (`*0x1AED2E0`), 7 the horse, 8 the box.
    fn toy(&self, _kind: u8) -> Option<Toy> {
        None
    }
    /// The object of `kind` it touches (`0x486760`, `hit+4`), e.g. 0xCB a ledge it can
    /// climb: its position. For items (0x83 fruit, 0x85 seed, 0x87 animal, 0x93 hat) only
    /// ones no Chao holds (ALW `+6 & 2`), and for seeds none another Chao is after
    /// (`0x5304D0`). 0xA2 is the object `0x56A110`'s collision (`0x564760`).
    fn touch(&self, _kind: u8) -> Option<V3> {
        None
    }
    /// Chao in this garden (`0x5319F0`).
    fn chao_count(&self) -> usize {
        1
    }
    /// Song objects alive (`*0x1AEDC00`, counted up by `0x5AB200`): a Chao won't start a
    /// song while there are 100.
    fn songs(&self) -> usize {
        0
    }
    /// Player 1's facing, BAMS (`*0x1DEA6C0 + 0xC`): a held Chao turns with its holder.
    fn player_rot_y(&self) -> Option<u16> {
        None
    }
    /// Player 1 is whistling (its action byte `*0x1DEA6C0 == 0x2A`, `0x5637B0`); the
    /// brain checks the range from `player_pos`.
    fn whistling(&self) -> bool {
        false
    }
    /// The garden's ball (`*0x1DBE570`, `*0x1DBE574`, `*0x1DBE56C`).
    fn ball(&self) -> Option<Ball> {
        None
    }
    /// How player 1 let go of this Chao, asked on the frame `Chao::held` goes false while it
    /// is in a held behaviour (`0x565DC0`).
    fn release(&self) -> Release {
        Release::PutDown
    }
    /// A player's hand on it, petting (signal and touch kind 2: `0x54B1F0(2)`,
    /// `0x486760(obj, 2)`).
    fn petted(&self) -> bool {
        false
    }
    /// A player's attack landed on it this frame (Status `D+4 & 4`, set by the collision):
    /// where the attacker is (`0x486800(obj, 1)`).
    fn hit_by(&self) -> Option<V3> {
        None
    }
    /// What it holds in its hands (its ALW link `+0x40`, `0x56CFF0`), once the server has
    /// linked an item it took (`brain::ItemEvent::Take`).
    fn carrying(&self) -> Option<Carried> {
        None
    }
    /// An empty tree spot whose planting sphere it touches (`0x54B1F0(1, 0xAC)`): GrowTree
    /// collision entry 3, kind 0xAC, r 30 around the tree, on only while the spot is empty
    /// (tree mode 0; planting turns it off, `0x54686D`, and withering back to 0 on again).
    /// Its position (tree `Data1+0x14`); the seed behaviours measure their own distance.
    fn plant_spot(&self) -> Option<V3> {
        None
    }
    /// A tree whose watering sphere it touches (`0x486880(obj, 1, 0xAE)` with `hit+4` set):
    /// GrowTree collision entry 5, kind 0xAE, r 20, on from planting until the seed has
    /// been watered (mode 1), and for a grown tree (modes 4, 7) once its life `Data1+0x3C`
    /// is under 6000, until watered (0xAD) again. Its position (tree `Data1+0x14`).
    fn watering_spot(&self) -> Option<V3> {
        None
    }
    /// A free slot in the save's hat list (24 of 0x14 at `0x19F6460 + 0x4D8`, free when the
    /// first word is 0xFFFF): without one a knocked Chao keeps its hat.
    fn hat_room(&self) -> bool {
        false
    }
}

/// The garden's ball.
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub pos: V3,
    /// Rolling (`*0x1DBE574`: kicked and faster than 0.4).
    pub moving: bool,
    /// Held (`Data1+4 & 0x8000`).
    pub held: bool,
    /// In the water this frame (`*0x1DBE56C`).
    pub in_water: bool,
}

/// How the player let go of a held Chao (`0x565DC0`). The server writes a throw's velocity
/// into `body.vel` when it lets go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// Player control off (`*0x174AFFE == 0`): it hops down (`0x53D720`).
    NoControl,
    /// The player's action list has 0x14, the put-down: it settles (`0x565BE0`).
    PutDown,
    /// Otherwise dropped or thrown: it lands upset (`0x564320`).
    Dropped,
}

/// An item in a Chao's hands.
#[derive(Clone, Copy, Debug)]
pub struct Carried {
    /// The touch kind it was taken by: 0x83 fruit, 0x85 seed, 0x87 animal or drive, 0x93 hat.
    pub touch: u8,
    /// Its kind (`Data1+8`; a hat's is what `rec+0xD5` takes when worn).
    pub kind: u8,
}

/// Nothing around.
pub struct Alone;

impl Senses for Alone {}

#[derive(Clone, Copy, Debug)]
pub struct Signal {
    pub pos: V3,
    /// The object that sent it (`hit+4`, then `+8`): its position and what it's doing.
    pub parent: Option<(V3, u32)>,
}

/// A garden toy as the Chao sees it.
#[derive(Clone, Copy, Debug)]
pub struct Toy {
    /// `Data1+0x14` and facing `Data1+0xC` (BAMS).
    pub pos: V3,
    pub rot_y: u16,
    /// Linked with another Chao (its ALW `+0x40` set and not this Chao): the horse's rider.
    pub taken: bool,
}

/// What a Chao did to a garden toy, for the server to apply to the toy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToyEvent {
    /// Signal 0xAF (`0x55A920`) at `pos`, `radius` wide, for `frames` frames. A toy it
    /// touches that is off switches on: TV and radio mode 0 → 1 (`0x55C540`, `0x57C840`),
    /// box mode 0 → 1 (`0x5801C0`); the horse at rest rocks for 1800 frames (`0x580BC0`
    /// mode 0 → 1, motion 3). `toy` is the kind it meant (its target or link).
    Press {
        toy: Option<u8>,
        pos: V3,
        radius: f32,
        frames: u16,
    },
    /// It linked itself with toy `kind` both ways (`0x530640`): the toy's partner is now
    /// this Chao, until its `toy_link` clears. The horse stops rocking (motion 4, mode 0)
    /// and unlinks when its partner's link isn't mutual (`0x580C0A`).
    Link(u8),
}

#[derive(Clone, Copy, Debug)]
pub struct Tree {
    pub pos: V3,
    /// Its distance in the perception list (entry `+8`).
    pub dist: f32,
    /// Its growth state (tree `Data1+0x31`).
    pub state: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Fruit {
    /// The server's id for it.
    pub id: u32,
    /// Its kind (`D+8`), the row of the fruit table (re/chao-garden-items.md §1.4).
    pub kind: u8,
    /// Bites left (`D+0x10`): 4 for a whole fruit.
    pub bites: u8,
    pub pos: V3,
    /// Someone holds it (`D+4 & 0x8000`).
    pub held: bool,
    /// Another Chao has it in mind (ALW `+0x40` set and not this Chao).
    pub taken: bool,
}

/// `(v − t)/(10000 − t)` above `t`, else 0 (`0x562440`).
pub(super) fn f16(v: u16, t: i32) -> f32 {
    let v = i32::from(v as i16);
    if v > t {
        (v - t) as f32 / (10_000 - t) as f32
    } else {
        0.0
    }
}

/// The same over 200 (`0x562490`).
fn f8(v: u8, t: i32) -> f32 {
    let v = i32::from(v);
    if v > t {
        (v - t) as f32 / (200 - t) as f32
    } else {
        0.0
    }
}

/// `s·(0.4 + 0.6r)` (`0x5624D0`).
pub(super) fn jitter(s: f32, rng: &mut Rng) -> f32 {
    s * 0.4 + rng.r() * (1.0 - 0.4) * s
}

/// Illness behaviours by slot (`0x11C9BF4`).
const ILLNESS: [u32; 6] = [0x599D40, 0x599F30, 0x59A0B0, 0x59A260, 0x59A400, 0x59A5A0];

/// Think-timer slots (`D+0x1D4 + 4·i`).
pub(super) const TIMER_TOY: usize = 8;
pub(super) const TIMER_FRUIT: usize = 14;
pub(super) const TIMER_DROWSY: usize = 15;

impl Chao {
    /// Standing still on the ground (`0x54AC20`).
    pub(super) fn standing(&self) -> bool {
        let v = self.body.vel;
        self.body.on_ground() && (v[0] * v[0] + v[2] * v[2]).sqrt() < 0.04
    }

    pub(super) fn choose(&mut self, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) {
        let mut best = 0.2f32;
        let child = self.rec.u8(off::TYPE) == TYPE_CHILD;
        self.illness(&mut best, rng);
        self.drowsy(&mut best, rng);
        self.deep_sleep(&mut best, rng);
        self.go_to_bed(&mut best, rng);
        self.hunger(&mut best, senses, rng);
        self.thirst(&mut best, rng);
        // #7..#10: watch or join another Chao's performance (acts/play.rs).
        self.join_skill_a(&mut best, senses, rng);
        self.join_skill_b(&mut best, senses, rng);
        self.watch_skill_c(&mut best, senses, rng);
        self.scorn_dark_trick(&mut best, senses, rng);
        // #22..#24 (drink from a Chao, lonely, mate) need a signal or a player: they come
        // in with `Senses`.
        // #11 `0x599A10` TV and #12 `0x5998C0` radio.
        super::acts::join_toy(self, &mut best, 0xC9, senses, rng);
        super::acts::join_toy(self, &mut best, 0xCA, senses, rng);
        self.toy(&mut best, garden, senses, rng);
        self.learned_play(&mut best, senses, rng);
        self.tired(&mut best, rng);
        self.scared(&mut best, senses, rng);
        self.sad(&mut best);
        self.angry(&mut best, rng);
        self.happy(&mut best, rng);
        self.evolve(&mut best);
        if !child {
            self.mischief(&mut best, senses, rng);
            self.water_tree(&mut best, senses, rng);
            // #23 `0x5A04A0` lonely and #24 `0x562950` courter come in with `Senses`.
            self.breed(&mut best, senses);
            self.death(&mut best);
        }
        if best <= 0.2 {
            self.set(Behaviour::Wander, -1);
        }
    }

    /// #1 `0x599CB0`: the last ill slot whose roll beats CONDITN wins.
    fn illness(&mut self, best: &mut f32, rng: &mut Rng) {
        if *best >= 1.0 {
            return;
        }
        let condition = f32::from(self.rec.u16(off::CONDITN));
        for (i, behaviour) in ILLNESS.into_iter().enumerate() {
            if self.rec.i8(off::ILLNESS + i) < 0 && 30_000.0 * rng.r() > condition + 20_000.0 {
                self.set(Behaviour::Game(behaviour), -1);
                *best = 0.99;
            }
        }
    }

    /// #2 `0x54E970`; rewrites its cooldown on every run.
    fn drowsy(&mut self, best: &mut f32, rng: &mut Rng) {
        let v = self.rec.u16(off::SLEEPY);
        if self.timers[TIMER_DROWSY] == 0 && v > 6000 {
            let v = f32::from(v);
            let s = if v < 8000.0 {
                (v - 6000.0) / 2000.0
            } else if v < 9000.0 {
                1.0 - (v - 8000.0) / 1000.0
            } else {
                0.0
            };
            let s = jitter(s * 0.99, rng);
            if s > *best {
                self.set(Behaviour::Game(0x54F980), -1);
                *best = s;
            }
        }
        let sleepy = f64::from(self.rec.u16(off::SLEEPY));
        self.timers[TIMER_DROWSY] =
            (600.0 + 2400.0 * 0.5 * ((10_000.0 - sleepy) / 10_000.0 + f64::from(rng.r()))) as i32;
    }

    /// #3 `0x54EB20`.
    fn deep_sleep(&mut self, best: &mut f32, rng: &mut Rng) {
        let v = self.rec.u16(off::SLPDPTH);
        if v > 500 {
            let f = f16(v, 500);
            let s = jitter(f + 0.5 * f, rng);
            if s > *best {
                self.set(Behaviour::Game(0x54FCF0), -1);
                *best = s;
            }
        }
    }

    /// #4 `0x54EBA0`: not under water and SLEEPY over 8000.
    fn go_to_bed(&mut self, best: &mut f32, rng: &mut Rng) {
        let v = self.rec.u16(off::SLEEPY);
        if self.body.pos[1] >= self.body.water_y && v > 8000 {
            let s = jitter(0.99 * f16(v, 8000), rng);
            if s > *best {
                self.set(Behaviour::Game(0x54EE10), -1);
                *best = s;
            }
        }
    }

    /// #5 `0x59EB00`: go for fruit (A), or just be hungry (B).
    fn hunger(&mut self, best: &mut f32, senses: &dyn Senses, rng: &mut Rng) {
        let hunger = self.rec.u16(off::HUNGER);
        let fruit_path = self.timers[TIMER_FRUIT] != 0 || rng.r() >= 0.6;
        if fruit_path {
            match senses.nearest_fruit() {
                Some(fruit) if fruit.taken => {}
                Some(fruit) => {
                    let s = jitter(0.99 * f16(hunger, 5000), rng);
                    if s > *best {
                        if fruit.held {
                            self.set(Behaviour::Game(0x59E8C0), -1);
                            self.push(Behaviour::Game(0x59EA00));
                        } else {
                            self.set(Behaviour::Game(0x5614D0), -1);
                        }
                        *best = s;
                    }
                }
                // A tree but no fruit: the game works out a score and does nothing with it.
                None if senses.tree_seen() => {
                    let _ = jitter(0.99 * f16(hunger, 5000), rng);
                }
                None => {}
            }
            return;
        }
        let s = jitter(0.85 * f16(hunger, 5500), rng);
        if s > *best {
            self.set(Behaviour::Game(0x59ED60), -1);
            *best = s;
        }
        if hunger > 9900 {
            let s = jitter(1.0, rng);
            if s > *best {
                self.set(Behaviour::Game(0x59EF40), -1);
                *best = s;
            }
        }
    }

    /// #6 `0x562520`.
    fn thirst(&mut self, best: &mut f32, rng: &mut Rng) {
        let v = self.rec.u16(off::THIRSTY);
        if *best < 1.0 && v > 9000 {
            let s = jitter(0.99 * f16(v, 9000), rng);
            if s > *best {
                self.set(Behaviour::GoToWater, -1);
                *best = s;
            }
        }
    }

    /// #13 `0x599B60`: no `best < 1` gate; the boredom cost is paid even for toys with no
    /// behaviour.
    fn toy(&mut self, best: &mut f32, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) {
        if self.timers[TIMER_TOY] != 0 {
            return;
        }
        let Some((kind, pos)) = senses.nearest_toy() else {
            return;
        };
        let s = jitter(0.99 * f16(self.rec.u16(off::TEDIOUS), 2000), rng);
        if s <= *best {
            return;
        }
        let behaviour = match kind {
            2 => Some(0x599210),
            3 => Some(0x599400),
            4..=6 => Some(0x40AD80),
            // A kind-7 toy in a cell of value 3 is left alone, boredom and all.
            7 if garden.cell_at(pos[0], pos[2]) == Some(3) => return,
            7 => Some(0x5995F0),
            8 => Some(0x599770),
            _ => None,
        };
        if let Some(b) = behaviour {
            self.set(Behaviour::Game(b), -1);
        }
        self.rec.add_need(off::TEDIOUS, -5000);
        *best = s;
    }

    /// #14 `0x5A10B0`: a learned skill, song or game: r < 0.4 skills A/B/C or a song
    /// (`0x59D410`, `0x59C6D0`, `0x59B900`, `0x5AB5D0`, by a second roll), else a toy skill
    /// (`0x55E7A0`, second roll < 0.5) or animal play (`0x5A2F10`) (acts/toys.rs,
    /// acts/play.rs). Only an install spends TEDIOUS (−5000) and sets best 0.99.
    fn learned_play(&mut self, best: &mut f32, senses: &dyn Senses, rng: &mut Rng) {
        let v = self.rec.u16(off::TEDIOUS);
        if *best >= 1.0 || v <= 2000 {
            return;
        }
        let s = jitter(0.99 * f16(v, 2000), rng);
        if s <= *best {
            return;
        }
        let installed = if rng.r() < 0.4 {
            match rng.r() {
                r if r < 0.25 => self.skill_a(senses, rng),
                r if r < 0.5 => self.skill_b(senses, rng),
                r if r < 0.75 => self.skill_c(senses, rng),
                _ => self.sing(senses, rng),
            }
        } else if rng.r() < 0.5 {
            super::acts::toy_skill(self, rng)
        } else {
            self.animal_play(senses, rng)
        };
        if installed {
            self.rec.add_need(off::TEDIOUS, -5000);
            *best = 0.99;
        }
    }

    /// #15 `0x5626D0`: tired enough by temperament, rest.
    fn tired(&mut self, best: &mut f32, rng: &mut Rng) {
        if *best >= 1.0 {
            return;
        }
        let spread = f64::from(self.rec.p(0x14A) - self.rec.p(0x155) + 200) / 400.0;
        let threshold = 2000 + (spread * 4000.0) as i32;
        let v = self.rec.u16(off::TIRE);
        if i32::from(v) > threshold {
            let s = jitter(0.99 * f16(v, threshold), rng);
            if s > *best {
                self.set(Behaviour::StandUpSlow, -1);
                self.push(Behaviour::Game(0x5A1BA0));
                *best = s;
            }
        }
    }

    /// #16 `0x59F210` (emotion `0x12F`).
    fn scared(&mut self, best: &mut f32, senses: &dyn Senses, rng: &mut Rng) {
        let v = self.rec.u8(0x12F);
        if v <= 100 {
            return;
        }
        if let Some(bond) = senses.player_seen() {
            if self.rec.u8(off::BONDS + 1 + bond * 6) > 75 {
                let s = jitter(0.99 * f8(v, 75), rng);
                if s > *best {
                    self.set(Behaviour::Game(0x59F350), -1);
                    self.push(Behaviour::Game(0x59F5F0));
                    *best = s;
                }
            }
        } else {
            let s = 1.0 - (self.rec.p(0x14C) + 100) as f32 * 0.005 * (1.0 - 0.7);
            if s > *best {
                self.set(Behaviour::Game(0x59F350), -1);
                *best = s;
            }
        }
    }

    /// #17 `0x59F860` (emotion `0x12E`).
    fn sad(&mut self, best: &mut f32) {
        if self.rec.u8(0x12E) > 90 {
            let s = 1.0 - (self.rec.p(0x14C) + 100) as f32 * 0.005 * 0.3;
            if s > *best {
                self.set(Behaviour::Game(0x59F8E0), -1);
                *best = s;
            }
        }
    }

    /// #18 `0x5A0010` (emotion `0x131`).
    fn angry(&mut self, best: &mut f32, rng: &mut Rng) {
        if self.rec.u8(0x131) > 150 {
            let s = jitter(1.0 - (100 - self.rec.p(0x14C)) as f32 * 0.005 * 0.5, rng);
            if s > *best {
                self.set(Behaviour::Game(0x5A00A0), -1);
                *best = s;
            }
        }
    }

    /// #19 `0x5A0B10` (emotion `0x12C`).
    fn happy(&mut self, best: &mut f32, rng: &mut Rng) {
        if self.rec.u8(0x12C) > 100 {
            let s = jitter(1.0 - (100 - self.rec.p(0x155)) as f32 * 0.005 * 0.4, rng);
            if s > *best {
                self.set(Behaviour::Game(0x5A0BA0), -1);
                *best = s;
            }
        }
    }

    /// #20 `0x562800`: a grown child standing still cocoons. Clamps evolution first.
    fn evolve(&mut self, best: &mut f32) {
        if *best >= 1.0
            || self.rec.u8(off::TYPE) != TYPE_CHILD
            || self.rec.f32(off::EVOLUTION) < 1.0
        {
            return;
        }
        self.rec.set_f32(off::EVOLUTION, 1.0);
        if self.standing() {
            self.set(Behaviour::Game(0x568D30), -1);
            *best = 1.0;
        }
    }

    /// #21 `0x5A1240`: dark adults' mischief.
    fn mischief(&mut self, best: &mut f32, senses: &dyn Senses, rng: &mut Rng) {
        let kind = self.rec.u8(off::TYPE);
        let dark = kind >= 7 && (kind - 7) <= 15 && (kind - 7).is_multiple_of(3);
        let v = self.rec.u8(0x12D);
        if self.rec.p(0x14B) > 30 || !dark || v <= 80 {
            return;
        }
        let p14c = self.rec.p(0x14C);
        let disliked = senses
            .player_seen()
            .filter(|&b| self.rec.i8(off::BONDS + b * 6) < 0);
        let (s, chain): (f32, &[u32]) = match disliked {
            Some(_) if p14c > 30 && rng.r() < 0.7 => {
                (jitter(0.99 * f8(v, 80), rng), &[0x5A1460, 0x5A18A0])
            }
            Some(_) if p14c > -50 => (jitter(0.99 * f8(v, 80), rng), &[0x5A1800]),
            _ => (1.0 - (100 - p14c) as f32 * 0.005 * 0.3, &[0x5A1460]),
        };
        if s > *best {
            self.set(Behaviour::Game(chain[0]), -1);
            if let Some(&next) = chain.get(1) {
                self.push(Behaviour::Game(next));
            }
            *best = s;
        }
    }

    /// #22 `0x5625B0`: a tree that wants water nearby (`watering_spot`), scored by how
    /// little the Chao has watered lately (`rec+0x148`, THIRSTY): jitter(0.891·(5000 − v)/5000)
    /// below 5000, else jitter(0) (`0x562638`: still a roll). The winner goes to the tree
    /// (`0x567FA0`, its ALW target the tree's entry). The picker's pause `D+0x1CC/0x1CE` it
    /// clears isn't the server's.
    fn water_tree(&mut self, best: &mut f32, senses: &dyn Senses, rng: &mut Rng) {
        if *best >= 1.0 {
            return;
        }
        let Some(tree) = senses.watering_spot() else {
            return;
        };
        let v = i32::from(self.rec.u16(off::THIRSTY));
        let f = if v < 5000 {
            (5000 - v) as f32 / 5000.0
        } else {
            0.0
        };
        let s = jitter(0.891 * f, rng);
        if s > *best {
            self.set(Behaviour::Game(0x567FA0), -1);
            self.link = Some(Link::At(tree));
            *best = s;
        }
    }

    /// #25 `0x562880`: ready to breed. Too many Chao in the garden spends the urge.
    fn breed(&mut self, best: &mut f32, senses: &dyn Senses) {
        let kind = self.rec.u8(off::TYPE);
        if *best >= 1.0 || (20..=25).contains(&kind) || senses.signal(0xAB).is_some() {
            return;
        }
        if self.rec.u16(off::BREED) < 10_000 {
            return;
        }
        if senses.chao_count() >= 8 {
            self.rec.set_u16(off::BREED, 0);
        } else if self.standing() {
            self.set(Behaviour::StandUpSlow, -1);
            self.push(Behaviour::Game(0x59BB50));
            *best = 1.0;
        }
    }

    /// #26 `0x562A20`: life run out, standing still.
    fn death(&mut self, best: &mut f32) {
        if *best < 1.0 && self.rec.u16(off::LIFE) == 0 && self.standing() {
            self.set(Behaviour::Game(0x568F60), -1);
            *best = 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::land::GRID_CELLS;
    use super::*;

    fn garden() -> Garden {
        let mut grid = vec![1; GRID_CELLS];
        grid[100] = 9;
        Garden {
            tris: Vec::new(),
            grid,
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    fn pick(set: impl Fn(&mut Chao)) -> Option<Behaviour> {
        let mut chao = Chao::new([0.0; 3], 0, Default::default());
        chao.body.flags |= super::super::physics::ON_GROUND;
        chao.rec.set_u8(off::TYPE, TYPE_CHILD);
        set(&mut chao);
        chao.choose(&garden(), &Alone, &mut Rng::new(3));
        chao.current()
    }

    #[test]
    fn nothing_pressing_means_wander() {
        assert_eq!(pick(|_| {}), Some(Behaviour::Wander));
    }

    #[test]
    fn needs_win_when_high() {
        assert_eq!(
            pick(|c| c.rec.set_u16(off::THIRSTY, 10_000)),
            Some(Behaviour::GoToWater)
        );
        assert_eq!(
            pick(|c| c.rec.set_u16(off::SLEEPY, 10_000)),
            Some(Behaviour::Game(0x54EE10))
        );
        assert_eq!(
            pick(|c| c.rec.set_u16(off::TIRE, 10_000)),
            Some(Behaviour::StandUpSlow)
        );
        assert_eq!(
            pick(|c| c.rec.set_u16(off::SLPDPTH, 10_000)),
            Some(Behaviour::Game(0x54FCF0))
        );
    }

    #[test]
    fn a_sad_chao_alone_is_sad() {
        // 1 − 100·0.005·0.3 = 0.85 with personality 0, not jittered.
        assert_eq!(
            pick(|c| c.rec.set_u8(0x12E, 120)),
            Some(Behaviour::Game(0x59F8E0))
        );
    }

    #[test]
    fn a_grown_child_standing_still_cocoons() {
        let chao = pick(|c| c.rec.set_f32(off::EVOLUTION, 1.1));
        assert_eq!(chao, Some(Behaviour::Game(0x568D30)));
    }

    #[test]
    fn a_crowded_garden_spends_the_urge_to_breed() {
        struct Crowd;
        impl Senses for Crowd {
            fn chao_count(&self) -> usize {
                8
            }
        }
        let mut chao = Chao::new([0.0; 3], 0, Default::default());
        chao.rec.set_u8(off::TYPE, 5);
        chao.rec.set_u16(off::BREED, 10_000);
        chao.rec.set_u16(off::LIFE, 100);
        chao.choose(&garden(), &Crowd, &mut Rng::new(3));
        assert_eq!(chao.rec.u16(off::BREED), 0);
    }
}
