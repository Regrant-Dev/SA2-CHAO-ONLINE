//! Checked recovery for the native stage-select texture fallback (re/menu-textures.md).
//! Mod Loader's normal PAK/folder overrides run first. Only its legacy PRS fallback for
//! stageMap/stageMapBG uses these owned archives; all other native calls are preserved.

pub fn install(_mod_path: &str) {
    #[cfg(all(windows, target_arch = "x86"))]
    native::install(_mod_path);
}

#[cfg(any(test, all(windows, target_arch = "x86")))]
mod archives {
    use std::path::Path;

    pub const NAMES: [&str; 2] = ["stageMap", "stageMapBG"];
    const COUNTS: [usize; 2] = [16, 18];

    pub fn index(filename: &[u8]) -> Option<usize> {
        NAMES.iter().position(|name| {
            filename.len() == name.len() + 4
                && filename[..name.len()].eq_ignore_ascii_case(name.as_bytes())
                && filename[name.len()..].eq_ignore_ascii_case(b".prs")
        })
    }

    /// This recovery accepts only the two base GVM layouts. Native parsing swaps the
    /// header in place and assumes enough records/images without checking chunk bounds.
    fn validate(bytes: &[u8], expected: usize) -> Result<(), String> {
        if bytes.len() < 12 || &bytes[..4] != b"GVMH" {
            return Err("missing GVM header".into());
        }
        let word = |at| u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
        if word(8) != 15 || word(10) != expected {
            return Err("unexpected menu texture layout".into());
        }
        let mut at = 0usize;
        let mut images = 0;
        while at < bytes.len() {
            let header = bytes.get(at..at + 8).ok_or("truncated GVM chunk")?;
            let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let end = at
                .checked_add(8)
                .and_then(|v| v.checked_add(size))
                .filter(|&end| end <= bytes.len())
                .ok_or("GVM chunk out of bounds")?;
            match &header[..4] {
                b"GVMH" if at == 0 && size >= 4 + expected * 38 => {
                    // The native header helper uses C names from these 38-byte records.
                    for rec in bytes[12..12 + expected * 38].as_chunks::<38>().0 {
                        if !rec[2..30].contains(&0) {
                            return Err("unterminated GVM texture name".into());
                        }
                    }
                }
                b"GVRT" if size >= 8 => {
                    // Base menu packs use only RGB5A3 and CMPR, without palettes/mipmaps.
                    let format = bytes[at + 11];
                    let (w, h) = (word(at + 12), word(at + 14));
                    if w == 0 || h == 0 || w > 1024 || h > 1024 || w % 8 != 0 || h % 8 != 0 {
                        return Err("invalid menu texture dimensions".into());
                    }
                    let pixels = match format {
                        5 => w * h * 2,
                        14 => w * h / 2,
                        _ => return Err("unexpected menu texture format".into()),
                    };
                    if bytes[at + 8..at + 10] != [0, 0]
                        || !matches!(bytes[at + 10], 0 | 8)
                        || size != 8 + pixels
                    {
                        return Err("unexpected menu image payload".into());
                    }
                    images += 1;
                }
                _ => return Err("unexpected menu GVM chunk".into()),
            }
            at = end;
        }
        if images != expected {
            return Err("menu image count mismatch".into());
        }
        Ok(())
    }

    pub struct Archive {
        pristine: Box<[u8]>,
        // The parser can retain pointers into the data. This allocation never moves or
        // goes away; each new load restores the header's original byte order first.
        working: Box<[u8]>,
        pub loads: u32,
    }

    impl Archive {
        pub fn from_prs(bytes: &[u8], index: usize) -> Result<Self, String> {
            let decoded = crate::prs::decode(bytes)?;
            validate(&decoded, COUNTS[index])?;
            Ok(Self {
                working: decoded.clone().into_boxed_slice(),
                pristine: decoded.into_boxed_slice(),
                loads: 0,
            })
        }

