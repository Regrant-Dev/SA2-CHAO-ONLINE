//! A turn's events as the battle screen plays them: one `Beat` at a time, each with its
//! message, what the arena's Chao do (`Cue`) and what changes on the HUD (`Update`).
//!
//! The engine has already finished the turn when its events come, so the HUD shows a `View` of
//! its own that each beat moves on; the narrator keeps a second one, ahead of it, to know who
//! stands where while it turns the events into beats.

use super::data::Effectiveness;
use super::engine::Battle;
use super::event::{Blocked, Condition, Event, HurtBy, Outcome, Slot};
use super::field::{Screen, Weather};
use super::fighter::{Ailment, Stat};

/// Frames a beat lasts: a message, a quick HP change, the longer moments.
const MESSAGE: u16 = 90;
const QUICK: u16 = 30;
pub const ATTACK: u16 = 50;
const FAINT: u16 = 80;
const END: u16 = 150;

/// What the arena's Chao do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// Team member `member` of `slot.side` stands at `slot`, in place of whoever was there.
    Show {
        slot: Slot,
        member: usize,
    },
    /// Nobody stands at `slot`.
    Hide {
        slot: Slot,
    },
    /// Uses move `move_id`: attacks (a short step toward `toward`), or for a status move, a
    /// show of strength. `charging`: the first turn of a two-turn move, which only gets ready.
    Attack {
        by: Slot,
        toward: Option<Slot>,
        status: bool,
        move_id: u16,
        charging: bool,
    },
    /// Takes a hit: 0 light, 1 medium, 2 heavy.
    Hurt {
        target: Slot,
        heavy: u8,
    },
    Faint {
        at: Slot,
    },
    /// The fighter at `slot` has this status now (also as it comes out with one).
    Ailment {
        slot: Slot,
        ailment: Option<Ailment>,
    },
    /// The fighter at `slot` became confused (`on`) or came out of it.
    Confusion {
        slot: Slot,
        on: bool,
    },
    /// A moment's sign on the fighter at `slot`.
    Sign {
        slot: Slot,
        sign: Sign,
    },
    /// The end: the winners cheer, the others droop (`None` for a draw).
    Result {
        winner: Option<usize>,
    },
    Sound(Sound),
}

/// What a fighter shows for a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// A move's hit landed (`strong`: a critical or strong hit).
    Struck {
        strong: bool,
    },
    /// Acting while confused.
    Confused,
    Flinched,
    /// Too paralyzed to move.
    Paralyzed,
    /// Asleep through its turn.
    Snoring,
    /// Frozen through its turn.
    Frozen,
    /// Hurt by its burn.
    Burned,
    /// Hurt by poison.
    Poisoned,
    /// Its shield stopped a move.
    Guarded,
    StatUp,
    StatDown,
    Healed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Cursor,
    Confirm,
    Back,
    Hit,
    Faint,
}

/// A change to what the HUD shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Active {
        slot: Slot,
        member: Option<usize>,
    },
    Hp {
        side: usize,
        member: usize,
        hp: u16,
    },
    Ailment {
        side: usize,
        member: usize,
        ailment: Option<Ailment>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Beat {
    pub text: Option<String>,
    pub cues: Vec<Cue>,
    pub updates: Vec<Update>,
    pub frames: u16,
}

/// Who stands where, and each member's HP and status, as shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub active: [[Option<usize>; 2]; 2],
    pub hp: [Vec<u16>; 2],
    pub max_hp: [Vec<u16>; 2],
    pub ailment: [Vec<Option<Ailment>>; 2],
}

impl View {
    /// Both teams before anyone is sent out.
    pub fn new(battle: &Battle) -> View {
        let mut v = View::default();
        for side in 0..2 {
            let team = battle.team(side);
            v.hp[side] = team.iter().map(|f| f.hp).collect();
            v.max_hp[side] = team.iter().map(|f| f.stats.hp).collect();
            v.ailment[side] = team.iter().map(|f| f.ailment).collect();
        }
        v
    }

    pub fn at(&self, slot: Slot) -> Option<usize> {
        self.active
            .get(slot.side)
            .and_then(|s| s.get(slot.pos))
            .copied()
            .flatten()
    }

    pub fn apply(&mut self, update: &Update) {
        match *update {
            Update::Active { slot, member } => {
                if let Some(s) = self
                    .active
                    .get_mut(slot.side)
                    .and_then(|s| s.get_mut(slot.pos))
                {
                    *s = member;
                }
            }
            Update::Hp { side, member, hp } => {
                if let Some(h) = self.hp.get_mut(side).and_then(|t| t.get_mut(member)) {
                    *h = hp;
                }
            }
            Update::Ailment {
                side,
                member,
                ailment,
            } => {
                if let Some(a) = self.ailment.get_mut(side).and_then(|t| t.get_mut(member)) {
                    *a = ailment;
                }
            }
        }
    }

    /// HP at `slot` and its full HP.
    pub fn hp_at(&self, slot: Slot) -> Option<(u16, u16)> {
        let m = self.at(slot)?;
        Some((*self.hp[slot.side].get(m)?, *self.max_hp[slot.side].get(m)?))
    }
}

