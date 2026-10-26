//! Additional ordinary move semantics. Each supported identity is explicit; unknown source
//! effects never opt into this module merely by resembling a damaging record.
use super::super::data::MoveData;
use super::super::data::types;
use super::super::event::{Condition, Event, HurtBy, Slot};
use super::super::field::{Terrain, Weather};
use super::super::fighter::Ailment;
use super::super::fighter::{Stat, staged};
use super::Battle;

pub fn status_message(id: u16) -> &'static str {
    match id {
        366 => "made its side faster!",
        381 => "protected its side from critical hits!",
        392 => "surrounded itself with healing water!",
        393 => "rose above the ground!",
        433 => "reversed the move order!",
        472 => "swapped the field's defenses!",
        390 | 446 | 564 => "set an entry hazard!",
        _ => "changed the battle conditions!",
    }
}

pub fn implemented(id: u16) -> bool {
    if matches!(
        id,
        355 | 366
            | 375
            | 379
            | 381
            | 384
            | 385
            | 388
            | 390
            | 391
            | 392
            | 393
            | 432
            | 433
            | 446
            | 456
            | 470
            | 471
            | 472
            | 478
            | 487
            | 493
            | 494
            | 505
            | 513
            | 564
            | 576
            | 659
            | 666
            | 668
            | 683
            | 685
    ) {
        return true;
    }
    matches!(
        id,
        360 | 362
            | 371
            | 372
            | 376
            | 378
            | 386
            | 419
            | 462
            | 473
            | 474
            | 480
            | 484
            | 486
            | 492
            | 500
            | 506
            | 512
            | 524
            | 535
            | 540
            | 548
            | 663
            | 681
    )
}

impl Battle {
    pub(super) fn ordinary_heal(
        &mut self,
        at: Slot,
        amount: u32,
        by: Slot,
        ev: &mut Vec<Event>,
    ) -> bool {
        let f = self.fm(at);
        if f.volatile.heal_barred > 0 {
            ev.push(Event::Failed { by });
            return false;
        }
        let amount = f.heal(amount.max(1));
        if amount == 0 {
            ev.push(Event::Failed { by });
            false
        } else {
            ev.push(Event::Healed {
                target: at,
                amount,
                hp: f.hp,
            });
            true
        }
    }