        pub fn len(&self) -> usize {
            self.pristine.len()
        }

        pub fn restore(&mut self) -> *mut u8 {
            self.working.copy_from_slice(&self.pristine);
            self.loads = self.loads.saturating_add(1);
            self.working.as_mut_ptr()
        }
    }

    pub fn load(root: &Path) -> Result<[Archive; 2], String> {
        use std::io::Read;
        let mut result = Vec::new();
        for (index, name) in NAMES.iter().enumerate() {
            // Limit both the compressed read and the decoded output (prs::decode).
            let mut bytes = Vec::new();
            std::fs::File::open(root.join(format!("{name}.prs")))
                .map_err(|e| format!("{name}: {e}"))?
                .take((1 << 20) + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 1 << 20 {
                return Err("menu PRS file too large".into());
            }
            result.push(Archive::from_prs(&bytes, index).map_err(|e| format!("{name}: {e}"))?);
        }
        result.try_into().map_err(|_| "missing menu archive".into())
    }

    #[cfg(test)]
    pub fn fixture(count: usize) -> Vec<u8> {
        let size = 4 + count * 38;
        let mut bytes = b"GVMH".to_vec();
        bytes.extend((size as u32).to_le_bytes());
        bytes.extend(15u16.to_be_bytes());
        bytes.extend((count as u16).to_be_bytes());
        bytes.resize(8 + size, 0);
        for _ in 0..count {
            bytes.extend(b"GVRT");
            bytes.extend(40u32.to_le_bytes());
            bytes.extend([0, 0, 0, 14, 0, 8, 0, 8]);
            bytes.extend([0; 32]);
        }
        bytes
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fallback_matches_only_exact_menu_filenames() {
            assert_eq!(index(b"STAGEMAP.PRS"), Some(0));
            assert_eq!(index(b"stageMapBG.prs"), Some(1));
            for name in [
                b"stageMap.pak".as_slice(),
                b"stageMap",
                b"other.PRS",
                b"xstageMap.PRS",
            ] {
                assert_eq!(index(name), None);
            }
        }

        #[test]
        fn rejects_corrupt_chunks_and_incomplete_archives_before_native_parsing() {
            let original = fixture(16);
            assert!(validate(&original, 16).is_ok());
            for len in [0, 7, 11, 20, original.len() - 1] {
                assert!(validate(&original[..len], 16).is_err());
            }
            let mut bad = original.clone();
            bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(validate(&bad, 16).is_err());
            let mut bad = original.clone();
            bad[12 + 2..12 + 30].fill(b'x');
            assert!(validate(&bad, 16).is_err());
            let mut bad = original.clone();
            let image = 12 + 16 * 38;
            bad[image + 14..image + 16].copy_from_slice(&1024u16.to_be_bytes());
            assert!(validate(&bad, 16).is_err());
            assert!(validate(&original, 18).is_err());
            let mut bad = original.clone();
            bad.extend(&original[image..image + 48]);
            assert!(validate(&bad, 16).is_err());
        }

        #[test]
        fn repeated_native_header_mutation_restores_in_place() {
            let bytes = fixture(16);
            let mut archive = Archive::from_prs(&crate::prs::encode(&bytes), 0).unwrap();
            let first = archive.restore();
            archive.working[8..12].reverse();
            assert_eq!(archive.restore(), first);
            assert_eq!(&*archive.working, bytes);
            assert_eq!(archive.loads, 2);
            assert_eq!(archive.len(), bytes.len());
        }

        #[test]
        fn installed_base_menu_archives_validate_when_available() {
            let Ok(dir) = std::env::var("SA2_DIR") else {
                return;
            };
            let archives = load(&Path::new(&dir).join("resource/gd_PC")).unwrap();
            assert_eq!(archives[0].len(), 721776);
            assert_eq!(archives[1].len(), 492528);
        }
    }
}

#[cfg(all(windows, target_arch = "x86"))]
mod native {
    use super::archives::{self, Archive};
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Mutex, OnceLock};

