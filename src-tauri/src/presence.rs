//! Whether the user is at the machine: is the screen locked, and how long since the last key
//! or pointer move. Asked of the system the moment something may go to the phone
//! (`core/relay.rs`), never kept and never polled. What a system can't tell is `None`, and
//! Settings offers no choice that would rest on it (`TELLS_LOCKED`, `TELLS_IDLE`).

use std::time::Duration;

/// This system can tell a locked screen.
pub const TELLS_LOCKED: bool = cfg!(target_os = "macos");
/// This system can tell the time since the last input.
pub const TELLS_IDLE: bool = cfg!(any(target_os = "macos", windows));

/// The user's presence as the system tells it right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Presence {
    /// The screen is locked.
    pub locked: Option<bool>,
    /// The time since the last input.
    pub idle: Option<Duration>,
}

pub fn now() -> Presence {
    Presence { locked: system::locked(), idle: system::idle() }
}

#[cfg(target_os = "macos")]
mod system {
    use std::time::Duration;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSString;

    /// `kCGEventSourceStateHIDSystemState`: what the keyboard and the pointer sent, not what a
    /// program posted.
    const HARDWARE: i32 = 1;
    /// `kCGAnyInputEventType`.
    const ANY_INPUT: u32 = u32::MAX;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        /// The login session's state, a CFDictionary the caller owns; null without a session
        /// on the screen.
        fn CGSessionCopyCurrentDictionary() -> *mut AnyObject;
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event: u32) -> f64;
    }

    pub fn locked() -> Option<bool> {
        // SAFETY: a CFDictionary is an NSDictionary, and "Copy" hands over one retain, which
        // `Retained` gives back when it goes.
        let session: Retained<AnyObject> = unsafe { Retained::from_raw(CGSessionCopyCurrentDictionary()) }?;
        // SAFETY: NSDictionary's own message, with an NSString for its key; what comes back is
        // null or an object the dictionary keeps alive.
        let value: *mut AnyObject = unsafe { msg_send![&*session, objectForKey: &*NSString::from_str("CGSSessionScreenIsLocked")] };
        // The key is there only while the screen is locked.
        // SAFETY: the value under that key is a CFBoolean, an NSNumber.
        Some(!value.is_null() && unsafe { msg_send![value, boolValue] })
    }

    pub fn idle() -> Option<Duration> {
        // SAFETY: takes two numbers, gives one.
        let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(HARDWARE, ANY_INPUT) };
        (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
    }
}

#[cfg(windows)]
mod system {
    use std::time::Duration;

    /// `LASTINPUTINFO`.
    #[repr(C)]
    struct LastInput {
        size: u32,
        /// The tick count at the last input.
        tick: u32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetLastInputInfo(info: *mut LastInput) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount() -> u32;
    }

    /// Only a window that listens for the session's changes hears of a lock.
    pub fn locked() -> Option<bool> {
        None
    }

    pub fn idle() -> Option<Duration> {
        let mut last = LastInput { size: std::mem::size_of::<LastInput>() as u32, tick: 0 };
        // SAFETY: a `LASTINPUTINFO` of our own, its size set.
        if unsafe { GetLastInputInfo(&mut last) } == 0 {
            return None;
        }
        // Both count the milliseconds since the system started, and wrap together.
        // SAFETY: takes nothing, gives a number.
        let now = unsafe { GetTickCount() };
        Some(Duration::from_millis(u64::from(now.wrapping_sub(last.tick))))
    }
}

/// Linux: every desktop tells it its own way (X11's screen saver, GNOME's and KDE's own D-Bus
/// names, none under some Wayland compositors).
#[cfg(not(any(target_os = "macos", windows)))]
mod system {
    use std::time::Duration;

    pub fn locked() -> Option<bool> {
        None
    }

    pub fn idle() -> Option<Duration> {
        None
    }
}
