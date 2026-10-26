//! Using a move: the checks before it, who it goes to, and its hits.

use super::super::abilities::{self, Absorb, Contact};
use super::super::damage::{self, Attack};
use super::super::data::{Effectiveness, MoveData, Target, flags, types};
use super::super::effects::{self, Added};
use super::super::event::{Blocked, Condition, Event, HurtBy, Slot};
use super::super::field::{Delayed, Weather};
use super::super::fighter::{Ailment, Fighter, Lock, LockKind, Stat};
use super::{Battle, Format};

/// Accuracy stage ratios (-6..=+6): 3 / (3 - stage) down, (3 + stage) / 3 up.
const ACCURACY_RATIOS: [(u32, u32); 13] = [
    (3, 9),
    (3, 8),
    (3, 7),
    (3, 6),
    (3, 5),
    (3, 4),
    (1, 1),
    (4, 3),
    (5, 3),
    (2, 1),
    (7, 3),
    (8, 3),
    (3, 1),
];

/// Odds (in 65536) of a shielding move working after this many in a row.
const SHIELD_ODDS: [u32; 4] = [65536, 21845, 7281, 2427];

/// How a move is being used.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Use {
    /// The out-of-PP move: no type and no same-type bonus.
    pub out_of_pp_move: bool,
    /// Carrying on with a lock or a charged move.
    pub cont: bool,
    /// Hitting a foe as it switches out (double power).
    pub pursuing: bool,
    /// A power rolled once for the whole move.
    pub power: Option<u32>,
}

/// Set damage, from the effect or the battle so far.
enum Fixed {
    Formula,
    Damage(u32),
    Fail,
}

/// Bit for a slot in `ThisTurn::damaged_by`.
fn bit(s: Slot) -> u8 {
    1 << (s.side * 2 + s.pos)
}

