//! The end of a turn, in the catalog's order: the field (screens, heals, weather), then each
//! fighter by speed (rooting, abilities, seeds, status damage, haunted sleep, hexes, binding,
//! noise, and the countdowns of seals, repeats, sure hits, power-ups, provokes and
//! drowsiness), then delayed attacks and countdowns.

use super::super::abilities::{self, EndOfTurn};
use super::super::data::types;
use super::super::event::{Condition, Event, HurtBy, Slot};
use super::super::field::{Screen, Weather};
use super::super::fighter::{Ailment, LockKind, STAGE_MAX};
use super::Battle;

impl Battle {
    pub(super) fn end_of_turn(&mut self, ev: &mut Vec<Event>) {
        for side in 0..2 {
            for screen in Screen::ALL {
                let s = &mut self.field.sides[side];
                let left = s.get(screen);
                if left > 0 {
                    s.set(screen, left - 1);
                    if left == 1 {
                        ev.push(Event::ScreenEnded { side, screen });
                    }
                }
            }
        }
        self.heals(ev);
        self.weather_end(ev);
        if self.field.terrain == super::super::field::Terrain::Overgrown {
            for at in self.on_field() {
                let f = self.fm(at);
                if f.grounded() && !f.fainted() {
                    let amount = f.heal((u32::from(f.stats.hp) / 16).max(1));
                    if amount > 0 {
                        ev.push(Event::Healed {
                            target: at,
                            amount,
                            hp: f.hp,
                        });
                    }
                }
            }
        }

        let mut order = self.slots();
        order.sort_by_key(|&s| std::cmp::Reverse(self.speed(s)));
        for at in order {
            self.fighter_end(at, ev);
        }

        if self.field.weather != Weather::Clear && self.field.weather_turns > 0 {
            self.field.weather_turns -= 1;
            if self.field.weather_turns == 0 {
                let weather = self.field.weather;
                self.field.weather = Weather::Clear;
                ev.push(Event::WeatherEnded { weather });
                self.weather_types(ev);
            }
        }

        self.delayed_hits(ev);
        self.countdowns(ev);
        self.ordinary_end();
        if self.field.terrain_turns > 0 {
            self.field.terrain_turns -= 1;
            if self.field.terrain_turns == 0 {
                let terrain = self.field.terrain;
                self.field.terrain = super::super::field::Terrain::None;
                if let Some(target) = self.on_field().first().copied() {
                    ev.push(Event::Ended {
                        target,
                        condition: Condition::Terrain(terrain),
                    });
                }
            }
        }

        for at in self.slots() {
            if let Some(m) = self.member(at) {
                let v = &mut self.teams[at.side][m].volatile;
                v.flinched = false;
                v.fresh = v.fresh.saturating_sub(1);
            }
        }
    }

