//! Status moves, and the stat, status and condition changes moves cause.

use super::super::abilities;
use super::super::data::{MoveData, flags, types};
use super::super::effects::{self, Changes, Primary};
use super::super::event::{Condition, Event, HurtBy, Slot};
use super::super::field::{HAZARD_MAX, PendingHeal, SCREEN_TURNS, WEATHER_TURNS};
use super::super::fighter::{Ailment, MoveSlot, STAGE_MAX, Stat};
use super::Battle;

impl Battle {
    /// A move without normal damage: a foe's stealing or bouncing move may take it, then it
    /// works on its user or each target.
    pub(super) fn status_move(
        &mut self,
        at: Slot,
        mv: &MoveData,
        targets: &[Slot],
        ev: &mut Vec<Event>,
    ) {
        let Some(p) = effects::primary(mv.effect) else {
            ev.push(Event::Nothing);
            return;
        };
        if mv.flags & flags::STEALABLE != 0
            && let Some(taker) = self
                .foes(at)
                .into_iter()
                .find(|&s| self.fm(s).volatile.turn.stealing)
        {
            self.fm(taker).volatile.turn.stealing = false;
            ev.push(Event::Stolen { by: taker });
            self.self_effect(taker, mv, p, &[taker], ev);
            return;
        }
        if !p.on_target() {
            self.self_effect(at, mv, p, targets, ev);
            return;
        }
        for &t in targets {
            if !self.alive(t) {
                continue;
            }
            let (user, target) = if mv.flags & flags::REFLECTABLE != 0
                && t != at
                && self.fm(t).volatile.turn.bouncing
            {
                self.fm(t).volatile.turn.bouncing = false;
                ev.push(Event::Bounced { by: t });
                (t, at)
            } else {
                (at, t)
            };
            if !self.reach_status(user, target, mv, ev) {
                continue;
            }
            if p.stopped_by_decoy() && user != target && self.fm(target).volatile.decoy > 0 {
                ev.push(Event::Failed { by: user });
                continue;
            }
            self.target_effect(user, target, mv, p, ev);
        }
    }

    /// Whether a status move reaches `t`: shields, hiding, the hit roll, a sound-blocking or
    /// fire-taking ability.
    fn reach_status(&mut self, at: Slot, t: Slot, mv: &MoveData, ev: &mut Vec<Event>) -> bool {
        if at == t {
            return true;
        }
        let ability = self.fm(t).ability;
        if abilities::blocks_sound(ability) && self.sound_moves.contains(&mv.id) {
            ev.push(Event::Ability { at: t, ability });
            ev.push(Event::NoEffect { target: t });
            return false;
        }
        let frozen = self.fm(t).ailment == Some(Ailment::Freeze);
        if abilities::absorbs(ability, mv.type_id, false, frozen).is_some() {
            if mv.flags & flags::BLOCKED_BY_PROTECTION != 0 && self.fm(t).volatile.turn.shielded {
                ev.push(Event::Shielded { target: t });
                return false;
            }
            let f = self.fm(t);
            ev.push(Event::Ability { at: t, ability });
            if !f.volatile.fire_boost {
                f.volatile.fire_boost = true;
                ev.push(Event::Started {
                    target: t,
                    condition: Condition::FireBoost,
                });
            }
            return false;
        }
        self.lands(at, t, mv, ev).is_some()
    }

