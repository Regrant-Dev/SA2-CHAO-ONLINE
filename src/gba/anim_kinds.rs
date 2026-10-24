//! Which way each move's effect sprites move, read from its animation script
//! (re/chao-battle-visuals.md 6): the sprites it makes and their callbacks, the tasks it starts,
//! how many and how far apart. The callbacks and tasks are known only by their ROM addresses;
//! what each does to its sprite is sorted into a few movements here, and the arena draws each
//! movement its own way.

use super::battle_anim::{HIT_SCRIPT, Play, Step, Timed};
use super::rom::Rom;

/// How a move's sprites move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Movement {
    /// Nothing of its own (it only tints or moves the fighters).
    #[default]
    None,
    /// Only the plain hit, on the target.
    HitOnly,
    /// One shot from the user to the target.
    Straight,
    /// A quick stream of shots.
    Beam,
    /// A few shots one after another.
    Multi,
    /// A few lobbed shots.
    MultiArc,
    /// One lobbed shot.
    Arc,
    /// Shots twisting around their way to the target.
    Corkscrew,
    /// Swirling in onto a fighter.
    Spiral,
    /// Circling a fighter.
    Orbit,
    /// Falling from above onto a fighter.
    Rain,
    /// Rising from the ground around a fighter.
    Rise,
    /// From the target back to the user.
    Drain,
    /// Closing in on the user from all round.
    Gather,
    /// Bursting on a fighter.
    Burst,
    /// Showing on a fighter for a while.
    Aura,
}

impl Movement {
    /// Its sprites go from the user to the target.
    pub fn travels(self) -> bool {
        matches!(
            self,
            Movement::Straight
                | Movement::Beam
                | Movement::Multi
                | Movement::MultiArc
                | Movement::Arc
                | Movement::Corkscrew
                | Movement::Drain
        )
    }
}

/// Which fighter a movement that stays in one place shows on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Place {
    User,
    #[default]
    Target,
}

/// What a move's main sprites are drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A template's file offset.
    Template(usize),
    /// A sheet a task draws from (by tag).
    Sheet(u16),
}

/// What a move's script does, as the arena shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub movement: Movement,
    pub place: Place,
    pub main: Option<Source>,
    /// Sprites in the main group, and the median frames between them.
    pub count: u16,
    pub gap: u16,
    /// A burst on the target beside the main movement.
    pub impact: Option<usize>,
    /// Plain hits shown.
    pub hits: u16,
    /// The target shakes; the screen shakes (a big hit).
    pub shake: bool,
    pub big: bool,
    /// The commonest colour it tints with (BGR555).
    pub flash: Option<u16>,
    /// The user lunges (a sprite-less move of the user).
    pub lunge: bool,
    pub two_turn: bool,
    /// Frames the script runs (waits counted as `WAIT_FRAMES`).
    pub frames: u16,
    /// The main shot's frames and arc height (GBA pixels), when its callback takes them.
    pub duration: Option<u16>,
    pub height: Option<i16>,
}

/// Where a callback's sprite is anchored: a fighter, or the one its argument names (0 or 2
/// the user's side, else the target's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    User,
    Target,
    Arg(usize),
}

/// What a group of sprites does before it's sorted into a `Movement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Way {
    Straight,
    Wave,
    Sweep,
    Arc,
    Corkscrew,
    Spiral,
    Orbit,
    Rain,
    Rise,
    Drain,
    Gather,
    Burst,
    Aura,
}

