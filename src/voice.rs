//! Skin voices: a skin's own clips instead of the character's voice (re/skins.md "Voices").
//!
//! A skin may have `sounds/<moment>/*.wav` (16-bit PCM). While it's on, the character's own
//! voice lines (sound ids 0x3000..0x3030, which 0x4372E0 plays) are dropped, and the skin
//! speaks instead: for the moments the game voices (a homing attack, ...) and some it doesn't
//! (jumping, getting hurt, the garden actions, standing about).
//!
//! It's meant not to get annoying: quick grunts often, longer lines rarely, never the same
//! clip twice in a row, nothing cutting into a line already playing (except hurt and death),
//! and the idle chatter and random lines only now and then.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::game;
use crate::log;

/// Moments a skin can have clips for (its `sounds/` folder names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    Jump,
    Attack,
    Boost,
    Hurt,
    Death,
    FallDeath,
    Pickup,
    Throw,
    PutDown,
    Pet,
    Whistle,
    ShakeTree,
    Happy,
    Idle,
    Kachow,
    /// The game's other voice lines: a short jump, attack or hurt clip (no folder of its own).
    Grunt,
}

const MOMENTS: [(Moment, &str); 15] = [
    (Moment::Jump, "jump"),
    (Moment::Attack, "attack"),
    (Moment::Boost, "boost"),
    (Moment::Hurt, "hurt"),
    (Moment::Death, "death"),
    (Moment::FallDeath, "fall_death"),
    (Moment::Pickup, "pickup"),
    (Moment::Throw, "throw"),
    (Moment::PutDown, "put_down"),
    (Moment::Pet, "pet"),
    (Moment::Whistle, "whistle"),
    (Moment::ShakeTree, "shake_tree"),
    (Moment::Happy, "happy"),
    (Moment::Idle, "idle"),
    (Moment::Kachow, "kachow"),
];

impl Moment {
    fn index(self) -> usize {
        MOMENTS
            .iter()
            .position(|&(m, _)| m == self)
            .unwrap_or(MOMENTS.len())
    }

    /// Higher interrupts lower; random lines (0) only start in silence.
    fn priority(self) -> u8 {
        match self {
            Moment::Death | Moment::FallDeath => 3,
            Moment::Hurt => 2,
            Moment::Idle | Moment::Happy | Moment::Kachow => 0,
            _ => 1,
        }
    }

    /// Least time between two of these.
    fn cooldown(self) -> Duration {
        Duration::from_millis(match self {
            Moment::Jump => 2500,
            Moment::Attack | Moment::Boost | Moment::Grunt => 1500,
            Moment::Hurt => 1000,
            Moment::Death | Moment::FallDeath => 3000,
            Moment::ShakeTree => 8000,
            Moment::Happy => 8000,
            Moment::Idle => 60_000,
            Moment::Kachow => 25_000,
            _ => 3000,
        })
    }

    /// Where clips come from when the skin has none of its own (in order).
    fn fallbacks(self) -> &'static [Moment] {
        match self {
            Moment::Boost => &[Moment::Attack],
            Moment::FallDeath => &[Moment::Death],
            Moment::PutDown => &[Moment::Throw],
            _ => &[],
        }
    }
}

/// How often a jump gets a voice (1 in N), so it doesn't on every hop.
const JUMP_ONE_IN: u32 = 3;
/// Random lines wait this long after any line.
const QUIET_BEFORE_RANDOM: Duration = Duration::from_secs(10);
/// Standing still this long starts the idle chatter.
const IDLE_AFTER: Duration = Duration::from_secs(20);
/// Ground speed (units a frame) counted as standing still, and as going fast.
const STILL: f32 = 0.05;
const FAST: f32 = 4.0;
/// A grunt (for the game's other voice lines) is a jump, attack or hurt clip at most this long.
const GRUNT_MAX: Duration = Duration::from_millis(1000);
/// Clips' loudness here (they're normalised when made; the game's sounds are a little quieter).
const VOLUME: f32 = 0.8;
const MAX_CLIP_BYTES: usize = 4 << 20;
const MAX_CLIPS: usize = 32;

/// Player 1's rings and lives (i16 each; AddRings 0x44CE10 and 0x44CB10 write them).
const RINGS: usize = 0x0174_B028;
const LIVES: usize = 0x0174_B024;