/// Names for messages: the fighters' and the ROM's.
pub trait Names {
    fn fighter(&self, side: usize, member: usize) -> String;
    fn move_name(&self, id: u16) -> String;
    fn ability(&self, id: u8) -> String;
    fn type_name(&self, id: u8) -> String;
    /// A move that deals no damage of its own.
    fn is_status_move(&self, id: u16) -> bool;
}

/// Turns `events` into beats, moving `planned` along as it goes.
pub fn beats(events: &[Event], planned: &mut View, names: &dyn Names) -> Vec<Beat> {
    let mut n = Narrator {
        planned,
        names,
        out: Vec::new(),
        ahead: &[],
    };
    for (i, e) in events.iter().enumerate() {
        n.ahead = &events[i + 1..];
        n.event(e);
    }
    n.out
}

struct Narrator<'a> {
    planned: &'a mut View,
    names: &'a dyn Names,
    out: Vec<Beat>,
    /// The events after the one being told.
    ahead: &'a [Event],
}

impl Narrator<'_> {
    /// "Name" for the player's side, "Foe Name" for the other.
    fn who(&self, slot: Slot) -> String {
        let name = self
            .planned
            .at(slot)
            .map_or_else(|| "?".into(), |m| self.names.fighter(slot.side, m));
        if slot.side == 0 {
            name
        } else {
            format!("Foe {name}")
        }
    }

    fn member_name(&self, side: usize, member: usize) -> String {
        let name = self.names.fighter(side, member);
        if side == 0 {
            name
        } else {
            format!("Foe {name}")
        }
    }

    fn push(&mut self, text: Option<String>, cues: Vec<Cue>, updates: Vec<Update>, frames: u16) {
        for u in &updates {
            self.planned.apply(u);
        }
        self.out.push(Beat {
            text,
            cues,
            updates,
            frames,
        });
    }

    fn say(&mut self, text: String) {
        self.push(Some(text), Vec::new(), Vec::new(), MESSAGE);
    }

    fn say_with(&mut self, text: String, cues: Vec<Cue>) {
        self.push(Some(text), cues, Vec::new(), MESSAGE);
    }

    /// The `Ailment` cue for `slot` with `ailment`.
    fn ailment_cue(slot: Slot, ailment: Option<Ailment>) -> Vec<Cue> {
        vec![Cue::Ailment { slot, ailment }]
    }

    fn hp(&self, slot: Slot, hp: u16) -> Vec<Update> {
        self.planned
            .at(slot)
            .map(|member| Update::Hp {
                side: slot.side,
                member,
                hp,
            })
            .into_iter()
            .collect()
    }

    fn ailment(&self, slot: Slot, ailment: Option<Ailment>) -> Vec<Update> {
        self.planned
            .at(slot)
            .map(|member| Update::Ailment {
                side: slot.side,
                member,
                ailment,
            })
            .into_iter()
            .collect()
    }

    /// How hard a loss of `damage` lands on the fighter at `slot`.
    fn heaviness(&self, slot: Slot, damage: u16) -> u8 {
        let full = self.planned.hp_at(slot).map_or(1, |(_, full)| full.max(1));
        match u32::from(damage) * 100 / u32::from(full) {
            0..=14 => 0,
            15..=39 => 1,
            _ => 2,
        }
    }

    /// Who a move `by` just used lands on (or misses), for the attack's step: its first
    /// target other than `by` itself, else the foe it faces.
    fn aimed_at(&self, by: Slot) -> Option<Slot> {
        let target = self
            .ahead
            .iter()
            .take_while(|e| !matches!(e, Event::UsedMove { .. }))
            .find_map(|e| match *e {
                Event::Hit { by: b, target, .. } | Event::Missed { by: b, target } if b == by => {
                    Some(target)
                }
                Event::NoEffect { target } | Event::Shielded { target } => Some(target),
                _ => None,
            });
        target.filter(|&t| t != by).or_else(|| self.facing(by))
    }

    /// The move `by` uses now only gets ready this turn (a two-turn move's first turn).
    fn readying(&self, by: Slot) -> bool {
        self.ahead
            .iter()
            .take_while(|e| !matches!(e, Event::UsedMove { .. }))
            .any(|e| {
                matches!(*e, Event::Started { target, condition: Condition::Readying(_) } if target == by)
            })
    }

    /// The foe standing in front of `by`, or the other one when that one is down.
    fn facing(&self, by: Slot) -> Option<Slot> {
        let ahead = Slot::new(1 - by.side, by.pos);
        let other = Slot::new(1 - by.side, 1 - by.pos);
        [ahead, other]
            .into_iter()
            .find(|&s| self.planned.hp_at(s).is_some_and(|(hp, _)| hp > 0))
    }

    fn event(&mut self, e: &Event) {
        match *e {
            Event::TurnStarted { .. } => {}
            Event::SwitchedIn { at, member } => {
                let name = self.names.fighter(at.side, member);
                let text = if at.side == 0 {
                    format!("Go, {name}!")
                } else {
                    format!("The foe sends out {name}!")
                };
                let mut cues = vec![Cue::Show { slot: at, member }];
                if let Some(a) = self.planned.ailment[at.side].get(member).copied().flatten() {
                    cues.push(Cue::Ailment {
                        slot: at,
                        ailment: Some(a),
                    });
                }
                self.push(
                    Some(text),
                    cues,
                    vec![Update::Active {
                        slot: at,
                        member: Some(member),
                    }],
                    MESSAGE,
                );
            }
            Event::Withdrawn { at, member } => {
                let name = self.member_name(at.side, member);
                self.push(
                    Some(format!("{name} comes back.")),
                    vec![Cue::Hide { slot: at }],
                    vec![Update::Active {
                        slot: at,
                        member: None,
                    }],
                    MESSAGE,
                );
            }
            Event::ForcedOut { at, member } => {
                let name = self.member_name(at.side, member);
                self.push(
                    Some(format!("{name} is driven out!")),
                    vec![Cue::Hide { slot: at }],
                    vec![Update::Active {
                        slot: at,
                        member: None,
                    }],
                    MESSAGE,
                );
            }
            Event::UsedMove { by, move_id } => {
                let text = format!("{} uses {}!", self.who(by), self.names.move_name(move_id));
                let cue = Cue::Attack {
                    by,
                    toward: self.aimed_at(by),
                    status: self.names.is_status_move(move_id),
                    move_id,
                    charging: self.readying(by),
                };
                self.push(Some(text), vec![cue], Vec::new(), ATTACK);
            }
            Event::Called { by, move_id } => {
                let text = format!(
                    "{} calls on {}!",
                    self.who(by),
                    self.names.move_name(move_id)
                );
                self.say(text);
            }
            Event::CantAct { by, why } => {
                let who = self.who(by);
                let sign = match why {
                    Blocked::Asleep => Some(Sign::Snoring),
                    Blocked::Frozen => Some(Sign::Frozen),
                    Blocked::FullyParalyzed => Some(Sign::Paralyzed),
                    Blocked::Flinched => Some(Sign::Flinched),
                    Blocked::Confusion => Some(Sign::Confused),
                    _ => None,
                };
                let cues = sign
                    .map(|sign| Cue::Sign { slot: by, sign })
                    .into_iter()
                    .collect();
                let text = match why {
                    Blocked::Asleep => format!("{who} is sound asleep."),
                    Blocked::Frozen => format!("{who} is frozen solid!"),
                    Blocked::FullyParalyzed => format!("{who} is paralyzed and can't move!"),
                    Blocked::Flinched => format!("{who} flinched!"),
                    Blocked::Confusion => format!("{who} is so confused it hits itself!"),
                    Blocked::Recharging => format!("{who} has to rest!"),
                    Blocked::Loafing => format!("{who} is lazing about."),
                    Blocked::Sealed | Blocked::SealedByFoe => {
                        format!("{who} can't use that move!")
                    }
                    Blocked::Provoked => format!("{who} is too provoked for that!"),
                    Blocked::LostFocus => format!("{who} lost its focus!"),
                    Blocked::Storing => format!("{who} keeps building up power."),
                };
                self.say_with(text, cues);
            }
            Event::NoTarget { .. } => self.say("There was nobody to hit...".into()),
            Event::Missed { by, .. } => {
                let text = format!("{}'s attack missed!", self.who(by));
                self.say(text);
            }
            Event::NoEffect { target } => {
                let text = format!("{} isn't affected...", self.who(target));
                self.say(text);
            }
            Event::Failed { .. } => self.say("But it failed!".into()),
            Event::Hit {
                target,
                damage,
                hp,
                effect,
                crit,
                ..
            } => {
                let mut lines = Vec::new();
                if crit {
                    lines.push("A critical hit!".to_string());
                }
                match effect {
                    Effectiveness::Super => lines.push("A strong hit!".into()),
                    Effectiveness::NotVery => lines.push("A weak hit...".into()),
                    Effectiveness::None => lines.push("It had no effect...".into()),
                    Effectiveness::Normal => {}
                }
                let text = (!lines.is_empty()).then(|| lines.join(" "));
                let frames = if text.is_some() { MESSAGE } else { QUICK };
                let mut cues = Vec::new();
                if damage > 0 {
                    let strong = crit || effect == Effectiveness::Super;
                    cues.push(Cue::Sign {
                        slot: target,
                        sign: Sign::Struck { strong },
                    });
                    cues.push(Cue::Hurt {
                        target,
                        heavy: self.heaviness(target, damage),
                    });
                    cues.push(Cue::Sound(Sound::Hit));
                }
                let updates = self.hp(target, hp);
                self.push(text, cues, updates, frames);
            }
            Event::HitTimes { count } => self.say(format!("Hit {count} times!")),
            Event::Hurt {
                target,
                damage,
                hp,
                by,
            } => {
                let who = self.who(target);
                let text = match by {
                    HurtBy::Recoil => format!("{who} is hit by recoil!"),
                    HurtBy::Confusion => format!("{who} is hurt in its confusion."),
                    HurtBy::Poison => format!("{who} suffers from poison!"),
                    HurtBy::Burn => format!("{who} suffers from its burn!"),
                    HurtBy::Weather(w) => format!("{who} is hurt by the {}!", weather(w)),
                    HurtBy::Sacrifice => format!("{who} gives everything!"),
                    HurtBy::Cost => format!("{who} pays with its health."),
                    HurtBy::Seed => format!("{who} is drained by the seed!"),
                    HurtBy::BadDreams => format!("{who} is caught in a nightmare!"),
                    HurtBy::Hex => format!("{who} is hurt by the hex!"),
                    HurtBy::Bound(id) => {
                        format!("{who} is hurt by {}!", self.names.move_name(id))
                    }
                    HurtBy::Hazard => format!("{who} is hurt by the hazards!"),
                    HurtBy::Crash => format!("{who} crashed!"),
                    HurtBy::Ability(id) => {
                        format!("{who} is hurt by {}!", self.names.ability(id))
                    }
                    HurtBy::DrainBackfire => format!("{who} drank something foul!"),
                    HurtBy::Countdown => format!("{who}'s countdown ran out!"),
                    HurtBy::Vengeance => format!("{who} is taken down too!"),
                    HurtBy::Delayed(id) => {
                        format!("{who} is hit by {}!", self.names.move_name(id))
                    }
                };
                let heavy = self.heaviness(target, damage);
                let updates = self.hp(target, hp);
                let mut cues = vec![Cue::Hurt { target, heavy }];
                let sign = match by {
                    HurtBy::Poison => Some(Sign::Poisoned),
                    HurtBy::Burn => Some(Sign::Burned),
                    _ => None,
                };
                if let Some(sign) = sign {
                    cues.push(Cue::Sign { slot: target, sign });
                }
                self.push(Some(text), cues, updates, MESSAGE);
            }
            Event::Healed { target, hp, .. } => {
                let text = format!("{} regains health!", self.who(target));
                let updates = self.hp(target, hp);
                let cues = vec![Cue::Sign {
                    slot: target,
                    sign: Sign::Healed,
                }];
                self.push(Some(text), cues, updates, MESSAGE);
            }
            Event::HpSet { target, hp } => {
                let updates = self.hp(target, hp);
                self.push(None, Vec::new(), updates, QUICK);
            }
            Event::Ailment { target, ailment } => {
                let text = format!("{} {}!", self.who(target), ailment_started(ailment));
                let updates = self.ailment(target, Some(ailment));
                let cues = Self::ailment_cue(target, Some(ailment));
                self.push(Some(text), cues, updates, MESSAGE);
            }
            Event::AlreadyAilment { target, .. } => {
                let text = format!("{} already has a condition!", self.who(target));
                self.say(text);
            }
            Event::WokeUp { target } | Event::NoiseWoke { target } => {
                let text = format!("{} woke up!", self.who(target));
                let updates = self.ailment(target, None);
                self.push(
                    Some(text),
                    Self::ailment_cue(target, None),
                    updates,
                    MESSAGE,
                );
            }
            Event::Thawed { target } => {
                let text = format!("{} thawed out!", self.who(target));
                let updates = self.ailment(target, None);
                self.push(
                    Some(text),
                    Self::ailment_cue(target, None),
                    updates,
                    MESSAGE,
                );
            }
            Event::Cured { target, .. } => {
                let text = format!("{} is cured!", self.who(target));
                let updates = self.ailment(target, None);
                self.push(
                    Some(text),
                    Self::ailment_cue(target, None),
                    updates,
                    MESSAGE,
                );
            }
            Event::TeamCured { side } => {
                let updates = (0..self.planned.ailment[side].len())
                    .map(|member| Update::Ailment {
                        side,
                        member,
                        ailment: None,
                    })
                    .collect();
                let whose = if side == 0 { "Your" } else { "The foe's" };
                let cues = (0..2)
                    .map(|pos| Slot::new(side, pos))
                    .filter(|&slot| self.planned.at(slot).is_some())
                    .map(|slot| Cue::Ailment {
                        slot,
                        ailment: None,
                    })
                    .collect();
                self.push(
                    Some(format!("{whose} team is cured!")),
                    cues,
                    updates,
                    MESSAGE,
                );
            }
            Event::Confused { target } => {
                let text = format!("{} is confused!", self.who(target));
                let cues = vec![Cue::Confusion {
                    slot: target,
                    on: true,
                }];
                self.say_with(text, cues);
            }
            Event::AlreadyConfused { target } => {
                let text = format!("{} is confused already!", self.who(target));
                self.say(text);
            }
            Event::IsConfused { by } => {
                let text = format!("{} is confused...", self.who(by));
                let cues = vec![Cue::Sign {
                    slot: by,
                    sign: Sign::Confused,
                }];
                self.say_with(text, cues);
            }
            Event::ConfusionEnded { target } => {
                let text = format!("{} is no longer confused!", self.who(target));
                let cues = vec![Cue::Confusion {
                    slot: target,
                    on: false,
                }];
                self.say_with(text, cues);
            }
            Event::StatChanged { target, stat, by } => {
                let how = match by {
                    i8::MIN..=-2 => "fell a lot",
                    -1 => "fell",
                    1 => "rose",
                    _ => "rose a lot",
                };
                let text = format!("{}'s {} {how}!", self.who(target), stat_name(stat));
                let sign = if by > 0 { Sign::StatUp } else { Sign::StatDown };
                self.say_with(text, vec![Cue::Sign { slot: target, sign }]);
            }
            Event::StatAtLimit { target, stat, up } => {
                let way = if up { "higher" } else { "lower" };
                let text = format!(
                    "{}'s {} won't go any {way}!",
                    self.who(target),
                    stat_name(stat)
                );
                self.say(text);
            }
            Event::StatProtected { target } => {
                let text = format!("{}'s stats are protected!", self.who(target));
                self.say(text);
            }
            Event::StagesReset => self.say("All stat changes are gone!".into()),
            Event::StagesCopied { by, from } => {
                let text = format!("{} copies {}'s stat changes!", self.who(by), self.who(from));
                self.say(text);
            }
            Event::Focused { target } => {
                let text = format!("{} is focused on critical hits!", self.who(target));
                self.say(text);
            }
            Event::Fainted { at, member } => {
                let name = self.member_name(at.side, member);
                self.push(
                    Some(format!("{name} fainted!")),
                    vec![Cue::Faint { at }, Cue::Sound(Sound::Faint)],
                    vec![Update::Hp {
                        side: at.side,
                        member,
                        hp: 0,
                    }],
                    FAINT,
                );
                self.push(None, vec![Cue::Hide { slot: at }], Vec::new(), 1);
            }
            Event::WeatherStarted { weather: w } => self.say(weather_started(w)),
            Event::WeatherContinues { weather: w } => {
                self.say(format!("The {} goes on.", weather(w)))
            }
            Event::WeatherEnded { weather: w } => self.say(format!("The {} stopped.", weather(w))),
            Event::ScreenUp { side, screen } => {
                let whose = if side == 0 { "your" } else { "the foe's" };
                self.say(format!("{} went up on {whose} side!", screen_name(screen)));
            }
            Event::ScreenEnded { side, screen } => {
                let whose = if side == 0 { "your" } else { "the foe's" };
                self.say(format!("{} on {whose} side wore off.", screen_name(screen)));
            }
            Event::Ability { at, ability } => {
                let text = format!("{}'s {}!", self.who(at), self.names.ability(ability));
                self.say(text);
            }
            Event::Nothing => self.say("Nothing happened.".into()),
            Event::Started { target, condition } => {
                let text = format!("{} {}", self.who(target), condition_started(condition));
                self.say(text);
            }
            Event::Ended { target, condition } => {
                let text = format!("{} {}", self.who(target), condition_ended(condition));
                self.say(text);
            }
            Event::Shielded { target } => {
                let text = format!("{} shielded itself!", self.who(target));
                let cues = vec![Cue::Sign {
                    slot: target,
                    sign: Sign::Guarded,
                }];
                self.say_with(text, cues);
            }
            Event::HungOn { target } => {
                let text = format!("{} hung on!", self.who(target));
                self.say(text);
            }
            Event::DecoyTook { target, .. } => {
                let text = format!("The decoy took the hit for {}!", self.who(target));
                self.say(text);
            }
            Event::DecoyFaded { target } => {
                let text = format!("{}'s decoy faded!", self.who(target));
                self.say(text);
            }
            Event::KnockedOut { .. } => self.say("A one-hit knockout!".into()),
            Event::TypeChanged { target, types } => {
                let names = if types[0] == types[1] {
                    self.names.type_name(types[0])
                } else {
                    format!(
                        "{}/{}",
                        self.names.type_name(types[0]),
                        self.names.type_name(types[1])
                    )
                };
                let text = format!("{} became {names} type!", self.who(target));
                self.say(text);
            }
            Event::AbilityChanged { target, ability } => {
                let text = if ability == 0 {
                    format!("{} has no ability now!", self.who(target))
                } else {
                    format!(
                        "{} now has {}!",
                        self.who(target),
                        self.names.ability(ability)
                    )
                };
                self.say(text);
            }
            Event::TookForm { by, into } => {
                let text = format!("{} took the form of {}!", self.who(by), self.who(into));
                self.say(text);
            }
            Event::MoveCopied { by, move_id } => {
                let text = format!(
                    "{} learned {}!",
                    self.who(by),
                    self.names.move_name(move_id)
                );
                self.say(text);
            }
            Event::PpCut {
                target, move_id, ..
            } => {
                let text = format!(
                    "{}'s {} lost PP!",
                    self.who(target),
                    self.names.move_name(move_id)
                );
                self.say(text);
            }
            Event::ScreensBroken { side } => {
                let whose = if side == 0 { "your" } else { "the foe's" };
                self.say(format!("The walls on {whose} side broke!"));
            }
            Event::HazardLaid { side, .. } => {
                let whose = if side == 0 { "your" } else { "the foe's" };
                self.say(format!("Hazards lie around {whose} side!"));
            }
            Event::Passing { at } => {
                let text = format!("{} passes its state on!", self.who(at));
                self.say(text);
            }
            Event::Bounced { by } => {
                let text = format!("{}'s move was bounced back!", self.who(by));
                self.say(text);
            }
            Event::Stolen { by } => {
                let text = format!("{} took the move!", self.who(by));
                self.say(text);
            }
            Event::Drawn { to } => {
                let text = format!("{} drew the move in!", self.who(to));
                self.say(text);
            }
            Event::Freed { at } => {
                let text = format!("{} broke free!", self.who(at));
                self.say(text);
            }
            Event::TeamHit { member } => {
                let text = format!("A teammate joins in! ({})", member + 1);
                self.say(text);
            }
            Event::BattleEnded { outcome } => {
                let (text, winner) = match outcome {
                    Outcome::Won(0) => ("You won!", Some(0)),
                    Outcome::Won(_) => ("You lost...", Some(1)),
                    Outcome::Draw => ("It's a draw.", None),
                };
                self.push(
                    Some(text.into()),
                    vec![Cue::Result { winner }],
                    Vec::new(),
                    END,
                );
            }
        }
    }
}

