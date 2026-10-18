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
    /// The skin's catchphrase: on some jumps, now and then going fast, in idle chatter.
    Signature,
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
    (Moment::Signature, "signature"),
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
            Moment::Idle | Moment::Happy | Moment::Signature => 0,
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
            // The skin's own timing (`Tuning`) decides these.
            Moment::Idle | Moment::Signature => 0,
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

/// Ground speed (units a frame) counted as standing still.
const STILL: f32 = 0.05;
/// A grunt (for the game's other voice lines) is a jump, attack or hurt clip at most this long.
const GRUNT_MAX: Duration = Duration::from_millis(1000);
/// Jumps, attacks, hurts and boosts are quick sounds: longer clips (lines) never play for them.
const QUICK_MAX: Duration = GRUNT_MAX;

/// How chatty a skin is: `voice_*` lines in its skin.ini (docs/skins.md "Voice"), each defaulting to
/// what suits most skins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// 1 in this many jumps gets a voice (`voice_jump`; 0 = none).
    pub jump: u32,
    /// Of those, 1 in this many is a signature line instead (`voice_signature_jump`; 0 = none).
    pub signature_jump: u32,
    /// Going fast: about one signature line this often (`voice_signature_every`, seconds;
    /// 0 = none), and the speed that counts as fast (`voice_fast`, units a frame).
    pub signature_every: Duration,
    pub fast: f32,
    /// Standing still this long starts idle chatter (`voice_idle_after`), then at most one line
    /// this often (`voice_idle_every`).
    pub idle_after: Duration,
    pub idle_every: Duration,
    /// Random lines wait this long after any line (`voice_quiet`).
    pub quiet: Duration,
    /// The same clip doesn't play again this soon: a grunt, a line (`voice_grunt_rest`,
    /// `voice_line_rest`). A skin with few clips speaks less; one with plenty can talk more.
    pub rest: (Duration, Duration),
    /// Loudness of its clips, 0..1 (`voice_volume`).
    pub volume: f32,
    /// Least time between two lines of a moment (`voice_cooldown_<folder>`, seconds), where
    /// the skin sets one; else the moment's own.
    pub cooldowns: [Option<Duration>; MOMENTS.len()],
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            jump: 3,
            signature_jump: 4,
            signature_every: Duration::from_secs(25),
            fast: 4.0,
            idle_after: Duration::from_secs(20),
            idle_every: Duration::from_secs(60),
            quiet: Duration::from_secs(10),
            rest: (Duration::from_secs(4), Duration::from_secs(30)),
            volume: 0.8,
            cooldowns: [None; MOMENTS.len()],
        }
    }
}

impl Tuning {
    /// The `voice_*` lines of a skin.ini (others ignored; bad values keep the default).
    pub fn parse(ini: &str) -> Self {
        let mut t = Tuning::default();
        for line in ini.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let Ok(v) = v.trim().parse::<f32>() else {
                continue;
            };
            if !v.is_finite() || v < 0.0 {
                continue;
            }
            let secs = |v: f32| Duration::from_secs_f32(v.min(3600.0));
            let count = |v: f32| v.min(1000.0) as u32;
            match k.trim().to_ascii_lowercase().as_str() {
                "voice_jump" => t.jump = count(v),
                "voice_signature_jump" => t.signature_jump = count(v),
                "voice_signature_every" => t.signature_every = secs(v),
                "voice_fast" => t.fast = v.min(100.0),
                "voice_idle_after" => t.idle_after = secs(v),
                "voice_idle_every" => t.idle_every = secs(v),
                "voice_quiet" => t.quiet = secs(v),
                "voice_grunt_rest" => t.rest.0 = secs(v),
                "voice_line_rest" => t.rest.1 = secs(v),
                "voice_volume" => t.volume = v.min(1.0),
                key => {
                    if let Some(folder) = key.strip_prefix("voice_cooldown_")
                        && let Some(i) = MOMENTS.iter().position(|&(_, f)| f == folder)
                    {
                        t.cooldowns[i] = Some(secs(v));
                    }
                }
            }
        }
        t
    }
}
const MAX_CLIP_BYTES: usize = 4 << 20;
const MAX_CLIPS: usize = 32;