    pub(super) fn ordinary_status(
        &mut self,
        at: Slot,
        target: Slot,
        mv: &MoveData,
        ev: &mut Vec<Event>,
    ) {
        let id = mv.details.special;
        let mut changed = true;
        match id {
            355 | 456 | 659 => {
                let max = u32::from(self.fm(at).stats.hp);
                let amount = if id == 659 && self.weather() == Weather::Sand {
                    max * 2 / 3
                } else {
                    max.div_ceil(2)
                };
                if self.ordinary_heal(at, amount, at, ev) && id == 355 {
                    let f = self.fm(at);
                    f.volatile.landed_types = Some(f.types);
                }
                return;
            }
            505 | 666 => {
                let max = u32::from(self.fm(target).stats.hp);
                let amount = if id == 666 && self.field.terrain == Terrain::Overgrown {
                    max * 2 / 3
                } else if id == 505
                    && self.fm(at).ability == super::super::abilities::PULSE_HEAL_BOOST
                {
                    max * 3 / 4
                } else {
                    max.div_ceil(2)
                };
                self.ordinary_heal(target, amount, at, ev);
                return;
            }
            375 => {
                if let Some(ailment) = self.fm(at).ailment
                    && self.can_take(at, target, ailment)
                {
                    self.set_ailment(target, ailment, Some(at), ev);
                    self.fm(at).ailment = None;
                    ev.push(Event::Cured {
                        target: at,
                        ailment,
                    });
                    return;
                }
                changed = false;
            }
            366 => {
                let s = &mut self.field.sides[at.side];
                if s.doubled_speed > 0 {
                    changed = false
                } else {
                    s.doubled_speed = 4;
                }
            }
            381 => {
                let s = &mut self.field.sides[at.side];
                if s.crit_guard > 0 {
                    changed = false
                } else {
                    s.crit_guard = 5;
                }
            }
            390 => {
                let s = &mut self.field.sides[1 - at.side];
                if s.poison_hazard >= 2 {
                    changed = false
                } else {
                    s.poison_hazard += 1;
                }
            }
            446 => {
                let s = &mut self.field.sides[1 - at.side];
                changed = !s.rock_hazard;
                s.rock_hazard = true;
            }
            564 => {
                let s = &mut self.field.sides[1 - at.side];
                changed = !s.slowing_hazard;
                s.slowing_hazard = true;
            }
            392 => {
                let f = self.fm(at);
                changed = !f.volatile.water_heal;
                f.volatile.water_heal = true;
            }
            393 => {
                if self.field.strong_pull > 0
                    || self.fm(at).volatile.rooted
                    || self.fm(at).volatile.floating > 0
                {
                    changed = false
                } else {
                    self.fm(at).volatile.floating = 5;
                }
            }
            433 => {
                self.field.reversed_order = if self.field.reversed_order == 0 { 5 } else { 0 };
            }
            472 => {
                self.field.swapped_defenses = if self.field.swapped_defenses == 0 {
                    5
                } else {
                    0
                };
            }
            478 => {
                self.field.items_suppressed = if self.field.items_suppressed == 0 {
                    5
                } else {
                    0
                };
            }
            379 => {
                let f = self.fm(at);
                f.keep_original(false);
                std::mem::swap(&mut f.stats.attack, &mut f.stats.defense);
            }
            384 | 385 | 391 => {
                let a = self.fm(at).volatile.stages;
                let b = self.fm(target).volatile.stages;
                for stat in Stat::ALL {
                    if id == 391
                        || (id == 384 && matches!(stat, Stat::Attack | Stat::SpAttack))
                        || (id == 385 && matches!(stat, Stat::Defense | Stat::SpDefense))
                    {
                        self.fm(at).volatile.stages[stat as usize] = b[stat as usize];
                        self.fm(target).volatile.stages[stat as usize] = a[stat as usize];
                    }
                }
            }
            470 | 471 => {
                let a = self.fm(at).stats;
                let b = self.fm(target).stats;
                for slot in [at, target] {
                    let f = self.fm(slot);
                    f.keep_original(false);
                    if id == 470 {
                        f.stats.defense =
                            ((u32::from(a.defense) + u32::from(b.defense)) / 2) as u16;
                        f.stats.sp_defense =
                            ((u32::from(a.sp_defense) + u32::from(b.sp_defense)) / 2) as u16;
                    } else {
                        f.stats.attack = ((u32::from(a.attack) + u32::from(b.attack)) / 2) as u16;
                        f.stats.sp_attack =
                            ((u32::from(a.sp_attack) + u32::from(b.sp_attack)) / 2) as u16;
                    }
                }
            }
            487 => {
                self.change_type(target, types::WATER, ev);
                return;
            }
            388 | 493 | 494 => {
                let ability = if id == 388 {
                    15
                } else if id == 493 {
                    86
                } else {
                    self.fm(at).ability
                };
                if self.fm(target).ability == ability
                    || super::super::abilities::uncopyable(self.fm(target).ability)
                    || self.fm(target).ability == super::super::abilities::LOAFS
                {
                    changed = false
                } else {
                    self.set_ability(target, ability, ev);
                    return;
                }
            }
            513 => {
                let types = self.fm(target).battle_types();
                let f = self.fm(at);
                f.keep_original(false);
                f.types = types;
                ev.push(Event::TypeChanged { target: at, types });
                return;
            }
            576 => {
                let f = self.fm(target);
                changed = f.volatile.stages.iter().any(|&n| n != 0);
                for n in &mut f.volatile.stages {
                    *n = -*n;
                }
            }
            668 => {
                let f = self.fm(target);
                if f.stage(Stat::Attack) <= -6 {
                    changed = false
                } else {
                    let amount = staged(u32::from(f.stats.attack), f.stage(Stat::Attack));
                    self.lower_stat(at, target, Stat::Attack, -1, true, ev);
                    self.ordinary_heal(at, amount, at, ev);
                    return;
                }
            }
            683 => {
                let a = self.fm(at).stats.speed;
                let b = self.fm(target).stats.speed;
                self.fm(at).keep_original(false);
                self.fm(target).keep_original(false);
                self.fm(at).stats.speed = b;
                self.fm(target).stats.speed = a;
            }
            685 => {
                if let Some(ailment) = self.fm(target).ailment.take() {
                    ev.push(Event::Cured { target, ailment });
                    let amount = u32::from(self.fm(at).stats.hp).div_ceil(2);
                    self.ordinary_heal(at, amount, at, ev);
                    return;
                }
                changed = false;
            }
            432 => {
                if self.fm(target).volatile.decoy == 0 {
                    self.lower_stat(at, target, Stat::Evasion, -1, true, ev);
                }
                for s in &mut self.field.sides {
                    s.hazards = 0;
                    s.poison_hazard = 0;
                    s.rock_hazard = false;
                    s.slowing_hazard = false;
                }
                let s = &mut self.field.sides[target.side];
                s.physical_wall = 0;
                s.special_wall = 0;
                s.stat_guard = 0;
                s.status_guard = 0;
            }
            _ => changed = false,
        }
        if changed {
            ev.push(Event::Started {
                target: at,
                condition: Condition::Ordinary(id),
            });
        } else {
            ev.push(Event::Failed { by: at });
        }
    }

