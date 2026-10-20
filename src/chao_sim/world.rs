//! One garden on the server: its ground, its Chao and the game's clocks, stepped a frame at
//! a time (60 a second, like the game).

use std::sync::Arc;

use super::brain::{Chao, Rng};
use super::land::{Blocker, Garden, GardenToy, Ledge, V3};
use super::life::life_tick;
use super::motion::MotionTable;
use super::picker::{Ball, Carried, Fruit, Release, Senses, Signal, Toy, Tree};

/// Life-tick slots (`0x1DC0FFC`, 32 of 0x48): a slot gets its tick every 3 frames in turn.
const LIFE_SLOTS: usize = 32;
/// Chao a world holds at most. Past `LIFE_SLOTS`, slot `n` ticks with slot `n % LIFE_SLOTS`,
/// so each still gets its life tick every 96 frames, as in the game.
pub const MAX_CHAO: usize = 2 * LIFE_SLOTS;

pub struct World {
    pub garden: Garden,
    /// 0 Neutral, 1 Hero, 2 Dark.
    pub index: usize,
    /// Chao by life-tick slot; a free slot still takes its turn (`0x5363D0`).
    pub chao: Vec<Option<Chao>>,
    pub rng: Rng,
    /// The game's motion table, when a player has sent it.
    pub motions: Option<Arc<MotionTable>>,
    /// The Chao whose life tick ran this frame, and what it changed (`chao::changes::diff`)
    /// in the record its save keeps (`Chao::kept`): its owner's game ticks it too, so the
    /// server doesn't send these back.
    pub ticked: Vec<(usize, Vec<(u16, f32)>)>,
    /// The scheduler's counters (`0x1DBED74`, `0x1DBED70`).
    phase: u16,
    slot: u32,
}

impl World {
    pub fn new(garden: Garden, index: usize, seed: u32) -> Self {
        Self {
            garden,
            index,
            chao: Vec::new(),
            rng: Rng::new(seed),
            motions: None,
            ticked: Vec::new(),
            phase: 0,
            slot: 0,
        }
    }

    /// Puts a Chao in the first free slot; its slot, or None when all `MAX_CHAO` are taken.
    pub fn add(&mut self, mut chao: Chao) -> Option<usize> {
        chao.set_motions(self.motions.clone());
        if let Some(i) = self.chao.iter().position(Option::is_none) {
            self.chao[i] = Some(chao);
            return Some(i);
        }
        if self.chao.len() >= MAX_CHAO {
            return None;
        }
        self.chao.push(Some(chao));
        Some(self.chao.len() - 1)
    }

    /// Gives every Chao here (and later ones) the game's motion table.
    pub fn set_motions(&mut self, table: Option<Arc<MotionTable>>) {
        for chao in self.chao.iter_mut().flatten() {
            chao.set_motions(table.clone());
        }
        self.motions = table;
    }

    pub fn remove(&mut self, slot: usize) -> Option<Chao> {
        self.chao.get_mut(slot)?.take()
    }

    /// The game's garden area for this garden (1 Neutral, 2 Hero, 3 Dark).
    pub fn area(&self) -> u8 {
        self.index as u8 + 1
    }