/// Player 1's rings and lives (i16 each; AddRings 0x44CE10 and 0x44CB10 write them).
const RINGS: usize = 0x0174_B028;
const LIVES: usize = 0x0174_B024;

/// The game's sound-at-a-position call (EDI = sound id, ESI = position) and its first bytes.
#[cfg(target_arch = "x86")]
const PLAY_SOUND: usize = 0x0043_72E0;
#[cfg(target_arch = "x86")]
const PLAY_SOUND_START: [u8; 6] = [0x51, 0x53, 0x8A, 0x5C, 0x24, 0x14];
/// The numbered voice call (voice id pushed, EDX = 2; returns -1 when it doesn't play). Its
/// start is already patched by the mod loader, so the garden's calls to it are redirected
/// instead: the pet line (ChaonadeExec 0x476FD0 at frame 30, by character, `jmp`) and the
/// tree-shake line (`call`), each from its own spot so no other voice is touched.
#[cfg(target_arch = "x86")]
const PLAY_VOICE: usize = 0x0044_3130;
#[cfg(target_arch = "x86")]
const GARDEN_VOICE_JUMPS: [usize; 6] = [
    0x0047_705B, // pet: Sonic 0x5ED
    0x0047_7071, // Shadow 0x5EC
    0x0047_7087, // Tails 0x6C1
    0x0047_709D, // Eggman 0x6BE
    0x0047_70B3, // Knuckles 0x4DA
    0x0047_70C9, // Rouge 0x4DB
];
#[cfg(target_arch = "x86")]
const GARDEN_VOICE_CALLS: [usize; 4] = [
    0x0047_553D, // grabbing a tree (0x4754F0): 0x6C0
    0x0047_5557, // 0x6C3
    0x0073_DAB1, // starting to shake it: 0x6C0
    0x0074_EEB3, // 0x6C3
];
/// The character voice bank's ids (Sonic 0x3000.., Shadow 0x3012.., Amy and Metal 0x3021..).
#[cfg(target_arch = "x86")]
const VOICE_IDS: (u32, u32) = (0x3000, 0x3030);

/// Where the player characters' own code is (their mains and actions, and the shared player
/// helpers: picking up, petting, ...): numbered voices called from here are theirs. Cutscenes,
/// Omochao and the story call from elsewhere and are left alone.
#[cfg(target_arch = "x86")]
const PLAYER_CODE: [(usize, usize); 2] = [(0x0045_0000, 0x0047_8000), (0x0071_0000, 0x0076_0000)];
/// Where the numbered voice call's first `jmp` (the mod loader's) went, to go on to it.
#[cfg(target_arch = "x86")]
static VOICE_NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Set while a voiced skin is on: the hook drops character voices and leaves the id here.
static MUTE: AtomicU8 = AtomicU8::new(0);

/// The player's skin voice volume (F9 menu, config.ini `SkinVoiceVolume`), percent, on top of each
/// skin's own `voice_volume`.
static VOLUME: AtomicU32 = AtomicU32::new(100);
pub const MAX_VOLUME: u32 = 150;

/// Skin voices at `percent` of their own loudness (up to `MAX_VOLUME`).
pub fn set_volume(percent: u32) {
    VOLUME.store(percent.min(MAX_VOLUME), Ordering::Relaxed);
}

/// `samples` (16-bit PCM) at `factor` of their loudness, held within range.
fn scale(samples: &mut [u8], factor: f32) {
    for s in samples.as_chunks_mut::<2>().0 {
        let v = f32::from(i16::from_le_bytes(*s)) * factor;
        *s = (v.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16).to_le_bytes();
    }
}
static PENDING: AtomicU32 = AtomicU32::new(0);

struct Clip {
    wav: Box<[u8]>,
    /// Where its samples are in `wav`.
    samples: std::ops::Range<usize>,
    length: Duration,
}