/// The game's sound-at-a-position call (EDI = sound id, ESI = position) and its first bytes.
const PLAY_SOUND: usize = 0x0043_72E0;
const PLAY_SOUND_START: [u8; 6] = [0x51, 0x53, 0x8A, 0x5C, 0x24, 0x14];
/// The numbered voice call (voice id pushed, EDX = 2; returns -1 when it doesn't play). Its
/// start is already patched by the mod loader, so the garden's calls to it are redirected
/// instead: the pet line (ChaonadeExec 0x476FD0 at frame 30, by character, `jmp`) and the
/// tree-shake line (`call`), each from its own spot so no other voice is touched.
const PLAY_VOICE: usize = 0x0044_3130;
const GARDEN_VOICE_JUMPS: [usize; 6] = [
    0x0047_705B, // pet: Sonic 0x5ED
    0x0047_7071, // Shadow 0x5EC
    0x0047_7087, // Tails 0x6C1
    0x0047_709D, // Eggman 0x6BE
    0x0047_70B3, // Knuckles 0x4DA
    0x0047_70C9, // Rouge 0x4DB
];
const GARDEN_VOICE_CALLS: [usize; 4] = [
    0x0047_553D, // grabbing a tree (0x4754F0): 0x6C0
    0x0047_5557, // 0x6C3
    0x0073_DAB1, // starting to shake it: 0x6C0
    0x0074_EEB3, // 0x6C3
];
/// The character voice bank's ids (Sonic 0x3000.., Shadow 0x3012.., Amy and Metal 0x3021..).
const VOICE_IDS: (u32, u32) = (0x3000, 0x3030);

/// Set while a voiced skin is on: the hook drops character voices and leaves the id here.
static MUTE: AtomicU8 = AtomicU8::new(0);
static PENDING: AtomicU32 = AtomicU32::new(0);

struct Clip {
    wav: Box<[u8]>,
    length: Duration,
}

struct Voices {
    dir: PathBuf,
    clips: Vec<Vec<Clip>>,
    /// Short jump/attack/hurt clips, for the game's other voice lines.
    grunts: Vec<(usize, usize)>,
}

impl Voices {
    fn has(&self, m: Moment) -> bool {
        self.clips.get(m.index()).is_some_and(|c| !c.is_empty())
    }
}

#[derive(Default)]
struct Watch {
    position: Option<[f32; 3]>,
    grounded: bool,
    action: u8,
    rings: i16,
    lives: i16,
    falling: u32,
    still_since: Option<Instant>,
}

struct State {
    voices: Option<Voices>,
    /// The clip playing (kept alive while it plays) and when it ends, at what priority.
    playing: Option<(Box<[u8]>, Instant, u8)>,
    last: Vec<Option<Instant>>,
    last_clip: Option<(usize, usize)>,
    last_line: Option<Instant>,
    watch: Watch,
    rng: u32,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Puts the voice hook in (once, at start).
pub fn install() {
    #[cfg(target_arch = "x86")]
    {
        let thunk = voice_thunk as *const () as usize;
        let rel = thunk.wrapping_sub(PLAY_SOUND + 5) as u32;
        let mut jump = [0x90u8; 6];
        jump[0] = 0xE9;
        jump[1..5].copy_from_slice(&rel.to_le_bytes());
        if !crate::patch::replace_code(PLAY_SOUND, &PLAY_SOUND_START, &jump) {
            log!("voice: couldn't hook the sound call; skins keep the character's voice");
        }
        // The garden's pet and tree lines: each `jmp` / `call` to the voice call goes through
        // ours instead (only where it still points at the voice call).
        let garden = garden_voice_thunk as *const () as usize;
        let sites = GARDEN_VOICE_JUMPS
            .iter()
            .map(|&a| (a, 0xE9u8))
            .chain(GARDEN_VOICE_CALLS.iter().map(|&a| (a, 0xE8u8)));
        for (site, op) in sites {
            let to = |target: usize| {
                let mut b = [op, 0, 0, 0, 0];
                b[1..].copy_from_slice(&(target.wrapping_sub(site + 5) as u32).to_le_bytes());
                b
            };
            if !crate::patch::replace_code(site, &to(PLAY_VOICE), &to(garden)) {
                log!("voice: garden voice at {site:#x} not redirected");
            }
        }
    }
}

/// The garden's pet and tree lines: dropped while a voiced skin is on (the skin has its own
/// for those moments), else the voice call as before. Entered by `call` or a tail `jmp`, so
/// the stack is the same either way: a return address, then the voice id.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn garden_voice_thunk() {
    std::arch::naked_asm!(
        "cmp byte ptr [{mute}], 0",
        "je 2f",
        // What the voice call returns when it doesn't play (EDX is left as the caller set it).
        "or eax, -1",
        "ret",
        "2:",
        "push {voice}",
        "ret",
        mute = sym MUTE,
        voice = const PLAY_VOICE,
    )
}