    /// One frame: the life-tick scheduler (`0x5363D0`), then every Chao, each sensing the
    /// others' signals (as they were at the end of the last frame), the garden's ledges and
    /// blockers, and from `around` the rest (players, items) where it stands.
    pub fn frame(&mut self, around: &dyn Around) {
        self.phase += 1;
        self.ticked.clear();
        if self.phase >= 3 {
            self.phase = 0;
            let first = (self.slot & 0x1F) as usize;
            let area = self.area();
            for slot in (first..self.chao.len()).step_by(LIFE_SLOTS) {
                if let Some(Some(chao)) = self.chao.get_mut(slot) {
                    let mut rolls = self.rng.clone();
                    let before = chao.rec.0.clone();
                    life_tick(&mut chao.rec, area, &mut self.rng);
                    let mut diff = crate::chao::changes::diff(&before, &chao.rec.0);
                    // What its save keeps lives the same tick, with the same rolls.
                    if let Some(saved) = chao.saved.as_mut() {
                        let before = saved.0.clone();
                        life_tick(saved, area, &mut rolls);
                        diff = crate::chao::changes::diff(&before, &saved.0);
                    }
                    self.ticked.push((slot, diff));
                }
            }
            self.slot = self.slot.wrapping_add(1);
        }
        let heard: Vec<Heard> = self
            .chao
            .iter()
            .enumerate()
            .filter_map(|(slot, c)| c.as_ref().map(|c| (slot, c)))
            .flat_map(|(slot, c)| {
                let doing = c.current().map_or(0, |b| b.address());
                c.signals.iter().map(move |s| Heard {
                    from: slot,
                    kind: s.kind,
                    pos: s.pos,
                    radius: s.radius,
                    sender: (c.body.pos, doing),
                })
            })
            .collect();
        // Fruit each Chao has in mind or in its hands (ALW `+0x40`): another Chao's is taken.
        let claims: Vec<(usize, u32)> = self
            .chao
            .iter()
            .enumerate()
            .filter_map(|(slot, c)| c.as_ref().map(|c| (slot, c)))
            .flat_map(|(slot, c)| {
                let linked = match c.link {
                    Some(super::brain::Link::Fruit(id)) => Some(id),
                    _ => None,
                };
                [linked, c.hands.map(|h| h.fruit)]
                    .into_iter()
                    .flatten()
                    .map(move |id| (slot, id))
            })
            .collect();
        // Toys each Chao is linked with both ways (the horse's rider).
        let riders: Vec<(usize, u8)> = self
            .chao
            .iter()
            .enumerate()
            .filter_map(|(slot, c)| Some((slot, c.as_ref()?.toy_link?)))
            .collect();
        let count = self.chao.iter().flatten().count();
        let songs = self
            .chao
            .iter()
            .flatten()
            .filter(|c| c.song.is_some())
            .count();
        for (slot, chao) in self.chao.iter_mut().enumerate() {
            let Some(chao) = chao else {
                continue;
            };
            let near = Near {
                around: around.senses(slot, chao.body.pos, chao.body.rot_y),
                ledges: &self.garden.ledges,
                blockers: &self.garden.blockers,
                heard: &heard,
                claims: &claims,
                toys: &self.garden.toys,
                riders: &riders,
                me: slot,
                count,
                songs,
                pos: chao.body.pos,
                rot_y: chao.body.rot_y,
            };
            chao.frame(&self.garden, self.index, &near, &mut self.rng);
        }
        // A Chao that joined a performance changes its performer's LONELY (`0x59D0D0`,
        // `0x59C3D0`): the Chao whose signal of that kind it touches.
        let joins: Vec<(usize, i32)> = self
            .chao
            .iter_mut()
            .enumerate()
            .filter_map(|(slot, c)| {
                let c = c.as_mut()?;
                let (kind, by) = c.performer_lonely.take()?;
                let me = [c.body.pos[0], c.body.pos[1] + 2.0, c.body.pos[2]];
                let h = heard.iter().find(|h| {
                    h.from != slot && h.kind == kind && distance(h.pos, me) < h.radius + 1.5
                })?;
                Some((h.from, by))
            })
            .collect();
        for (performer, by) in joins {
            if let Some(Some(c)) = self.chao.get_mut(performer) {
                c.rec.add_need(super::record::off::LONELY, by);
            }
        }
    }
}

/// What a garden offers the Chao in `slot` standing at `pos` facing `rot_y` (players, items):
/// the server answers from what its players report.
pub trait Around {
    fn senses(&self, slot: usize, pos: V3, rot_y: u16) -> Box<dyn Senses + '_>;
}

impl Around for super::picker::Alone {
    fn senses(&self, _slot: usize, _pos: V3, _rot_y: u16) -> Box<dyn Senses + '_> {
        Box::new(super::picker::Alone)
    }
}

/// A signal a Chao in this garden sent (`0x55A920`).
struct Heard {
    from: usize,
    kind: u8,
    pos: V3,
    radius: f32,
    /// Its sender's position and what it's doing.
    sender: (V3, u32),
}