    pub(super) fn ordinary_hazards(&mut self, at: Slot, ev: &mut Vec<Event>) {
        let state = self.field.sides[at.side];
        if !self.alive(at) {
            return;
        }
        if state.rock_hazard {
            let f = self.fighter(at).unwrap();
            let mult = self.chart.multiplier(types::ROCK, f.battle_types(), false);
            let amount = u32::from(f.stats.hp) * mult / 800;
            let f = self.fm(at);
            let damage = f.hurt(amount.max(1));
            ev.push(Event::Hurt {
                target: at,
                damage,
                hp: f.hp,
                by: HurtBy::Hazard,
            });
        }
        if !self.alive(at) {
            return;
        }
        let grounded = self.field.strong_pull > 0 || self.fm(at).grounded();
        if grounded && state.hazards > 0 {
            let f = self.fm(at);
            let div = (5 - u32::from(state.hazards.min(3))) * 2;
            let damage = f.hurt((u32::from(f.stats.hp) / div).max(1));
            ev.push(Event::Hurt {
                target: at,
                damage,
                hp: f.hp,
                by: HurtBy::Hazard,
            });
        }
        if self.alive(at) && grounded && state.poison_hazard > 0 {
            if self.fm(at).has_type(types::POISON) {
                self.field.sides[at.side].poison_hazard = 0;
            } else {
                let ailment = if state.poison_hazard == 2 {
                    Ailment::BadPoison
                } else {
                    Ailment::Poison
                };
                let f = self.fighter(at).unwrap();
                if f.ailment.is_none()
                    && self.type_allows(f, ailment)
                    && !super::super::abilities::prevents_ailment(f.ability, ailment)
                {
                    self.set_ailment(at, ailment, None, ev);
                }
            }
        }
        if self.alive(at) && grounded && state.slowing_hazard {
            self.lower_stat(Slot::new(1 - at.side, 0), at, Stat::Speed, -1, true, ev);
        }
        self.report_faints(ev);
    }

    pub(super) fn ordinary_end(&mut self) {
        for side in &mut self.field.sides {
            side.doubled_speed = side.doubled_speed.saturating_sub(1);
            side.crit_guard = side.crit_guard.saturating_sub(1);
        }
        self.field.reversed_order = self.field.reversed_order.saturating_sub(1);
        self.field.swapped_defenses = self.field.swapped_defenses.saturating_sub(1);
        self.field.items_suppressed = self.field.items_suppressed.saturating_sub(1);
        for at in self.on_field() {
            let f = self.fm(at);
            f.volatile.landed_types = None;
            f.volatile.floating = f.volatile.floating.saturating_sub(1);
            f.volatile.lifted = f.volatile.lifted.saturating_sub(1);
            f.volatile.heal_barred = f.volatile.heal_barred.saturating_sub(1);
            f.volatile.silenced = f.volatile.silenced.saturating_sub(1);
        }
    }

    pub(super) fn ordinary_power(&self, at: Slot, target: Slot, mv: &MoveData, power: u32) -> u32 {
        let (Some(user), Some(foe)) = (self.fighter(at), self.fighter(target)) else {
            return power;
        };
        match mv.details.special {
            360 => (25 * self.speed(target) / self.speed(at).max(1) + 1).min(150),
            362 if foe.hp <= foe.stats.hp / 2 => power * 2,
            371 if foe.volatile.turn.acted && foe.volatile.fresh == 0 => power * 2,
            372 if foe.volatile.turn.hurt => power * 2,
            376 => match user
                .volatile
                .last_index
                .and_then(|i| user.moves.get(i))
                .map(|m| m.pp)
                .unwrap_or(4)
            {
                0 => 200,
                1 => 80,
                2 => 60,
                3 => 50,
                _ => 40,
            },
            378 | 462 => {
                let ratio = u64::from(foe.hp) * 4096 / u64::from(foe.stats.hp.max(1));
                (((12000 * ratio + 2047) / 4096) / 100).max(1) as u32
            }
            386 => (60
                + 20 * foe
                    .volatile
                    .stages
                    .iter()
                    .map(|&n| u32::from(n.max(0) as u8))
                    .sum::<u32>())
            .min(200),
            419 if user.volatile.turn.damaged_by(target) => power * 2,
            474 if matches!(foe.ailment, Some(Ailment::Poison | Ailment::BadPoison)) => power * 2,
            484 | 535 => {
                let ratio = user.battle_weight() / foe.battle_weight();
                match ratio {
                    5.. => 120,
                    4 => 100,
                    3 => 80,
                    2 => 60,
                    _ => 40,
                }
            }
            486 => {
                [40, 60, 80, 120, 150][(self.speed(at) / self.speed(target).max(1)).min(4) as usize]
            }
            500 | 681 => {
                20 + 20
                    * user
                        .volatile
                        .stages
                        .iter()
                        .map(|&n| u32::from(n.max(0) as u8))
                        .sum::<u32>()
            }
            506 if foe.ailment.is_some() => power * 2,
            512 => power * 2, // The declared battle format has no held items.
            _ => power,
        }
    }
}