    const DECODE_CALL: usize = 0x0044_C699;
    const PARSE_CALL: usize = 0x0044_C6B3;
    static DECODE_NEXT: AtomicUsize = AtomicUsize::new(0x0048_F980);
    static PARSE_NEXT: AtomicUsize = AtomicUsize::new(0x0042_FC30);
    static ARCHIVES: OnceLock<Mutex<[Archive; 2]>> = OnceLock::new();

    fn call(site: usize, target: usize) -> [u8; 5] {
        let mut bytes = [0xe8, 0, 0, 0, 0];
        bytes[1..].copy_from_slice(&(target.wrapping_sub(site + 5) as u32).to_le_bytes());
        bytes
    }

    pub fn install(mod_path: &str) {
        if ARCHIVES.get().is_some() {
            return;
        }
        // Check both calls before touching either. The Mod Loader's entry hook stays intact.
        let decode = call(DECODE_CALL, 0x0048_F980);
        let parse = call(PARSE_CALL, 0x0042_FC30);
        // SAFETY: read-only code bytes at the two verified retail call sites.
        if unsafe { std::ptr::read(DECODE_CALL as *const [u8; 5]) } != decode
            || unsafe { std::ptr::read(PARSE_CALL as *const [u8; 5]) } != parse
        {
            crate::log!("menu textures: fallback hooks unavailable (code differs)");
            return;
        }
        let root = std::path::Path::new(mod_path).join("../../resource/gd_PC");
        let loaded = match archives::load(&root) {
            Ok(loaded) => loaded,
            Err(e) => {
                crate::log!("menu textures: recovery unavailable ({e})");
                return;
            }
        };
        if ARCHIVES.set(Mutex::new(loaded)).is_err() {
            return;
        }
        // Install during Init, before the game can execute this loader. If the second
        // patch fails, restore the first call.
        let parse_hook = call(PARSE_CALL, parse_thunk as *const () as usize);
        if !crate::patch::replace_code(PARSE_CALL, &parse, &parse_hook) {
            return;
        }
        if !crate::patch::replace_code(
            DECODE_CALL,
            &decode,
            &call(DECODE_CALL, decode_thunk as *const () as usize),
        ) {
            crate::patch::replace_code(PARSE_CALL, &parse_hook, &parse);
            crate::log!("menu textures: couldn't install recovery");
            return;
        }
        crate::log!("menu textures: checked PRS fallback installed");
    }

