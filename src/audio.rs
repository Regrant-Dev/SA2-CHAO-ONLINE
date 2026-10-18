//! The game's volume as a whole (it has none of its own: its sound options are mono/stereo and
//! the voice language). The game's music (CRI ADX on DirectSound), effects and voices (CRI
//! Sound Renderer on XAudio2) and the skins' voices all play in this process's audio session,
//! so its volume (the game's slider in the Windows volume mixer, ISimpleAudioVolume) turns
//! all of them up or down together.

/// Sets the game's volume (0..=100 percent). Done on a thread of its own: it talks to the
/// system's audio service.
pub fn set_volume(percent: u32) {
    let percent = percent.min(100);
    std::thread::spawn(move || {
        if let Err(e) = session_volume(percent as f32 / 100.0) {
            crate::log!("audio: can't set the game's volume: {e}");
        }
    });
}

/// At start: the game's volume from config.ini (`GameVolume=<percent>`), if it's set below 100
/// (otherwise left as it is, e.g. as the Windows volume mixer has it).
pub fn init(mod_path: &str) {
    let percent = std::fs::read_to_string(std::path::Path::new(mod_path).join("config.ini"))
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                k.trim()
                    .eq_ignore_ascii_case("gamevolume")
                    .then(|| v.trim().parse::<u32>().ok())
                    .flatten()
            })
        });
    if let Some(percent) = percent.filter(|&p| p < 100) {
        set_volume(percent);
    }
}

#[cfg(windows)]
fn session_volume(level: f32) -> Result<(), String> {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);
    // MMDeviceEnumerator, IMMDeviceEnumerator, IAudioSessionManager (mmdeviceapi.h,
    // audiopolicy.h).
    const CLSID_ENUMERATOR: Guid = Guid(
        0xBCDE_0395,
        0xE52F,
        0x467C,
        [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
    );
    const IID_ENUMERATOR: Guid = Guid(
        0xA956_64D2,
        0x9614,
        0x4F35,
        [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
    );
    const IID_SESSION_MANAGER: Guid = Guid(
        0xBFA9_71F1,
        0x4D5E,
        0x40BB,
        [0x93, 0x5E, 0x96, 0x70, 0x39, 0xBF, 0xBE, 0xE4],
    );
    const CLSCTX_ALL: u32 = 0x17;
    const COINIT_MULTITHREADED: u32 = 0;
    // eRender, eConsole.
    const RENDER: u32 = 0;
    const CONSOLE: u32 = 0;

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut c_void,
            context: u32,
            iid: *const Guid,
            out: *mut *mut c_void,
        ) -> i32;
    }

    /// Entry `n` of a COM object's function table.
    unsafe fn method(object: *mut c_void, n: usize) -> *const c_void {
        // SAFETY: a live COM object: its first word is its function table.
        unsafe { *(*(object as *const *const *const c_void)).add(n) }
    }
    unsafe fn release(object: *mut c_void) {
        if !object.is_null() {
            // SAFETY: IUnknown::Release, entry 2.
            unsafe {
                std::mem::transmute::<*const c_void, extern "system" fn(*mut c_void) -> u32>(
                    method(object, 2),
                )(object)
            };
        }
    }
    let check = |hr: i32, what: &str| {
        if hr < 0 {
            Err(format!("{what} failed ({:#x})", hr as u32))
        } else {
            Ok(())
        }
    };

    // SAFETY: COM calls on this thread, each object released once.
    unsafe {
        let init = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
        let (mut enumerator, mut device, mut manager, mut volume) =
            (null_mut(), null_mut(), null_mut(), null_mut());
        let result = (|| {
            check(
                CoCreateInstance(
                    &CLSID_ENUMERATOR,
                    null_mut(),
                    CLSCTX_ALL,
                    &IID_ENUMERATOR,
                    &mut enumerator,
                ),
                "MMDeviceEnumerator",
            )?;
            // IMMDeviceEnumerator::GetDefaultAudioEndpoint (4).
            let get_default = std::mem::transmute::<
                *const c_void,
                extern "system" fn(*mut c_void, u32, u32, *mut *mut c_void) -> i32,
            >(method(enumerator, 4));
            check(
                get_default(enumerator, RENDER, CONSOLE, &mut device),
                "default output",
            )?;
            // IMMDevice::Activate (3).
            let activate = std::mem::transmute::<
                *const c_void,
                extern "system" fn(
                    *mut c_void,
                    *const Guid,
                    u32,
                    *mut c_void,
                    *mut *mut c_void,
                ) -> i32,
            >(method(device, 3));
            check(
                activate(
                    device,
                    &IID_SESSION_MANAGER,
                    CLSCTX_ALL,
                    null_mut(),
                    &mut manager,
                ),
                "session manager",
            )?;
            // IAudioSessionManager::GetSimpleAudioVolume (4): the process's default session.
            let get_volume = std::mem::transmute::<
                *const c_void,
                extern "system" fn(*mut c_void, *const Guid, u32, *mut *mut c_void) -> i32,
            >(method(manager, 4));
            check(
                get_volume(manager, std::ptr::null(), 0, &mut volume),
                "session volume",
            )?;
            // ISimpleAudioVolume::SetMasterVolume (3).
            let set = std::mem::transmute::<
                *const c_void,
                extern "system" fn(*mut c_void, f32, *const Guid) -> i32,
            >(method(volume, 3));
            check(
                set(volume, level.clamp(0.0, 1.0), std::ptr::null()),
                "set volume",
            )
        })();
        for object in [volume, manager, device, enumerator] {
            release(object);
        }
        if init >= 0 {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
fn session_volume(_level: f32) -> Result<(), String> {
    Ok(())
}