    /// A status move on its user or the field.
    fn self_effect(
        &mut self,
        at: Slot,
        mv: &MoveData,
        p: Primary,
        targets: &[Slot],
        ev: &mut Vec<Event>,
    ) {
        let fail = |ev: &mut Vec<Event>| ev.push(Event::Failed { by: at });
        match p {
            Primary::Stats(_, changes) => self.raise_or_fail(at, changes, ev),
            Primary::Weather(w) => {
                if self.field.weather == w {
                    fail(ev);
                } else {
                    self.field.weather = w;
                    self.field.weather_turns = WEATHER_TURNS;
                    ev.push(Event::WeatherStarted { weather: w });
                    self.weather_types(ev);
                }
            }
            Primary::Screen(s) => {
                let side = &mut self.field.sides[at.side];
                if side.get(s) > 0 {
                    fail(ev);
                } else {
                    side.set(s, SCREEN_TURNS);
                    ev.push(Event::ScreenUp {
                        side: at.side,
                        screen: s,
                    });
                }
            }
            Primary::Heal(n, d) => self.heal_share(at, n, d, ev),
            Primary::WeatherHeal => {
                let (n, d) = effects::weather_heal(self.weather());
                self.heal_share(at, n, d, ev);
            }
            Primary::SleepHeal => {
                let f = self.fm(at);
                if f.hp == f.stats.hp || abilities::prevents_ailment(f.ability, Ailment::Sleep) {
                    fail(ev);
                    return;
                }
                if let Some(old) = f.ailment.filter(|&a| a != Ailment::Sleep) {
                    ev.push(Event::Cured {
                        target: at,
                        ailment: old,
                    });
                }
                f.ailment = Some(Ailment::Sleep);
                f.sleep_turns = 3;
                ev.push(Event::Ailment {
                    target: at,
                    ailment: Ailment::Sleep,
                });
                let amount = f.heal(u32::from(f.stats.hp));
                ev.push(Event::Healed {
                    target: at,
                    amount,
                    hp: f.hp,
                });
            }
            Primary::Focus => {
                let f = self.fm(at);
                if f.volatile.crit_stages >= 2 {
                    fail(ev);
                } else {
                    f.volatile.crit_stages = 2;
                    ev.push(Event::Focused { target: at });
                }
            }
            Primary::ResetStages => {
                for s in self.slots() {
                    if let Some(m) = self.member(s) {
                        self.teams[s.side][m].volatile.stages = Default::default();
                    }
                }
                ev.push(Event::StagesReset);
            }
            Primary::Nothing => ev.push(Event::Nothing),
            Primary::Fails => fail(ev),
            Primary::Decoy => {
                let f = self.fm(at);
                let cost = (u32::from(f.stats.hp) / 4).max(1);
                if f.volatile.decoy > 0 || u32::from(f.hp) <= cost {
                    fail(ev);
                    return;
                }
                let damage = f.hurt(cost);
                f.volatile.decoy = damage;
                f.volatile.bound = None;
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: f.hp,
                    by: HurtBy::Cost,
                });
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Decoy,
                });
            }
            Primary::Shield | Primary::Brace => {
                if self.shield_roll(at) {
                    let v = &mut self.fm(at).volatile;
                    v.shield_streak = v.shield_streak.saturating_add(1);
                    let condition = if p == Primary::Shield {
                        v.turn.shielded = true;
                        Condition::Shielded
                    } else {
                        v.turn.bracing = true;
                        Condition::Bracing
                    };
                    ev.push(Event::Started {
                        target: at,
                        condition,
                    });
                } else {
                    self.fm(at).volatile.shield_streak = 0;
                    fail(ev);
                }
            }
            Primary::Hazard => {
                let side = &mut self.field.sides[1 - at.side];
                if side.hazards >= HAZARD_MAX {
                    fail(ev);
                } else {
                    side.hazards += 1;
                    ev.push(Event::HazardLaid {
                        side: 1 - at.side,
                        layers: side.hazards,
                    });
                }
            }
            Primary::Countdown => {
                let mut any = false;
                for s in self.on_field() {
                    let f = self.fm(s);
                    if f.volatile.countdown > 0 || abilities::blocks_sound(f.ability) {
                        continue;
                    }
                    f.volatile.countdown = 4;
                    any = true;
                }
                if any {
                    ev.push(Event::Started {
                        target: at,
                        condition: Condition::Countdown(3),
                    });
                } else {
                    fail(ev);
                }
            }
            Primary::Vengeance => {
                self.fm(at).volatile.vengeful = true;
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Vengeful,
                });
            }
            Primary::PpRancor => {
                self.fm(at).volatile.rancor = true;
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Rancor,
                });
            }
            Primary::CureTeam => {
                let sound = self.sound_moves.contains(&mv.id);
                let active = self.active[at.side];
                for (m, f) in self.teams[at.side].iter_mut().enumerate() {
                    if sound && abilities::blocks_sound(f.ability) && active.contains(&Some(m)) {
                        continue;
                    }
                    f.ailment = None;
                    f.volatile.bad_dreams = false;
                }
                ev.push(Event::TeamCured { side: at.side });
            }
            Primary::Shrink => {
                self.fm(at).volatile.shrunk = true;
                self.raise_stat(at, Stat::Evasion, 1, true, ev);
            }
            Primary::Curl => {
                self.fm(at).volatile.curled = true;
                self.raise_stat(at, Stat::Defense, 1, true, ev);
            }
            Primary::MaxAttack => {
                let f = self.fm(at);
                let cost = u32::from(f.stats.hp) / 2;
                if u32::from(f.hp) <= cost || f.stage(Stat::Attack) >= STAGE_MAX {
                    fail(ev);
                    return;
                }
                let damage = f.hurt(cost);
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: f.hp,
                    by: HurtBy::Cost,
                });
                self.raise_stat(at, Stat::Attack, 12, false, ev);
            }
            Primary::PassState => {
                let bench = self.bench_for(at);
                let picked = self
                    .pass_picks
                    .iter()
                    .find(|p| p.0 == at)
                    .map(|p| p.1)
                    .filter(|m| bench.contains(m));
                let best = picked.or_else(|| {
                    bench.into_iter().max_by_key(|&m| {
                        let f = &self.teams[at.side][m];
                        (
                            u32::from(f.hp) * 1000 / u32::from(f.stats.hp.max(1)),
                            std::cmp::Reverse(m),
                        )
                    })
                });
                match best {
                    Some(m) => self.pass_to(at, m, ev),
                    None => fail(ev),
                }
            }
            Primary::StoreUp => {
                let v = &mut self.fm(at).volatile;
                if v.stored >= 3 {
                    fail(ev);
                } else {
                    v.stored += 1;
                    let n = v.stored;
                    ev.push(Event::Started {
                        target: at,
                        condition: Condition::Stored(n),
                    });
                }
            }
            Primary::ReleaseHeal => {
                let n = self.fm(at).volatile.stored;
                if n == 0 {
                    fail(ev);
                    return;
                }
                let f = self.fm(at);
                if f.hp == f.stats.hp {
                    fail(ev);
                } else {
                    let max = u32::from(f.stats.hp);
                    let amount = f.heal(match n {
                        1 => max / 4,
                        2 => max / 2,
                        _ => max,
                    });
                    ev.push(Event::Healed {
                        target: at,
                        amount,
                        hp: f.hp,
                    });
                }
                self.fm(at).volatile.stored = 0;
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::Stored(0),
                });
            }
            Primary::PowerUpElectric => {
                self.fm(at).volatile.powered = 2;
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Powered,
                });
            }
            Primary::HelpAlly => match self.ally(at) {
                Some(a) if !self.fm(a).volatile.turn.acted => {
                    self.fm(a).volatile.turn.helped = true;
                    ev.push(Event::Started {
                        target: a,
                        condition: Condition::Helped,
                    });
                }
                _ => fail(ev),
            },
            Primary::DrawMoves => {
                self.field.sides[at.side].drawing = Some(at.pos);
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Drawing,
                });
            }
            Primary::DelayedHeal => {
                let amount = (self.fm(at).stats.hp / 2).max(1);
                let slot = &mut self.field.heals[at.side][at.pos];
                if slot.is_some() {
                    fail(ev);
                } else {
                    *slot = Some(PendingHeal { turns: 2, amount });
                    ev.push(Event::Started {
                        target: at,
                        condition: Condition::HealPending,
                    });
                }
            }
            Primary::Root => {
                let v = &mut self.fm(at).volatile;
                if v.rooted {
                    fail(ev);
                } else {
                    v.rooted = true;
                    ev.push(Event::Started {
                        target: at,
                        condition: Condition::Rooted,
                    });
                }
            }
            Primary::BounceStatus => {
                self.fm(at).volatile.turn.bouncing = true;
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Bouncing,
                });
            }
            Primary::StealMove => {
                self.fm(at).volatile.turn.stealing = true;
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Stealing,
                });
            }
            Primary::SealShared => {
                let mine: Vec<u16> = self.fm(at).moves.iter().map(|m| m.data.id).collect();
                let shared = self
                    .foes(at)
                    .into_iter()
                    .any(|s| self.fm(s).moves.iter().any(|m| mine.contains(&m.data.id)));
                if self.fm(at).volatile.sealing || !shared {
                    fail(ev);
                } else {
                    self.fm(at).volatile.sealing = true;
                    ev.push(Event::Started {
                        target: at,
                        condition: Condition::Sealing,
                    });
                }
            }
            Primary::CureSelf => {
                let f = self.fm(at);
                match f.ailment {
                    Some(
                        a @ (Ailment::Poison
                        | Ailment::BadPoison
                        | Ailment::Burn
                        | Ailment::Paralysis),
                    ) => {
                        f.ailment = None;
                        ev.push(Event::Cured {
                            target: at,
                            ailment: a,
                        });
                    }
                    _ => fail(ev),
                }
            }
            Primary::ConfuseAll => {
                for &t in targets {
                    if !self.alive(t) || !self.reach_status(at, t, mv, ev) {
                        continue;
                    }
                    self.try_confuse(at, t, ev);
                }
            }
            Primary::Dull(electric) => {
                let v = &mut self.fm(at).volatile;
                let set = if electric {
                    &mut v.dull_electric
                } else {
                    &mut v.dull_fire
                };
                if *set {
                    fail(ev);
                } else {
                    *set = true;
                    ev.push(Event::Started {
                        target: at,
                        condition: if electric {
                            Condition::ElectricDulled
                        } else {
                            Condition::FireDulled
                        },
                    });
                }
            }
            Primary::ArenaType => self.change_type(at, types::NORMAL, ev),
            Primary::HexOrBoost => {
                if self.fm(at).has_type(types::GHOST) {
                    let Some(&t) = targets.iter().find(|&&t| t != at && self.alive(t)) else {
                        fail(ev);
                        return;
                    };
                    let tf = self.fm(t);
                    if tf.volatile.hexed || tf.volatile.decoy > 0 {
                        fail(ev);
                        return;
                    }
                    tf.volatile.hexed = true;
                    let f = self.fm(at);
                    let damage = f.hurt((u32::from(f.stats.hp) / 2).max(1));
                    ev.push(Event::Hurt {
                        target: at,
                        damage,
                        hp: f.hp,
                        by: HurtBy::Cost,
                    });
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::Hexed,
                    });
                } else {
                    let f = self.fm(at);
                    if f.stage(Stat::Attack) >= STAGE_MAX && f.stage(Stat::Defense) >= STAGE_MAX {
                        fail(ev);
                        return;
                    }
                    self.raise_stat(at, Stat::Speed, -1, false, ev);
                    self.raise_stat(at, Stat::Attack, 1, false, ev);
                    self.raise_stat(at, Stat::Defense, 1, false, ev);
                }
            }
            Primary::TypeFromOwnMove => {
                let f = self.fm(at);
                let options: Vec<u8> = f
                    .moves
                    .iter()
                    .map(|m| m.data.type_id)
                    .filter(|&t| t != types::TYPELESS && !f.has_type(t))
                    .collect();
                if options.is_empty() {
                    fail(ev);
                } else {
                    let t = options[self.rng.below(options.len() as u32) as usize];
                    self.change_type(at, t, ev);
                }
            }
            Primary::TypeResistLast => {
                let f = self.fm(at);
                let Some(last) = f.volatile.last_hit_type else {
                    fail(ev);
                    return;
                };
                let current = f.types;
                let options: Vec<u8> = (0..types::COUNT)
                    .filter(|&t| t != types::TYPELESS && !current.contains(&t))
                    .filter(|&t| self.chart.multiplier(last, [t, t], false) < 100)
                    .collect();
                if options.is_empty() {
                    fail(ev);
                } else {
                    let t = options[self.rng.below(options.len() as u32) as usize];
                    self.change_type(at, t, ev);
                }
            }
            // Target effects never get here.
            _ => fail(ev),
        }
    }

    fn change_type(&mut self, at: Slot, t: u8, ev: &mut Vec<Event>) {
        let f = self.fm(at);
        if f.types == [t, t] {
            ev.push(Event::Failed { by: at });
            return;
        }
        f.keep_original(false);
        f.types = [t, t];
        ev.push(Event::TypeChanged {
            target: at,
            types: [t, t],
        });
    }

    /// A status move on one target (already reached).
    fn target_effect(&mut self, at: Slot, t: Slot, mv: &MoveData, p: Primary, ev: &mut Vec<Event>) {
        let fail = |ev: &mut Vec<Event>| ev.push(Event::Failed { by: at });
        match p {
            Primary::Ailment(a) => self.inflict_primary(at, t, a, mv, ev),
            Primary::Confuse => self.try_confuse(at, t, ev),
            Primary::ConfuseAndRaise(stat, by) => {
                let raised = self.raise_stat(t, stat, by, false, ev);
                let f = self.fm(t);
                if f.volatile.confusion > 0 && !raised {
                    fail(ev);
                } else {
                    self.try_confuse(at, t, ev);
                }
            }
            Primary::Stats(_, changes) => {
                for (stat, by) in changes.iter() {
                    if by < 0 {
                        self.lower_stat(at, t, stat, by, true, ev);
                    } else {
                        self.raise_stat(t, stat, by, true, ev);
                    }
                }
            }
            Primary::Seed => {
                let f = self.fm(t);
                if f.has_type(types::GRASS) {
                    ev.push(Event::NoEffect { target: t });
                } else if f.volatile.seeded_by.is_some() {
                    fail(ev);
                } else {
                    f.volatile.seeded_by = Some(at);
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::Seeded,
                    });
                }
            }
            Primary::SealLastMove => {
                let turns = self.rng.below(4) as u8 + 2;
                let f = self.fm(t);
                let last = f
                    .volatile
                    .last_move
                    .filter(|&id| f.moves.iter().any(|m| m.data.id == id && m.pp > 0));
                match last {
                    Some(id) if f.volatile.sealed.is_none() => {
                        f.volatile.sealed = Some((id, turns));
                        ev.push(Event::Started {
                            target: t,
                            condition: Condition::Sealed(id),
                        });
                    }
                    _ => fail(ev),
                }
            }
            Primary::RepeatLast => {
                let turns = self.rng.below(4) as u8 + 3;
                let out_id = self.rules.out_of_pp.id;
                let f = self.fm(t);
                let ok = f.volatile.repeat.is_none()
                    && f.volatile.last_index.is_some_and(|i| {
                        f.moves.get(i).is_some_and(|m| {
                            m.pp > 0
                                && Some(m.data.id) == f.volatile.last_move
                                && m.data.id != out_id
                                && !matches!(
                                    m.data.effect,
                                    effects::REPEAT_LAST
                                        | effects::COPY_MOVE_TEMP
                                        | effects::COPY_FOES_MOVE
                                )
                        })
                    });
                if ok {
                    let index = f.volatile.last_index.unwrap_or(0);
                    f.volatile.repeat = Some((index, turns));
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::Repeating,
                    });
                } else {
                    fail(ev);
                }
            }
            Primary::Provoke => {
                let v = &mut self.fm(t).volatile;
                if v.provoked > 0 {
                    fail(ev);
                } else {
                    v.provoked = 2;
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::Provoked,
                    });
                }
            }
            Primary::NoRepeats => {
                let v = &mut self.fm(t).volatile;
                if v.no_repeats {
                    fail(ev);
                } else {
                    v.no_repeats = true;
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::NoRepeats,
                    });
                }
            }
            Primary::BlockEscape => {
                let v = &mut self.fm(t).volatile;
                if v.held_by.is_some() {
                    fail(ev);
                } else {
                    v.held_by = Some(at);
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::CantEscape,
                    });
                }
            }
            Primary::BadDreams => {
                let f = self.fm(t);
                if f.ailment != Some(Ailment::Sleep) || f.volatile.bad_dreams {
                    fail(ev);
                } else {
                    f.volatile.bad_dreams = true;
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::BadDreams,
                    });
                }
            }
            Primary::Identify => {
                self.fm(t).volatile.identified = true;
                ev.push(Event::Started {
                    target: t,
                    condition: Condition::Identified,
                });
            }
            Primary::SureHitNext => {
                self.fm(t).volatile.sure_hit = Some((at, 2));
                ev.push(Event::Started {
                    target: t,
                    condition: Condition::SureHit(at),
                });
            }
            Primary::CutPp => {
                let amount = self.rng.below(4) as u8 + 2;
                let f = self.fm(t);
                let last = f.volatile.last_move;
                match f
                    .moves
                    .iter_mut()
                    .find(|m| Some(m.data.id) == last && m.pp > 0)
                {
                    Some(m) => {
                        let cut = amount.min(m.pp);
                        m.pp -= cut;
                        let move_id = m.data.id;
                        ev.push(Event::PpCut {
                            target: t,
                            move_id,
                            amount: cut,
                        });
                    }
                    None => fail(ev),
                }
            }
            Primary::ShareHp => {
                let total = u32::from(self.fm(at).hp) + u32::from(self.fm(t).hp);
                let share = (total / 2) as u16;
                for s in [at, t] {
                    let f = self.fm(s);
                    f.hp = share.min(f.stats.hp);
                    ev.push(Event::HpSet {
                        target: s,
                        hp: f.hp,
                    });
                }
            }
            Primary::CopyStages => {
                let stages = self.fm(t).volatile.stages;
                self.fm(at).volatile.stages = stages;
                ev.push(Event::StagesCopied { by: at, from: t });
            }
            Primary::CopyAbility => {
                let ability = self.fm(t).ability;
                if abilities::uncopyable(ability) || self.fm(at).ability == ability {
                    fail(ev);
                } else {
                    self.set_ability(at, ability, ev);
                }
            }
            Primary::SwapAbilities => {
                let (a, b) = (self.fm(at).ability, self.fm(t).ability);
                let either = |x| a == x || b == x;
                if either(abilities::ONLY_SUPER_EFFECTIVE)
                    || (a, b) == (abilities::NONE, abilities::NONE)
                {
                    fail(ev);
                } else {
                    self.set_ability(at, b, ev);
                    self.set_ability(t, a, ev);
                }
            }
            Primary::TakeForm => {
                let other = self.fm(t).clone();
                if other
                    .volatile
                    .original
                    .as_ref()
                    .is_some_and(|o| o.moves.is_some())
                    || other.volatile.hidden.is_some()
                {
                    fail(ev);
                    return;
                }
                let f = self.fm(at);
                f.keep_original(true);
                f.types = other.types;
                f.ability = other.ability;
                let hp = f.stats.hp;
                f.stats = other.stats;
                f.stats.hp = hp;
                f.volatile.stages = other.volatile.stages;
                f.volatile.copied_slot = None;
                f.moves = other
                    .moves
                    .iter()
                    .map(|m| MoveSlot {
                        data: m.data,
                        pp: m.data.pp.min(5),
                    })
                    .collect();
                ev.push(Event::TookForm { by: at, into: t });
            }
            Primary::CopyMove(keep) => {
                let out_id = self.rules.out_of_pp.id;
                let last = self.fm(t).volatile.last_move;
                let index = self.fm(at).volatile.last_index;
                let data = last.and_then(|id| self.find_move(id));
                let f = self.fm(at);
                let ok = data.filter(|d| {
                    d.id != out_id
                        && !matches!(
                            d.effect,
                            effects::RANDOM_MOVE
                                | effects::COPY_MOVE_KEEP
                                | effects::COPY_MOVE_TEMP
                        )
                        && !f.moves.iter().any(|m| m.data.id == d.id)
                });
                match (ok, index) {
                    (Some(d), Some(i)) if i < f.moves.len() => {
                        let old = f.moves[i];
                        if keep {
                            f.moves[i] = MoveSlot { data: d, pp: d.pp };
                        } else {
                            if f.volatile.copied_slot.is_none() {
                                f.volatile.copied_slot = Some((i, old));
                            }
                            f.moves[i] = MoveSlot {
                                data: d,
                                pp: d.pp.min(5),
                            };
                        }
                        ev.push(Event::MoveCopied {
                            by: at,
                            move_id: d.id,
                        });
                    }
                    _ => fail(ev),
                }
            }
            Primary::Drowse => {
                let f = self.fm(t);
                if f.volatile.drowsy > 0 || !self.can_take(at, t, Ailment::Sleep) {
                    fail(ev);
                } else {
                    self.fm(t).volatile.drowsy = 2;
                    ev.push(Event::Started {
                        target: t,
                        condition: Condition::Drowsy,
                    });
                }
            }
            Primary::ForceSwitch => {
                let f = self.fm(t);
                if f.ability == abilities::NO_FORCED_SWITCH {
                    let ability = f.ability;
                    ev.push(Event::Ability { at: t, ability });
                    fail(ev);
                } else if f.volatile.rooted || !self.force_out(t, ev) {
                    fail(ev);
                }
            }
            Primary::FaintLowerTarget => {
                self.lower_stat(at, t, Stat::Attack, -2, true, ev);
                self.lower_stat(at, t, Stat::SpAttack, -2, true, ev);
                let f = self.fm(at);
                let damage = f.hurt(u32::from(f.hp));
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: 0,
                    by: HurtBy::Sacrifice,
                });
            }
            Primary::Infatuate => ev.push(Event::NoEffect { target: t }),
            // User and field effects never get here.
            _ => fail(ev),
        }
    }

    /// Confuses `t` if it can be.
    fn try_confuse(&mut self, at: Slot, t: Slot, ev: &mut Vec<Event>) {
        let f = self.fm(t);
        if f.volatile.confusion > 0 {
            ev.push(Event::AlreadyConfused { target: t });
        } else if abilities::prevents_confusion(f.ability) {
            let ability = f.ability;
            ev.push(Event::Ability { at: t, ability });
            ev.push(Event::NoEffect { target: t });
        } else if (at != t && self.fm(t).volatile.decoy > 0) || self.guarded(at, t) {
            ev.push(Event::Failed { by: at });
        } else {
            self.confuse(t, ev);
        }
    }

    fn heal_share(&mut self, at: Slot, n: u32, d: u32, ev: &mut Vec<Event>) {
        let f = self.fm(at);
        if f.hp == f.stats.hp {
            ev.push(Event::Failed { by: at });
            return;
        }
        let amount = f.heal((u32::from(f.stats.hp) * n / d).max(1));
        ev.push(Event::Healed {
            target: at,
            amount,
            hp: f.hp,
        });
    }

    /// Stage changes from a status move on its user; each one already at its limit says so.
    fn raise_or_fail(&mut self, at: Slot, changes: Changes, ev: &mut Vec<Event>) {
        for (stat, by) in changes.iter() {
            self.raise_stat(at, stat, by, true, ev);
        }
    }

    /// A stage change the fighter takes from itself (or one nothing guards against, up or
    /// down). Returns whether the stage moved; `show_limit` reports a stage already at its
    /// limit.
    pub(super) fn raise_stat(
        &mut self,
        at: Slot,
        stat: Stat,
        by: i8,
        show_limit: bool,
        ev: &mut Vec<Event>,
    ) -> bool {
        let moved = self.fm(at).change_stage(stat, by);
        if moved != 0 {
            ev.push(Event::StatChanged {
                target: at,
                stat,
                by: moved,
            });
            true
        } else {
            if show_limit {
                ev.push(Event::StatAtLimit {
                    target: at,
                    stat,
                    up: by > 0,
                });
            }
            false
        }
    }

    /// A stage drop from another fighter: the side's stat guard and drop-blocking abilities
    /// stop it (`show` reports why).
    pub(super) fn lower_stat(
        &mut self,
        by: Slot,
        t: Slot,
        stat: Stat,
        amount: i8,
        show: bool,
        ev: &mut Vec<Event>,
    ) -> bool {
        if by != t {
            if self.field.sides[t.side].stat_guard > 0 && by.side != t.side {
                if show {
                    ev.push(Event::StatProtected { target: t });
                }
                return false;
            }
            let ability = self.fm(t).ability;
            if abilities::prevents_drop(ability, stat) {
                if show {
                    ev.push(Event::Ability { at: t, ability });
                    ev.push(Event::StatProtected { target: t });
                }
                return false;
            }
        }
        self.raise_stat(t, stat, amount, show, ev)
    }

    pub(super) fn confuse(&mut self, t: Slot, ev: &mut Vec<Event>) {
        let turns = self.rng.below(4) as u8 + 2;
        self.fm(t).volatile.confusion = turns;
        ev.push(Event::Confused { target: t });
    }

    fn inflict_primary(
        &mut self,
        by: Slot,
        t: Slot,
        a: Ailment,
        mv: &MoveData,
        ev: &mut Vec<Event>,
    ) {
        let Some(f) = self.fighter(t).filter(|f| !f.fainted()) else {
            return;
        };
        // Paralysis from a move follows the type chart (no effect on immune types).
        let immune_by_chart = a == Ailment::Paralysis
            && self
                .chart
                .multiplier(mv.type_id, f.types, f.volatile.identified)
                == 0;
        if f.ailment == Some(a) {
            ev.push(Event::AlreadyAilment {
                target: t,
                ailment: a,
            });
        } else if immune_by_chart || !self.type_allows(f, a) {
            ev.push(Event::NoEffect { target: t });
        } else if f.ailment.is_some() {
            ev.push(Event::Failed { by });
        } else if abilities::prevents_ailment(f.ability, a) {
            ev.push(Event::Ability {
                at: t,
                ability: f.ability,
            });
            ev.push(Event::NoEffect { target: t });
        } else if self.guarded(by, t) || (a == Ailment::Sleep && self.noise_on()) {
            ev.push(Event::Failed { by });
        } else {
            self.set_ailment(t, a, Some(by), ev);
        }
    }

    /// Gives `t` the status; a status-sharing ability passes poison, burn or paralysis from a
    /// foe back to it.
    pub(super) fn set_ailment(
        &mut self,
        t: Slot,
        a: Ailment,
        by: Option<Slot>,
        ev: &mut Vec<Event>,
    ) {
        let turns = self.rng.below(4) as u8 + 2;
        let f = self.fm(t);
        f.ailment = Some(a);
        match a {
            Ailment::Sleep => f.sleep_turns = turns,
            Ailment::BadPoison => f.volatile.bad_poison_turns = 0,
            _ => {}
        }
        let ability = f.ability;
        ev.push(Event::Ailment {
            target: t,
            ailment: a,
        });
        if ability == abilities::SHARES_STATUS
            && let Some(src) = by
            && src.side != t.side
            && matches!(
                a,
                Ailment::Poison | Ailment::BadPoison | Ailment::Burn | Ailment::Paralysis
            )
        {
            let back = if a == Ailment::BadPoison {
                Ailment::Poison
            } else {
                a
            };
            if self.alive(src) && self.can_take(t, src, back) {
                ev.push(Event::Ability { at: t, ability });
                self.set_ailment(src, back, None, ev);
            }
        }
    }
}