/// What one Chao senses: the garden's ledges where it stands, the rest from around it.
struct Near<'a> {
    around: Box<dyn Senses + 'a>,
    ledges: &'a [Ledge],
    blockers: &'a [Blocker],
    heard: &'a [Heard],
    claims: &'a [(usize, u32)],
    toys: &'a [GardenToy],
    riders: &'a [(usize, u8)],
    me: usize,
    count: usize,
    songs: usize,
    pos: V3,
    rot_y: u16,
}

fn distance(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// In view of a Chao at `pos` facing `rot_y` (`0x590C70`): within 50 and inside ±0x271C
/// BAMS of its facing (re/chao-ai.md §7.2).
pub fn in_view(pos: V3, rot_y: u16, at: V3) -> bool {
    let bearing = ((at[0] - pos[0]).atan2(at[2] - pos[2]) * 10430.38) as i32;
    distance(at, pos) < 50.0
        && i32::from((rot_y as i32).wrapping_sub(bearing) as i16).abs() < 0x271C
}

impl Near<'_> {
    /// `fruit`, taken when another Chao here has it in mind or in its hands.
    fn mark(&self, fruit: Fruit) -> Fruit {
        let taken = self
            .claims
            .iter()
            .any(|&(slot, id)| slot != self.me && id == fruit.id);
        Fruit {
            taken: fruit.taken || taken,
            ..fruit
        }
    }
}

impl Senses for Near<'_> {
    fn touch(&self, kind: u8) -> Option<V3> {
        if kind == 0xCB
            && let Some(l) = self.ledges.iter().find(|l| l.touches(self.pos))
        {
            return Some(l.pos);
        }
        if kind == 0xCC
            && let Some(b) = self.blockers.iter().find(|b| b.touches(self.pos))
        {
            return Some(b.pos);
        }
        self.around.touch(kind)
    }
    /// Another Chao's signal its collision sphere (1.5 around 2 up) reaches (`0x486880`).
    fn signal(&self, kind: u8) -> Option<Signal> {
        let me = [self.pos[0], self.pos[1] + 2.0, self.pos[2]];
        self.heard
            .iter()
            .find(|h| {
                let d = [h.pos[0] - me[0], h.pos[1] - me[1], h.pos[2] - me[2]];
                h.from != self.me
                    && h.kind == kind
                    && (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < h.radius + 1.5
            })
            .map(|h| Signal {
                pos: h.pos,
                parent: Some(h.sender),
            })
            .or_else(|| self.around.signal(kind))
    }
    fn player_seen(&self) -> Option<usize> {
        self.around.player_seen()
    }
    fn player_pos(&self) -> Option<V3> {
        self.around.player_pos()
    }
    fn nearest_fruit(&self) -> Option<Fruit> {
        self.around.nearest_fruit().map(|f| self.mark(f))
    }
    fn tree_seen(&self) -> bool {
        self.around.tree_seen()
    }
    fn nearest_tree(&self) -> Option<Tree> {
        self.around.nearest_tree()
    }
    fn fruit(&self, id: u32) -> Option<Fruit> {
        self.around.fruit(id).map(|f| self.mark(f))
    }
    fn touching_fruit(&self) -> Option<Fruit> {
        self.around.touching_fruit().map(|f| self.mark(f))
    }
    fn player_character(&self) -> Option<u8> {
        self.around.player_character()
    }
    /// The nearest toy in view (`0x590D20`), the ball among them (kind 4). Without the
    /// garden's toys, `around`'s.
    fn nearest_toy(&self) -> Option<(u8, V3)> {
        if self.toys.is_empty() {
            return self.around.nearest_toy();
        }
        let ball = self.around.ball().map(|b| (4, b.pos));
        self.toys
            .iter()
            .filter(|t| t.kind != 4)
            .map(|t| (t.kind, t.pos))
            .chain(ball)
            .filter(|&(_, at)| in_view(self.pos, self.rot_y, at))
            .min_by(|a, b| distance(a.1, self.pos).total_cmp(&distance(b.1, self.pos)))
    }
    fn toy(&self, kind: u8) -> Option<Toy> {
        if self.toys.is_empty() {
            return self.around.toy(kind);
        }
        let t = self.toys.iter().find(|t| t.kind == kind)?;
        Some(Toy {
            pos: t.pos,
            rot_y: t.rot_y,
            taken: self
                .riders
                .iter()
                .any(|&(slot, k)| slot != self.me && k == kind),
        })
    }
    fn chao_count(&self) -> usize {
        self.count
    }
    fn songs(&self) -> usize {
        self.songs
    }
    fn player_rot_y(&self) -> Option<u16> {
        self.around.player_rot_y()
    }
    fn whistling(&self) -> bool {
        self.around.whistling()
    }
    fn ball(&self) -> Option<Ball> {
        self.around.ball()
    }
    fn release(&self) -> Release {
        self.around.release()
    }
    fn petted(&self) -> bool {
        self.around.petted()
    }
    fn hit_by(&self) -> Option<V3> {
        self.around.hit_by()
    }
    fn carrying(&self) -> Option<Carried> {
        self.around.carrying()
    }
    fn hat_room(&self) -> bool {
        self.around.hat_room()
    }
    fn plant_spot(&self) -> Option<V3> {
        self.around.plant_spot()
    }
    fn watering_spot(&self) -> Option<V3> {
        self.around.watering_spot()
    }
}

