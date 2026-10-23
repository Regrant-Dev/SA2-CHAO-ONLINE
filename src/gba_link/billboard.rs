//! Partners' pictures in the garden (re/chao-animals.md "Pictures"): each picture's frames
//! become game textures through the game's own GVM parser (`0x42FC30`, RGB5A3), drawn on a
//! 4-vertex chunk-model quad (puppet::draw_solid) turned about Y to face the camera.

// Off the game (Linux builds) only the tests use the builders.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

/// The quad's size in world units, its feet at the partner's position.
pub const SIZE: f32 = 6.0;
/// Global texture indices: `BASE + slot * 2 + frame` (the game's own are far from here).
const GBIX_BASE: u32 = 0x7E5A_0000;
/// Pictures kept at once (a garden holds 4 partners, 6 wild visitors and the link machine's
/// screen).
const SLOTS: usize = 12;
/// Frames shown this long each (milliseconds).
const FRAME_TIME: u32 = 600;

/// One RGBA8 pixel as RGB5A3 (big-endian on disk): opaque `0x8000 | r5 g5 b5`, otherwise
/// `a3 r4 g4 b4`.
pub fn rgb5a3(p: [u8; 4]) -> u16 {
    let [r, g, b, a] = p.map(u16::from);
    if a >= 0xE0 {
        0x8000 | (r >> 3) << 10 | (g >> 3) << 5 | b >> 3
    } else {
        (a >> 5) << 12 | (r >> 4) << 8 | (g >> 4) << 4 | b >> 4
    }
}

/// Transparent pixels take a neighbour's colour (alpha kept at 0), so filtering doesn't darken
/// the outline.
pub fn bleed(rgba: &mut [u8], w: usize, h: usize) {
    for _ in 0..2 {
        let before = rgba.to_vec();
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                if before[o + 3] != 0 {
                    continue;
                }
                let near = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                    .into_iter()
                    .filter_map(|(dx, dy)| {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        ((0..w as i32).contains(&nx) && (0..h as i32).contains(&ny))
                            .then(|| (ny as usize * w + nx as usize) * 4)
                    })
                    .find(|&n| before[n + 3] != 0 || before[n..n + 3] != [0, 0, 0]);
                if let Some(n) = near {
                    rgba[o..o + 3].copy_from_slice(&before[n..n + 3]);
                }
            }
        }
    }
}

/// A GVM archive of `frames` (RGBA8, `w` x `h`, both multiples of 4) as RGB5A3 textures with
/// global indices `gbix + i`: a GVMH chunk (flags 0x08: global indices only, 6-byte entries,
/// big-endian) and one GVRT chunk per frame (format 5, 4x4 tiles of big-endian u16).
pub fn gvm(frames: &[Vec<u8>], w: usize, h: usize, gbix: u32) -> Vec<u8> {
    let mut out = b"GVMH".to_vec();
    out.extend(0x18u32.to_le_bytes());
    out.extend(8u16.to_be_bytes());
    out.extend((frames.len() as u16).to_be_bytes());
    for i in 0..frames.len() {
        out.extend((i as u16).to_be_bytes());
        out.extend((gbix + i as u32).to_be_bytes());
    }
    out.resize(0x20, 0);
    for frame in frames {
        let mut rgba = frame.clone();
        bleed(&mut rgba, w, h);
        out.extend(b"GVRT");
        out.extend((8 + (w * h * 2) as u32).to_le_bytes());
        out.extend([0, 0, 0, 5]);
        out.extend((w as u16).to_be_bytes());
        out.extend((h as u16).to_be_bytes());
        for ty in (0..h).step_by(4) {
            for tx in (0..w).step_by(4) {
                for y in ty..ty + 4 {
                    for x in tx..tx + 4 {
                        let o = (y * w + x) * 4;
                        let p = [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]];
                        out.extend(rgb5a3(p).to_be_bytes());
                    }
                }
            }
        }
    }
    out
}

