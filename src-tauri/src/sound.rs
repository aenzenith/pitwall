//! Pitwall's own sounds, made for it (`sounds/`): Settings picks one per event. The app plays
//! them itself; the system notification stays silent.

fn bytes(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "radio" => include_bytes!("../sounds/radio.wav"),
        "lights" => include_bytes!("../sounds/lights.wav"),
        "chime" => include_bytes!("../sounds/chime.wav"),
        "boxbox" => include_bytes!("../sounds/boxbox.wav"),
        "limiter" => include_bytes!("../sounds/limiter.wav"),
        "purple" => include_bytes!("../sounds/purple.wav"),
        "lightsout" => include_bytes!("../sounds/lightsout.wav"),
        "yellowflag" => include_bytes!("../sounds/yellowflag.wav"),
        "wheelgun" => include_bytes!("../sounds/wheelgun.wav"),
        "pitboard" => include_bytes!("../sounds/pitboard.wav"),
        _ => return None,
    })
}

/// Plays `id` (nothing for an empty or unknown one), stopping the one still playing. Call on
/// the main thread.
pub fn play(id: &str) {
    let Some(bytes) = bytes(id) else {
        return;
    };

    #[cfg(target_os = "macos")]
    mac::play(bytes);

    #[cfg(not(target_os = "macos"))]
    let _ = bytes;
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::AllocAnyThread;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;

    thread_local! {
        /// The sound playing now, kept alive until it ends or the next one replaces it.
        static PLAYING: RefCell<Option<Retained<NSSound>>> = const { RefCell::new(None) };
    }

    pub fn play(bytes: &'static [u8]) {
        let Some(sound) = NSSound::initWithData(NSSound::alloc(), &NSData::with_bytes(bytes)) else {
            return;
        };

        PLAYING.with(|playing| {
            if let Some(previous) = playing.borrow_mut().take() {
                previous.stop();
            }
            sound.play();
            *playing.borrow_mut() = Some(sound);
        });
    }
}