fn weather(w: Weather) -> &'static str {
    match w {
        Weather::Clear => "calm",
        Weather::Rain => "rain",
        Weather::Sun => "sunshine",
        Weather::Sand => "sandstorm",
        Weather::Hail => "hail",
    }
}

fn weather_started(w: Weather) -> String {
    match w {
        Weather::Clear => "The weather cleared.",
        Weather::Rain => "Rain began to fall.",
        Weather::Sun => "The sun blazes down.",
        Weather::Sand => "A sandstorm rose.",
        Weather::Hail => "Hail began to fall.",
    }
    .into()
}

fn screen_name(s: Screen) -> &'static str {
    match s {
        Screen::PhysicalWall => "A wall against physical moves",
        Screen::SpecialWall => "A wall against special moves",
        Screen::StatGuard => "A guard against stat drops",
        Screen::StatusGuard => "A guard against status",
    }
}

pub fn stat_name(s: Stat) -> &'static str {
    match s {
        Stat::Attack => "Attack",
        Stat::Defense => "Defense",
        Stat::Speed => "Speed",
        Stat::SpAttack => "Sp. Attack",
        Stat::SpDefense => "Sp. Defense",
        Stat::Accuracy => "accuracy",
        Stat::Evasion => "evasion",
    }
}

fn ailment_started(a: Ailment) -> &'static str {
    match a {
        Ailment::Sleep => "fell asleep",
        Ailment::Poison => "is poisoned",
        Ailment::BadPoison => "is badly poisoned",
        Ailment::Burn => "is burned",
        Ailment::Freeze => "is frozen solid",
        Ailment::Paralysis => "is paralyzed",
    }
}