/// The quad's vertex list (chunk 0x29: position and normal) and polygon list (white diffuse
/// material with source-alpha blend, texture 0 clamped, one 4-vertex UV strip lit by nothing
/// but its texture, with alpha), as u32 words: `size` wide and tall, feet at the origin,
/// facing +Z, `u` from 0 at the left.
pub fn quad(size: f32) -> (Vec<u32>, Vec<u32>) {
    let half = size / 2.0;
    let mut v = vec![0x0019_0029, 0x0004_0000];
    for (x, y) in [(-half, size), (half, size), (-half, 0.0), (half, 0.0)] {
        v.extend([x, y, 0.0, 0.0, 0.0, 1.0].map(f32::to_bits));
    }
    v.push(0xFF);
    let p16: [u16; 22] = [
        0x2511, 2, 0xFFFF, 0xFFFF, // material: diffuse, blend SA / 1-SA
        0x3008, 0, // texture 0, clamp U and V
        0x1941, 14, 1, 4, // strip (UV 0..255), flags light off + alpha, 1 strip of 4
        0, 0, 0, 2, 0, 255, 1, 255, 0, 3, 255, 255, // TL, BL, TR, BR
    ];
    let mut p16 = p16.to_vec();
    p16.push(0x00FF);
    if p16.len() % 2 == 1 {
        p16.push(0);
    }
    let p = p16
        .chunks(2)
        .map(|c| u32::from(c[0]) | u32::from(c[1]) << 16)
        .collect();
    (v, p)
}

/// The camera's position from a view matrix (3x4 row-major, translation in m[3], m[7], m[11]).
pub fn camera(m: &[f32; 12]) -> [f32; 3] {
    std::array::from_fn(|j| -(m[j] * m[3] + m[4 + j] * m[7] + m[8 + j] * m[11]))
}

/// The Y angle (BAMS) that turns +Z from `at` toward `camera`.
pub fn facing(at: [f32; 3], camera: [f32; 3]) -> i32 {
    let a = (camera[0] - at[0]).atan2(camera[2] - at[2]);
    (a * 65536.0 / std::f32::consts::TAU) as i32
}

/// Which frame of `count` shows `ms` milliseconds in.
pub fn frame_at(ms: u32, count: usize) -> usize {
    if count < 2 {
        0
    } else {
        (ms / FRAME_TIME) as usize % count
    }
}

#[cfg(target_arch = "x86")]
pub use game::{Textures, draw_panel, draw_picture, forget};

/// Draws the picture of `species` (in its usual colours) at the animal position `at`.
pub fn draw(species: u16, at: [f32; 3], held: bool) {
    draw_picture(species, false, at, held);
}

#[cfg(not(target_arch = "x86"))]
pub fn draw_picture(_species: u16, _shiny: bool, _at: [f32; 3], _held: bool) {}

#[cfg(not(target_arch = "x86"))]
pub fn draw_panel(_species: u16, _feet: [f32; 3], _yaw: i32, _height: f32) {}