/// The sound call's new start: a character voice while a voiced skin is on is dropped (and
/// noted for the skin to answer); anything else plays as before.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn voice_thunk() {
    std::arch::naked_asm!(
        "cmp byte ptr [{mute}], 0",
        "je 2f",
        "cmp edi, {first}",
        "jb 2f",
        "cmp edi, {end}",
        "jae 2f",
        "mov dword ptr [{pending}], edi",
        // What the call returns when it couldn't play.
        "or al, 0xff",
        "ret",
        "2:",
        "push ecx",
        "push ebx",
        "mov bl, byte ptr [esp + 0x14]",
        "push {resume}",
        "ret",
        mute = sym MUTE,
        pending = sym PENDING,
        first = const VOICE_IDS.0,
        end = const VOICE_IDS.1,
        resume = const PLAY_SOUND + 6,
    )
}

/// Every frame (game thread): the skin whose voice we use now (its folder), if any.
pub fn frame(skin: Option<&Path>) {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let s = guard.get_or_insert_with(|| State {
        voices: None,
        playing: None,
        last: vec![None; MOMENTS.len() + 1],
        last_clip: None,
        last_line: None,
        watch: Watch::default(),
        rng: 0x9E37_79B9 ^ std::process::id(),
    });
    let wanted = skin.map(|d| d.join("sounds"));
    if s.voices.as_ref().map(|v| &v.dir) != wanted.as_ref() {
        s.voices = wanted.as_deref().and_then(load);
        s.watch = Watch::default();
    }
    let voiced = s.voices.is_some();
    MUTE.store(voiced as u8, Ordering::Relaxed);
    let pending = PENDING.swap(0, Ordering::Relaxed);
    if !voiced {
        return;
    }
    let Some(me) = game::local_player() else {
        return;
    };
    for m in events(s, &me, pending) {
        play(s, m);
    }
}

/// What happened this frame that the skin answers.
fn events(s: &mut State, me: &game::LocalPlayer, pending: u32) -> Vec<Moment> {
    let mut out = Vec::new();
    let now = Instant::now();
    let at = [me.position.x, me.position.y, me.position.z];
    // SAFETY: the game's own counters, plain i16s in its data section.
    let (rings, lives) = unsafe {
        (
            (RINGS as *const i16).read_volatile(),
            (LIVES as *const i16).read_volatile(),
        )
    };
    let w = &mut s.watch;
    let first = w.position.is_none();
    let (speed, rise) = match w.position {
        Some(l) => {
            let (dx, dy, dz) = (at[0] - l[0], at[1] - l[1], at[2] - l[2]);
            let d = (dx * dx + dz * dz).sqrt();
            if d.is_finite() && d < 60.0 {
                (d, dy)
            } else {
                (0.0, 0.0)
            }
        }
        None => (0.0, 0.0),
    };
    if !first {
        // The game's own voice lines, answered in the skin's voice.
        match pending {
            0 => {}
            // Homing attack (Sonic, Shadow).
            0x3009 | 0x301B | 0x3025 => out.push(Moment::Attack),
            // Light dash.
            0x3008 | 0x301A | 0x3024 => out.push(Moment::Boost),
            // Picking up, putting down, throwing: answered from the action below.
            0x3004 | 0x3005 | 0x3016 | 0x3017 | 0x3023 => {}
            _ => out.push(Moment::Grunt),
        }
        if w.grounded && !me.grounded && rise > 0.3 {
            out.push(Moment::Jump);
        }
        let in_stage = game::current_level() > 0 && !game::in_garden();
        if in_stage && lives < w.lives {
            out.push(if w.falling > 60 {
                Moment::FallDeath
            } else {
                Moment::Death
            });
        } else if in_stage && rings < w.rings {
            out.push(Moment::Hurt);
        }
        if me.action != w.action {
            match me.action {
                19 | 41 | 49 => out.push(Moment::Pickup),
                26 | 33 => out.push(Moment::Throw),
                20 => out.push(Moment::PutDown),
                43 => out.push(Moment::Pet),
                34 => out.push(Moment::ShakeTree),
                42 => out.push(Moment::Whistle),
                _ => {}
            }
        }
        // Standing about: now and then a line (idle, happy, or for McQueen a Kachow).
        if speed < STILL && me.grounded && me.action == 0 {
            let since = *w.still_since.get_or_insert(now);
            if now.duration_since(since) > IDLE_AFTER {
                out.push(Moment::Idle);
            }
        } else {
            w.still_since = None;
        }
        // Going fast: the odd Kachow.
        if speed > FAST && me.grounded {
            out.push(Moment::Kachow);
        }
    }
    w.falling = if !me.grounded && rise < -0.5 {
        w.falling + 1
    } else {
        0
    };
    w.position = Some(at);
    w.grounded = me.grounded;
    w.action = me.action;
    w.rings = rings;
    w.lives = lives;
    out
}