/// A short tag for the HUD.
pub fn ailment_tag(a: Ailment) -> &'static str {
    match a {
        Ailment::Sleep => "SLP",
        Ailment::Poison | Ailment::BadPoison => "PSN",
        Ailment::Burn => "BRN",
        Ailment::Freeze => "FRZ",
        Ailment::Paralysis => "PAR",
    }
}

fn condition_started(c: Condition) -> String {
    match c {
        Condition::Readying(_) => "is getting ready!",
        Condition::Decoy => "made a decoy!",
        Condition::Shielded => "is shielding itself!",
        Condition::Bracing => "is bracing for hits!",
        Condition::Locked(_) => "is caught up in its move!",
        Condition::Storing => "is building up power!",
        Condition::Seeded => "has a seed on it!",
        Condition::Bound(_) => "is bound!",
        Condition::CantEscape => "is trapped!",
        Condition::Sealed(_) => "had a move sealed!",
        Condition::Repeating => "must repeat its move!",
        Condition::Provoked => "fell for the provocation!",
        Condition::NoRepeats => "can't use a move twice in a row!",
        Condition::Identified => "was identified!",
        Condition::BadDreams => "began having nightmares!",
        Condition::Hexed => "was hexed!",
        Condition::Countdown(_) => "is counting down!",
        Condition::Vengeful => "wants to take its foe down with it!",
        Condition::Rancor => "bears a grudge!",
        Condition::SureHit(_) => "is taking aim!",
        Condition::Stored(_) => "stored energy!",
        Condition::Powered => "is charged up!",
        Condition::Rooted => "put down roots!",
        Condition::Drowsy => "grew drowsy!",
        Condition::FireBoost => "is fired up!",
        Condition::ElectricDulled => "dulled electric moves!",
        Condition::FireDulled => "dulled fire moves!",
        Condition::Sealing => "sealed the foe's moves!",
        Condition::Bouncing => "is ready to bounce moves back!",
        Condition::Stealing => "waits to take a move!",
        Condition::Drawing => "is drawing moves to itself!",
        Condition::Helped => "is backing up its ally!",
        Condition::Focusing => "is concentrating hard!",
        Condition::HealPending => "is waiting for a heal.",
        Condition::Delayed(_) => "has an attack coming!",
        Condition::Fuming => "is building its rage!",
    }
    .into()
}

