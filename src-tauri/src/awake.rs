//! Keeps the computer awake while a lap runs (`core/laps/run.rs`, a job's "Mac'i uyanık tut"):
//! macOS an `IOPMAssertion` of kind `PreventUserIdleSystemSleep`, taken in this process (no
//! `caffeinate` child); Windows `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` on a
//! thread kept for it, since that state is a thread's; nothing elsewhere. It only keeps an idle
//! system from sleeping: it wakes nothing, and a closed lid or the user's own sleep still win.

/// Held while it lives; let go when dropped.
pub struct Awake {
    #[cfg(target_os = "macos")]
    id: u32,
    /// Dropped, the thread that holds the state lets it go and ends.
    #[cfg(windows)]
    _held: std::sync::mpsc::Sender<()>,
}

impl Awake {
    /// Takes the hold, `reason` saying why where the system lists it (`pmset -g assertions`).
    /// None where it isn't possible, or the system refused it.
    pub fn hold(reason: &str) -> Option<Self> {
        hold(reason)
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_char, c_void, CString};

    pub type CFStringRef = *const c_void;
    /// `kCFStringEncodingUTF8`.
    pub const UTF8: u32 = 0x0800_0100;
    /// `kIOPMAssertionLevelOn`.
    pub const LEVEL_ON: u32 = 255;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(alloc: *const c_void, text: *const c_char, encoding: u32) -> CFStringRef;
        fn CFRelease(object: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        pub fn IOPMAssertionCreateWithName(kind: CFStringRef, level: u32, name: CFStringRef, id: *mut u32) -> i32;
        pub fn IOPMAssertionRelease(id: u32) -> i32;
    }

    /// A CoreFoundation string of `text`, released when dropped.
    pub struct CfString(pub CFStringRef);

    impl CfString {
        pub fn new(text: &str) -> Option<Self> {
            let text = CString::new(text.replace('\0', "")).ok()?;
            // SAFETY: `text` is a NUL-terminated UTF-8 string that outlives the call; a null
            // allocator is the default one.
            let made = unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) };
            (!made.is_null()).then_some(Self(made))
        }
    }

    impl Drop for CfString {
        fn drop(&mut self) {
            // SAFETY: made by `CFStringCreateWithCString` (we own one reference), released once.
            unsafe { CFRelease(self.0) }
        }
    }
}

#[cfg(target_os = "macos")]
fn hold(reason: &str) -> Option<Awake> {
    let kind = mac::CfString::new("PreventUserIdleSystemSleep")?;
    let name = mac::CfString::new(reason)?;
    let mut id: u32 = 0;
    // SAFETY: both strings live through the call; `id` is written by it.
    let done = unsafe { mac::IOPMAssertionCreateWithName(kind.0, mac::LEVEL_ON, name.0, &mut id) };
    (done == 0).then_some(Awake { id })
}

#[cfg(target_os = "macos")]
impl Drop for Awake {
    fn drop(&mut self) {
        // SAFETY: an assertion this value took, released once.
        unsafe {
            mac::IOPMAssertionRelease(self.id);
        }
    }
}

#[cfg(windows)]
mod win {
    /// `ES_CONTINUOUS`: the state holds until it is set again on the same thread.
    pub const CONTINUOUS: u32 = 0x8000_0000;
    /// `ES_SYSTEM_REQUIRED`: the system doesn't sleep while idle.
    pub const SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn SetThreadExecutionState(flags: u32) -> u32;
    }
}

/// The state is a thread's own: a thread of its own sets it, waits until the hold is dropped
/// (its channel closes), then sets it back and ends.
#[cfg(windows)]
fn hold(_reason: &str) -> Option<Awake> {
    let (held, dropped) = std::sync::mpsc::channel::<()>();
    let (told, set) = std::sync::mpsc::channel::<bool>();
    std::thread::spawn(move || {
        // SAFETY: plain flags; the call only changes this thread's state.
        let ok = unsafe { win::SetThreadExecutionState(win::CONTINUOUS | win::SYSTEM_REQUIRED) } != 0;
        let _ = told.send(ok);
        if !ok {
            return;
        }
        // Until every sender is gone.
        while dropped.recv().is_ok() {}
        // SAFETY: as above, back to what the thread had before.
        unsafe {
            win::SetThreadExecutionState(win::CONTINUOUS);
        }
    });
    set.recv().ok().filter(|ok| *ok).map(|_| Awake { _held: held })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn hold(_reason: &str) -> Option<Awake> {
    None
}