    unsafe fn menu_index(filename: *const u8) -> Option<usize> {
        // The native loader has just written a 0x100-byte snprintf filename buffer.
        // Read only through its terminator, never assume the full buffer is initialized.
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: caller supplies the native filename buffer on its live stack.
            *byte = unsafe { filename.add(i).read() };
            if *byte == 0 {
                return archives::index(&bytes[..i]);
            }
        }
        None
    }

    extern "C" fn decoded_length(filename: *const u8) -> usize {
        // SAFETY: the thunk passes the verified outer loader's stack buffer.
        let Some(index) = (unsafe { menu_index(filename) }) else {
            return 0;
        };
        ARCHIVES.get().map_or(0, |a| {
            a.lock().unwrap_or_else(|e| e.into_inner())[index].len()
        })
    }

    extern "C" fn parse_menu(filename: *const u8, texlist: usize) -> i32 {
        // SAFETY: same live filename buffer as the decompression call.
        let Some(index) = (unsafe { menu_index(filename) }) else {
            return 0;
        };
        let Some(archives) = ARCHIVES.get() else {
            return 0;
        };
        let mut archives = archives.lock().unwrap_or_else(|e| e.into_inner());
        let archive = &mut archives[index];
        let data = archive.restore();
        if archive.loads == 1 {
            crate::log!(
                "menu textures: recovered {} from checked base PRS",
                archives::NAMES[index]
            );
        }
        // SAFETY: archive was validated before hooking, has permanent storage, and the
        // native loader supplies a live texture list. Native parser uses EAX/EBX.
        let result: i32;
        unsafe {
            core::arch::asm!(
                "push ebx", "mov ebx, {list}", "call dword ptr [{next}]", "pop ebx",
                list = in(reg) texlist, next = sym PARSE_NEXT,
                inlateout("eax") data => result, clobber_abi("C"),
            );
        }
        result
    }

    // At 44C699 entry: [ESP+4] destination, filename starts at ESP+20h.
    // Preserve ECX/EDX and the original stack for the ECX/stack usercall; EAX is its result.
    #[unsafe(naked)]
    unsafe extern "C" fn decode_thunk() {
        core::arch::naked_asm!(
            "push ecx", "push edx", "push ebp", "mov ebp, esp",
            "and esp, -16", "sub esp, 16",
            "lea eax, [ebp + 0x2c]", "mov [esp], eax", "call {length}",
            "mov esp, ebp", "pop ebp", "pop edx", "pop ecx",
            "test eax, eax", "jz 2f", "ret",
            "2:", "jmp dword ptr [{next}]",
            length = sym decoded_length, next = sym DECODE_NEXT,
        );
    }

    // At 44C6B3 entry: filename starts at ESP+14h; input EAX, texlist EBX.
    #[unsafe(naked)]
    unsafe extern "C" fn parse_thunk() {
        core::arch::naked_asm!(
            "push eax", "push ecx", "push edx", "push ebp", "mov ebp, esp",
            "and esp, -16", "sub esp, 16",
            "lea eax, [ebp + 0x24]", "mov [esp], eax", "mov [esp + 4], ebx",
            "call {parse}", "mov esp, ebp", "pop ebp",
            "pop edx", "pop ecx", "test eax, eax", "jz 2f",
            "add esp, 4", "ret",
            "2:", "pop eax", "jmp dword ptr [{next}]",
            parse = sym parse_menu, next = sym PARSE_NEXT,
        );
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::Ordering;

        static INPUT: AtomicUsize = AtomicUsize::new(0);
        static DEST: AtomicUsize = AtomicUsize::new(0);
        static FLAGS: AtomicUsize = AtomicUsize::new(0);
        static COUNT: AtomicUsize = AtomicUsize::new(0);

        #[unsafe(naked)]
        unsafe extern "C" fn decode_stub() {
            core::arch::naked_asm!(
                "mov [{input}], ecx", "mov eax, [esp + 4]", "mov [{dest}], eax",
                "mov eax, 0x55", "ret",
                input = sym INPUT, dest = sym DEST,
            );
        }

        #[unsafe(naked)]
        unsafe extern "C" fn parse_stub() {
            core::arch::naked_asm!(
                "mov [{input}], eax", "mov [{dest}], ebx",
                "cmp eax, 0x44445555", "je 2f",
                "movzx edx, word ptr [eax + 8]", "mov [{flags}], edx",
                "movzx edx, word ptr [eax + 10]", "mov [{count}], edx",
                // Like the real parser, mutate the header's byte order in place.
                "mov word ptr [eax + 8], 15", "mov word ptr [eax + 10], 16",
                "2:", "mov eax, 1", "ret",
                input = sym INPUT, dest = sym DEST, flags = sym FLAGS, count = sym COUNT,
            );
        }

        /// Reproduce the two native call-site stack layouts with every 4-byte stack
        /// alignment. Save all observed registers plus ESP before/after each thunk.
        #[unsafe(naked)]
        unsafe extern "C" fn drive(mode: usize, name: *const u8, out: *mut usize, skew: usize) {
            core::arch::naked_asm!(
                "push ebp", "mov ebp, esp", "push ebx", "push esi", "push edi",
                "sub esp, [ebp + 20]", "sub esp, 0x100",
                "mov esi, [ebp + 12]", "lea edi, [esp + 0x1c]",
                "cmp dword ptr [ebp + 8], 0", "je 2f", "lea edi, [esp + 0x10]",
                "2:", "lodsb", "stosb", "test al, al", "jnz 2b",
                "mov eax, [ebp + 16]", "mov [eax + 24], esp",
                "mov ecx, 0x12345678", "mov edx, 0x76543210", "mov ebx, 0x11112222",
                "mov esi, 0x33334444", "mov edi, 0x55556666", "mov eax, 0x44445555",
                "mov dword ptr [esp], 0x11223344",
                "cmp dword ptr [ebp + 8], 0", "jne 3f",
                "call {decode}", "jmp 4f", "3:", "call {parse}", "4:",
                "push edi", "mov edi, [ebp + 16]",
                "mov [edi], eax", "mov [edi + 4], ecx", "mov [edi + 8], edx",
                "mov [edi + 12], ebx", "mov [edi + 16], esi",
                "pop eax", "mov [edi + 20], eax", "mov [edi + 28], esp",
                "lea esp, [ebp - 12]", "pop edi", "pop esi", "pop ebx", "pop ebp", "ret",
                decode = sym decode_thunk, parse = sym parse_thunk,
            );
        }

        #[test]
        fn x86_usercalls_preserve_stack_and_delegate_non_menu_archives() {
            let make =
                |n, i| Archive::from_prs(&crate::prs::encode(&archives::fixture(n)), i).unwrap();
            assert!(ARCHIVES.set(Mutex::new([make(16, 0), make(18, 1)])).is_ok());
            DECODE_NEXT.store(decode_stub as *const () as usize, Ordering::Relaxed);
            PARSE_NEXT.store(parse_stub as *const () as usize, Ordering::Relaxed);
            let mut pointer = [0; 2];
            for skew in [0, 4, 8, 12] {
                for (i, name) in [c"stageMap.PRS", c"stageMapBG.PRS", c"other.PRS"]
                    .iter()
                    .enumerate()
                {
                    for mode in [0, 1] {
                        let mut out = [0; 8];
                        INPUT.store(0, Ordering::Relaxed);
                        // SAFETY: the harness supplies live NUL-terminated names and
                        // output storage, and both native callees are controlled stubs.
                        unsafe {
                            drive(mode, name.as_ptr().cast(), out.as_mut_ptr(), skew);
                        }
                        assert_eq!(&out[3..6], &[0x11112222, 0x33334444, 0x55556666]);
                        assert_eq!(out[6], out[7], "unbalanced stack");
                        let input = INPUT.load(Ordering::Relaxed);
                        if mode == 0 {
                            assert_eq!(&out[1..3], &[0x12345678, 0x76543210]);
                            if i < 2 {
                                assert_eq!(out[0], 12 + (16 + 2 * i) * (38 + 48));
                                assert_eq!(input, 0, "menu decoder must skip unsafe native codec");
                            } else {
                                assert_eq!(
                                    (out[0], input, DEST.load(Ordering::Relaxed)),
                                    (0x55, 0x12345678, 0x11223344)
                                );
                            }
                        } else {
                            assert_eq!(out[0], 1);
                            assert_eq!(DEST.load(Ordering::Relaxed), 0x11112222);
                            if i < 2 {
                                assert_ne!(input, 0x44445555);
                                assert_eq!(
                                    FLAGS.load(Ordering::Relaxed),
                                    0x0f00,
                                    "header must be restored"
                                );
                                assert_eq!(COUNT.load(Ordering::Relaxed), (16 + 2 * i) << 8);
                                if pointer[i] == 0 {
                                    pointer[i] = input;
                                }
                                assert_eq!(pointer[i], input, "archive storage must remain stable");
                            } else {
                                assert_eq!(
                                    input, 0x44445555,
                                    "other archives keep original EAX input"
                                );
                            }
                        }
                    }
                }
            }
            DECODE_NEXT.store(0x0048_F980, Ordering::Relaxed);
            PARSE_NEXT.store(0x0042_FC30, Ordering::Relaxed);
        }
    }
}