/// Plays a clip for `m` if the rules allow it now.
fn play(s: &mut State, m: Moment) {
    let now = Instant::now();
    let Some(v) = &s.voices else {
        return;
    };
    // Which clips: its own, else a fallback's.
    let pool = std::iter::once(m)
        .chain(m.fallbacks().iter().copied())
        .find(|&c| c != Moment::Grunt && v.has(c));
    if pool.is_none() && (m != Moment::Grunt || v.grunts.is_empty()) {
        return;
    }
    let i = m.index();
    if s.last[i].is_some_and(|t| now.duration_since(t) < m.cooldown()) {
        return;
    }
    let busy = s.playing.as_ref().filter(|(_, end, _)| *end > now);
    if let Some((_, _, p)) = busy
        && m.priority() <= *p
    {
        return;
    }
    if m.priority() == 0
        && s.last_line
            .is_some_and(|t| now.duration_since(t) < QUIET_BEFORE_RANDOM)
    {
        return;
    }
    // Chance: jumps 1 in 3; random lines rarely (they're checked every frame).
    let roll = next(&mut s.rng);
    match m {
        Moment::Jump if !roll.is_multiple_of(JUMP_ONE_IN) => return,
        Moment::Kachow if !roll.is_multiple_of(900) => return,
        Moment::Idle | Moment::Happy if !roll.is_multiple_of(240) => return,
        _ => {}
    }
    // Idle chatter mixes in the happy lines and Kachows.
    let mut choices: Vec<(usize, usize)> = Vec::new();
    let add = |choices: &mut Vec<(usize, usize)>, c: Moment| {
        let k = c.index();
        choices.extend((0..v.clips[k].len()).map(|n| (k, n)));
    };
    match pool {
        Some(c) => add(&mut choices, c),
        None => choices.extend(v.grunts.iter().copied()),
    }
    if m == Moment::Idle {
        add(&mut choices, Moment::Happy);
        add(&mut choices, Moment::Kachow);
    }
    if choices.is_empty() {
        return;
    }
    if choices.len() > 1 {
        choices.retain(|&c| Some(c) != s.last_clip);
    }
    let (k, n) = choices[next(&mut s.rng) as usize % choices.len()];
    let clip = &v.clips[k][n];
    let wav = clip.wav.clone();
    let end = now + clip.length;
    start(&wav);
    s.playing = Some((wav, end, m.priority()));
    s.last[i] = Some(now);
    s.last_clip = Some((k, n));
    s.last_line = Some(now);
}

fn next(rng: &mut u32) -> u32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    *rng
}

/// Loads a skin's `sounds/` (none, or nothing usable: `None`).
fn load(dir: &Path) -> Option<Voices> {
    let mut clips: Vec<Vec<Clip>> = (0..MOMENTS.len()).map(|_| Vec::new()).collect();
    for (i, (_, name)) in MOMENTS.iter().enumerate() {
        let Ok(files) = std::fs::read_dir(dir.join(name)) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = files
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
            .collect();
        paths.sort();
        for p in paths.into_iter().take(MAX_CLIPS) {
            match std::fs::read(&p).map_err(|e| e.to_string()).and_then(clip) {
                Ok(c) => clips[i].push(c),
                Err(e) => log!("voice: {} refused: {e}", p.display()),
            }
        }
    }
    let total: usize = clips.iter().map(Vec::len).sum();
    if total == 0 {
        return None;
    }
    let grunts = [Moment::Jump, Moment::Attack, Moment::Hurt]
        .iter()
        .flat_map(|m| {
            let k = m.index();
            clips[k]
                .iter()
                .enumerate()
                .filter(|(_, c)| c.length <= GRUNT_MAX)
                .map(move |(n, _)| (k, n))
        })
        .collect::<Vec<_>>();
    log!("voice: {} clips from {}", total, dir.display());
    Some(Voices {
        dir: dir.to_path_buf(),
        clips,
        grunts,
    })
}