    fn heals(&mut self, ev: &mut Vec<Event>) {
        for at in self.slots() {
            let Some(mut w) = self.field.heals[at.side][at.pos] else {
                continue;
            };
            w.turns = w.turns.saturating_sub(1);
            if w.turns > 0 {
                self.field.heals[at.side][at.pos] = Some(w);
                continue;
            }
            self.field.heals[at.side][at.pos] = None;
            if self.alive(at) {
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::HealPending,
                });
                let f = self.fm(at);
                let amount = f.heal(u32::from(w.amount));
                if amount > 0 {
                    ev.push(Event::Healed {
                        target: at,
                        amount,
                        hp: f.hp,
                    });
                }
            }
        }
    }

    fn weather_end(&mut self, ev: &mut Vec<Event>) {
        let weather = self.field.weather;
        if weather == Weather::Clear {
            return;
        }
        ev.push(Event::WeatherContinues { weather });
        if !matches!(weather, Weather::Sand | Weather::Hail) || self.weather() != weather {
            return;
        }
        for at in self.slots() {
            if !self.alive(at) {
                continue;
            }
            let f = self.fm(at);
            let spared = match weather {
                Weather::Sand => [types::ROCK, types::STEEL, types::GROUND]
                    .iter()
                    .any(|&t| f.has_type(t)),
                _ => f.has_type(types::ICE),
            };
            if spared
                || abilities::ignores_weather_damage(f.ability, weather)
                || f.volatile.hidden.is_some_and(|h| {
                    matches!(
                        h,
                        super::super::fighter::Hidden::Underground
                            | super::super::fighter::Hidden::Underwater
                    )
                })
            {
                continue;
            }
            let damage = f.hurt((u32::from(f.stats.hp) / 16).max(1));
            ev.push(Event::Hurt {
                target: at,
                damage,
                hp: f.hp,
                by: HurtBy::Weather(weather),
            });
        }
        self.report_faints(ev);
    }

    /// HP lost to an end-of-turn effect.
    fn drain_hp(&mut self, at: Slot, amount: u32, by: HurtBy, ev: &mut Vec<Event>) -> u16 {
        let f = self.fm(at);
        let damage = f.hurt(amount.max(1));
        ev.push(Event::Hurt {
            target: at,
            damage,
            hp: f.hp,
            by,
        });
        damage
    }

    fn fighter_end(&mut self, at: Slot, ev: &mut Vec<Event>) {
        if !self.alive(at) {
            return;
        }
        let weather = self.weather();
        let f = self.fm(at);
        let max = u32::from(f.stats.hp);

        if f.volatile.rooted && f.hp < f.stats.hp {
            let amount = f.heal((max / 16).max(1));
            ev.push(Event::Healed {
                target: at,
                amount,
                hp: f.hp,
            });
        }

        let ability = f.ability;
        if ability == abilities::RAIN_CURES
            && weather == Weather::Rain
            && let Some(ailment) = f.ailment.take()
        {
            f.sleep_turns = 0;
            ev.push(Event::Cured {
                target: at,
                ailment,
            });
        }
        match abilities::end_of_turn(ability, weather, f.volatile.fresh == 2) {
            Some(EndOfTurn::Heal16) if f.hp < f.stats.hp => {
                ev.push(Event::Ability { at, ability });
                let amount = f.heal((max / 16).max(1));
                ev.push(Event::Healed {
                    target: at,
                    amount,
                    hp: f.hp,
                });
            }
            Some(EndOfTurn::Raise(stat)) if f.stage(stat) < STAGE_MAX => {
                ev.push(Event::Ability { at, ability });
                self.raise_stat(at, stat, 1, false, ev);
            }
            Some(EndOfTurn::MayShed) => {
                if let Some(a) = f.ailment
                    && self.rng.below(3) == 0
                {
                    let f = self.fm(at);
                    f.ailment = None;
                    f.volatile.haunted_sleep = false;
                    ev.push(Event::Ability { at, ability });
                    ev.push(Event::Cured {
                        target: at,
                        ailment: a,
                    });
                }
            }
            _ => {}
        }
        if ability == abilities::LOAFS {
            let v = &mut self.fm(at).volatile;
            v.loafing = !v.loafing;
        }

        // Seeds feed whoever is in the seeder's slot, or hurt it when draining the fighter does.
        if let Some(src) = self.fm(at).volatile.seeded_by
            && self.alive(src)
        {
            let lost = u32::from(self.drain_hp(at, max / 8, HurtBy::Seed, ev));
            if self.fm(at).ability == abilities::DRAINS_HURT {
                self.drain_hp(src, lost, HurtBy::DrainBackfire, ev);
            } else {
                let f = self.fm(src);
                let amount = f.heal(lost);
                if amount > 0 {
                    ev.push(Event::Healed {
                        target: src,
                        amount,
                        hp: f.hp,
                    });
                }
            }
        }

        if self.alive(at) {
            let f = self.fm(at);
            let hurt = match f.ailment {
                Some(Ailment::Poison) => Some((max / 8, HurtBy::Poison)),
                Some(Ailment::BadPoison) => {
                    f.volatile.bad_poison_turns = (f.volatile.bad_poison_turns + 1).min(15);
                    let n = u32::from(f.volatile.bad_poison_turns);
                    Some(((max / 16).max(1) * n, HurtBy::Poison))
                }
                Some(Ailment::Burn) => Some((
                    max / if f.ability == abilities::HALVES_FIRE {
                        32
                    } else {
                        16
                    },
                    HurtBy::Burn,
                )),
                _ => None,
            };
            if let Some((amount, by)) = hurt {
                self.drain_hp(at, amount, by, ev);
            }
        }

        if self.alive(at) && self.fm(at).volatile.haunted_sleep {
            if self.fm(at).ailment == Some(Ailment::Sleep) {
                self.drain_hp(at, max / 4, HurtBy::HauntedSleep, ev);
            } else {
                self.fm(at).volatile.haunted_sleep = false;
            }
        }
        if self.alive(at) && self.fm(at).volatile.hexed {
            self.drain_hp(at, max / 4, HurtBy::Hex, ev);
        }
        if self.alive(at)
            && let Some((turns, by, move_id)) = self.fm(at).volatile.bound
        {
            let left = turns.saturating_sub(1);
            if left == 0 {
                self.fm(at).volatile.bound = None;
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::Bound(move_id),
                });
            } else {
                self.fm(at).volatile.bound = Some((left, by, move_id));
                self.drain_hp(at, max / 8, HurtBy::Bound(move_id), ev);
            }
        }

        // Noise wakes every sleeper.
        if self.alive(at)
            && self
                .fm(at)
                .volatile
                .lock
                .is_some_and(|l| l.kind == LockKind::Noise)
        {
            for s in self.on_field() {
                let f = self.fm(s);
                if f.ailment == Some(Ailment::Sleep) && !abilities::blocks_sound(f.ability) {
                    f.ailment = None;
                    f.sleep_turns = 0;
                    f.volatile.haunted_sleep = false;
                    ev.push(Event::NoiseWoke { target: s });
                }
            }
        }

        if self.alive(at) {
            self.counters_end(at, ev);
        }
        self.report_faints(ev);
    }

    /// The countdowns of seals, repeats, sure hits, power-ups, provokes and drowsiness.
    fn counters_end(&mut self, at: Slot, ev: &mut Vec<Event>) {
        let f = self.fm(at);
        if let Some((id, turns)) = f.volatile.sealed {
            if !f.moves.iter().any(|m| m.data.id == id) || turns <= 1 {
                f.volatile.sealed = None;
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::Sealed(id),
                });
            } else {
                f.volatile.sealed = Some((id, turns - 1));
            }
        }
        let f = self.fm(at);
        if let Some((index, turns)) = f.volatile.repeat {
            let pp = f.moves.get(index).map_or(0, |m| m.pp);
            if turns <= 1 || pp == 0 {
                f.volatile.repeat = None;
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::Repeating,
                });
            } else {
                f.volatile.repeat = Some((index, turns - 1));
            }
        }
        let v = &mut self.fm(at).volatile;
        if let Some((by, turns)) = v.sure_hit {
            v.sure_hit = (turns > 1).then_some((by, turns - 1));
        }
        if v.powered > 0 {
            v.powered -= 1;
        }
        if v.provoked > 0 {
            v.provoked -= 1;
            if v.provoked == 0 {
                ev.push(Event::Ended {
                    target: at,
                    condition: Condition::Provoked,
                });
            }
        }
        let v = &mut self.fm(at).volatile;
        if v.drowsy > 0 {
            v.drowsy -= 1;
            if v.drowsy == 0 && self.can_take(at, at, Ailment::Sleep) {
                self.set_ailment(at, Ailment::Sleep, None, ev);
            }
        }
    }

    /// Countdowns: each one shows its count; a fighter faints when it runs out.
    fn countdowns(&mut self, ev: &mut Vec<Event>) {
        for at in self.slots() {
            if !self.alive(at) {
                continue;
            }
            let f = self.fm(at);
            if f.volatile.countdown == 0 {
                continue;
            }
            f.volatile.countdown -= 1;
            let count = f.volatile.countdown;
            if count == 0 {
                let damage = f.hurt(u32::from(f.hp));
                ev.push(Event::Hurt {
                    target: at,
                    damage,
                    hp: 0,
                    by: HurtBy::Countdown,
                });
            } else {
                ev.push(Event::Started {
                    target: at,
                    condition: Condition::Countdown(count),
                });
            }
        }
        self.report_faints(ev);
    }
}
