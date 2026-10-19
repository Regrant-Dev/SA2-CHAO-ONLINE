//! Stage races and stage records (re/stage-race.md).
//!
//! - **Finish:** the game's level-clear routine sets `0x174B002` on the frame a player clears the
//!   stage. Then the game's clear time (its timer, which rewinds to the last checkpoint on a
//!   death) goes to the records, and in a stage started together our race time (counted from
//!   the GO, deaths included) goes to the room.
//! - **Live positions:** SA2 has no "how far along the stage" value, so the first clear of a
//!   stage records its route (a point every ~50 units, loops from deaths cut out), saved in
//!   `routes/` and shared with the others. Progress is where we are along that route.
//! - In a Race room, a position line shows while racing ("2nd of 3"), and finishes are
//!   announced ("Mason finished 1st: 1:23.45").

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Vector};
use crate::hud;
use crate::log;
use crate::net::protocol::{RACE_PROGRESS_MAX, RACE_PROGRESS_UNKNOWN};
use crate::together::Peer;

/// Set on the frame the stage is cleared (0 again at load and restart).
const CLEARED: usize = 0x0174_B002;
/// The game's timer: minutes, seconds, frames (1/60 s), bytes.
const TIMER: usize = 0x0174_AFDB;
/// Route points are this far apart.
const STEP: f32 = 50.0;
/// Send our race status every this many frames (~4 per second).
const SEND_EVERY: u32 = 15;
/// Others' statuses count this long.
const STALE: Duration = Duration::from_secs(3);
/// Search this many route points around the last match before searching all of it.
const NEAR_POINTS: usize = 40;
/// How long after a finish an unfinished status from the same player is taken as a late one.
const LATE_STATUS: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct Other {
    progress: u16,
    finish: Option<(u32, u32)>,
    at: Instant,
    announced: bool,
}

/// Our race status to send: (level, mission, progress, finish, reliably).
type Status = (i16, u8, u16, Option<(u32, u32)>, bool);
/// A route to send: (level, mission, points).
type RouteOut = (i16, u8, Vec<[f32; 3]>);

struct Race {
    stage: Option<(i16, u8)>,
    /// `game::stage_run` of `stage`: a replay is a new race.
    run: u32,
    route: Option<Vec<Vector>>,
    recording: Vec<Vector>,
    near: usize,
    /// Our latest progress (`RACE_PROGRESS_*`).
    mine: u16,
    cleared: bool,
    finish: Option<(u32, u32)>,
    frame: u32,
    others: HashMap<u8, (i16, u8, Other)>,
    status_out: Option<Status>,
    route_out: Option<RouteOut>,
}

static RACE: Mutex<Option<Race>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Race) -> R) -> R {
    let mut r = RACE.lock().unwrap_or_else(|e| e.into_inner());
    f(r.get_or_insert_with(|| Race {
        stage: None,
        run: 0,
        route: None,
        recording: Vec::new(),
        near: 0,
        mine: RACE_PROGRESS_UNKNOWN,
        cleared: false,
        finish: None,
        frame: 0,
        others: HashMap::new(),
        status_out: None,
        route_out: None,
    }))
}

fn route_file(level: i16, mission: u8) -> PathBuf {
    PathBuf::from(crate::online::mod_path())
        .join("routes")
        .join(format!("{level}_{mission}.route"))
}

fn load_route(level: i16, mission: u8) -> Option<Vec<Vector>> {
    let text = std::fs::read_to_string(route_file(level, mission)).ok()?;
    let points: Vec<Vector> = text
        .lines()
        .filter_map(|l| {
            let v: Vec<f32> = l
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            (v.len() == 3).then(|| Vector {
                x: v[0],
                y: v[1],
                z: v[2],
            })
        })
        .collect();
    (points.len() >= 2).then_some(points)
}

fn save_route(level: i16, mission: u8, points: &[Vector]) {
    let file = route_file(level, mission);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = points
        .iter()
        .map(|p| format!("{} {} {}\n", p.x, p.y, p.z))
        .collect();
    match std::fs::write(&file, text) {
        Ok(()) => log!(
            "race: route saved to {} ({} points)",
            file.display(),
            points.len()
        ),
        Err(e) => log!("race: can't save {}: {e}", file.display()),
    }
}

fn distance2(a: Vector, b: Vector) -> f32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