/// Sprite callbacks by what they do with their sprite.
mod callback {
    pub const BURST_AT_TARGET: &[u32] = &[
        0x080A7939, 0x080FF375, 0x08100A51, 0x08100AE1, 0x08100B89, 0x081010CD, 0x081015AD,
        0x08101899, 0x08101F41, 0x08101FA9, 0x08101FF1, 0x08102845, 0x08102BCD, 0x08104365,
        0x08104415, 0x0810501D, 0x081061C5, 0x081069D1, 0x08106B55, 0x08106C81, 0x08107895,
        0x08108F09, 0x08108FBD, 0x08109199, 0x08109365, 0x0810A309, 0x0810A5BD, 0x0810A75D,
        0x0810AA2D, 0x0810B975, 0x0810BE49, 0x0810CE69, 0x0810CEB5, 0x0810D10D, 0x0810D1B5,
        0x0810D279, 0x0810D2E5, 0x0810D37D, 0x0810D985, 0x0810DA7D, 0x0810DCD1, 0x08110369,
        0x08110439, 0x081105B5, 0x0811067D, 0x08110AB5, 0x08111389, 0x08111B9D, 0x08112B79,
        0x081139DD, 0x081144BD, 0x08114A7D, 0x08116421, 0x081164F1, 0x0815A0D5, 0x0815A1B1,
        0x0815A255, 0x0815A2F1, 0x0815A8AD, 0x0815A935, 0x0815CC95, 0x0815F48D, 0x08160339,
    ];
    pub const BURST_ARG0: &[u32] = &[
        0x08106CD1, 0x0810AD31, 0x0810CF31, 0x0810D40D, 0x0810D4F5, 0x0810EE15, 0x0810FBA9,
        0x08115245, 0x08116459, 0x0815E6D9, 0x0815E955, 0x0815EE85,
    ];
    pub const BURST_ARG2: &[u32] = &[0x0810DA11, 0x08116389, 0x081163D1, 0x08116561, 0x081165A9];
    pub const BURST_ARG3: &[u32] = &[0x0810CEE1];
    pub const AURA_AT_USER: &[u32] = &[
        0x080A8B65, 0x08100EF1, 0x0810217D, 0x08102269, 0x081024E1, 0x08102FB9, 0x08103165,
        0x08103209, 0x08103285, 0x08103391, 0x081037D9, 0x08104B1D, 0x081051C5, 0x081052A5,
        0x081054E9, 0x08106879, 0x08106945, 0x081069B9, 0x08107731, 0x0810B23D, 0x0810EC35,
        0x0810F1ED, 0x0810F635, 0x0810F6B1, 0x081129F1, 0x08113A91, 0x0815A7B1, 0x0815ABD1,
        0x0815B395, 0x0815EA15, 0x081603A9,
    ];
    pub const AURA_ARG0: &[u32] = &[0x08104BAD, 0x0810ACD9, 0x08110995, 0x0815AAA5];
    pub const AURA_ARG2: &[u32] = &[0x080A77C9, 0x0810130D, 0x0810135D, 0x0810F58D];
    pub const STRAIGHT: &[u32] = &[
        0x080A7821, 0x080FE931, 0x081038C9, 0x08104089, 0x081041C5, 0x0810744D, 0x081077C1,
        0x08108C95, 0x08109AFD, 0x0810AD99, 0x0810B8ED, 0x0810BA25, 0x0810BC95, 0x0810C2F1,
        0x0810D609, 0x0810D715, 0x0810D875, 0x0810DFA9, 0x0810E045, 0x0810E1C9, 0x0810F085,
        0x081104E5, 0x08110721, 0x08111419, 0x0811160D, 0x081119E1, 0x08113065, 0x08113225,
        0x081135ED, 0x08114AF1, 0x0815B27D, 0x0815E01D, 0x0815E445,
    ];
    pub const WAVE: &[u32] = &[
        0x080FEB45, 0x08102CD5, 0x08106141, 0x08107261, 0x081075ED, 0x08108BE1, 0x0810C561,
        0x0815D7B5,
    ];
    pub const SWEEP: &[u32] = &[0x08100641, 0x0810EAA1, 0x08110E4D, 0x0815C6B1, 0x0815CB89];
    pub const ARC: &[u32] = &[
        0x080A78AD, 0x080FEC49, 0x080FF0F5, 0x080FFB19, 0x0810CC89, 0x0810DBAD, 0x0810DC2D,
        0x08110851, 0x08114995, 0x0815A6C5,
    ];
    pub const SPIRAL_AT_TARGET: &[u32] = &[
        0x080FED29, 0x080FF269, 0x08108EC9, 0x08109CB1, 0x0810A629, 0x0810B8AD, 0x0810BED1,
        0x0810DE71, 0x081117F5,
    ];
    pub const SPIRAL_ARG6: &[u32] = &[0x08110B39];
    pub const CORKSCREW: &[u32] = &[0x0810921D, 0x0810A46D];
    pub const ORBIT_AT_USER: &[u32] = &[
        0x080FEE79, 0x080FFDBD, 0x08102EB1, 0x08104305, 0x0810673D, 0x08106F01, 0x08106F61,
        0x081093A5, 0x08113251, 0x0815CDB5,
    ];
    pub const ORBIT_ARG0: &[u32] = &[0x0815ACD1];
    pub const RAIN_AT_TARGET: &[u32] = &[
        0x080A8EE5, 0x080FE841, 0x0810413D, 0x0810916D, 0x08109A11, 0x0810A1A9, 0x0810E315,
        0x0810EC95, 0x081109F1, 0x0815B49D, 0x0815F18D,
    ];
    pub const RAIN_AT_USER: &[u32] = &[0x080FEF45, 0x08102541, 0x08105DE9];
    pub const RAIN_ARG2: &[u32] = &[0x0815C95D];
    pub const RISE_AT_TARGET: &[u32] =
        &[0x0810624D, 0x08106AD1, 0x08108035, 0x0810DD51, 0x0810DDC5];
    pub const RISE_AT_USER: &[u32] = &[
        0x080A8E31, 0x080FEFFD, 0x080FF769, 0x080FF7ED, 0x080FF935, 0x081014F5, 0x08103029,
        0x0810387D, 0x08105539, 0x08105E61, 0x081070AD, 0x08108F4D, 0x0810E13D, 0x0810ED29,
        0x08110241, 0x08110F31, 0x0815A7ED, 0x0815B70D, 0x0815BE05, 0x0815C401,
    ];
    pub const RISE_ARG0: &[u32] = &[0x081020D9, 0x08106E01, 0x081131B5, 0x08114B81, 0x081151A1];
    pub const RISE_ARG1: &[u32] = &[0x0810EEF9];
    pub const RISE_ARG3: &[u32] = &[0x081078D1];
    pub const DRAIN: &[u32] = &[0x080FEAD9, 0x080FF699, 0x0810234D];
    pub const GATHER: &[u32] = &[0x080FE8E1, 0x08100899];

