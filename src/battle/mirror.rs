//! A battle seen from side 1: in a live battle the accepter's game runs the same battle as the
//! challenger's (side order decides the rolls) and shows it with the sides swapped, so their own
//! Chao stand on the left and the messages call the challenger's the foe.

use super::event::{Condition, Event, Outcome, Slot};
use super::narrate::View;

/// The other side's index.
pub fn side(s: usize) -> usize {
    1 - s.min(1)
}

pub fn slot(s: Slot) -> Slot {
    Slot::new(side(s.side), s.pos)
}

/// `view` with the sides swapped.
pub fn view(mut v: View) -> View {
    v.active.swap(0, 1);
    v.hp.swap(0, 1);
    v.max_hp.swap(0, 1);
    v.ailment.swap(0, 1);
    v
}

fn condition(c: Condition) -> Condition {
    match c {
        Condition::SureHit(at) => Condition::SureHit(slot(at)),
        c => c,
    }
}

/// `e` with the sides swapped.
pub fn event(e: &Event) -> Event {
    use Event::*;
    let s = slot;
    match e.clone() {
        SwitchedIn { at, member } => SwitchedIn { at: s(at), member },
        Withdrawn { at, member } => Withdrawn { at: s(at), member },
        UsedMove { by, move_id } => UsedMove { by: s(by), move_id },
        CantAct { by, why } => CantAct { by: s(by), why },
        NoTarget { by } => NoTarget { by: s(by) },
        Missed { by, target } => Missed {
            by: s(by),
            target: s(target),
        },
        NoEffect { target } => NoEffect { target: s(target) },
        Failed { by } => Failed { by: s(by) },
        Hit {
            by,
            target,
            damage,
            hp,
            effect,
            crit,
        } => Hit {
            by: s(by),
            target: s(target),
            damage,
            hp,
            effect,
            crit,
        },
        Hurt {
            target,
            damage,
            hp,
            by,
        } => Hurt {
            target: s(target),
            damage,
            hp,
            by,
        },
        Healed { target, amount, hp } => Healed {
            target: s(target),
            amount,
            hp,
        },
        Ailment { target, ailment } => Ailment {
            target: s(target),
            ailment,
        },
        AlreadyAilment { target, ailment } => AlreadyAilment {
            target: s(target),
            ailment,
        },
        WokeUp { target } => WokeUp { target: s(target) },
        Thawed { target } => Thawed { target: s(target) },
        Cured { target, ailment } => Cured {
            target: s(target),
            ailment,
        },
        Confused { target } => Confused { target: s(target) },
        AlreadyConfused { target } => AlreadyConfused { target: s(target) },
        IsConfused { by } => IsConfused { by: s(by) },
        ConfusionEnded { target } => ConfusionEnded { target: s(target) },
        StatChanged { target, stat, by } => StatChanged {
            target: s(target),
            stat,
            by,
        },
        StatAtLimit { target, stat, up } => StatAtLimit {
            target: s(target),
            stat,
            up,
        },
        StatProtected { target } => StatProtected { target: s(target) },
        StagesCopied { by, from } => StagesCopied {
            by: s(by),
            from: s(from),
        },
        Focused { target } => Focused { target: s(target) },
        Fainted { at, member } => Fainted { at: s(at), member },
        ScreenUp { side: d, screen } => ScreenUp {
            side: side(d),
            screen,
        },
        ScreenEnded { side: d, screen } => ScreenEnded {
            side: side(d),
            screen,
        },
        Ability { at, ability } => Ability { at: s(at), ability },
        Started {
            target,
            condition: c,
        } => Started {
            target: s(target),
            condition: condition(c),
        },
        Ended {
            target,
            condition: c,
        } => Ended {
            target: s(target),
            condition: condition(c),
        },
        Shielded { target } => Shielded { target: s(target) },
        HungOn { target } => HungOn { target: s(target) },
        DecoyTook { target, damage } => DecoyTook {
            target: s(target),
            damage,
        },
        DecoyFaded { target } => DecoyFaded { target: s(target) },
        KnockedOut { target } => KnockedOut { target: s(target) },
        TypeChanged { target, types } => TypeChanged {
            target: s(target),
            types,
        },
        AbilityChanged { target, ability } => AbilityChanged {
            target: s(target),
            ability,
        },
        TookForm { by, into } => TookForm {
            by: s(by),
            into: s(into),
        },
        MoveCopied { by, move_id } => MoveCopied { by: s(by), move_id },
        PpCut {
            target,
            move_id,
            amount,
        } => PpCut {
            target: s(target),
            move_id,
            amount,
        },
        HpSet { target, hp } => HpSet {
            target: s(target),
            hp,
        },
        TeamCured { side: d } => TeamCured { side: side(d) },
        ScreensBroken { side: d } => ScreensBroken { side: side(d) },
        HazardLaid { side: d, layers } => HazardLaid {
            side: side(d),
            layers,
        },
        ForcedOut { at, member } => ForcedOut { at: s(at), member },
        Passing { at } => Passing { at: s(at) },
        Bounced { by } => Bounced { by: s(by) },
        Stolen { by } => Stolen { by: s(by) },
        Drawn { to } => Drawn { to: s(to) },
        Freed { at } => Freed { at: s(at) },
        NoiseWoke { target } => NoiseWoke { target: s(target) },
        Called { by, move_id } => Called { by: s(by), move_id },
        BattleEnded {
            outcome: Outcome::Won(w),
        } => BattleEnded {
            outcome: Outcome::Won(side(w)),
        },
        e @ (TurnStarted { .. }
        | HitTimes { .. }
        | StagesReset
        | WeatherStarted { .. }
        | WeatherContinues { .. }
        | WeatherEnded { .. }
        | Nothing
        | TeamHit { .. }
        | BattleEnded { .. }) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Effectiveness;
    use crate::battle::event::HurtBy;
    use crate::battle::field::Screen;

    #[test]
    fn swapping_twice_gives_the_same_event() {
        let a = Slot::new(0, 1);
        let b = Slot::new(1, 0);
        let events = [
            Event::Hit {
                by: a,
                target: b,
                damage: 10,
                hp: 5,
                effect: Effectiveness::Super,
                crit: true,
            },
            Event::Hurt {
                target: b,
                damage: 1,
                hp: 2,
                by: HurtBy::Poison,
            },
            Event::Started {
                target: a,
                condition: Condition::SureHit(b),
            },
            Event::ScreenUp {
                side: 0,
                screen: Screen::PhysicalWall,
            },
            Event::BattleEnded {
                outcome: Outcome::Won(1),
            },
            Event::BattleEnded {
                outcome: Outcome::Draw,
            },
            Event::TurnStarted { turn: 3 },
        ];
        for e in &events {
            assert_eq!(event(&event(e)), *e);
        }
        assert_eq!(
            event(&events[0]),
            Event::Hit {
                by: Slot::new(1, 1),
                target: Slot::new(0, 0),
                damage: 10,
                hp: 5,
                effect: Effectiveness::Super,
                crit: true,
            }
        );
        assert_eq!(
            event(&events[2]),
            Event::Started {
                target: Slot::new(1, 1),
                condition: Condition::SureHit(Slot::new(0, 0)),
            }
        );
        assert_eq!(
            event(&events[3]),
            Event::ScreenUp {
                side: 1,
                screen: Screen::PhysicalWall,
            }
        );
    }
}