fn condition_ended(c: Condition) -> String {
    match c {
        Condition::Decoy => "lost its decoy.".into(),
        Condition::Bound(_) => "is free!".into(),
        Condition::Stored(0) => "used up its stores.".into(),
        _ => "is back to normal.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;

    impl Names for Plain {
        fn fighter(&self, side: usize, member: usize) -> String {
            format!("C{side}{member}")
        }
        fn move_name(&self, id: u16) -> String {
            format!("M{id}")
        }
        fn ability(&self, id: u8) -> String {
            format!("A{id}")
        }
        fn type_name(&self, id: u8) -> String {
            format!("T{id}")
        }
        fn is_status_move(&self, id: u16) -> bool {
            id >= 100
        }
    }

    fn view() -> View {
        View {
            active: [[None; 2]; 2],
            hp: [vec![100, 100], vec![80]],
            max_hp: [vec![100, 100], vec![80]],
            ailment: [vec![None; 2], vec![None]],
        }
    }

    #[test]
    fn an_attack_steps_toward_its_target() {
        let (a, a2) = (Slot::new(0, 0), Slot::new(0, 1));
        let (b, b2) = (Slot::new(1, 0), Slot::new(1, 1));
        let mut planned = View {
            active: [[Some(0), Some(1)], [Some(0), Some(1)]],
            hp: [vec![100, 100], vec![80, 80]],
            max_hp: [vec![100, 100], vec![80, 80]],
            ailment: [vec![None; 2], vec![None; 2]],
        };
        let toward = |events: &[Event], planned: &mut View| {
            beats(events, planned, &Plain)
                .iter()
                .flat_map(|b| b.cues.clone())
                .find_map(|c| match c {
                    Cue::Attack { toward, .. } => Some(toward),
                    _ => None,
                })
                .flatten()
        };
        let used = Event::UsedMove { by: a, move_id: 5 };
        // The diagonal foe, the ally, and a miss.
        for target in [b2, a2] {
            let hit = Event::Hit {
                by: a,
                target,
                damage: 1,
                hp: 79,
                effect: Effectiveness::Normal,
                crit: false,
            };
            assert_eq!(toward(&[used.clone(), hit], &mut planned), Some(target));
        }
        let missed = Event::Missed { by: a, target: b2 };
        assert_eq!(toward(&[used.clone(), missed], &mut planned), Some(b2));
        // Nothing aimed: the foe in front.
        let other = Event::UsedMove { by: b, move_id: 5 };
        let later = Event::Missed { by: b, target: b2 };
        assert_eq!(toward(&[used, other, later], &mut planned), Some(b));
    }

    #[test]
    fn a_turn_plays_as_beats_and_moves_the_view_on() {
        let (a, b) = (Slot::new(0, 0), Slot::new(1, 0));
        let mut planned = view();
        let events = vec![
            Event::SwitchedIn { at: a, member: 0 },
            Event::SwitchedIn { at: b, member: 0 },
            Event::TurnStarted { turn: 1 },
            Event::UsedMove { by: a, move_id: 5 },
            Event::Hit {
                by: a,
                target: b,
                damage: 50,
                hp: 30,
                effect: Effectiveness::Super,
                crit: false,
            },
            Event::Ailment {
                target: b,
                ailment: Ailment::Burn,
            },
            Event::UsedMove {
                by: b,
                move_id: 100,
            },
            Event::Fainted { at: b, member: 0 },
            Event::BattleEnded {
                outcome: Outcome::Won(0),
            },
        ];
        let beats = beats(&events, &mut planned, &Plain);
        let texts: Vec<Option<&str>> = beats.iter().map(|b| b.text.as_deref()).collect();
        assert_eq!(
            texts,
            [
                Some("Go, C00!"),
                Some("The foe sends out C10!"),
                Some("C00 uses M5!"),
                Some("A strong hit!"),
                Some("Foe C10 is burned!"),
                Some("Foe C10 uses M100!"),
                Some("Foe C10 fainted!"),
                None,
                Some("You won!"),
            ]
        );
        assert_eq!(beats[0].cues, [Cue::Show { slot: a, member: 0 }]);
        assert_eq!(
            beats[2].cues,
            [Cue::Attack {
                by: a,
                toward: Some(b),
                status: false,
                move_id: 5,
                charging: false,
            }]
        );
        // 50 of 80 HP is a heavy hit, and a strong one.
        assert_eq!(
            beats[3].cues[..2],
            [
                Cue::Sign {
                    slot: b,
                    sign: Sign::Struck { strong: true }
                },
                Cue::Hurt {
                    target: b,
                    heavy: 2
                }
            ]
        );
        assert_eq!(
            beats[4].cues,
            [Cue::Ailment {
                slot: b,
                ailment: Some(Ailment::Burn)
            }]
        );
        assert!(matches!(beats[5].cues[0], Cue::Attack { status: true, .. }));
        assert_eq!(beats[7].cues, [Cue::Hide { slot: b }]);
        assert_eq!(beats[8].cues, [Cue::Result { winner: Some(0) }]);

        // The shown view follows beat by beat.
        let mut shown = view();
        for beat in &beats[..4] {
            for u in &beat.updates {
                shown.apply(u);
            }
        }
        assert_eq!(shown.hp_at(b), Some((30, 80)));
        assert_eq!(shown.ailment[1][0], None);
        for beat in &beats[4..] {
            for u in &beat.updates {
                shown.apply(u);
            }
        }
        assert_eq!(shown.ailment[1][0], Some(Ailment::Burn));
        assert_eq!(shown, planned);
        assert_eq!(planned.hp[1][0], 0);
    }

    #[test]
    fn switching_names_the_member_that_leaves_and_the_one_that_comes() {
        let a = Slot::new(0, 0);
        let mut planned = view();
        planned.active[0][0] = Some(0);
        let beats = beats(
            &[
                Event::Withdrawn { at: a, member: 0 },
                Event::SwitchedIn { at: a, member: 1 },
            ],
            &mut planned,
            &Plain,
        );
        assert_eq!(beats[0].text.as_deref(), Some("C00 comes back."));
        assert_eq!(beats[0].cues, [Cue::Hide { slot: a }]);
        assert_eq!(beats[1].text.as_deref(), Some("Go, C01!"));
        assert_eq!(planned.at(a), Some(1));
    }

    #[test]
    fn a_plain_hit_is_quick_and_silent() {
        let (a, b) = (Slot::new(0, 0), Slot::new(1, 0));
        let mut planned = view();
        planned.active = [[Some(0), None], [Some(0), None]];
        let beats = beats(
            &[Event::Hit {
                by: a,
                target: b,
                damage: 5,
                hp: 75,
                effect: Effectiveness::Normal,
                crit: false,
            }],
            &mut planned,
            &Plain,
        );
        assert_eq!(beats[0].text, None);
        assert_eq!(beats[0].frames, QUICK);
        assert_eq!(
            beats[0].cues[..2],
            [
                Cue::Sign {
                    slot: b,
                    sign: Sign::Struck { strong: false }
                },
                Cue::Hurt {
                    target: b,
                    heavy: 0
                }
            ]
        );
    }

    #[test]
    fn status_shows_on_the_fighters() {
        let (a, b) = (Slot::new(0, 0), Slot::new(1, 0));
        let mut planned = view();
        planned.active = [[Some(0), None], [Some(0), None]];
        planned.ailment[0][1] = Some(Ailment::Sleep);
        let events = [
            Event::Confused { target: b },
            Event::IsConfused { by: b },
            Event::CantAct {
                by: a,
                why: Blocked::Flinched,
            },
            Event::StatChanged {
                target: a,
                stat: Stat::Attack,
                by: -1,
            },
            Event::Hurt {
                target: b,
                damage: 5,
                hp: 75,
                by: HurtBy::Burn,
            },
            Event::ConfusionEnded { target: b },
            Event::Withdrawn { at: a, member: 0 },
            Event::SwitchedIn { at: a, member: 1 },
            Event::WokeUp { target: a },
        ];
        let cues: Vec<Cue> = beats(&events, &mut planned, &Plain)
            .into_iter()
            .flat_map(|b| b.cues)
            .collect();
        assert_eq!(
            cues,
            [
                Cue::Confusion { slot: b, on: true },
                Cue::Sign {
                    slot: b,
                    sign: Sign::Confused
                },
                Cue::Sign {
                    slot: a,
                    sign: Sign::Flinched
                },
                Cue::Sign {
                    slot: a,
                    sign: Sign::StatDown
                },
                Cue::Hurt {
                    target: b,
                    heavy: 0
                },
                Cue::Sign {
                    slot: b,
                    sign: Sign::Burned
                },
                Cue::Confusion { slot: b, on: false },
                Cue::Hide { slot: a },
                // The one coming in is still asleep.
                Cue::Show { slot: a, member: 1 },
                Cue::Ailment {
                    slot: a,
                    ailment: Some(Ailment::Sleep)
                },
                Cue::Ailment {
                    slot: a,
                    ailment: None
                },
            ]
        );
    }

    #[test]
    fn a_two_turn_moves_first_use_is_marked_as_getting_ready() {
        let (a, b) = (Slot::new(0, 0), Slot::new(1, 0));
        let mut planned = View {
            active: [[Some(0), None], [Some(0), None]],
            hp: [vec![100], vec![80]],
            max_hp: [vec![100], vec![80]],
            ailment: [vec![None], vec![None]],
        };
        let charging = |events: &[Event], planned: &mut View| -> Vec<bool> {
            beats(events, planned, &Plain)
                .into_iter()
                .flat_map(|b| b.cues)
                .filter_map(|c| match c {
                    Cue::Attack { charging, .. } => Some(charging),
                    _ => None,
                })
                .collect()
        };
        let ready = [
            Event::UsedMove { by: a, move_id: 7 },
            Event::Started {
                target: a,
                condition: Condition::Readying(7),
            },
            Event::UsedMove { by: b, move_id: 5 },
        ];
        assert_eq!(charging(&ready, &mut planned), [true, false]);
        // Someone else getting ready later doesn't count.
        let other = [
            Event::UsedMove { by: a, move_id: 5 },
            Event::UsedMove { by: b, move_id: 7 },
            Event::Started {
                target: b,
                condition: Condition::Readying(7),
            },
        ];
        assert_eq!(charging(&other, &mut planned), [false, true]);
    }
}