    /// Sprites with no sheet: the user lunging, a tint (colour in an argument), a shake.
    pub const LUNGE: &[u32] = &[
        0x080D5861, 0x080D58F9, 0x080D5969, 0x080D5A95, 0x080D5B49, 0x08101941,
    ];
    pub const TINT_COLOUR_4: u32 = 0x081157FD;
    pub const TINT_COLOUR_3: u32 = 0x081158A5;
    /// Its argument 3: 0 or 1 shakes the screen, 2 or 3 a fighter.
    pub const SHAKE: u32 = 0x081160A5;
    /// The straight and arc callbacks that take a duration (argument 4) and an arc height
    /// (argument 5).
    pub const TIMED: &[u32] = &[0x080A7821, 0x080A78AD, 0x08110851];
    pub const WITH_HEIGHT: &[u32] = &[0x080A78AD, 0x08110851];
}

/// Tasks by what they do.
mod task {
    /// Shake a fighter (argument 0: 1 or 3 the target's side).
    pub const SHAKE: &[u32] = &[
        0x080D51AD, 0x080D52D1, 0x080D5485, 0x080D5645, 0x080D6389, 0x0810A095, 0x080D5EB9,
        0x0815C479,
    ];
    /// Shakes the whole screen.
    pub const SCREEN_SHAKE: u32 = 0x081162A5;
    /// Shakes the screen with argument 0 of 4 or more, else one fighter.
    pub const SHAKE_SCREEN_OR_ONE: u32 = 0x081152DD;
    /// Tints with the colour in argument 4 or 5.
    pub const TINT_COLOUR_4: &[u32] = &[0x08116621, 0x08116665];
    pub const TINT_COLOUR_5: &[u32] = &[0x08115A05, 0x08115B0D, 0x08115C81];
    /// Tasks that draw sprites of their own.
    pub const RAIN_AT_TARGET: &[u32] =
        &[0x08107189, 0x0810C919, 0x081085C9, 0x0810A7DD, 0x0810B55D];
    pub const RAIN_AT_USER: &[u32] = &[0x081088E5, 0x0815BB85];
    pub const SWEEP: &[u32] = &[0x08107955];
    pub const RISE_AT_USER: &[u32] = &[0x08109461, 0x081080E5];
    pub const STRAIGHT: &[u32] = &[0x0810AFCD, 0x0810B29D, 0x08103CF1, 0x080FEA59];
    pub const BURST_AT_TARGET: &[u32] = &[0x0810EB89, 0x0815A161, 0x080A9EF5];
    pub const BURST_AT_USER: &[u32] = &[0x08105AAD, 0x0815DCA5];
    pub const ARC: &[u32] = &[0x0815E47D];
    pub const GATHER: &[u32] = &[0x0810AAFD];
    pub const ORBIT: &[u32] = &[0x0810F941, 0x08112C6D];
}