struct Voices {
    dir: PathBuf,
    tuning: Tuning,
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
    /// When each clip last played (moment folder, clip), for `Tuning::rest`.
    played: std::collections::HashMap<(usize, usize), Instant>,
    /// A signature line for a jump (not a random one while going fast).
    jump_signature: bool,
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
        // Every numbered voice the player's own code plays: through ours first, then on to
        // whatever the call's first `jmp` went to (the mod loader's hook).
        // SAFETY: the game's code at a fixed address, read only.
        let start: [u8; 5] = unsafe { std::ptr::read(PLAY_VOICE as *const [u8; 5]) };
        if start[0] == 0xE9 {
            let rel = i32::from_le_bytes([start[1], start[2], start[3], start[4]]);
            VOICE_NEXT.store(
                (PLAY_VOICE + 5).wrapping_add(rel as usize),
                Ordering::Relaxed,
            );
            let ours = numbered_voice_thunk as *const () as usize;
            let mut jump = [0xE9u8, 0, 0, 0, 0];
            jump[1..].copy_from_slice(&(ours.wrapping_sub(PLAY_VOICE + 5) as u32).to_le_bytes());
            if !crate::patch::replace_code(PLAY_VOICE, &start, &jump) {
                log!("voice: couldn't hook the numbered voices");
            }
        } else {
            log!("voice: the numbered voice call isn't as expected; character lines stay");
        }
    }
}

/// The numbered voice call's new start: while a voiced skin is on, a voice the player
/// characters' own code asks for (by where it's called from) is dropped; anything else goes
/// on as before.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn numbered_voice_thunk() {
    std::arch::naked_asm!(
        "cmp byte ptr [{mute}], 0",
        "je 3f",
        "push ecx",
        "mov ecx, dword ptr [esp + 4]",
        "cmp ecx, {a0}",
        "jb 2f",
        "cmp ecx, {a1}",
        "jb 1f",
        "cmp ecx, {b0}",
        "jb 2f",
        "cmp ecx, {b1}",
        "jae 2f",
        "1:",
        "pop ecx",
        // What the voice call returns when it doesn't play.
        "or eax, -1",
        "ret",
        "2:",
        "pop ecx",
        "3:",
        "jmp dword ptr [{next}]",
        mute = sym MUTE,
        next = sym VOICE_NEXT,
        a0 = const PLAYER_CODE[0].0,
        a1 = const PLAYER_CODE[0].1,
        b0 = const PLAYER_CODE[1].0,
        b1 = const PLAYER_CODE[1].1,
    )
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
        played: std::collections::HashMap::new(),
        jump_signature: false,
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
        // Some of the jumps that get a voice get the skin's signature line instead.
        let signature = m == Moment::Jump
            && s.voices.as_ref().is_some_and(|v| {
                let t = v.tuning;
                v.has(Moment::Signature)
                    && t.jump > 0
                    && t.signature_jump > 0
                    && next(&mut s.rng).is_multiple_of(t.jump * t.signature_jump)
            });
        if signature {
            s.jump_signature = true;
            play(s, Moment::Signature);
            s.jump_signature = false;
        } else {
            play(s, m);
        }
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
    let tuning = s.voices.as_ref().map(|v| v.tuning).unwrap_or_default();
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
        // Standing about: now and then a line (idle, happy or signature).
        if speed < STILL && me.grounded && me.action == 0 {
            let since = *w.still_since.get_or_insert(now);
            if now.duration_since(since) > tuning.idle_after {
                out.push(Moment::Idle);
            }
        } else {
            w.still_since = None;
        }
        // Going fast: now and then the signature line.
        if speed > tuning.fast && me.grounded {
            out.push(Moment::Signature);
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
    let t = v.tuning;
    let i = m.index();
    let cooldown = match t.cooldowns.get(i).copied().flatten() {
        Some(set) => set,
        None => match m {
            Moment::Idle => t.idle_every,
            Moment::Signature if !s.jump_signature => t.signature_every,
            _ => m.cooldown(),
        },
    };
    if s.last[i].is_some_and(|at| now.duration_since(at) < cooldown) {
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
            .is_some_and(|at| now.duration_since(at) < t.quiet)
    {
        return;
    }
    // Chance: 1 in `voice_jump` jumps; random lines a few seconds after they may (they're
    // checked every frame).
    let roll = next(&mut s.rng);
    match m {
        Moment::Jump if t.jump == 0 || !roll.is_multiple_of(t.jump) => return,
        Moment::Signature
            if !s.jump_signature && (t.signature_every.is_zero() || !roll.is_multiple_of(300)) =>
        {
            return;
        }
        Moment::Idle | Moment::Happy if !roll.is_multiple_of(240) => return,
        _ => {}
    }
    // Idle chatter mixes in the happy lines and the signature ones.
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
        add(&mut choices, Moment::Signature);
    }
    let quick = matches!(
        m,
        Moment::Jump | Moment::Attack | Moment::Boost | Moment::Hurt | Moment::Grunt
    );
    // Quick moments: quick sounds only. Every clip: rested since it last played.
    choices.retain(|&(k, n)| {
        let c = &v.clips[k][n];
        let rest = if c.length <= GRUNT_MAX {
            t.rest.0
        } else {
            t.rest.1
        };
        (!quick || c.length <= QUICK_MAX)
            && s.played
                .get(&(k, n))
                .is_none_or(|t| now.duration_since(*t) >= rest)
    });
    if choices.len() > 1 {
        choices.retain(|&c| Some(c) != s.last_clip);
    }
    if choices.is_empty() {
        return;
    }
    let (k, n) = choices[next(&mut s.rng) as usize % choices.len()];
    let clip = &v.clips[k][n];
    let mut wav = clip.wav.clone();
    let volume = VOLUME.load(Ordering::Relaxed);
    if volume != 100 {
        scale(&mut wav[clip.samples.clone()], volume as f32 / 100.0);
    }
    let end = now + clip.length;
    start(&wav);
    s.playing = Some((wav, end, m.priority()));
    s.last[i] = Some(now);
    s.last_clip = Some((k, n));
    s.played.insert((k, n), now);
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
    // How chatty: the skin's skin.ini (next to `sounds/`).
    let tuning = dir
        .parent()
        .and_then(|skin| std::fs::read_to_string(skin.join("skin.ini")).ok())
        .map_or_else(Tuning::default, |ini| Tuning::parse(&ini));
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
            match std::fs::read(&p)
                .map_err(|e| e.to_string())
                .and_then(|b| clip(b, tuning.volume))
            {
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
        tuning,
        clips,
        grunts,
    })
}

