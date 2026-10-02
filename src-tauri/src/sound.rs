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
    device::play(bytes);
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

/// Windows and Linux: the default output device through rodio, from a thread of its own. The
/// device is opened for a sound and let go a moment after the last one ends, so an idle Pitwall
/// holds no audio stream.
#[cfg(not(target_os = "macos"))]
mod device {
    use std::io::Cursor;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::sync::OnceLock;
    use std::thread;
    use std::time::Duration;

    use rodio::{Decoder, DeviceSinkBuilder, Player};

    /// How often the thread looks whether the sound has ended.
    const POLL: Duration = Duration::from_millis(100);
    /// Quiet polls before the device is let go, so the sound's last samples still play.
    const LINGER: u32 = 10;

    static QUEUE: OnceLock<Sender<&'static [u8]>> = OnceLock::new();

    pub fn play(bytes: &'static [u8]) {
        let queue = QUEUE.get_or_init(|| {
            let (queue, sounds) = mpsc::channel();
            let _ = thread::Builder::new().name("pitwall-sound".into()).spawn(move || run(&sounds));
            queue
        });
        let _ = queue.send(bytes);
    }

    fn run(sounds: &Receiver<&'static [u8]>) {
        while let Ok(first) = sounds.recv() {
            // No output device (or none that opens): this sound is skipped, the next one tries again.
            let Ok(mut device) = DeviceSinkBuilder::open_default_sink() else {
                continue;
            };
            device.log_on_drop(false);

            let mut next = Some(first);
            let mut playing: Option<Player> = None;
            let mut quiet = 0;

            loop {
                if let Some(bytes) = next.take() {
                    if let Some(previous) = playing.take() {
                        previous.stop();
                    }
                    let player = Player::connect_new(device.mixer());
                    if let Ok(sound) = Decoder::new_wav(Cursor::new(bytes)) {
                        player.append(sound);
                    }
                    playing = Some(player);
                    quiet = 0;
                }

                match sounds.recv_timeout(POLL) {
                    Ok(bytes) => next = Some(bytes),
                    Err(RecvTimeoutError::Timeout) => {
                        if playing.as_ref().is_none_or(Player::empty) {
                            quiet += 1;
                            if quiet >= LINGER {
                                break;
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}