/// What a sprite callback does with its sprite.
fn callback_way(cb: u32) -> Option<(Way, Anchor)> {
    use Anchor::*;
    use callback as c;
    let table: [(&[u32], Way, Anchor); 25] = [
        (c::BURST_AT_TARGET, Way::Burst, Target),
        (c::BURST_ARG0, Way::Burst, Arg(0)),
        (c::BURST_ARG2, Way::Burst, Arg(2)),
        (c::BURST_ARG3, Way::Burst, Arg(3)),
        (c::AURA_AT_USER, Way::Aura, User),
        (c::AURA_ARG0, Way::Aura, Arg(0)),
        (c::AURA_ARG2, Way::Aura, Arg(2)),
        (c::STRAIGHT, Way::Straight, User),
        (c::WAVE, Way::Wave, User),
        (c::SWEEP, Way::Sweep, User),
        (c::ARC, Way::Arc, User),
        (c::SPIRAL_AT_TARGET, Way::Spiral, Target),
        (c::SPIRAL_ARG6, Way::Spiral, Arg(6)),
        (c::CORKSCREW, Way::Corkscrew, User),
        (c::ORBIT_AT_USER, Way::Orbit, User),
        (c::ORBIT_ARG0, Way::Orbit, Arg(0)),
        (c::RAIN_AT_TARGET, Way::Rain, Target),
        (c::RAIN_AT_USER, Way::Rain, User),
        (c::RAIN_ARG2, Way::Rain, Arg(2)),
        (c::RISE_AT_TARGET, Way::Rise, Target),
        (c::RISE_AT_USER, Way::Rise, User),
        (c::RISE_ARG0, Way::Rise, Arg(0)),
        (c::RISE_ARG1, Way::Rise, Arg(1)),
        (c::RISE_ARG3, Way::Rise, Arg(3)),
        (c::DRAIN, Way::Drain, User),
    ];
    if c::GATHER.contains(&cb) {
        return Some((Way::Gather, User));
    }
    table
        .iter()
        .find(|(list, _, _)| list.contains(&cb))
        .map(|&(_, way, anchor)| (way, anchor))
}

/// What a task draws of its own, if anything.
fn task_way(function: u32) -> Option<(Way, Place)> {
    use task as t;
    let table: [(&[u32], Way, Place); 10] = [
        (t::RAIN_AT_TARGET, Way::Rain, Place::Target),
        (t::RAIN_AT_USER, Way::Rain, Place::User),
        (t::SWEEP, Way::Sweep, Place::User),
        (t::RISE_AT_USER, Way::Rise, Place::User),
        (t::STRAIGHT, Way::Straight, Place::User),
        (t::BURST_AT_TARGET, Way::Burst, Place::Target),
        (t::BURST_AT_USER, Way::Burst, Place::User),
        (t::ARC, Way::Arc, Place::User),
        (t::GATHER, Way::Gather, Place::User),
        (t::ORBIT, Way::Orbit, Place::User),
    ];
    table
        .iter()
        .find(|(list, _, _)| list.contains(&function))
        .map(|&(_, way, place)| (way, place))
}

/// Argument `i` names the user's side (0 or 2) or the target's.
fn place_of(anchor: Anchor, args: &[i16]) -> Place {
    match anchor {
        Anchor::User => Place::User,
        Anchor::Target => Place::Target,
        Anchor::Arg(i) => match args.get(i).copied().unwrap_or(0) {
            0 | 2 => Place::User,
            _ => Place::Target,
        },
    }
}

/// Which group wins: shots from the user to the target first, then movements on the target,
/// on the user, bursts, auras.
fn tier(way: Way, place: Place) -> u8 {
    match way {
        Way::Straight | Way::Wave | Way::Sweep | Way::Arc | Way::Drain | Way::Corkscrew => 4,
        Way::Rain | Way::Spiral | Way::Rise if place == Place::Target => 3,
        Way::Orbit | Way::Gather | Way::Rise | Way::Rain | Way::Spiral => 2,
        Way::Burst => 1,
        Way::Aura => 0,
    }
}