#[cfg(not(target_arch = "x86"))]
pub fn forget() {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;

    use super::{GBIX_BASE, SIZE, SLOTS, camera, facing, frame_at, gvm, quad};
    use crate::log;

    /// The GVM parser (EAX = archive, EBX = NJS_TEXLIST; 1 when done).
    const PARSE_GVM: usize = 0x0042_FC30;
    /// Releases a texture list's textures (cdecl NJS_TEXLIST*).
    const RELEASE: usize = 0x0077_F9F0;
    /// Texture records (0x18 bytes; free when +0xC is 0) and infos (0x28 bytes; free when +0 is
    /// -1): base and count pointers.
    const RECORDS: usize = 0x0267_0590;
    const RECORD_COUNT: usize = 0x0267_058C;
    const INFOS: usize = 0x025F_0278;
    const INFO_COUNT: usize = 0x025E_FFE0;
    /// Info +4: the texture's format. 0xE (as for CMPR) has alpha strips drawn with alpha test
    /// and depth writes instead of blending without them (0x42CA20).
    const INFO_FORMAT: usize = 4;
    const ALPHA_TEST_FORMAT: u32 = 0xE;
    /// The game's matrix stack top (the view matrix in a display).
    const MATRIX_TOP: usize = 0x01A5_57FC;
    /// Lower than the animal's position, as its own display draws it (more when carried).
    const DROP: f32 = 1.0;
    const DROP_CARRIED: f32 = 1.2;
    const TEXNAME: usize = 12;

    /// Game textures made from RGBA8 frames, as one texture list. They belong to the scene they
    /// were made in.
    pub struct Textures {
        /// NJS_TEXNAME entries (name, attr, texaddr) for every frame.
        names: Box<[u32]>,
        /// The NJS_TEXLIST the parser fills (all frames).
        all: Box<[u32; 2]>,
        count: usize,
        /// The scene they were made in and their records then.
        scene: Scene,
        signature: Vec<[usize; 4]>,
    }

    // SAFETY: the raw words are only used on the game thread.
    unsafe impl Send for Textures {}

    impl Textures {
        /// Makes `frames` (RGBA8, `w` x `h`, multiples of 4) into textures with global indices
        /// `gbix + i`; `alpha_test`: their alpha strips draw with alpha test and depth writes.
        /// Game thread.
        pub fn make(
            frames: &[Vec<u8>],
            w: usize,
            h: usize,
            gbix: u32,
            alpha_test: bool,
        ) -> Option<Self> {
            let count = frames.len();
            if count == 0 || !room(count + 4) {
                return None;
            }
            let archive = gvm(frames, w, h, gbix);
            let mut names = vec![0u32; count * 3].into_boxed_slice();
            let all = Box::new([names.as_mut_ptr() as u32, count as u32]);
            let result: i32;
            // SAFETY: a checked archive we built (the parser byte-swaps its header in place
            // and keeps no pointer into it), a list with an entry per texture, on the game
            // thread.
            unsafe {
                std::arch::asm!(
                    "push ebx",
                    "mov ebx, {list}",
                    "call {f}",
                    "pop ebx",
                    list = in(reg) all.as_ptr(),
                    f = in(reg) PARSE_GVM,
                    inlateout("eax") archive.as_ptr() => result,
                    clobber_abi("C"),
                );
            }
            let made = (0..count).all(|i| names[i * 3 + 2] != 0);
            if result != 1 || !made {
                return None;
            }
            if alpha_test {
                for i in 0..count {
                    let record = names[i * 3 + 2] as usize;
                    // SAFETY: our texture's record and info, just made.
                    unsafe {
                        let info = ((record + 0xC) as *const usize).read();
                        if info != 0 {
                            ((info + INFO_FORMAT) as *mut u32).write(ALPHA_TEST_FORMAT);
                        }
                    }
                }
            }
            let signature = signature(&names, count);
            Some(Self {
                names,
                all,
                count,
                scene: scene(),
                signature,
            })
        }

        /// Still ours: made in this scene, and the game hasn't reused the records.
        pub fn ours(&self) -> bool {
            self.scene == scene()
                && self.signature.iter().all(|s| s[0] != 0 && s[3] != 0)
                && signature(&self.names, self.count) == self.signature
        }

        /// Releases them if they're still ours (else the game freed them with their scene).
        pub fn release(&self) {
            if self.ours() {
                // SAFETY: our list; its records are still ours in this scene.
                unsafe {
                    std::mem::transmute::<usize, extern "C" fn(usize)>(RELEASE)(
                        self.all.as_ptr() as usize
                    )
                };
            }
        }

        /// The NJS_TEXLIST of every frame.
        pub fn list(&self) -> usize {
            self.all.as_ptr() as usize
        }

        /// The NJS_TEXNAME entry of frame `i`.
        fn entry(&self, i: usize) -> u32 {
            self.names.as_ptr() as u32 + (i.min(self.count - 1) * TEXNAME) as u32
        }
    }

    /// One picture's textures.
    struct Picture {
        species: u16,
        shiny: bool,
        slot: usize,
        textures: Textures,
        /// The list drawn (one frame).
        shown: Box<[u32; 2]>,
        /// When it was last drawn: the one drawn longest ago makes room for a new one.
        used: std::time::Instant,
    }

    /// The quad model: NJS_OBJECT, NJS_CNK_MODEL and the two chunk lists.
    struct Model {
        object: Box<[u32; 13]>,
        _model: Box<[u32; 6]>,
        _vlist: Box<[u32]>,
        _plist: Box<[u32]>,
    }

    struct Pictures {
        list: Vec<Picture>,
        model: Option<Model>,
        /// Species (and shiny) that failed (not tried again this scene).
        failed: Vec<((u16, bool), Scene)>,
    }

    // SAFETY: the raw words are only used on the game thread.
    unsafe impl Send for Pictures {}

    static PICTURES: Mutex<Pictures> = Mutex::new(Pictures {
        list: Vec::new(),
        model: None,
        failed: Vec::new(),
    });
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

    /// The level, its run and the Chao area: textures belong to one scene.
    type Scene = (i16, u32, i32);

    fn scene() -> Scene {
        (
            crate::game::current_level(),
            crate::game::stage_run(),
            crate::game::current_chao_area(),
        )
    }

    fn model() -> Model {
        let (vlist, plist) = quad(SIZE);
        let vlist = vlist.into_boxed_slice();
        let plist = plist.into_boxed_slice();
        let half = (SIZE / 2.0).to_bits();
        let model = Box::new([
            vlist.as_ptr() as u32,
            plist.as_ptr() as u32,
            0,
            half,
            0,
            0, // radius 0: never culled by its sphere
        ]);
        let one = 1f32.to_bits();
        let mut object = Box::new([0u32; 13]);
        // Unit position, angle and scale, and no children (0x10).
        object[0] = 0x17;
        object[1] = model.as_ptr() as u32;
        object[8] = one;
        object[9] = one;
        object[10] = one;
        Model {
            object,
            _model: model,
            _vlist: vlist,
            _plist: plist,
        }
    }

    /// Records (+0, +8, +0xC) of a list's entries.
    fn signature(names: &[u32], count: usize) -> Vec<[usize; 4]> {
        (0..count)
            .map(|i| {
                let record = names[i * 3 + 2] as usize;
                if record == 0 {
                    return [0; 4];
                }
                // SAFETY: a texture record the game handed us (its pool is static).
                unsafe {
                    [
                        record,
                        (record as *const usize).read(),
                        ((record + 8) as *const usize).read(),
                        ((record + 0xC) as *const usize).read(),
                    ]
                }
            })
            .collect()
    }

    /// Free texture records and infos, at least.
    fn room(n: usize) -> bool {
        // SAFETY: the game's texture pools (base pointers and counts, statics).
        unsafe {
            let free = |base: usize, count: usize, stride: usize, at: usize, free: usize| {
                let base = (base as *const usize).read();
                let count = (count as *const u32).read() as usize;
                if base == 0 {
                    return 0;
                }
                (0..count.min(0x2000))
                    .filter(|i| ((base + i * stride + at) as *const usize).read() == free)
                    .count()
            };
            free(RECORDS, RECORD_COUNT, 0x18, 0xC, 0) >= n
                && free(INFOS, INFO_COUNT, 0x28, 0, usize::MAX) >= n
        }
    }

    /// Makes the picture of `species` (in its shiny colours when `shiny`) in `slot`.
    fn make(species: u16, shiny: bool, slot: usize) -> Option<Picture> {
        let rom = super::super::loaded_rom()?;
        let sprite = if shiny {
            rom.shiny_front_sprite(species)?
        } else {
            rom.front_sprite(species)?
        };
        let frames = sprite.frames.len().min(2);
        let gbix = GBIX_BASE + (slot * 2) as u32;
        let Some(textures) = Textures::make(
            &sprite.frames[..frames],
            sprite.width as usize,
            sprite.height as usize,
            gbix,
            true,
        ) else {
            log!("gba link: textures for species {species} weren't made");
            return None;
        };
        let shown = Box::new([textures.entry(0), 1]);
        Some(Picture {
            species,
            shiny,
            slot,
            textures,
            shown,
            used: std::time::Instant::now(),
        })
    }

    /// The garden is going: our textures go (or went with it).
    pub fn forget() {
        let mut s = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
        for p in s.list.drain(..) {
            p.textures.release();
        }
        s.failed.clear();
    }

    /// Draws the picture of `species` (`shiny`: in its shiny colours) at the animal position
    /// `at` (`held`: carried by the player), turned to the camera. Display pass, game thread.
    pub fn draw_picture(species: u16, shiny: bool, at: [f32; 3], held: bool) {
        // SAFETY: the display pass; the matrix stack's top is the view matrix.
        let view = unsafe {
            let top = (MATRIX_TOP as *const usize).read();
            if top == 0 {
                return;
            }
            (top as *const [f32; 12]).read()
        };
        let cam = camera(&view);
        let feet = [at[0], at[1] - if held { DROP_CARRIED } else { DROP }, at[2]];
        draw_at(species, shiny, feet, facing(feet, cam), 1.0);
    }

    /// Draws the picture of `species` standing at `feet`, `height` tall, its front turned to
    /// `yaw` (BAMS) whatever the camera does (a screen). Display pass, game thread.
    pub fn draw_panel(species: u16, feet: [f32; 3], yaw: i32, height: f32) {
        draw_at(species, false, feet, yaw, height / SIZE);
    }

    fn draw_at(species: u16, shiny: bool, feet: [f32; 3], yaw: i32, scale: f32) {
        let mut s = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
        let now = scene();
        let key = (species, shiny);
        let this = |p: &Picture| (p.species, p.shiny) == key;
        // Gone with an old scene: dropped without releasing (the game freed them).
        s.list.retain(|p| p.textures.ours());
        s.failed.retain(|(_, sc)| *sc == now);
        if s.failed.iter().any(|(k, _)| *k == key) {
            return;
        }
        if !s.list.iter().any(this) {
            if s.list.len() >= SLOTS {
                let oldest = (0..s.list.len())
                    .min_by_key(|&i| s.list[i].used)
                    .unwrap_or(0);
                let old = s.list.remove(oldest);
                old.textures.release();
            }
            let used: Vec<usize> = s.list.iter().map(|p| p.slot).collect();
            let Some(slot) = (0..SLOTS).find(|i| !used.contains(i)) else {
                return;
            };
            match make(species, shiny, slot) {
                Some(p) => s.list.push(p),
                None => {
                    s.failed.push((key, now));
                    return;
                }
            }
        }
        if s.model.is_none() {
            s.model = Some(model());
        }
        let ms = START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis() as u32;
        let Some(object) = s.model.as_ref().map(|m| m.object.as_ptr() as usize) else {
            return;
        };
        let Some(p) = s.list.iter_mut().find(|p| this(p)) else {
            return;
        };
        let frame = frame_at(ms, p.textures.count);
        p.shown[0] = p.textures.entry(frame);
        p.used = std::time::Instant::now();
        // SAFETY: display pass on the game thread; our checked quad and texture list.
        unsafe {
            crate::puppet::draw_solid(
                object,
                p.shown.as_ptr() as usize,
                feet,
                [0, 0x8000i32.wrapping_sub(yaw), 0],
                scale,
                crate::puppet::Tilt::default(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb5a3_opaque_and_clear() {
        assert_eq!(rgb5a3([255, 255, 255, 255]), 0xFFFF);
        assert_eq!(rgb5a3([255, 0, 0, 255]), 0x8000 | 31 << 10);
        assert_eq!(rgb5a3([0, 0, 0, 0]), 0);
        assert_eq!(rgb5a3([255, 255, 255, 0]), 0x0FFF);
        assert_eq!(rgb5a3([0, 0, 255, 128]), 4 << 12 | 15);
    }

    #[test]
    fn gvm_layout() {
        let frame = vec![0u8; 64 * 64 * 4];
        let mut second = frame.clone();
        // Pixel (5, 1) opaque red: tile 1, row 1, column 1.
        let o = (64 + 5) * 4;
        second[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
        let g = gvm(&[frame, second], 64, 64, 0x7E5A_0002);
        assert_eq!(g.len(), 0x20 + 2 * (0x10 + 64 * 64 * 2));
        assert_eq!(&g[..4], b"GVMH");
        assert_eq!(g[8..12], [0, 8, 0, 2]);
        assert_eq!(g[0x0E..0x12], 0x7E5A_0002u32.to_be_bytes());
        assert_eq!(g[0x14..0x18], 0x7E5A_0003u32.to_be_bytes());
        let second_at = 0x20 + 0x10 + 0x2000;
        assert_eq!(&g[second_at..second_at + 4], b"GVRT");
        assert_eq!(g[second_at + 0xB], 5);
        assert_eq!(g[second_at + 0xC..second_at + 0x10], [0, 64, 0, 64]);
        let pixels = second_at + 0x10;
        let at = pixels + (16 + 4 + 1) * 2;
        assert_eq!(u16::from_be_bytes([g[at], g[at + 1]]), 0x8000 | 31 << 10);
        // Its clear neighbour took its colour, alpha 0.
        let left = pixels + (16 + 4) * 2;
        assert_eq!(u16::from_be_bytes([g[left], g[left + 1]]), 0x0F00);
    }

    #[test]
    fn quad_chunks() {
        let (v, p) = quad(10.0);
        assert_eq!(v[0], 0x0019_0029);
        // Header, offset/count, 4 x 6 floats, end.
        assert_eq!(v.len(), 2 + 24 + 1);
        assert_eq!(*v.last().unwrap(), 0xFF);
        assert_eq!(f32::from_bits(v[2]), -5.0);
        assert_eq!(f32::from_bits(v[3]), 10.0);
        let p16: Vec<u16> = p
            .iter()
            .flat_map(|w| [*w as u16, (*w >> 16) as u16])
            .collect();
        assert_eq!(p16[..6], [0x2511, 2, 0xFFFF, 0xFFFF, 0x3008, 0]);
        assert_eq!(p16[6..10], [0x1941, 14, 1, 4]);
        assert_eq!(p16[22], 0x00FF);
    }

    #[test]
    fn camera_and_facing() {
        // A camera at (0, 5, 10) looking down -Z with no turn: view = [I | -cam].
        let m = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -5.0, 0.0, 0.0, 1.0, -10.0,
        ];
        assert_eq!(camera(&m), [0.0, 5.0, 10.0]);
        assert_eq!(facing([0.0, 0.0, 0.0], [0.0, 5.0, 10.0]), 0);
        assert_eq!(facing([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]), 0x4000);
        assert_eq!(frame_at(0, 2), 0);
        assert_eq!(frame_at(super::FRAME_TIME, 2), 1);
        assert_eq!(frame_at(12345, 1), 0);
    }
}