/// Adds `at` to a route being recorded: a point every `STEP`; coming back near an earlier point
/// (a restart after a death) cuts the loop off.
fn record(points: &mut Vec<Vector>, at: Vector) {
    // Longer routes couldn't be shared (`MAX_ROUTE`).
    if points.len() >= crate::net::protocol::MAX_ROUTE {
        return;
    }
    let Some(&last) = points.last() else {
        points.push(at);
        return;
    };
    if distance2(last, at) < STEP * STEP {
        return;
    }
    let back = points
        .iter()
        .take(points.len().saturating_sub(3))
        .rposition(|p| distance2(*p, at) < STEP * STEP);
    if let Some(i) = back {
        points.truncate(i + 1);
    } else {
        points.push(at);
    }
}

/// Where `at` is along `route` (0..=1), searching near point `near` first.
fn progress(route: &[Vector], at: Vector, near: &mut usize) -> f32 {
    let closest = |range: std::ops::Range<usize>| {
        range
            .map(|i| (i, distance2(route[i], at)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    };
    let n = route.len();
    let lo = near.saturating_sub(NEAR_POINTS);
    let hi = (*near + NEAR_POINTS).min(n);
    let mut best = closest(lo..hi).unwrap_or((0, f32::MAX));
    // Far from the route near the last match (a restart, or a shortcut): search all of it.
    if best.1 > (STEP * 4.0) * (STEP * 4.0) {
        best = closest(0..n).unwrap_or(best);
    }
    *near = best.0;
    // A fraction towards the next point.
    let i = best.0;
    let frac = if i + 1 < n {
        let (a, b) = (route[i], route[i + 1]);
        let seg = Vector {
            x: b.x - a.x,
            y: b.y - a.y,
            z: b.z - a.z,
        };
        let len2 = seg.x * seg.x + seg.y * seg.y + seg.z * seg.z;
        if len2 > 0.0 {
            (((at.x - a.x) * seg.x + (at.y - a.y) * seg.y + (at.z - a.z) * seg.z) / len2)
                .clamp(0.0, 1.0)
        } else {
            0.0
        }
    } else {
        0.0
    };
    ((i as f32 + frac) / (n - 1).max(1) as f32).clamp(0.0, 1.0)
}

/// Whether `to` is further along this stage's route than `from`: always without `from`, never
/// without a route (the stage hasn't been cleared here yet, so there's no telling).
pub fn is_further(from: Option<[f32; 3]>, to: [f32; 3]) -> bool {
    let v = |p: [f32; 3]| Vector {
        x: p[0],
        y: p[1],
        z: p[2],
    };
    let Some(from) = from else {
        return true;
    };
    with(|r| {
        r.route.as_ref().is_some_and(|route| {
            let (mut a, mut b) = (0, 0);
            progress(route, v(to), &mut b) >= progress(route, v(from), &mut a)
        })
    })
}

/// The game's clear time in centiseconds.
fn game_time() -> u32 {
    // SAFETY: the game's static timer bytes.
    let [m, s, f] = unsafe { (TIMER as *const [u8; 3]).read_volatile() }.map(u32::from);
    ((m * 60 + s) * 60 + f) * 100 / 60
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Room players in our stage still racing (not finished, recently heard from).
pub fn others_racing() -> bool {
    let (level, mission) = (game::current_level(), game::mission());
    with(|r| {
        r.others.values().any(|(l, m, o)| {
            (*l, *m) == (level, mission) && o.finish.is_none() && o.at.elapsed() < STALE
        })
    })
}

/// Network: another player's race status.
pub fn received_status(
    from: u8,
    level: i16,
    mission: u8,
    progress: u16,
    finish: Option<(u32, u32)>,
) {
    with(|r| {
        let before = r
            .others
            .get(&from)
            .filter(|(l, m, _)| (*l, *m) == (level, mission))
            .map(|(_, _, o)| *o);
        // A status from before their finish arriving after it (sent unreliably) doesn't
        // take the finish back.
        if finish.is_none()
            && before.is_some_and(|o| o.finish.is_some() && o.at.elapsed() < LATE_STATUS)
        {
            return;
        }
        let announced = before.is_some_and(|o| o.announced);
        r.others.insert(
            from,
            (
                level,
                mission,
                Other {
                    progress,
                    finish,
                    at: Instant::now(),
                    announced,
                },
            ),
        );
    });
}

/// Network: a stage route someone recorded. Kept if we don't have one for that stage.
pub fn received_route(level: i16, mission: u8, points: Vec<[f32; 3]>) {
    let points: Vec<Vector> = points
        .into_iter()
        .map(|[x, y, z]| Vector { x, y, z })
        .collect();
    if points.len() < 2 {
        return;
    }
    // The files are read and written on a thread of their own: the network thread holds the
    // lock the game thread waits on, and a disk can be slow.
    let saved = std::thread::Builder::new()
        .name("sa2-chao-online route".into())
        .spawn(move || {
            let _one_at_a_time = ROUTE_FILES.lock().unwrap_or_else(|e| e.into_inner());
            if load_route(level, mission).is_some() {
                return;
            }
            save_route(level, mission, &points);
            with(|r| {
                if r.stage == Some((level, mission)) && r.route.is_none() {
                    r.route = Some(points);
                }
            });
        });
    if let Err(e) = saved {
        log!("race: can't save a route: {e}");
    }
}

/// Held while a received route is checked and saved.
static ROUTE_FILES: Mutex<()> = Mutex::new(());

/// To send: our race status (level, mission, progress, finish, reliably), and a route we
/// recorded.
pub fn take_outgoing() -> (Option<Status>, Option<RouteOut>) {
    with(|r| (r.status_out.take(), r.route_out.take()))
}

/// Call every frame (game thread) with the other players' latest states.
pub fn on_frame(peers: &[Peer], connected: bool) {
    let level = game::current_level();
    let mission = game::mission();
    let in_stage = level > 0 && level != game::LEVEL_CHAO_WORLD && !game::two_player();
    with(|r| {
        let stage = in_stage.then_some((level, mission));
        let run = game::stage_run();
        if r.stage != stage || r.run != run {
            r.stage = stage;
            r.run = run;
            r.route = stage.and_then(|(l, m)| load_route(l, m));
            r.recording.clear();
            r.near = 0;
            r.mine = RACE_PROGRESS_UNKNOWN;
            r.cleared = false;
            r.finish = None;
        }
        if !in_stage || game::game_state() != game::GAME_STATE_INGAME {
            return;
        }
        let kart = crate::kart::in_kart_race();
        let story_kart = level == crate::kart::STORY_KART_LEVEL;
        // SAFETY: a static byte the game's level-clear routine sets.
        let cleared = if kart {
            crate::kart::finished().is_some()
        } else {
            !story_kart && unsafe { (CLEARED as *const u8).read_volatile() } != 0
        };
        let started = crate::together::started_at();
        let shared_boss = connected
            && crate::room::in_room()
            && crate::room::mode() == crate::room::Mode::Coop
            && crate::bosses::is_boss_stage(level);
        let position = game::player_position().map(|(p, _)| p);
        let just_cleared = cleared && !r.cleared;
        if just_cleared {
            let game_cs = if kart {
                crate::kart::finished().unwrap_or(0)
            } else {
                game_time()
            };
            // Karts start together on their own clock: it's the race time too.
            let race_cs = if kart {
                started.map(|_| game_cs)
            } else {
                started
                    .map(|at| (at.elapsed().as_millis() / 10) as u32)
                    .or_else(|| shared_boss.then_some(game_cs))
            };
            log!(
                "race: cleared level {level} mission {} in {} (game clock)",
                mission + 1,
                crate::places::time(game_cs)
            );
            if let Some(race_cs) = race_cs {
                r.finish = Some((race_cs, game_cs));
                r.status_out = Some((level, mission, RACE_PROGRESS_MAX, r.finish, true));
            }
            // The first clear of this stage gives it a route.
            if !kart
                && !crate::places::is_hunt_stage(level)
                && r.route.is_none()
                && r.recording.len() >= 10
            {
                if let Some(p) = position {
                    r.recording.push(p);
                }
                save_route(level, mission, &r.recording);
                r.route_out = Some((
                    level,
                    mission,
                    r.recording.iter().map(|p| [p.x, p.y, p.z]).collect(),
                ));
                r.route = Some(std::mem::take(&mut r.recording));
            }
        }
        r.cleared = cleared;
        // A restart (after a death) clears the flag again: a new attempt, same race.
        if !cleared
            && !kart
            && r.route.is_none()
            && let Some(p) = position
        {
            record(&mut r.recording, p);
        }

        // Our status, ~4 times a second while racing in a room.
        r.frame = r.frame.wrapping_add(1);
        if shared_boss && cleared && !just_cleared && r.frame.is_multiple_of(SEND_EVERY) {
            r.status_out = Some((level, mission, RACE_PROGRESS_MAX, r.finish, false));
        }
        if started.is_some()
            && connected
            && r.frame.is_multiple_of(SEND_EVERY)
            && r.finish.is_none()
        {
            let mut near = r.near;
            let progress = match (&r.route, position) {
                _ if kart => crate::kart::progress().map_or(RACE_PROGRESS_UNKNOWN, |p| {
                    (p * f32::from(RACE_PROGRESS_MAX)) as u16
                }),
                (Some(route), Some(p)) => {
                    (progress(route, p, &mut near) * f32::from(RACE_PROGRESS_MAX)) as u16
                }
                _ => RACE_PROGRESS_UNKNOWN,
            };
            r.near = near;
            r.mine = progress;
            r.status_out = Some((level, mission, progress, None, false));
        }

        // Finishes, and our position, in a Race room.
        let members = crate::room::members();
        r.others
            .retain(|_, (_, _, o)| o.at.elapsed() < STALE || o.finish.is_some());
        let here: Vec<(u8, Other)> = r
            .others
            .iter()
            .filter(|(id, (l, m, _))| members.contains(id) && (*l, *m) == (level, mission))
            .filter(|(id, _)| peers.iter().any(|p| p.id == **id && p.level == level))
            .map(|(id, (_, _, o))| (*id, *o))
            .collect();
        let race_room = crate::room::in_room() && crate::room::mode() == crate::room::Mode::Race;
        if started.is_none() || here.is_empty() {
            return;
        }
        // Rank: finished by race time, then by progress (unknown last).
        let key = |progress: u16, finish: Option<(u32, u32)>| match finish {
            Some((race, _)) => (0u8, race as i64),
            None if progress == RACE_PROGRESS_UNKNOWN => (2, 0),
            None => (1, -i64::from(progress)),
        };
        let mine = key(r.mine, r.finish);
        let mut ranks: Vec<((u8, i64), u8)> = here
            .iter()
            .map(|(id, o)| (key(o.progress, o.finish), *id))
            .collect();
        ranks.sort();
        for (id, o) in &here {
            if let Some((race, game_cs)) = o.finish
                && let Some((_, _, other)) = r.others.get_mut(id)
                && !other.announced
            {
                other.announced = true;
                let place = 1
                    + ranks
                        .iter()
                        .filter(|(k, _)| k.0 == 0 && k.1 < i64::from(race))
                        .count()
                    + usize::from(r.finish.is_some_and(|(mine, _)| mine < race));
                let name = crate::room::name_of(*id);
                hud::toast(format!(
                    "{name} finished {}: {} (game clock {})",
                    ordinal(place),
                    crate::places::time(race),
                    crate::places::time(game_cs)
                ));
            }
        }
        if race_room {
            let ahead = ranks.iter().filter(|(k, _)| *k < mine).count();
            let total = here.len() + 1;
            // Only the live position; finishes and the first run's note come as toasts.
            // A hunt has no way through (positions mean nothing; the hunt shows its score).
            let hunt = crate::places::is_hunt_stage(level);
            if r.finish.is_none() && (r.route.is_some() || kart) && !hunt {
                hud::corner(&format!("{} / {total}", ordinal(ahead + 1)));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32) -> Vector {
        Vector { x, y: 0.0, z: 0.0 }
    }

    #[test]
    fn routes_record_and_measure() {
        let mut route = Vec::new();
        for x in 0..=20 {
            record(&mut route, v(x as f32 * 30.0));
        }
        // A death sends us back to x = 120: the loop beyond it is cut.
        let len = route.len();
        record(&mut route, v(130.0));
        assert!(route.len() < len);
        for x in 5..=20 {
            record(&mut route, v(x as f32 * 30.0));
        }
        let mut near = 0;
        assert_eq!(progress(&route, v(0.0), &mut near), 0.0);
        let half = progress(&route, v(300.0), &mut near);
        assert!((half - 0.5).abs() < 0.1, "{half}");
        assert!(progress(&route, v(600.0), &mut near) > 0.95);
    }

    #[test]
    fn ordinals() {
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(4), "4th");
        assert_eq!(ordinal(11), "11th");
    }
}