/// Checks a WAV (16-bit PCM, 1 or 2 channels) and sets its loudness (`volume`, 0..1).
fn clip(mut wav: Vec<u8>, volume: f32) -> Result<Clip, String> {
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
    scale(&mut wav[from..to], volume);
    let samples = (to - from) / 2 / channels as usize;
    Ok(Clip {
        wav: wav.into_boxed_slice(),
        samples: from..to,
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
        let c = clip(wav(16, &[10_000; 44_100]), 0.8).unwrap();
        assert_eq!(c.length, Duration::from_secs(1));
        assert_eq!(i16::from_le_bytes([c.wav[44], c.wav[45]]), 8000);
        assert!(clip(wav(8, &[0; 100]), 0.8).is_err());
        assert!(clip(b"RIFF....WAVE".to_vec(), 0.8).is_err());
        // A data chunk claiming more than the file holds.
        let mut bad = wav(16, &[0; 10]);
        bad[40] = 0xFF;
        assert!(clip(bad, 0.8).is_err());
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

    #[test]
    fn a_skin_ini_tunes_its_voice() {
        let t = Tuning::parse(
            "name=Any Skin\nvoice_jump=2\nvoice_signature_jump=0\nvoice_idle_after=5.5\n\
             voice_volume=3\nvoice_line_rest=-1\nvoice_fast=nan\n",
        );
        assert_eq!(t.jump, 2);
        assert_eq!(t.signature_jump, 0);
        assert_eq!(t.idle_after, Duration::from_secs_f32(5.5));
        assert_eq!(t.volume, 1.0);
        // Bad values keep the default.
        assert_eq!(t.rest.1, Tuning::default().rest.1);
        assert_eq!(t.fast, Tuning::default().fast);
        assert_eq!(Tuning::parse(""), Tuning::default());
        let c = Tuning::parse("voice_cooldown_hurt=2\nvoice_cooldown_nothing=1\n");
        assert_eq!(
            c.cooldowns[Moment::Hurt.index()],
            Some(Duration::from_secs(2))
        );
        assert_eq!(c.cooldowns.iter().flatten().count(), 1);
    }
}