impl Battle {
    /// A fighter's go with a move: `index` is its move slot (`None`: the out-of-PP move). A
    /// locked fighter uses its locked move whatever was chosen.
    pub(super) fn take_turn(
        &mut self,
        at: Slot,
        index: Option<usize>,
        chosen: Option<Slot>,
        pursuing: bool,
        ev: &mut Vec<Event>,
    ) {
        let rules_pp = self.rules.out_of_pp;
        let f = self.fm(at);
        f.volatile.turn.acted = true;
        f.volatile.vengeful = false;
        f.volatile.rancor = false;
        let lock = f.volatile.lock;
        if f.volatile.recharge && lock.is_none() {
            f.volatile.recharge = false;
            f.volatile.turn.stopped = true;
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::Recharging,
            });
            return;
        }
        let (mv, index, chosen, out_of_pp_move) = match (
            lock,
            index.and_then(|i| f.moves.get(i).map(|m| (i, m.data))),
        ) {
            (Some(l), _) => (l.data, l.index, l.target.or(chosen), false),
            (None, Some((i, data))) => (data, Some(i), chosen, false),
            (None, None) => (rules_pp, None, chosen, true),
        };
        if !self.can_act(at, &mv, index, ev) {
            self.fm(at).volatile.turn.stopped = true;
            self.cancel_lock(at);
            self.report_faints(ev);
            return;
        }
        let cont = lock.is_some();
        if let Some(l) = lock {
            match l.kind {
                LockKind::Charging => self.fm(at).volatile.lock = None,
                LockKind::Storing if l.turns > 1 => {
                    if let Some(l) = self.fm(at).volatile.lock.as_mut() {
                        l.turns -= 1;
                    }
                    ev.push(Event::CantAct {
                        by: at,
                        why: Blocked::Storing,
                    });
                    return;
                }
                LockKind::Storing => self.fm(at).volatile.lock = None,
                _ => {}
            }
        }
        if !cont {
            if let Some(i) = index {
                let extra = self.extra_pp_cost(at, &mv, chosen);
                if let Some(slot) = self.fm(at).moves.get_mut(i) {
                    slot.pp = slot.pp.saturating_sub(1 + extra);
                }
            }
            let v = &mut self.fm(at).volatile;
            v.last_move = Some(mv.id);
            v.last_index = index;
        }
        let v = &mut self.fm(at).volatile;
        if mv.effect != effects::ANGER_HIT {
            v.fuming = false;
        }
        if mv.effect != effects::GROWING_HIT {
            v.growth = 0;
        }
        if !matches!(mv.effect, effects::SHIELD | effects::BRACE) {
            v.shield_streak = 0;
        }
        ev.push(Event::UsedMove {
            by: at,
            move_id: mv.id,
        });
        let u = Use {
            out_of_pp_move,
            cont,
            pursuing,
            power: None,
        };
        self.execute(at, mv, index, chosen, u, ev);

        // A lock that carries on counts down; a rampage ends in confusion.
        if let Some(l) = lock
            && matches!(
                l.kind,
                LockKind::Rampage | LockKind::Noise | LockKind::Rolling
            )
            && self.alive(at)
            && self.fm(at).volatile.lock.is_some()
        {
            let f = self.fm(at);
            let left = l.turns.saturating_sub(1);
            if left == 0 {
                f.volatile.lock = None;
                match l.kind {
                    LockKind::Rampage => {
                        if f.volatile.confusion == 0 && !abilities::prevents_confusion(f.ability) {
                            self.confuse(at, ev);
                        }
                    }
                    LockKind::Noise => ev.push(Event::Ended {
                        target: at,
                        condition: Condition::Locked(mv.id),
                    }),
                    _ => {}
                }
            } else if let Some(l) = f.volatile.lock.as_mut() {
                l.turns = left;
            }
        }
        self.report_faints(ev);
    }

    /// A foe striking `target` as it switches out.
    pub(super) fn pursuing(&mut self, at: Slot, index: usize, target: Slot, ev: &mut Vec<Event>) {
        self.take_turn(at, Some(index), Some(target), true, ev);
    }

    /// Extra PP for foes with the PP-costing ability that the move goes to.
    fn extra_pp_cost(&self, at: Slot, mv: &MoveData, chosen: Option<Slot>) -> u8 {
        let costs = |s: Slot| {
            self.fighter(s)
                .is_some_and(|f| !f.fainted() && f.ability == abilities::COSTS_EXTRA_PP)
        };
        match mv.target {
            Target::User | Target::UserOrSelected => 0,
            Target::BothFoes | Target::FoesField => {
                self.foes(at).into_iter().filter(|&s| costs(s)).count() as u8
            }
            Target::AllOthers => self
                .on_field()
                .into_iter()
                .filter(|&s| s != at && costs(s))
                .count() as u8,
            _ => {
                let t = chosen
                    .filter(|&t| self.alive(t))
                    .or_else(|| self.foes(at).first().copied());
                u8::from(t.is_some_and(|t| t != at && costs(t)))
            }
        }
    }

    /// The checks before acting, in the catalog's order: sleep, freeze, loafing, flinch, a sealed
    /// or provoked move, confusion, paralysis.
    fn can_act(
        &mut self,
        at: Slot,
        mv: &MoveData,
        index: Option<usize>,
        ev: &mut Vec<Event>,
    ) -> bool {
        let noise = self.noise_on();
        let f = self.fm(at);
        match f.ailment {
            Some(Ailment::Sleep) => {
                if noise && !abilities::blocks_sound(f.ability) {
                    f.ailment = None;
                    f.sleep_turns = 0;
                    f.volatile.haunted_sleep = false;
                    ev.push(Event::NoiseWoke { target: at });
                } else {
                    let step = if f.ability == abilities::WAKES_FAST {
                        2
                    } else {
                        1
                    };
                    f.sleep_turns = f.sleep_turns.saturating_sub(step);
                    if f.sleep_turns > 0 {
                        if !effects::works_asleep(mv.effect) {
                            ev.push(Event::CantAct {
                                by: at,
                                why: Blocked::Asleep,
                            });
                            return false;
                        }
                    } else {
                        f.ailment = None;
                        f.volatile.haunted_sleep = false;
                        ev.push(Event::WokeUp { target: at });
                    }
                }
            }
            Some(Ailment::Freeze) => {
                if effects::thaws_user(mv) || self.rng.below(5) == 0 {
                    self.fm(at).ailment = None;
                    ev.push(Event::Thawed { target: at });
                } else {
                    ev.push(Event::CantAct {
                        by: at,
                        why: Blocked::Frozen,
                    });
                    return false;
                }
            }
            _ => {}
        }
        let f = self.fm(at);
        if f.ability == abilities::LOAFS && f.volatile.loafing {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::Loafing,
            });
            return false;
        }
        if f.volatile.flinched {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::Flinched,
            });
            return false;
        }
        if f.volatile.sealed.is_some_and(|(id, _)| id == mv.id) {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::Sealed,
            });
            return false;
        }
        if f.volatile.provoked > 0 && mv.power == 0 {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::Provoked,
            });
            return false;
        }
        if index.is_some() && f.volatile.lock.is_none() && self.sealed_by_foe(at, mv.id) {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::SealedByFoe,
            });
            return false;
        }
        if self.fm(at).volatile.confusion > 0 {
            let f = self.fm(at);
            f.volatile.confusion -= 1;
            if f.volatile.confusion == 0 {
                ev.push(Event::ConfusionEnded { target: at });
            } else {
                ev.push(Event::IsConfused { by: at });
                if self.rng.below(100) < 33 {
                    let random = self.rng.below(16);
                    let f = self.fm(at);
                    let damage = f.hurt(damage::confusion(f, random).max(0) as u32);
                    ev.push(Event::CantAct {
                        by: at,
                        why: Blocked::Confusion,
                    });
                    ev.push(Event::Hurt {
                        target: at,
                        damage,
                        hp: f.hp,
                        by: HurtBy::Confusion,
                    });
                    return false;
                }
            }
        }
        if self.fm(at).ailment == Some(Ailment::Paralysis) && self.rng.below(4) == 0 {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::FullyParalyzed,
            });
            return false;
        }
        if mv.effect == effects::FOCUSED_HIT && self.fm(at).volatile.turn.damaged_by != 0 {
            ev.push(Event::CantAct {
                by: at,
                why: Blocked::LostFocus,
            });
            return false;
        }
        true
    }

    /// Someone on the field is making noise.
    pub(super) fn noise_on(&self) -> bool {
        self.on_field().into_iter().any(|s| {
            self.fighter(s)
                .and_then(|f| f.volatile.lock)
                .is_some_and(|l| l.kind == LockKind::Noise)
        })
    }

    /// Runs a move after the checks: calls another move, charges, or goes to its targets.
    pub(super) fn execute(
        &mut self,
        at: Slot,
        mv: MoveData,
        index: Option<usize>,
        chosen: Option<Slot>,
        u: Use,
        ev: &mut Vec<Event>,
    ) {
        if effects::calls_other_move(mv.effect) {
            // A called move never calls another in turn.
            match self
                .pick_called(at, &mv)
                .filter(|m| !effects::calls_other_move(m.effect))
            {
                Some(called) => {
                    ev.push(Event::Called {
                        by: at,
                        move_id: called.id,
                    });
                    ev.push(Event::UsedMove {
                        by: at,
                        move_id: called.id,
                    });
                    let u = Use { cont: false, ..u };
                    self.execute(at, called, None, None, u, ev);
                }
                None => ev.push(Event::Failed { by: at }),
            }
            return;
        }

        // Two-turn moves: the first turn charges (no charging for a sun move in sun).
        if effects::charges(mv.effect) {
            let sunny = mv.effect == effects::SUN_CHARGE && self.weather() == Weather::Sun;
            if !u.cont && !sunny {
                let target = self
                    .resolve_targets(at, &mv, chosen, &mut Vec::new())
                    .first()
                    .copied();
                let f = self.fm(at);
                f.volatile.lock = Some(Lock {
                    kind: LockKind::Charging,
                    data: mv,
                    index,
                    target,
                    turns: 0,
                });
                f.volatile.hidden = effects::hides(&mv);
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Readying(mv.id),
                });
                if mv.effect == effects::TWO_TURN_DEFENSE_UP {
                    self.raise_stat(at, Stat::Defense, 1, false, ev);
                }
                return;
            }
            self.fm(at).volatile.hidden = None;
        }

        match mv.effect {
            effects::STORE_DAMAGE if !u.cont => {
                let f = self.fm(at);
                f.volatile.lock = Some(Lock {
                    kind: LockKind::Storing,
                    data: mv,
                    index,
                    target: None,
                    turns: 2,
                });
                f.volatile.stored_damage = 0;
                f.volatile.stored_by = None;
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Storing,
                });
                return;
            }
            effects::SELF_FAINT => {
                if let Some(s) = self.ability_on_field(abilities::NO_SELF_FAINT_MOVES) {
                    ev.push(Event::Ability {
                        at: s,
                        ability: abilities::NO_SELF_FAINT_MOVES,
                    });
                    ev.push(Event::Failed { by: at });
                    return;
                }
                let f = self.fm(at);
                let damage = f.hurt(u32::from(f.hp));
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: 0,
                    by: HurtBy::Sacrifice,
                });
            }
            effects::RELEASE_HIT if self.fm(at).volatile.stored == 0 => {
                ev.push(Event::Failed { by: at });
                return;
            }
            effects::SLEEP_FLINCH_HIT if self.fm(at).ailment != Some(Ailment::Sleep) => {
                ev.push(Event::Failed { by: at });
                return;
            }
            effects::FIRST_TURN_FLINCH if self.fm(at).volatile.fresh == 0 => {
                ev.push(Event::Failed { by: at });
                return;
            }
            _ => {}
        }

        let targets = self.resolve_targets(at, &mv, chosen, ev);
        if targets.is_empty() {
            if mv.effect == effects::STORE_DAMAGE || matches!(mv.target, Target::Depends) {
                ev.push(Event::Failed { by: at });
            } else {
                ev.push(Event::NoTarget { by: at });
            }
            self.report_faints(ev);
            return;
        }

        // What a copying move can copy: the last move aimed at each target.
        if mv.flags & flags::COPYABLE != 0 {
            for &t in &targets {
                if t != at && self.alive(t) {
                    self.fm(t).volatile.last_hit_by = Some(mv);
                }
            }
        }

        if mv.effect == effects::DELAYED_HIT {
            self.delay(at, &mv, targets[0], ev);
            return;
        }
        if mv.effect == effects::TEAM_HITS {
            self.team_hits(at, &mv, targets[0], ev);
            return;
        }

        let damaging = mv.power > 0 || effects::is_fixed_damage(mv.effect) || u.out_of_pp_move;
        if !damaging {
            self.status_move(at, &mv, &targets, ev);
            self.report_faints(ev);
            return;
        }

        let u = Use {
            power: (mv.effect == effects::RANDOM_POWER)
                .then(|| u32::from(effects::random_power(&mut self.rng))),
            ..u
        };
        let mut landed = false;
        for &t in &targets {
            let hit = self.hit(at, t, &mv, index, u, ev);
            landed |= hit.is_some();
            if hit.is_none() && mv.effect == effects::CRASH_ON_MISS && self.alive(at) {
                self.crash(at, t, &mv, ev);
            }
            self.report_faints_except(Some(at), ev);
        }
        if self.alive(at) {
            self.after_move(at, &mv, index, landed, u, ev);
        }
        if mv.effect == effects::RELEASE_HIT {
            self.release_store(at, ev);
        }
    }

    /// What a damaging move leaves on its user: stage drops, rest, a lock, a growing count.
    fn after_move(
        &mut self,
        at: Slot,
        mv: &MoveData,
        index: Option<usize>,
        landed: bool,
        u: Use,
        ev: &mut Vec<Event>,
    ) {
        if landed && let Some(changes) = effects::after_hit(mv.effect) {
            for (stat, by) in changes.iter() {
                self.raise_stat(at, stat, by, false, ev);
            }
        }
        let f = self.fm(at);
        match mv.effect {
            effects::RECHARGE_AFTER if landed => f.volatile.recharge = true,
            effects::GROWING_HIT => {
                f.volatile.growth = if landed {
                    (f.volatile.growth + 1).min(5)
                } else {
                    0
                };
            }
            effects::ANGER_HIT => f.volatile.fuming = true,
            effects::RAMPAGE if !u.cont && landed => {
                let turns = self.rng.below(2) as u8 + 1;
                self.lock(at, LockKind::Rampage, mv, index, turns);
            }
            effects::ROLLING => {
                if !landed {
                    self.fm(at).volatile.lock = None;
                } else if !u.cont {
                    self.lock(at, LockKind::Rolling, mv, index, 4);
                }
            }
            effects::NOISE if !u.cont => {
                let turns = self.rng.below(4) as u8 + 1;
                self.lock(at, LockKind::Noise, mv, index, turns);
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Locked(mv.id),
                });
            }
            _ => {}
        }
    }

    fn lock(&mut self, at: Slot, kind: LockKind, mv: &MoveData, index: Option<usize>, turns: u8) {
        self.fm(at).volatile.lock = Some(Lock {
            kind,
            data: *mv,
            index,
            target: None,
            turns,
        });
    }

    /// A move that hurts its user when it misses: half the damage it would have done. A target
    /// the move can't affect costs nothing.
    fn crash(&mut self, at: Slot, t: Slot, mv: &MoveData, ev: &mut Vec<Event>) {
        let Some(tf) = self.fighter(t) else {
            return;
        };
        if abilities::immune_to_type(tf.ability, mv.type_id)
            || self
                .chart
                .multiplier(mv.type_id, tf.types, tf.volatile.identified)
                == 0
        {
            return;
        }
        let random = self.rng.below(16);
        let (d, _) = self.damage_with(
            at,
            t,
            mv,
            mv.type_id,
            u32::from(mv.power),
            false,
            false,
            random,
            1,
        );
        let f = self.fm(at);
        let damage = f.hurt((d.max(0) as u32 / 2).max(1));
        ev.push(Event::Hurt {
            target: at,
            damage,
            hp: f.hp,
            by: HurtBy::Crash,
        });
    }

    fn release_store(&mut self, at: Slot, ev: &mut Vec<Event>) {
        if self.alive(at) && self.fm(at).volatile.stored > 0 {
            self.fm(at).volatile.stored = 0;
            ev.push(Event::Ended {
                target: at,
                condition: Condition::Stored(0),
            });
        }
    }

    /// The move a calling effect uses, if any.
    fn pick_called(&mut self, at: Slot, mv: &MoveData) -> Option<MoveData> {
        let out_id = self.rules.out_of_pp.id;
        let pool: Vec<MoveData> = match mv.effect {
            effects::COPY_FOES_MOVE => {
                return self.fm(at).volatile.last_hit_by;
            }
            effects::ARENA_MOVE => {
                return self.find_move(effects::ARENA_MOVE_ID);
            }
            effects::RANDOM_MOVE => {
                let mut all: Vec<MoveData> = if self.move_table.is_empty() {
                    self.teams
                        .iter()
                        .flatten()
                        .flat_map(|f| f.moves.iter().map(|m| m.data))
                        .collect()
                } else {
                    self.move_table.clone()
                };
                all.retain(|m| {
                    m.id != 0
                        && m.id != out_id
                        && m.id != mv.id
                        && !effects::never_called(m.effect)
                        && !effects::calls_other_move(m.effect)
                });
                all
            }
            effects::SLEEP_RANDOM_MOVE => {
                if self.fm(at).ailment != Some(Ailment::Sleep) {
                    return None;
                }
                self.fm(at)
                    .moves
                    .iter()
                    .map(|m| m.data)
                    .filter(|m| !effects::not_for_sleep_call(m.effect))
                    .collect()
            }
            effects::ALLY_RANDOM_MOVE => {
                let me = self.member(at);
                self.teams[at.side]
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| Some(i) != me)
                    .flat_map(|(_, f)| f.moves.iter().map(|m| m.data))
                    .filter(|m| {
                        !effects::never_called(m.effect) && !effects::calls_other_move(m.effect)
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        if pool.is_empty() {
            None
        } else {
            Some(pool[self.rng.below(pool.len() as u32) as usize])
        }
    }

    /// A move by id: from the move table, else from what the fighters know.
    pub(super) fn find_move(&self, id: u16) -> Option<MoveData> {
        self.move_table
            .iter()
            .find(|m| m.id == id)
            .copied()
            .or_else(|| {
                self.teams
                    .iter()
                    .flatten()
                    .flat_map(|f| f.moves.iter())
                    .find(|m| m.data.id == id)
                    .map(|m| m.data)
            })
    }

    /// Who the move goes to now: a chosen foe that's gone passes to the other foe, a
    /// drawing ally or ability pulls single-target moves to itself.
    pub(super) fn resolve_targets(
        &mut self,
        at: Slot,
        mv: &MoveData,
        chosen: Option<Slot>,
        ev: &mut Vec<Event>,
    ) -> Vec<Slot> {
        let foes = self.foes(at);
        let random_foe = |b: &mut Battle| -> Option<Slot> {
            if foes.is_empty() {
                None
            } else {
                Some(foes[b.rng.below(foes.len() as u32) as usize])
            }
        };
        // A storing move hits back at whoever hurt it last.
        let kind = if mv.effect == effects::STORE_DAMAGE {
            Target::Depends
        } else {
            mv.target
        };
        let single = match kind {
            Target::User | Target::UserOrSelected | Target::FoesField => return vec![at],
            Target::BothFoes => return foes,
            Target::AllOthers => {
                return self.on_field().into_iter().filter(|&s| s != at).collect();
            }
            Target::Depends => {
                let v = &self.fm(at).volatile;
                let by = match mv.effect {
                    effects::RETURN_PHYSICAL => v.turn.physical_by,
                    effects::RETURN_SPECIAL => v.turn.special_by,
                    effects::STORE_DAMAGE => v.stored_by,
                    _ => return random_foe(self).into_iter().collect(),
                };
                return by
                    .filter(|&s| s.side != at.side && self.alive(s))
                    .or_else(|| {
                        (mv.effect == effects::STORE_DAMAGE && by.is_some())
                            .then(|| random_foe(self))
                            .flatten()
                    })
                    .into_iter()
                    .collect();
            }
            Target::RandomFoe => random_foe(self),
            Target::Selected => match chosen {
                Some(t) if t != at && self.alive(t) => Some(t),
                Some(t) if t.side == at.side && t != at => None,
                _ => random_foe(self),
            },
        };
        let Some(mut t) = single else {
            return Vec::new();
        };
        if self.format == Format::Doubles && t.side != at.side {
            let side = &self.field.sides[t.side];
            if let Some(pos) = side.drawing
                && self.alive(Slot::new(t.side, pos))
                && pos != t.pos
            {
                t = Slot::new(t.side, pos);
                ev.push(Event::Drawn { to: t });
            } else if mv.type_id == types::ELECTRIC
                && let Some(rod) = foes.iter().copied().find(|&s| {
                    self.fighter(s)
                        .is_some_and(|f| f.ability == abilities::DRAWS_ELECTRIC)
                })
                && rod != t
            {
                t = rod;
                ev.push(Event::Ability {
                    at: rod,
                    ability: abilities::DRAWS_ELECTRIC,
                });
                ev.push(Event::Drawn { to: t });
            }
        }
        vec![t]
    }

    /// Shielding moves, then the hit roll (a sure hit, hiding, a shrunk target for the moves that
    /// punish shrinking, accuracy and evasion). Reports a miss; returns the damage multiplier for
    /// a hidden target it reaches.
    pub(super) fn lands(
        &mut self,
        at: Slot,
        t: Slot,
        mv: &MoveData,
        ev: &mut Vec<Event>,
    ) -> Option<u32> {
        if at == t {
            return Some(1);
        }
        let tf = self.fighter(t)?;
        if at.side != t.side
            && self.effective_priority(at, mv) > 0
            && (matches!(
                tf.ability,
                abilities::NO_FOE_PRIORITY_A | abilities::NO_FOE_PRIORITY_B
            ) || self.ally(t).and_then(|s| self.fighter(s)).is_some_and(|f| {
                matches!(
                    f.ability,
                    abilities::NO_FOE_PRIORITY_A | abilities::NO_FOE_PRIORITY_B
                )
            }))
        {
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if at.side != t.side
            && tf.has_type(types::DARK)
            && self
                .fighter(at)
                .is_some_and(|f| f.ability == abilities::STATUS_PRIORITY)
            && mv.category == super::super::data::Category::Status
        {
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if at.side != t.side
            && tf.grounded()
            && self.field.terrain == super::super::field::Terrain::Strange
            && self.effective_priority(at, mv) > 0
        {
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if mv.flags & flags::POWDER != 0
            && (tf.has_type(types::GRASS) || tf.ability == abilities::WEATHER_POWDER_GUARD)
        {
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if mv.flags & flags::BLOCKED_BY_PROTECTION != 0 && tf.volatile.turn.shielded {
            ev.push(Event::Shielded { target: t });
            return None;
        }
        if tf.ability == abilities::ALL_MOVES_HIT
            || self
                .fighter(at)
                .is_some_and(|f| f.ability == abilities::ALL_MOVES_HIT)
        {
            return Some(1);
        }
        if tf.volatile.sure_hit.is_some_and(|(by, _)| by == at) {
            return Some(1);
        }
        let mut mult = 1;
        if let Some(h) = tf.volatile.hidden {
            match effects::reaches(mv, h) {
                Some(m) => mult = m,
                None => {
                    ev.push(Event::Missed { by: at, target: t });
                    return None;
                }
            }
        }
        if punishes_shrink(mv) && tf.volatile.shrunk {
            return Some(mult);
        }
        if mv.effect == effects::ONE_HIT_KO || self.accuracy_check(at, t, mv) {
            Some(mult)
        } else {
            ev.push(Event::Missed { by: at, target: t });
            None
        }
    }

    /// The hit roll: accuracy x the stage ratio of (user accuracy - target evasion), then
    /// abilities and the target's evasion edge.
    pub(super) fn accuracy_check(&mut self, at: Slot, target: Slot, mv: &MoveData) -> bool {
        if at == target {
            return true;
        }
        let Some(acc) = effects::accuracy(mv, self.weather()) else {
            return true;
        };
        let weather = self.weather();
        let (Some(user), Some(tgt)) = (self.fighter(at), self.fighter(target)) else {
            return false;
        };
        if mv.id == 92 && user.has_type(types::POISON) {
            return true;
        }
        if user.ability == abilities::ALL_MOVES_HIT || tgt.ability == abilities::ALL_MOVES_HIT {
            return true;
        }
        let evasion = if tgt.volatile.identified
            || user.ability == abilities::NO_ACCURACY_DROPS
            || mv.flags & flags::IGNORE_EVASION != 0
        {
            0
        } else {
            tgt.stage(Stat::Evasion)
        };
        let buff = (user.stage(Stat::Accuracy) - evasion).clamp(-6, 6);
        let (n, d) = ACCURACY_RATIOS[(buff + 6) as usize];
        let mut calc = u32::from(acc) * n / d;
        calc = abilities::accuracy(calc, user, tgt, mv.category, weather);
        if user.ability != abilities::TEAM_AIM
            && self
                .ally(at)
                .and_then(|s| self.fighter(s))
                .is_some_and(|f| f.ability == abilities::TEAM_AIM)
        {
            calc = calc * 110 / 100;
        }
        calc = calc * (100 - u32::from(tgt.evasion_edge.min(100))) / 100;
        self.rng.below(100) < calc
    }

    /// A shielding move's roll: less likely each time in a row, and it fails for the last
    /// fighter to move.
    pub(super) fn shield_roll(&mut self, at: Slot) -> bool {
        let streak = usize::from(self.fm(at).volatile.shield_streak.min(3));
        self.rng.below(65536) < SHIELD_ODDS[streak]
    }

    /// The move's type and power against `t` for this use (set damage leaves the power).
    fn power_type(&self, at: Slot, t: Slot, mv: &MoveData, u: Use) -> (u8, u32) {
        let weather = self.weather();
        let user = self.fighter(at).expect("the user on the field");
        let tgt = self.fighter(t);
        let mut ty = mv.type_id;
        let mut power = u32::from(mv.power);
        match mv.effect {
            effects::STAT_BITS_POWER => {
                let (t, _) = effects::stat_bits_power(user);
                ty = t;
                power = 60;
            }
            effects::WEATHER_POWER => {
                if let Some(t) = effects::weather_power(weather) {
                    ty = t;
                    power *= 2;
                }
            }
            effects::LOW_HP_POWER => {
                power = u32::from(effects::low_hp_power(user.hp, user.stats.hp));
            }
            effects::BOND_POWER | effects::BOND_POWER_INVERSE => {
                power = u32::from(effects::bond_power(
                    user.friendship,
                    mv.effect == effects::BOND_POWER_INVERSE,
                ));
            }
            effects::GROWING_HIT => {
                power = effects::growing_power(mv.power, user.volatile.growth + 1);
            }
            effects::ROLLING => {
                let n = match user.volatile.lock {
                    Some(l) if l.kind == LockKind::Rolling => 5 - l.turns.min(4) + 1,
                    _ => 1,
                };
                power = effects::rolling_power(mv.power, n.min(5), user.volatile.curled);
            }
            effects::HP_POWER => power = effects::hp_power(mv.power, user.hp, user.stats.hp),
            effects::STATUS_POWER => {
                if matches!(
                    user.ailment,
                    Some(Ailment::Poison | Ailment::BadPoison | Ailment::Burn | Ailment::Paralysis)
                ) {
                    power *= 2;
                }
            }
            effects::PARALYSIS_POWER_CURE => {
                if tgt.is_some_and(|f| f.ailment == Some(Ailment::Paralysis)) {
                    power *= 2;
                }
            }
            effects::HURT_FIRST_POWER => {
                if user.volatile.turn.damaged_by(t) {
                    power *= 2;
                }
            }
            effects::RELEASE_HIT => power = 100 * u32::from(user.volatile.stored),
            effects::WEIGHT_POWER => {
                power = match tgt.map(Fighter::battle_weight).unwrap_or(100) {
                    0..=99 => 20,
                    100..=249 => 40,
                    250..=499 => 60,
                    500..=999 => 80,
                    1000..=1999 => 100,
                    _ => 120,
                }
            }
            effects::HITS_SWITCHING if u.pursuing => power *= 2,
            _ => {}
        }
        if let Some(p) = u.power {
            power = p;
        }
        power = self.ordinary_power(at, t, mv, power);
        if user.ability == abilities::SOUND_TO_WATER && mv.flags & flags::SOUND != 0 {
            ty = types::WATER;
        }
        (ty, power)
    }

    /// One target of a damaging move: `Some(damage dealt)` when it landed.
    fn hit(
        &mut self,
        at: Slot,
        t: Slot,
        mv: &MoveData,
        index: Option<usize>,
        u: Use,
        ev: &mut Vec<Event>,
    ) -> Option<u32> {
        // A self-fainting user still lands its hit.
        if !self.alive(t) || (!self.alive(at) && mv.effect != effects::SELF_FAINT) {
            return None;
        }
        let (move_type, mut power) = self.power_type(at, t, mv, u);

        // A gift that heals instead.
        if mv.effect == effects::RANDOM_GIFT {
            match self.rng.below(256) {
                0..=101 => power = 40,
                102..=177 => power = 80,
                178..=203 => power = 120,
                _ => {
                    self.lands(at, t, mv, ev)?;
                    let f = self.fm(t);
                    if f.hp == f.stats.hp {
                        ev.push(Event::Failed { by: at });
                    } else {
                        let amount = f.heal((u32::from(f.stats.hp) / 4).max(1));
                        ev.push(Event::Healed {
                            target: t,
                            amount,
                            hp: f.hp,
                        });
                    }
                    return None;
                }
            }
        }
        if mv.effect == effects::DRAIN_SLEEPER {
            let f = self.fm(t);
            if f.ailment != Some(Ailment::Sleep) || f.volatile.decoy > 0 {
                ev.push(Event::NoEffect { target: t });
                return None;
            }
        }

        let tf = self.fighter(t).expect("a target on the field");
        if t != at && abilities::blocks_sound(tf.ability) && self.sound_moves.contains(&mv.id) {
            ev.push(Event::Ability {
                at: t,
                ability: tf.ability,
            });
            ev.push(Event::NoEffect { target: t });
            return None;
        }

        // An ability that takes the move in.
        let frozen = tf.ailment == Some(Ailment::Freeze);
        if !u.out_of_pp_move
            && t != at
            && let Some(a) = abilities::absorbs(tf.ability, move_type, true, frozen)
        {
            if mv.flags & flags::BLOCKED_BY_PROTECTION != 0 && tf.volatile.turn.shielded {
                ev.push(Event::Shielded { target: t });
                return None;
            }
            self.absorb(t, a, ev);
            return None;
        }

        let mut mult = self.lands(at, t, mv, ev)?;
        let tf = self.fighter(t).expect("a target on the field");
        if punishes_shrink(mv) && tf.volatile.shrunk {
            mult *= 2;
        }

        if !u.out_of_pp_move
            && !self.grounding_override(t)
            && abilities::immune_to_type(tf.ability, move_type)
        {
            ev.push(Event::Ability {
                at: t,
                ability: tf.ability,
            });
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if !u.out_of_pp_move
            && move_type == types::GROUND
            && !self.grounding_override(t)
            && !tf.grounded()
        {
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        let multiplier = if u.out_of_pp_move {
            100
        } else {
            self.chart.multiplier(
                move_type,
                self.effective_types(t, move_type),
                tf.volatile.identified,
            )
        };
        if multiplier == 0 {
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if !u.out_of_pp_move && tf.ability == abilities::ONLY_SUPER_EFFECTIVE && multiplier <= 100 {
            ev.push(Event::Ability {
                at: t,
                ability: tf.ability,
            });
            ev.push(Event::NoEffect { target: t });
            return None;
        }

        let ability = tf.ability;
        if mv.effect == effects::ONE_HIT_KO && ability == abilities::NO_ONE_HIT_KO {
            ev.push(Event::Ability { at: t, ability });
            ev.push(Event::NoEffect { target: t });
            return None;
        }
        if mv.effect == effects::BREAK_SCREENS {
            let side = &mut self.field.sides[t.side];
            if side.physical_wall > 0 || side.special_wall > 0 {
                side.physical_wall = 0;
                side.special_wall = 0;
                ev.push(Event::ScreensBroken { side: t.side });
            }
        }
        let fixed = self.fixed(at, t, mv);
        if let Fixed::Fail = fixed {
            ev.push(Event::Failed { by: at });
            return None;
        }
        let mut total = 0u32;
        let mut to_hp = 0u32;
        let hits = if self.fm(at).ability == abilities::MOST_HITS && mv.effect == effects::MULTI_HIT
        {
            5
        } else {
            effects::hit_count(mv.effect, &mut self.rng)
        };
        let mut landed = 0;
        for n in 0..hits {
            if !self.alive(t) || (landed > 0 && !self.alive(at)) {
                break;
            }
            if mv.effect == effects::TRIPLE_RISING && n > 0 && !self.accuracy_check(at, t, mv) {
                break;
            }
            let (d, crit, effect) = match fixed {
                Fixed::Damage(d) => (d, false, Effectiveness::Normal),
                _ => {
                    let p = if mv.effect == effects::TRIPLE_RISING {
                        effects::rising_power(mv.power, n)
                    } else {
                        power
                    };
                    let crit = self.roll_crit(at, t, mv);
                    let random = self.rng.below(16);
                    let (d, m) = self.damage_with(
                        at,
                        t,
                        mv,
                        move_type,
                        p,
                        u.out_of_pp_move,
                        crit,
                        random,
                        mult,
                    );
                    let effect = if u.out_of_pp_move {
                        Effectiveness::Normal
                    } else {
                        Effectiveness::from_multiplier(m)
                    };
                    (d.max(0) as u32, crit, effect)
                }
            };
            let (all, hp) = self.deal(at, t, mv, move_type, d, crit, effect, ev);
            total += all;
            to_hp += hp;
            landed += 1;
        }
        if hits > 1 {
            ev.push(Event::HitTimes { count: landed });
        }
        if mv.effect == effects::ONE_HIT_KO && self.fm(t).hp == 0 {
            ev.push(Event::KnockedOut { target: t });
        }

        if to_hp > 0 {
            self.on_faint_by(at, t, index, ev);
        }
        if let Some((n, d)) = (mv.details.drain_percent > 0)
            .then_some((u32::from(mv.details.drain_percent), 100))
            .or_else(|| effects::drain(mv.effect))
            && self.alive(at)
        {
            let amount = (total * n / d).max(1);
            if to_hp > 0 && self.fm(t).ability == abilities::DRAINS_HURT {
                let f = self.fm(at);
                let damage = f.hurt(amount);
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: f.hp,
                    by: HurtBy::DrainBackfire,
                });
            } else {
                let f = self.fm(at);
                let amount = f.heal(amount);
                if amount > 0 {
                    ev.push(Event::Healed {
                        target: at,
                        amount,
                        hp: f.hp,
                    });
                }
            }
        }
        if self.alive(t) && self.fm(t).ailment == Some(Ailment::Freeze) && move_type == types::FIRE
        {
            self.fm(t).ailment = None;
            ev.push(Event::Thawed { target: t });
        }
        if to_hp > 0 && self.alive(t) && mv.power > 0 && !u.out_of_pp_move {
            let f = self.fm(t);
            if f.ability == abilities::TYPE_FROM_HIT && !f.has_type(move_type) {
                f.keep_original(false);
                f.types = [move_type, move_type];
                let ability = f.ability;
                ev.push(Event::Ability { at: t, ability });
                ev.push(Event::TypeChanged {
                    target: t,
                    types: [move_type, move_type],
                });
            }
        }
        if to_hp > 0
            && mv.flags & flags::CONTACT != 0
            && self.fm(at).ability != abilities::NO_CONTACT
        {
            self.contact(at, t, ev);
        }
        self.added_effect(at, t, mv, total > 0 && to_hp == 0, ev);
        if mv.effect == effects::CLEAR_HAZARDS && self.alive(at) {
            self.spin(at, ev);
        }
        if let Some((n, d)) = (mv.details.recoil_percent > 0)
            .then_some((u32::from(mv.details.recoil_percent), 100))
            .or_else(|| effects::recoil(mv.effect))
            && self.alive(at)
            && total > 0
            && (u.out_of_pp_move || self.fm(at).ability != abilities::NO_RECOIL)
        {
            let f = self.fm(at);
            let amount = if u.out_of_pp_move {
                u32::from(f.stats.hp) / 4
            } else {
                total * n / d
            };
            let damage = f.hurt(amount.max(1));
            ev.push(Event::Hurt {
                target: at,
                damage,
                hp: f.hp,
                by: HurtBy::Recoil,
            });
        }
        Some(total)
    }

    /// Set damage for this target, if the effect has any.
    fn fixed(&mut self, at: Slot, t: Slot, mv: &MoveData) -> Fixed {
        let user = self.fighter(at).expect("the user").clone();
        let tgt = self.fighter(t).expect("the target").clone();
        let back = |dealt: u32, by: Option<Slot>| {
            if dealt > 0 && by.is_some_and(|s| s.side != at.side) {
                Fixed::Damage(dealt * 2)
            } else {
                Fixed::Fail
            }
        };
        match mv.effect {
            effects::RETURN_PHYSICAL => {
                back(user.volatile.turn.physical, user.volatile.turn.physical_by)
            }
            effects::RETURN_SPECIAL => {
                back(user.volatile.turn.special, user.volatile.turn.special_by)
            }
            effects::STORE_DAMAGE => {
                if user.volatile.stored_damage == 0 {
                    Fixed::Fail
                } else {
                    Fixed::Damage(user.volatile.stored_damage * 2)
                }
            }
            effects::ONE_HIT_KO => {
                if mv.id == 329 && tgt.has_type(types::ICE) {
                    return Fixed::Fail;
                }
                if tgt.ability == abilities::NO_ONE_HIT_KO {
                    return Fixed::Fail;
                }
                let sure = tgt.volatile.sure_hit.is_some_and(|(by, _)| by == at)
                    || user.ability == abilities::ALL_MOVES_HIT
                    || tgt.ability == abilities::ALL_MOVES_HIT;
                let chance = u32::from(if mv.id == 329 && !user.has_type(types::ICE) {
                    20
                } else {
                    mv.accuracy
                }) + u32::from(user.level.saturating_sub(tgt.level));
                if user.level < tgt.level || (!sure && self.rng.below(100) + 1 >= chance) {
                    Fixed::Fail
                } else {
                    Fixed::Damage(u32::from(tgt.hp).max(1))
                }
            }
            _ => match effects::fixed_damage(mv.effect, &user, &tgt, &mut self.rng) {
                Some(d) => Fixed::Damage(d),
                None if mv.effect == effects::MATCH_HP => Fixed::Fail,
                None => Fixed::Formula,
            },
        }
    }

    /// Puts `amount` on the target: its decoy takes it first; a bracing or one-HP-leaving move
    /// keeps it at 1 HP. Returns (dealt, of which to HP).
    #[allow(clippy::too_many_arguments)]
    fn deal(
        &mut self,
        at: Slot,
        t: Slot,
        mv: &MoveData,
        move_type: u8,
        amount: u32,
        crit: bool,
        effect: Effectiveness,
        ev: &mut Vec<Event>,
    ) -> (u32, u32) {
        let f = self.fm(t);
        if f.volatile.decoy > 0 && at != t && mv.flags & (flags::SOUND | flags::BYPASS_DECOY) == 0 {
            let d = amount.min(u32::from(f.volatile.decoy)) as u16;
            f.volatile.decoy -= d;
            let left = f.volatile.decoy;
            ev.push(Event::Hit {
                by: at,
                target: t,
                damage: 0,
                hp: f.hp,
                effect,
                crit,
            });
            ev.push(Event::DecoyTook {
                target: t,
                damage: d,
            });
            if left == 0 {
                ev.push(Event::DecoyFaded { target: t });
            }
            return (u32::from(d), 0);
        }
        let mut amount = amount;
        let endures_full = f.ability == abilities::NO_ONE_HIT_KO && f.hp == f.stats.hp;
        let mut hung_on = false;
        if amount >= u32::from(f.hp)
            && (endures_full || f.volatile.turn.bracing || mv.effect == effects::LEAVE_ONE_HP)
        {
            amount = u32::from(f.hp.saturating_sub(1));
            hung_on = f.volatile.turn.bracing || endures_full;
        }
        let damage = f.hurt(amount);
        ev.push(Event::Hit {
            by: at,
            target: t,
            damage,
            hp: f.hp,
            effect,
            crit,
        });
        if hung_on {
            ev.push(Event::HungOn { target: t });
        }
        let d = u32::from(damage);
        if d > 0 && at != t {
            let v = &mut f.volatile;
            v.turn.damaged_by |= bit(at);
            if mv.category == super::super::data::Category::Physical {
                v.turn.physical = d;
                v.turn.physical_by = Some(at);
            } else {
                v.turn.special = d;
                v.turn.special_by = Some(at);
            }
            v.last_hit_type = Some(move_type);
            if v.lock.is_some_and(|l| l.kind == LockKind::Storing) {
                v.stored_damage += d;
                v.stored_by = Some(at);
            }
            if v.fuming && f.hp > 0 {
                self.raise_stat(t, Stat::Attack, 1, false, ev);
            }
        }
        (d, d)
    }

    /// A fighter knocked out by `at`'s move takes `at` with it, or empties the move's PP.
    fn on_faint_by(&mut self, at: Slot, t: Slot, index: Option<usize>, ev: &mut Vec<Event>) {
        if self.fm(t).hp > 0 || at.side == t.side {
            return;
        }
        let v = &self.fm(t).volatile;
        let (bond, rancor) = (v.vengeful, v.rancor);
        if rancor
            && let Some(i) = index
            && self.alive(at)
            && let Some(slot) = self.fm(at).moves.get_mut(i)
        {
            let amount = slot.pp;
            slot.pp = 0;
            let move_id = slot.data.id;
            ev.push(Event::PpCut {
                target: at,
                move_id,
                amount,
            });
        }
        if bond && self.alive(at) {
            let f = self.fm(at);
            let damage = f.hurt(u32::from(f.hp));
            ev.push(Event::Hurt {
                target: at,
                damage,
                hp: 0,
                by: HurtBy::Vengeance,
            });
        }
    }

    /// An absorbing ability takes a move in.
    pub(super) fn absorb(&mut self, t: Slot, a: Absorb, ev: &mut Vec<Event>) {
        let f = self.fm(t);
        let ability = f.ability;
        ev.push(Event::Ability { at: t, ability });
        match a {
            Absorb::Stat(stat, by) => {
                self.raise_stat(t, stat, by, true, ev);
            }
            Absorb::Heal => {
                if f.hp == f.stats.hp {
                    ev.push(Event::NoEffect { target: t });
                } else {
                    let amount = f.heal((u32::from(f.stats.hp) / 4).max(1));
                    ev.push(Event::Healed {
                        target: t,
                        amount,
                        hp: f.hp,
                    });
                }
            }
            Absorb::FireBoost => {
                if f.volatile.fire_boost {
                    ev.push(Event::NoEffect { target: t });
                } else {
                    f.volatile.fire_boost = true;
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::FireBoost,
                    });
                }
            }
        }
    }

    /// The target's touch ability works on the attacker.
    fn contact(&mut self, at: Slot, t: Slot, ev: &mut Vec<Event>) {
        if !self.alive(at) {
            return;
        }
        let ability = self.fm(t).ability;
        let Some(c) = abilities::on_contact(ability) else {
            return;
        };
        let ailment = match c {
            Contact::Hurt => {
                ev.push(Event::Ability { at: t, ability });
                let f = self.fm(at);
                let damage = f.hurt((u32::from(f.stats.hp) / 8).max(1));
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: f.hp,
                    by: HurtBy::Ability(ability),
                });
                return;
            }
            Contact::Ailment(a, n) => {
                if self.rng.below(n) != 0 {
                    return;
                }
                a
            }
            Contact::Spores => {
                if self.fm(at).has_type(types::GRASS)
                    || self.fm(at).ability == abilities::WEATHER_POWDER_GUARD
                {
                    return;
                }
                if self.rng.below(100) >= 30 {
                    return;
                }
                [Ailment::Sleep, Ailment::Poison, Ailment::Paralysis][self.rng.below(3) as usize]
            }
        };
        if self.can_take(t, at, ailment) {
            ev.push(Event::Ability { at: t, ability });
            self.set_ailment(at, ailment, Some(t), ev);
        }
    }

    /// A damaging move's added effect, if the roll comes up. `decoyed`: a decoy took the hit
    /// (only effects on the user still happen).
    fn added_effect(
        &mut self,
        at: Slot,
        t: Slot,
        mv: &MoveData,
        decoyed: bool,
        ev: &mut Vec<Event>,
    ) {
        self.metadata_added_effect(at, t, mv, decoyed, ev);
        if mv.effect == effects::PARALYSIS_POWER_CURE
            && self.alive(t)
            && !decoyed
            && self.fm(t).ailment == Some(Ailment::Paralysis)
        {
            self.fm(t).ailment = None;
            ev.push(Event::Cured {
                target: t,
                ailment: Ailment::Paralysis,
            });
        }
        let Some(added) =
            effects::added(mv.effect).or(effects::certain(mv.effect).then_some(Added::Flinch))
        else {
            return;
        };
        let user_ability = self.fighter(at).map_or(0, |f| f.ability);
        let chance = abilities::effect_chance(user_ability, mv.chance);
        if !effects::certain(mv.effect) && (mv.chance == 0 || self.rng.below(100) >= chance) {
            return;
        }
        match added {
            Added::User(changes) => {
                if self.alive(at) {
                    for (stat, by) in changes.iter() {
                        self.raise_stat(at, stat, by, false, ev);
                    }
                }
                return;
            }
            Added::UserAllUp => {
                if self.alive(at) {
                    for stat in effects::ALL_FIVE {
                        self.raise_stat(at, stat, 1, false, ev);
                    }
                }
                return;
            }
            _ => {}
        }
        if !self.alive(t) || decoyed || abilities::blocks_added_effects(self.fm(t).ability) {
            return;
        }
        let a = match added {
            Added::Ailment(a) => a,
            Added::AnyOfThree => {
                [Ailment::Burn, Ailment::Freeze, Ailment::Paralysis][self.rng.below(3) as usize]
            }
            Added::Flinch => {
                // Only a target that hasn't moved yet flinches.
                let f = self.fm(t);
                if !abilities::prevents_flinch(f.ability) && !f.volatile.turn.acted {
                    f.volatile.flinched = true;
                }
                return;
            }
            Added::Confuse => {
                let f = self.fm(t);
                if f.volatile.confusion == 0
                    && !abilities::prevents_confusion(f.ability)
                    && !self.guarded(at, t)
                {
                    self.confuse(t, ev);
                }
                return;
            }
            Added::Target(stat, by) => {
                self.lower_stat(at, t, stat, by, false, ev);
                return;
            }
            Added::Bind => {
                if self.fm(t).volatile.bound.is_none() {
                    let turns = self.rng.below(2) as u8 + 5;
                    self.fm(t).volatile.bound = Some((turns, at, mv.id));
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::Bound(mv.id),
                    });
                }
                return;
            }
            Added::User(_) | Added::UserAllUp => return,
        };
        if self.can_take(at, t, a) {
            self.set_ailment(t, a, Some(at), ev);
        }
    }

    fn metadata_added_effect(
        &mut self,
        at: Slot,
        t: Slot,
        mv: &MoveData,
        decoyed: bool,
        ev: &mut Vec<Event>,
    ) {
        let receiver = if mv.details.self_stats { at } else { t };
        if self.alive(receiver)
            && (mv.details.self_stats
                || (!decoyed && !abilities::blocks_added_effects(self.fm(t).ability)))
        {
            for i in 0..7 {
                let by = mv.details.stages[i];
                let chance =
                    abilities::effect_chance(self.fm(at).ability, mv.details.stage_chances[i]);
                if by == 0 || chance == 0 || self.rng.below(100) >= chance {
                    continue;
                }
                if by < 0 && receiver != at {
                    self.lower_stat(at, receiver, Stat::ALL[i], by, false, ev);
                } else {
                    self.raise_stat(receiver, Stat::ALL[i], by, false, ev);
                }
            }
        }
        if !self.alive(t) || decoyed || abilities::blocks_added_effects(self.fm(t).ability) {
            return;
        }
        let chance = abilities::effect_chance(self.fm(at).ability, mv.details.inflict_chance);
        if let Some(status) = mv.details.inflict
            && chance > 0
            && self.rng.below(100) < chance
            && self.can_take(at, t, status)
        {
            self.set_ailment(t, status, Some(at), ev);
        }
        let chance = abilities::effect_chance(self.fm(at).ability, mv.details.confuse_chance);
        if chance > 0
            && self.rng.below(100) < chance
            && self.fm(t).volatile.confusion == 0
            && !abilities::prevents_confusion(self.fm(t).ability)
            && !(self.fm(t).grounded() && self.field.terrain == super::super::field::Terrain::Hazy)
        {
            self.confuse(t, ev);
        }
        let chance = abilities::effect_chance(self.fm(at).ability, mv.details.flinch_chance);
        let chance = if chance == 0
            && self.fm(at).ability == 1
            && effects::added(mv.effect) != Some(Added::Flinch)
        {
            10
        } else {
            chance
        };
        if chance > 0
            && self.rng.below(100) < chance
            && !self.fm(t).volatile.turn.acted
            && !abilities::prevents_flinch(self.fm(t).ability)
        {
            self.fm(t).volatile.flinched = true;
        }
    }

    /// Whether `a` sticks on `t` from `by` without a word: no status yet, types, ability,
    /// status guard and noise allow it.
    pub(super) fn can_take(&self, by: Slot, t: Slot, a: Ailment) -> bool {
        let Some(f) = self.fighter(t) else {
            return false;
        };
        f.ailment.is_none()
            && !f.fainted()
            && self.type_allows(f, a)
            && !abilities::prevents_ailment(f.ability, a)
            && !self.guarded(by, t)
            && !(a == Ailment::Sleep && self.noise_on())
    }

    /// Frees the user from binding, seeds and its side's hazards.
    fn spin(&mut self, at: Slot, ev: &mut Vec<Event>) {
        let v = &mut self.fm(at).volatile;
        let freed = v.bound.is_some() || v.seeded_by.is_some();
        v.bound = None;
        v.seeded_by = None;
        let hazard = self.field.sides[at.side].hazards > 0;
        self.field.sides[at.side].hazards = 0;
        if freed || hazard {
            ev.push(Event::Freed { at });
        }
    }

    /// Damage for one hit with a given roll (0..=15) and crit, and the type multiplier.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn damage_with(
        &self,
        at: Slot,
        target: Slot,
        mv: &MoveData,
        move_type: u8,
        power: u32,
        out_of_pp_move: bool,
        crit: bool,
        random: u32,
        mult: u32,
    ) -> (i32, u32) {
        let (Some(user), Some(tgt)) = (self.fighter(at), self.fighter(target)) else {
            return (0, 100);
        };
        let side = &self.field.sides[target.side];
        let screen = if mv.category == super::super::data::Category::Physical {
            side.physical_wall > 0
        } else {
            side.special_wall > 0
        };
        let up = (0..self.format.slots())
            .filter(|&p| self.alive(Slot::new(target.side, p)))
            .count();
        let doubles = self.format == Format::Doubles;
        let on_field: Vec<&Fighter> = self
            .on_field()
            .into_iter()
            .filter_map(|s| self.fighter(s))
            .collect();
        let pair = match user.ability {
            abilities::PAIR_BOOST_A => abilities::PAIR_BOOST_B,
            abilities::PAIR_BOOST_B => abilities::PAIR_BOOST_A,
            _ => abilities::NONE,
        };
        let a = Attack {
            user,
            target: tgt,
            move_type,
            category: mv.category,
            move_id: mv.id,
            terrain: self.field.terrain,
            flags: mv.flags
                | if self.field.swapped_defenses > 0 {
                    flags::SWAP_DEFENSE_STATS
                } else {
                    0
                },
            grounded_override: self.grounding_override(target),
            power,
            effect: mv.effect,
            recoil: mv.details.recoil_percent > 0 || effects::recoil(mv.effect).is_some(),
            crit,
            weather: self.weather(),
            screen,
            crowded: doubles && up == 2,
            spread: doubles
                && ((mv.target == Target::BothFoes && up == 2)
                    || (mv.target == Target::AllOthers && on_field.len() > 2)),
            mult,
            charged: user.volatile.powered > 0,
            helped: user.volatile.turn.helped,
            paired: pair != abilities::NONE
                && self
                    .ally(at)
                    .and_then(|s| self.fighter(s))
                    .is_some_and(|f| {
                        matches!(f.ability, abilities::PAIR_BOOST_A | abilities::PAIR_BOOST_B)
                    }),
            dull_electric: on_field.iter().any(|f| f.volatile.dull_electric),
            dull_fire: on_field.iter().any(|f| f.volatile.dull_fire),
        };
        let base = damage::base(&a);
        damage::finish(base, &a, (!out_of_pp_move).then_some(&self.chart), random)
    }

    fn roll_crit(&mut self, at: Slot, target: Slot, mv: &MoveData) -> bool {
        let (Some(user), Some(tgt)) = (self.fighter(at), self.fighter(target)) else {
            return false;
        };
        if abilities::prevents_crits(tgt.ability) || self.field.sides[target.side].crit_guard > 0 {
            return false;
        }
        if mv.flags & flags::ALWAYS_CRITICAL != 0 {
            return true;
        }
        let stage = effects::crit_stages(mv.effect) + user.volatile.crit_stages;
        let stage = stage + u8::from(user.ability == abilities::CRIT_STAGE_UP);
        let chance = damage::crit_chance(stage, user.luck);
        self.rng.below(4096) < chance
    }

    /// Damage move `index` of the fighter at `at` would do to `target` with an average roll
    /// and no critical hit, and the type multiplier (hundredths); for the AI and hints.
    pub fn estimate(&self, at: Slot, index: usize, target: Slot) -> (u32, u32) {
        let Some(mv) = self
            .fighter(at)
            .and_then(|f| f.moves.get(index))
            .map(|m| m.data)
        else {
            return (0, 100);
        };
        let (Some(user), Some(tgt)) = (self.fighter(at), self.fighter(target)) else {
            return (0, 100);
        };
        let weather = self.weather();
        let (mut ty, mut power) = (mv.type_id, u32::from(mv.power));
        match mv.effect {
            effects::STAT_BITS_POWER => {
                let (t, _) = effects::stat_bits_power(user);
                (ty, power) = (t, 60);
            }
            effects::WEATHER_POWER => {
                if let Some(t) = effects::weather_power(weather) {
                    (ty, power) = (t, power * 2);
                }
            }
            effects::LOW_HP_POWER => {
                power = u32::from(effects::low_hp_power(user.hp, user.stats.hp))
            }
            effects::BOND_POWER | effects::BOND_POWER_INVERSE => {
                power = u32::from(effects::bond_power(
                    user.friendship,
                    mv.effect == effects::BOND_POWER_INVERSE,
                ))
            }
            effects::HP_POWER => power = effects::hp_power(mv.power, user.hp, user.stats.hp),
            effects::RANDOM_POWER => power = 71,
            effects::RANDOM_GIFT => power = 52,
            effects::WEIGHT_POWER => {
                power = match tgt.battle_weight() {
                    0..=99 => 20,
                    100..=249 => 40,
                    250..=499 => 60,
                    500..=999 => 80,
                    1000..=1999 => 100,
                    _ => 120,
                }
            }
            effects::RELEASE_HIT => power = 100 * u32::from(user.volatile.stored),
            effects::MULTI_HIT => power *= 3,
            effects::TWO_HITS | effects::TWO_HITS_POISON_CHANCE => power *= 2,
            effects::TRIPLE_RISING => power *= 6,
            _ => {}
        }
        power = self.ordinary_power(at, target, &mv, power);
        let tf = tgt;
        if (ty == types::GROUND && !self.grounding_override(target) && !tf.grounded())
            || (!self.grounding_override(target) && abilities::immune_to_type(tf.ability, ty))
            || abilities::absorbs(tf.ability, ty, true, false).is_some()
        {
            return (0, 0);
        }
        let m = self
            .chart
            .multiplier(ty, self.effective_types(target, ty), tf.volatile.identified);
        if tf.ability == abilities::ONLY_SUPER_EFFECTIVE && m <= 100 {
            return (0, 0);
        }
        let fixed = match mv.effect {
            effects::ONE_HIT_KO => (user.level >= tf.level
                && tf.ability != abilities::NO_ONE_HIT_KO)
                .then_some(u32::from(tf.hp) * u32::from(mv.accuracy.max(1)) / 100),
            effects::RETURN_PHYSICAL | effects::RETURN_SPECIAL | effects::STORE_DAMAGE => Some(0),
            e if effects::is_fixed_damage(e) => {
                effects::fixed_damage(e, user, tf, &mut super::super::rng::Rng::new(0))
            }
            _ => None,
        };
        if let Some(fixed) = fixed {
            return (if m == 0 { 0 } else { fixed }, m.min(100));
        }
        if power == 0 {
            return (0, 100);
        }
        let crit = mv.flags & flags::ALWAYS_CRITICAL != 0
            && !abilities::prevents_crits(tf.ability)
            && self.field.sides[target.side].crit_guard == 0;
        let (d, m) = self.damage_with(at, target, &mv, ty, power, false, crit, 7, 1);
        (d.max(0) as u32, m)
    }

    /// Sets a delayed attack on the target's slot, its damage worked out now.
    fn delay(&mut self, at: Slot, mv: &MoveData, t: Slot, ev: &mut Vec<Event>) {
        if self.field.delayed[t.side][t.pos].is_some() {
            ev.push(Event::Failed { by: at });
            return;
        }
        let (Some(user), Some(tgt)) = (self.fighter(at), self.fighter(t)) else {
            return;
        };
        let mut a = Attack::new(user, tgt, mv.type_id, u32::from(mv.power));
        a.category = mv.category;
        a.move_id = mv.id;
        a.terrain = self.field.terrain;
        a.flags = mv.flags;
        a.grounded_override = self.grounding_override(t);
        a.weather = self.weather();
        let mut d = damage::base(&a).max(0) as u32;
        if user.volatile.turn.helped {
            d = d * 15 / 10;
        }
        self.field.delayed[t.side][t.pos] = Some(Delayed {
            turns: 3,
            damage: d,
            move_id: mv.id,
            accuracy: mv.accuracy,
        });
        ev.push(Event::Started {
            target: t,
            condition: Condition::Delayed(mv.id),
        });
    }

    /// One hit for each healthy member of the user's team, from that member's attack.
    fn team_hits(&mut self, at: Slot, mv: &MoveData, t: Slot, ev: &mut Vec<Event>) {
        if self.lands(at, t, mv, ev).is_none() {
            return;
        }
        let strikers: Vec<usize> = (0..self.teams[at.side].len())
            .filter(|&m| {
                let f = &self.teams[at.side][m];
                !f.fainted() && f.ailment.is_none()
            })
            .collect();
        if strikers.is_empty() {
            ev.push(Event::Failed { by: at });
            return;
        }
        let helped = self.fm(at).volatile.turn.helped;
        let mut count = 0;
        for m in strikers {
            if !self.alive(t) {
                break;
            }
            let f = &self.teams[at.side][m];
            let level = 2 * u32::from(f.level) / 5 + 2;
            let def = u32::from(self.fighter(t).map_or(1, |x| x.stats.defense).max(1));
            let mut d = u32::from(f.stats.attack) * u32::from(mv.power) * level / def / 50 + 2;
            if helped {
                d = d * 15 / 10;
            }
            d = d * (100 - self.rng.below(16)) / 100;
            ev.push(Event::TeamHit { member: m });
            self.deal(
                at,
                t,
                mv,
                types::TYPELESS,
                d.max(1),
                false,
                Effectiveness::Normal,
                ev,
            );
            count += 1;
        }
        ev.push(Event::HitTimes { count });
        self.report_faints(ev);
    }

    /// The hits of delayed attacks whose time has come.
    pub(super) fn delayed_hits(&mut self, ev: &mut Vec<Event>) {
        for at in self.slots() {
            let Some(mut d) = self.field.delayed[at.side][at.pos] else {
                continue;
            };
            d.turns = d.turns.saturating_sub(1);
            if d.turns > 0 {
                self.field.delayed[at.side][at.pos] = Some(d);
                continue;
            }
            self.field.delayed[at.side][at.pos] = None;
            if !self.alive(at) {
                continue;
            }
            let hits = d.accuracy == 0 || self.rng.below(100) < u32::from(d.accuracy);
            if !hits {
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::Delayed(d.move_id),
                });
                continue;
            }
            let roll = 100 - self.rng.below(16);
            let f = self.fm(at);
            let damage = f.hurt((d.damage * roll / 100).max(1));
            ev.push(Event::Hurt {
                target: at,
                damage,
                hp: f.hp,
                by: HurtBy::Delayed(d.move_id),
            });
            self.report_faints(ev);
        }
    }

    /// Whether `t`'s side has a status guard against `by`.
    pub(super) fn guarded(&self, by: Slot, t: Slot) -> bool {
        by.side != t.side && self.field.sides[t.side].status_guard > 0
    }

    /// Whether the type rules let `a` stick: poison and steel types can't be poisoned, fire
    /// types burned, ice types frozen (nor anyone in sun).
    pub(super) fn type_allows(&self, f: &Fighter, a: Ailment) -> bool {
        if f.ability == abilities::SUN_STATUS_GUARD && self.weather() == Weather::Sun {
            return false;
        }
        if f.grounded()
            && (self.field.terrain == super::super::field::Terrain::Hazy
                || (a == Ailment::Sleep
                    && self.field.terrain == super::super::field::Terrain::Charged))
        {
            return false;
        }
        match a {
            Ailment::Paralysis => !f.has_type(types::ELECTRIC),
            Ailment::Poison | Ailment::BadPoison => {
                !f.has_type(types::POISON) && !f.has_type(types::STEEL)
            }
            Ailment::Burn => !f.has_type(types::FIRE),
            Ailment::Freeze => !f.has_type(types::ICE) && self.weather() != Weather::Sun,
            _ => true,
        }
    }
}

/// `mv` never misses a shrunk target and hits it twice as hard.
fn punishes_shrink(mv: &MoveData) -> bool {
    mv.effect == effects::FLINCH_CHANCE_C || mv.flags & flags::PUNISHES_SHRINK != 0
}