/// Sprites of one kind in a script.
struct Group {
    way: Way,
    place: Place,
    callback: u32,
    /// Task groups draw from a loaded sheet (0 here).
    tag: u16,
    frames: Vec<u16>,
    templates: Vec<usize>,
    /// The first sprite's arguments.
    args: Vec<i16>,
}

/// Shots needed for a quick stream, and the most frames between them.
const BEAM_SHOTS: usize = 8;
const BEAM_GAP: u16 = 4;
/// Shakes of the target, or of fighters by their place on the field, that make a big hit
/// together.
const BIG_SHAKES: usize = 4;
const BIG_FIELD_SHAKES: usize = 3;

/// Sorts a script in play order (`hit`: the plain hit's sheet; `tag_of`: a template's sheet
/// when it has one).
pub fn sort(play: &Play, hit: Option<u16>, tag_of: impl Fn(usize) -> Option<u16>) -> Plan {
    let mut plan = Plan {
        two_turn: play.two_turn,
        frames: play.steps.last().map_or(0, |s| s.frame),
        ..Plan::default()
    };
    let mut groups: Vec<Group> = Vec::new();
    let mut colours: Vec<(u16, usize)> = Vec::new();
    let mut tint = |c: Option<&i16>| {
        if let Some(&c) = c {
            let c = c as u16 & 0x7FFF;
            match colours.iter_mut().find(|(k, _)| *k == c) {
                Some((_, n)) => *n += 1,
                None => colours.push((c, 1)),
            }
        }
    };
    let mut target_shakes = 0;
    let mut field_shakes = 0;
    let mut used_tags = Vec::new();
    let mut loaded = Vec::new();
    for Timed { frame, step } in &play.steps {
        match step {
            Step::Load(tag) => loaded.push(*tag),
            Step::Sprite {
                template,
                callback: cb,
                by_target,
                args,
            } => {
                let cb = *cb;
                if callback::LUNGE.contains(&cb) {
                    plan.lunge = true;
                    continue;
                }
                if cb == callback::TINT_COLOUR_4 {
                    tint(args.get(4));
                    continue;
                }
                if cb == callback::TINT_COLOUR_3 {
                    tint(args.get(3));
                    continue;
                }
                if cb == callback::SHAKE {
                    match args.get(3).copied().unwrap_or(2) {
                        0 | 1 => plan.big = true,
                        _ => plan.shake = true,
                    }
                    continue;
                }
                let Some(tag) = tag_of(*template) else {
                    continue;
                };
                if Some(tag) == hit {
                    plan.hits += 1;
                    continue;
                }
                used_tags.push(tag);
                let (way, place) = match callback_way(cb) {
                    Some((way, anchor)) => (way, place_of(anchor, args)),
                    // Not one we know: where its layering puts it.
                    None if *by_target => (Way::Burst, Place::Target),
                    None => (Way::Aura, Place::User),
                };
                match groups
                    .iter_mut()
                    .find(|g| g.way == way && g.place == place && g.callback == cb && g.tag == tag)
                {
                    Some(g) => {
                        g.frames.push(*frame);
                        g.templates.push(*template);
                    }
                    None => groups.push(Group {
                        way,
                        place,
                        callback: cb,
                        tag,
                        frames: vec![*frame],
                        templates: vec![*template],
                        args: args.clone(),
                    }),
                }
            }
            Step::Task { function, args } => {
                let f = *function;
                if task::SHAKE.contains(&f) {
                    plan.shake = true;
                    match args.first() {
                        Some(1 | 3) => target_shakes += 1,
                        Some(&a) if a >= 4 => field_shakes += 1,
                        _ => {}
                    }
                } else if f == task::SCREEN_SHAKE {
                    plan.big = true;
                } else if f == task::SHAKE_SCREEN_OR_ONE {
                    if args.first().is_some_and(|&a| a >= 4) {
                        plan.big = true;
                    } else {
                        plan.shake = true;
                    }
                } else if task::TINT_COLOUR_4.contains(&f) {
                    tint(args.get(4));
                } else if task::TINT_COLOUR_5.contains(&f) {
                    tint(args.get(5));
                } else if let Some((way, place)) = task_way(f) {
                    match groups
                        .iter_mut()
                        .find(|g| g.way == way && g.place == place && g.callback == f)
                    {
                        Some(g) => g.frames.push(*frame),
                        None => groups.push(Group {
                            way,
                            place,
                            callback: f,
                            tag: 0,
                            frames: vec![*frame],
                            templates: Vec::new(),
                            args: args.clone(),
                        }),
                    }
                }
            }
        }
    }
    if target_shakes >= BIG_SHAKES || field_shakes >= BIG_FIELD_SHAKES {
        plan.big = true;
    }
    plan.flash = colours
        .iter()
        .enumerate()
        .max_by_key(|&(i, &(_, n))| (n, std::cmp::Reverse(i)))
        .map(|(_, &(c, _))| c);
    // The main group: the best tier, the most sprites, the first met.
    let best = groups
        .iter()
        .enumerate()
        .max_by_key(|&(i, g)| (tier(g.way, g.place), g.frames.len(), std::cmp::Reverse(i)))
        .map(|(i, _)| i);
    let Some(main) = best.map(|i| &groups[i]) else {
        plan.movement = if plan.hits > 0 || plan.shake || plan.big {
            Movement::HitOnly
        } else {
            Movement::None
        };
        return plan;
    };
    let n = main.frames.len();
    let mut gaps: Vec<u16> = main.frames.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_unstable();
    let gap = gaps.get(gaps.len() / 2).copied().unwrap_or(0);
    let stream = n >= BEAM_SHOTS && gap <= BEAM_GAP;
    plan.movement = match main.way {
        Way::Straight | Way::Wave | Way::Sweep if stream => Movement::Beam,
        Way::Straight | Way::Wave | Way::Sweep if n >= 2 => Movement::Multi,
        Way::Straight => Movement::Straight,
        Way::Wave => Movement::Corkscrew,
        Way::Sweep => Movement::Beam,
        Way::Arc if n >= 2 && !stream => Movement::MultiArc,
        Way::Arc => Movement::Arc,
        Way::Corkscrew => Movement::Corkscrew,
        Way::Spiral => Movement::Spiral,
        Way::Orbit => Movement::Orbit,
        Way::Rain => Movement::Rain,
        Way::Rise => Movement::Rise,
        Way::Drain => Movement::Drain,
        Way::Gather => Movement::Gather,
        Way::Burst => Movement::Burst,
        Way::Aura => Movement::Aura,
    };
    plan.place = main.place;
    plan.count = n as u16;
    plan.gap = gap;
    plan.main = if main.templates.is_empty() {
        // A task's own sprites: the sheet it loaded that no sprite uses, else any but the hit.
        loaded
            .iter()
            .find(|t| !used_tags.contains(t) && Some(**t) != hit)
            .or_else(|| loaded.iter().find(|t| Some(**t) != hit))
            .map(|&t| Source::Sheet(t))
    } else {
        commonest(&main.templates).map(Source::Template)
    };
    if callback::TIMED.contains(&main.callback) {
        plan.duration = main.args.get(4).and_then(|&d| u16::try_from(d).ok());
    }
    if callback::WITH_HEIGHT.contains(&main.callback) {
        plan.height = main.args.get(5).copied();
    }
    plan.impact = groups
        .iter()
        .filter(|g| g.way == Way::Burst && !std::ptr::eq(*g, main) && !g.templates.is_empty())
        .max_by_key(|g| g.frames.len())
        .and_then(|g| commonest(&g.templates));
    plan
}