#[cfg(test)]
mod tests {
    use super::super::land::GRID_CELLS;
    use super::super::picker::Alone;
    use super::super::record::{Record, TYPE_CHILD, off};
    use super::*;

    fn child() -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, TYPE_CHILD);
        Chao::new([0.0; 3], 0, rec)
    }

    #[test]
    fn each_chao_gets_a_life_tick_every_96_frames() {
        let mut world = World::new(
            Garden {
                tris: Vec::new(),
                grid: vec![1; GRID_CELLS],
                ledges: Vec::new(),
                blockers: Vec::new(),
                toys: Vec::new(),
                tree_spots: Vec::new(),
            },
            0,
            1,
        );
        world.add(child());
        world.add(child());
        // 11 life ticks raise HUNGER once (by 70): 11·96 frames, give or take a slot.
        for _ in 0..11 * 96 + 6 {
            world.frame(&Alone);
        }
        for chao in world.chao.iter().flatten() {
            assert_eq!(chao.rec.u16(off::HUNGER), 70);
        }
    }

    #[test]
    fn slots_are_reused() {
        let mut world = World::new(
            Garden {
                tris: Vec::new(),
                grid: vec![1; GRID_CELLS],
                ledges: Vec::new(),
                blockers: Vec::new(),
                toys: Vec::new(),
                tree_spots: Vec::new(),
            },
            0,
            1,
        );
        assert_eq!(world.add(child()), Some(0));
        assert_eq!(world.add(child()), Some(1));
        world.remove(0);
        assert_eq!(world.add(child()), Some(0));
        for _ in 2..MAX_CHAO {
            assert!(world.add(child()).is_some());
        }
        assert_eq!(world.add(child()), None);
    }

    #[test]
    fn past_32_chao_each_still_ticks_every_96_frames() {
        let mut world = World::new(
            Garden {
                tris: Vec::new(),
                grid: vec![1; GRID_CELLS],
                ledges: Vec::new(),
                blockers: Vec::new(),
                toys: Vec::new(),
                tree_spots: Vec::new(),
            },
            0,
            1,
        );
        for _ in 0..40 {
            world.add(child());
        }
        let mut ticks = vec![Vec::new(); 40];
        for frame in 0..96 * 3 {
            world.frame(&Alone);
            for &(slot, _) in &world.ticked {
                ticks[slot].push(frame);
            }
        }
        for (slot, at) in ticks.iter().enumerate() {
            assert_eq!(at.len(), 3, "slot {slot}");
            assert!(
                at.windows(2).all(|w| w[1] - w[0] == 96),
                "slot {slot}: {at:?}"
            );
        }
        // Slot 33 ticks with slot 1.
        assert_eq!(ticks[33], ticks[1]);
    }
}