/// Checks a WAV (16-bit PCM, 1 or 2 channels) and turns it down to `VOLUME`.
fn clip(mut wav: Vec<u8>) -> Result<Clip, String> {
    if wav.len() > MAX_CLIP_BYTES
        || wav.len() < 44
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
    {
        return Err("not a WAV".into());
    }
    let u16_at = |b: &[u8], at: usize| u16::from_le_bytes([b[at], b[at + 1]]);
    let u32_at = |b: &[u8], at: usize| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while at + 8 <= wav.len() {
        let size = u32_at(&wav, at + 4) as usize;
        let body = at + 8;
        let end = body.checked_add(size).filter(|&e| e <= wav.len());
        match (&wav[at..at + 4], end) {
            (b"fmt ", Some(_)) if size >= 16 => {
                format = Some((
                    u16_at(&wav, body),
                    u16_at(&wav, body + 2),
                    u32_at(&wav, body + 4),
                    u16_at(&wav, body + 14),
                ))
            }
            (b"data", Some(e)) => data = Some((body, e)),
            _ => {}
        }
        at = body + size + (size & 1);
    }
    let (Some((1, channels @ 1..=2, rate @ 8000..=96_000, 16)), Some((from, to))) = (format, data)
    else {
        return Err("not 16-bit PCM".into());
    };
    for s in wav[from..to].as_chunks_mut::<2>().0 {
        *s = ((i16::from_le_bytes(*s) as f32 * VOLUME) as i16).to_le_bytes();
    }
    let samples = (to - from) / 2 / channels as usize;
    Ok(Clip {
        wav: wav.into_boxed_slice(),
        length: Duration::from_secs_f32(samples as f32 / rate as f32),
    })
}

/// Starts a WAV in memory playing (stopping the one before); it must stay alive meanwhile.
fn start(wav: &[u8]) {
    #[cfg(windows)]
    {
        #[link(name = "winmm")]
        unsafe extern "system" {
            fn PlaySoundA(sound: *const u8, module: usize, flags: u32) -> i32;
        }
        const SND_ASYNC: u32 = 0x1;
        const SND_NODEFAULT: u32 = 0x2;
        const SND_MEMORY: u32 = 0x4;
        // SAFETY: a checked WAV the caller keeps alive until it ends or the next one starts.
        unsafe { PlaySoundA(wav.as_ptr(), 0, SND_ASYNC | SND_NODEFAULT | SND_MEMORY) };
    }
    #[cfg(not(windows))]
    let _ = wav;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(bits: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&44100u32.to_le_bytes());
        w.extend_from_slice(&(44100u32 * 2).to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&bits.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        w
    }

    #[test]
    fn clips_are_checked_and_turned_down() {
        let c = clip(wav(16, &[10_000; 44_100])).unwrap();
        assert_eq!(c.length, Duration::from_secs(1));
        assert_eq!(i16::from_le_bytes([c.wav[44], c.wav[45]]), 8000);
        assert!(clip(wav(8, &[0; 100])).is_err());
        assert!(clip(b"RIFF....WAVE".to_vec()).is_err());
        // A data chunk claiming more than the file holds.
        let mut bad = wav(16, &[0; 10]);
        bad[40] = 0xFF;
        assert!(clip(bad).is_err());
    }

    #[test]
    fn every_moment_has_a_folder_but_grunts() {
        for (m, name) in MOMENTS {
            assert_eq!(MOMENTS[m.index()].1, name);
        }
        assert_eq!(Moment::Grunt.index(), MOMENTS.len());
        assert!(Moment::Death.priority() > Moment::Hurt.priority());
        assert_eq!(Moment::Idle.priority(), 0);
    }
}