/// The value met most often (the first of equals).
fn commonest(values: &[usize]) -> Option<usize> {
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for &v in values {
        match counts.iter_mut().find(|(k, _)| *k == v) {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    let best = counts.iter().map(|&(_, n)| n).max()?;
    counts.into_iter().find(|&(_, n)| n == best).map(|(v, _)| v)
}

impl Rom {
    /// The plain hit's sheet.
    pub fn hit_tag(&self) -> Option<u16> {
        let hit = self.general_anim(HIT_SCRIPT)?;
        hit.created
            .iter()
            .find_map(|c| self.template_tag(c.template))
    }

    /// How move `id`'s sprites move (`hit`: `hit_tag`). A script that shows no sprite of its
    /// own on its main path is read again taking its conditional jumps.
    pub fn move_plan(&self, id: u16, hit: Option<u16>) -> Option<Plan> {
        let tag_of = |t: usize| self.template_tag(t);
        let plan = sort(&self.move_play(id, false)?, hit, tag_of);
        if plan.main.is_some() {
            return Some(plan);
        }
        let shows = |p: &Plan| (p.main.is_some(), p.movement != Movement::None);
        let mut plan = match self.move_play(id, true).map(|p| sort(&p, hit, tag_of)) {
            Some(other) if shows(&other) > shows(&plan) => other,
            _ => plan,
        };
        // A move doing no damage whose script only shakes shows no hit (its shakes stay).
        let status = self.move_info(id).is_some_and(|m| m.power == 0);
        if plan.movement == Movement::HitOnly && plan.hits == 0 && status {
            plan.movement = Movement::None;
        }
        Some(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::rom::MOVE_COUNT;

    fn sprite(frame: u16, template: usize, callback: u32, args: &[i16]) -> Timed {
        Timed {
            frame,
            step: Step::Sprite {
                template,
                callback,
                by_target: false,
                args: args.to_vec(),
            },
        }
    }

    fn task(frame: u16, function: u32, args: &[i16]) -> Timed {
        Timed {
            frame,
            step: Step::Task {
                function,
                args: args.to_vec(),
            },
        }
    }

    /// Templates 1..=9 have sheets 10001..=10009; 135 the plain hit.
    fn tag_of(t: usize) -> Option<u16> {
        (1..=9)
            .contains(&t)
            .then_some(10_000 + t as u16)
            .or(match t {
                135 => Some(10_135),
                _ => None,
            })
    }

    fn sorted(steps: Vec<Timed>) -> Plan {
        sort(
            &Play {
                steps,
                two_turn: false,
            },
            Some(10_135),
            tag_of,
        )
    }

    const STRAIGHT: u32 = 0x080A7821;
    const ARC: u32 = 0x080A78AD;
    const BURST: u32 = 0x080A7939;
    const ON_FIGHTER: u32 = 0x080A77C9;

    #[test]
    fn a_stream_of_shots_is_a_beam_and_a_few_are_a_volley() {
        let beam: Vec<Timed> = (0..10).map(|i| sprite(i * 2, 1, STRAIGHT, &[])).collect();
        let plan = sorted(beam);
        assert_eq!(plan.movement, Movement::Beam);
        assert_eq!((plan.count, plan.gap), (10, 2));
        assert_eq!(plan.main, Some(Source::Template(1)));
        let volley: Vec<Timed> = (0..3)
            .map(|i| sprite(i * 10, 2, STRAIGHT, &[0, 0, 0, 0, 20]))
            .collect();
        let plan = sorted(volley);
        assert_eq!(plan.movement, Movement::Multi);
        assert_eq!(plan.duration, Some(20));
        let lobs: Vec<Timed> = (0..3)
            .map(|i| sprite(i * 6, 3, ARC, &[0, 0, 0, 0, 24, 30]))
            .collect();
        let plan = sorted(lobs);
        assert_eq!(plan.movement, Movement::MultiArc);
        assert_eq!((plan.duration, plan.height), (Some(24), Some(30)));
        assert_eq!(sorted(vec![sprite(0, 3, ARC, &[])]).movement, Movement::Arc);
    }

    #[test]
    fn shots_win_over_bursts_and_bursts_become_the_impact() {
        let plan = sorted(vec![
            sprite(0, 4, BURST, &[]),
            sprite(2, 4, BURST, &[]),
            sprite(4, 1, STRAIGHT, &[]),
            sprite(20, 135, BURST, &[]),
        ]);
        assert_eq!(plan.movement, Movement::Straight);
        assert_eq!(plan.impact, Some(4));
        assert_eq!(plan.hits, 1);
    }

    #[test]
    fn an_argument_names_the_fighter() {
        let user = sorted(vec![sprite(0, 5, ON_FIGHTER, &[0, 0, 0])]);
        assert_eq!((user.movement, user.place), (Movement::Aura, Place::User));
        let target = sorted(vec![sprite(0, 5, ON_FIGHTER, &[0, 0, 1])]);
        assert_eq!(target.place, Place::Target);
    }

    #[test]
    fn the_plain_hit_and_shakes_alone_are_a_hit_and_tints_are_kept() {
        let plan = sorted(vec![
            sprite(0, 135, BURST, &[]),
            task(0, task::SHAKE[0], &[1, 3, 0, 6, 1]),
            sprite(0, 0, callback::TINT_COLOUR_4, &[2, 0, 0, 16, 0x1F]),
            sprite(4, 0, callback::TINT_COLOUR_4, &[2, 0, 0, 16, 0x1F]),
            sprite(8, 0, callback::TINT_COLOUR_4, &[2, 0, 0, 16, 0x7C00]),
        ]);
        assert_eq!(plan.movement, Movement::HitOnly);
        assert!(plan.shake && !plan.big);
        assert_eq!(plan.flash, Some(0x1F));
        assert_eq!(plan.main, None);
        assert_eq!(sorted(Vec::new()).movement, Movement::None);
        // A screen shake, or many fighter shakes, make a big hit.
        assert!(sorted(vec![task(0, task::SCREEN_SHAKE, &[])]).big);
        assert!(sorted(vec![task(0, task::SHAKE_SCREEN_OR_ONE, &[5])]).big);
        assert!(!sorted(vec![task(0, task::SHAKE_SCREEN_OR_ONE, &[1])]).big);
        let shakes = (0..4).map(|i| task(i * 4, task::SHAKE[1], &[1])).collect();
        assert!(sorted(shakes).big);
    }

    #[test]
    fn a_task_drawn_movement_takes_the_sheet_it_loads() {
        let plan = sorted(vec![
            Timed {
                frame: 0,
                step: Step::Load(10_135),
            },
            Timed {
                frame: 0,
                step: Step::Load(10_007),
            },
            task(4, task::RAIN_AT_TARGET[0], &[]),
        ]);
        assert_eq!((plan.movement, plan.place), (Movement::Rain, Place::Target));
        assert_eq!(plan.main, Some(Source::Sheet(10_007)));
    }

    fn rom() -> Option<Rom> {
        let home = std::env::var_os("HOME")?;
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return None;
        }
        Some(Rom::open(&path).expect("ROM opens"))
    }

    #[test]
    fn every_move_sorts_and_the_movements_spread_out() {
        let Some(rom) = rom() else { return };
        let hit = rom.hit_tag();
        assert!(hit.is_some());
        let mut counts = std::collections::HashMap::new();
        let (mut big, mut shake, mut flash, mut two_turn) = (0, 0, 0, 0);
        for id in 1..MOVE_COUNT {
            let plan = rom.move_plan(id, hit).expect("script walks");
            *counts.entry(plan.movement).or_insert(0) += 1;
            big += usize::from(plan.big);
            shake += usize::from(plan.shake);
            flash += usize::from(plan.flash.is_some());
            two_turn += usize::from(plan.two_turn);
            let shown = !matches!(plan.movement, Movement::None | Movement::HitOnly);
            assert_eq!(shown, plan.main.is_some() || plan.count > 0, "move {id}");
            if let Some(Source::Template(t)) = plan.main {
                assert!(rom.anim_template(t).is_some(), "move {id}");
            }
            assert!(plan.frames < 600, "move {id}: {}", plan.frames);
        }
        let n = |m: Movement| counts.get(&m).copied().unwrap_or(0);
        // Every movement is some moves', none is most of them.
        for m in [
            Movement::HitOnly,
            Movement::Straight,
            Movement::Beam,
            Movement::Multi,
            Movement::MultiArc,
            Movement::Arc,
            Movement::Corkscrew,
            Movement::Spiral,
            Movement::Orbit,
            Movement::Rain,
            Movement::Rise,
            Movement::Drain,
            Movement::Gather,
            Movement::Burst,
            Movement::Aura,
        ] {
            assert!(n(m) > 0, "{m:?}: {counts:?}");
            assert!(n(m) < 100, "{m:?}: {counts:?}");
        }
        let travelling: usize = counts
            .iter()
            .filter(|(m, _)| m.travels())
            .map(|(_, &c)| c)
            .sum();
        assert!(travelling > 60, "{counts:?}");
        assert!(n(Movement::None) < 50, "{counts:?}");
        assert!((8..40).contains(&big), "{big}");
        assert!(shake > 150, "{shake}");
        assert!(flash > 60, "{flash}");
        assert!((5..30).contains(&two_turn), "{two_turn}");
    }
}
