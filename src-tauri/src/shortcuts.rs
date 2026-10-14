//! Pitwall's global shortcuts: the quick switcher's and the mini tower's. Each is registered
//! and dropped on its own, so new keys for one never cost the other its own, and the plugin's
//! one handler tells which of them fired.
//!
//! What is registered and what is dropped is decided here against `Keys`: the system's own
//! shortcuts in the app (the plugin), a list in the tests, which take nothing from the system.

use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::core::Core;
use crate::i18n::t;
use crate::settings::Settings;

/// What a shortcut is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Switcher,
    Tower,
}

/// The keys registered now: the switcher's, the tower's. Never held while one is registered or
/// dropped: that waits for the main thread, where a shortcut that fires meanwhile reads this.
type Held = Mutex<[Option<Shortcut>; 2]>;

static HELD: Held = Mutex::new([None, None]);

fn held(book: &Held) -> MutexGuard<'_, [Option<Shortcut>; 2]> {
    book.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What gives a shortcut's keys out and takes them back.
trait Keys {
    /// An error, in the user's words, when the keys can't be had: another app holds them, or
    /// Pitwall's other shortcut does (`keys`: as Settings writes them, for that error).
    fn register(&self, shortcut: Shortcut, keys: &str) -> Result<(), String>;
    fn unregister(&self, shortcut: Shortcut);
}

/// The system's own, through the plugin.
struct System<'a, R: Runtime>(&'a AppHandle<R>);

impl<R: Runtime> Keys for System<'_, R> {
    fn register(&self, shortcut: Shortcut, keys: &str) -> Result<(), String> {
        // Global shortcuts on Linux go through X11 (XWayland under Wayland). Without a display the
        // registration would "succeed" and never fire.
        #[cfg(target_os = "linux")]
        if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
            return Err(t!("core.error.shortcutNoDisplay", keys = keys));
        }

        self.0.global_shortcut().register(shortcut).map_err(|error| t!("core.error.shortcutTaken", keys = keys, error = error))
    }

    fn unregister(&self, shortcut: Shortcut) {
        let _ = self.0.global_shortcut().unregister(shortcut);
    }
}

/// Registers `which`'s shortcut (or none when off), leaving the other's as it is. An error
/// means the keys are invalid or already taken, by another app or by Pitwall's other shortcut;
/// nothing is registered for `which` then.
fn set(system: &impl Keys, book: &Held, which: Which, on: bool, keys: &str) -> Result<(), String> {
    let slot = which as usize;

    let wanted = match on {
        true => Some(keys.parse::<Shortcut>().map_err(|error| t!("core.error.invalidShortcut", keys = keys, error = error))?),
        false => None,
    };
    let before = held(book)[slot];
    if before == wanted {
        return Ok(());
    }

    if let Some(old) = before {
        system.unregister(old);
        held(book)[slot] = None;
    }
    let Some(shortcut) = wanted else {
        return Ok(());
    };

    let result = system.register(shortcut, keys);

    #[cfg(debug_assertions)]
    eprintln!("[pitwall] register {keys} ({which:?}): {result:?}");

    if result.is_ok() {
        held(book)[slot] = Some(shortcut);
    }
    result
}

/// Neither is registered: while new keys are recorded.
fn drop_both(system: &impl Keys, book: &Held) {
    for which in [Which::Switcher, Which::Tower] {
        let _ = set(system, book, which, false, "");
    }
}

/// New keys for `which`, as its recorder hands them over (both shortcuts stand down while it
/// records). The other one is registered again first, so keys it holds are refused as taken,
/// the way another app's are, instead of being taken from it without a word.
fn take_with(system: &impl Keys, book: &Held, which: Which, keys: &str, settings: &Settings) -> Result<(), String> {
    let _ = match which {
        Which::Switcher => set(system, book, Which::Tower, settings.tower.shortcut, &settings.tower.shortcut_keys),
        Which::Tower => set(system, book, Which::Switcher, settings.shortcut, &settings.shortcut_keys),
    };
    set(system, book, which, true, keys)
}

/// Both as `settings` has them. Should both be on with the same keys (a settings file that
/// names them twice, a backup's keys that are the tower's here), the quick switcher has them
/// and the tower none: true then.
fn put_back(system: &impl Keys, book: &Held, settings: &Settings) -> bool {
    let keys = |on: bool, keys: &str| if on { keys.parse::<Shortcut>().ok() } else { None };
    let switcher = keys(settings.shortcut, &settings.shortcut_keys);
    let tower = keys(settings.tower.shortcut, &settings.tower.shortcut_keys);
    let clash = switcher.is_some() && switcher == tower;
    let tower = if clash { None } else { tower };

    // What is to go goes first: one may be given the keys the other still holds.
    for (which, wanted) in [(Which::Switcher, switcher), (Which::Tower, tower)] {
        if held(book)[which as usize] != wanted {
            let _ = set(system, book, which, false, "");
        }
    }
    let _ = set(system, book, Which::Switcher, settings.shortcut, &settings.shortcut_keys);
    let _ = set(system, book, Which::Tower, settings.tower.shortcut && !clash, &settings.tower.shortcut_keys);
    clash
}

/// `put_back` as the core's settings are, and the tower's switch turned off there when the
/// quick switcher has its keys: a shortcut is never left on in Settings and dead.
fn resume_with(system: &impl Keys, book: &Held, core: &Core) {
    if put_back(system, book, &core.settings()) {
        let _ = core.set_tower_option("shortcut", false);
    }
}

/// Which of the two `shortcut` is, of those registered now.
fn whose(book: &Held, shortcut: &Shortcut) -> Option<Which> {
    let [switcher, tower] = *held(book);

    if tower.as_ref() == Some(shortcut) {
        Some(Which::Tower)
    } else if switcher.as_ref() == Some(shortcut) {
        Some(Which::Switcher)
    } else {
        None
    }
}

/// Registers `which`'s shortcut (or none when off), leaving the other's as it is. An error
/// means the keys are invalid or already taken, by another app or by Pitwall's other shortcut;
/// nothing is registered for `which` then.
pub fn apply<R: Runtime>(app: &AppHandle<R>, which: Which, on: bool, keys: &str) -> Result<(), String> {
    set(&System(app), &HELD, which, on, keys)
}

/// While new keys are recorded, neither shortcut fires.
pub fn suspend<R: Runtime>(app: &AppHandle<R>) {
    drop_both(&System(app), &HELD);
}

/// New keys for `which` from its recorder, registered if they can be had: not when another app
/// holds them, nor when Pitwall's other shortcut does. `resume` follows either way.
pub fn take<R: Runtime>(app: &AppHandle<R>, which: Which, keys: &str, settings: &Settings) -> Result<(), String> {
    take_with(&System(app), &HELD, which, keys, settings)
}

/// Both as Settings has them: at start, after a recording, after an import. Both on with the
/// same keys: the quick switcher keeps them and the tower's switch goes off, in Settings too.
pub fn resume<R: Runtime>(app: &AppHandle<R>, core: &Core) {
    resume_with(&System(app), &HELD, core);
}

/// A registered shortcut was pressed: the switcher's opens or closes the search, the tower's
/// shows or hides the tower.
pub fn fired<R: Runtime>(app: &AppHandle<R>, shortcut: &Shortcut) {
    match whose(&HELD, shortcut) {
        Some(Which::Tower) => crate::tower::toggle(app),
        Some(Which::Switcher) => crate::tray::toggle_switcher(app),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;

    use super::*;
    use crate::core::CoreConfig;

    const A: &str = "Ctrl+Alt+KeyP";
    const B: &str = "Ctrl+Alt+KeyK";
    const C: &str = "Ctrl+Shift+KeyJ";

    fn keys(keys: &str) -> Shortcut {
        keys.parse().unwrap()
    }

    /// The system's side, in memory: what Pitwall holds, and what another app does. As every
    /// system Pitwall runs on does, it gives the same keys out once.
    #[derive(Default)]
    struct Desk {
        ours: RefCell<Vec<Shortcut>>,
        theirs: Vec<Shortcut>,
    }

    impl Keys for Desk {
        fn register(&self, shortcut: Shortcut, keys: &str) -> Result<(), String> {
            if self.theirs.contains(&shortcut) || self.ours.borrow().contains(&shortcut) {
                return Err(format!("{keys} is taken"));
            }
            self.ours.borrow_mut().push(shortcut);
            Ok(())
        }

        fn unregister(&self, shortcut: Shortcut) {
            self.ours.borrow_mut().retain(|ours| *ours != shortcut);
        }
    }

    impl Desk {
        /// Whether Pitwall holds exactly these, in whatever order.
        fn holds(&self, wanted: &[&str]) -> bool {
            let ours = self.ours.borrow();
            ours.len() == wanted.len() && wanted.iter().all(|wanted| ours.contains(&keys(wanted)))
        }
    }

    /// Nothing registered yet.
    fn blank() -> Held {
        Mutex::new([None, None])
    }

    /// Settings with each shortcut on with its keys, or off.
    fn settings(switcher: Option<&str>, tower: Option<&str>) -> Settings {
        let mut settings = Settings { shortcut: switcher.is_some(), ..Settings::default() };
        if let Some(keys) = switcher {
            settings.shortcut_keys = keys.into();
        }
        settings.tower.shortcut = tower.is_some();
        if let Some(keys) = tower {
            settings.tower.shortcut_keys = keys.into();
        }
        settings
    }

    /// Whose keys fire what, of the three the tests use.
    fn fires(book: &Held) -> [Option<Which>; 3] {
        [A, B, C].map(|pressed| whose(book, &keys(pressed)))
    }

    const SWITCHER: Option<Which> = Some(Which::Switcher);
    const TOWER: Option<Which> = Some(Which::Tower);

    /// Each shortcut is registered and dropped alone: new keys for one, one turned off, or keys
    /// that can't be had never cost the other its own, and a press is told apart.
    #[test]
    fn a_change_to_one_shortcut_leaves_the_other_its_keys() {
        let (desk, book) = (Desk { theirs: vec![keys("Alt+Space")], ..Desk::default() }, blank());
        let both = settings(Some(A), Some(B));
        assert!(!put_back(&desk, &book, &both));
        assert!(desk.holds(&[A, B]));
        assert_eq!(fires(&book), [SWITCHER, TOWER, None]);

        // New keys for the tower, the tower turned off and on again: the switcher's stay.
        set(&desk, &book, Which::Tower, true, C).unwrap();
        assert!(desk.holds(&[A, C]));
        assert_eq!(fires(&book), [SWITCHER, None, TOWER]);
        set(&desk, &book, Which::Tower, false, C).unwrap();
        assert!(desk.holds(&[A]));
        assert_eq!(fires(&book), [SWITCHER, None, None]);
        set(&desk, &book, Which::Tower, true, B).unwrap();

        // The other way round: the tower's stay.
        set(&desk, &book, Which::Switcher, true, C).unwrap();
        assert!(desk.holds(&[B, C]));
        assert_eq!(fires(&book), [None, TOWER, SWITCHER]);
        set(&desk, &book, Which::Switcher, false, C).unwrap();
        assert!(desk.holds(&[B]));
        assert_eq!(fires(&book), [None, TOWER, None]);
        set(&desk, &book, Which::Switcher, true, A).unwrap();

        // Keys no shortcut can be made of, and keys another app holds: refused, and the other
        // shortcut is as it was.
        assert!(set(&desk, &book, Which::Tower, true, "Ctrl+Alt+Nothing").is_err());
        assert!(desk.holds(&[A, B]));
        assert!(set(&desk, &book, Which::Tower, true, "Alt+Space").is_err());
        assert!(desk.holds(&[A]));
        assert_eq!(fires(&book), [SWITCHER, None, None]);
        assert_eq!(whose(&book, &keys("Alt+Space")), None, "keys that were refused fire nothing");

        // While new keys are recorded neither fires; then both do again.
        drop_both(&desk, &book);
        assert!(desk.holds(&[]));
        assert_eq!(fires(&book), [None, None, None]);
        put_back(&desk, &book, &both);
        assert!(desk.holds(&[A, B]));
        assert_eq!(fires(&book), [SWITCHER, TOWER, None]);
    }

    /// A recording stands both shortcuts down, so the keys typed are free for the taking even
    /// when they are the other shortcut's: they are refused all the same, in both directions,
    /// and each keeps its own.
    #[test]
    fn a_recording_is_refused_the_keys_the_other_shortcut_holds() {
        for (which, others) in [(Which::Tower, A), (Which::Switcher, B)] {
            let (desk, book) = (Desk::default(), blank());
            let both = settings(Some(A), Some(B));
            put_back(&desk, &book, &both);

            drop_both(&desk, &book);
            assert!(take_with(&desk, &book, which, others, &both).is_err(), "{which:?} took the other's keys");
            // Refused: Settings stay as they were, and both come back.
            assert!(!put_back(&desk, &book, &both));
            assert!(desk.holds(&[A, B]));
            assert_eq!(fires(&book), [SWITCHER, TOWER, None], "{which:?}");

            // Keys nobody holds are taken, and the other shortcut is back with its own.
            drop_both(&desk, &book);
            take_with(&desk, &book, which, C, &both).unwrap();
            assert!(desk.holds(&[others, C]));
            assert_eq!(whose(&book, &keys(C)), Some(which));
            assert!(whose(&book, &keys(others)).is_some_and(|other| other != which));
        }

        // The other one is off: its keys are nobody's.
        let (desk, book) = (Desk::default(), blank());
        let alone = settings(Some(A), None);
        put_back(&desk, &book, &alone);
        drop_both(&desk, &book);
        take_with(&desk, &book, Which::Switcher, &alone.tower.shortcut_keys, &alone).unwrap();
    }

    /// Both on with the same keys, as a start may find them and an import may leave them: the
    /// quick switcher has the keys, and the tower's switch goes off in Settings, on the disk
    /// too. Never one of them on and dead.
    #[test]
    fn the_same_keys_for_both_leave_the_switcher_its_keys_and_the_towers_switch_off() {
        // A settings file that names the same keys twice.
        let (desk, book) = (Desk::default(), blank());
        assert!(put_back(&desk, &book, &settings(Some(A), Some(A))));
        assert!(desk.holds(&[A]));
        assert_eq!(fires(&book), [SWITCHER, None, None]);
        // The same keys written another way are the same keys.
        assert!(put_back(&desk, &book, &settings(Some(A), Some("Alt+Ctrl+KeyP"))));
        // With one of them off there is nothing to settle.
        assert!(!put_back(&desk, &book, &settings(Some(A), None)));
        assert!(!put_back(&desk, &book, &settings(None, Some(A))));
        assert_eq!(fires(&book), [TOWER, None, None]);

        // An import: the switcher is given the keys the tower holds here.
        let (desk, book) = (Desk::default(), blank());
        put_back(&desk, &book, &settings(Some(A), Some(B)));
        assert!(put_back(&desk, &book, &settings(Some(B), Some(B))));
        assert!(desk.holds(&[B]));
        assert_eq!(fires(&book), [None, SWITCHER, None]);
        // Each given the other's keys is no clash: both have theirs.
        assert!(!put_back(&desk, &book, &settings(Some(A), Some(B))));
        assert!(!put_back(&desk, &book, &settings(Some(B), Some(A))));
        assert!(desk.holds(&[A, B]));
        assert_eq!(fires(&book), [TOWER, SWITCHER, None]);

        // And what Settings says afterwards, with a core of its own in a folder of its own.
        let tmp = tempfile::tempdir().unwrap();
        let config = CoreConfig {
            registry_dir: tmp.path().join("registry"),
            claude_dir: tmp.path().join("claude"),
            settings_file: tmp.path().join("settings.json"),
            claude_settings: tmp.path().join("claude-settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        };
        let core = Core::new(config, Arc::new(|_| {}));
        core.set_shortcut_keys(A.into());
        core.set_tower_shortcut_keys(A.into());
        assert!(core.settings().shortcut && core.settings().tower.shortcut);

        let (desk, book) = (Desk::default(), blank());
        resume_with(&desk, &book, &core);
        assert!(desk.holds(&[A]));
        assert_eq!(fires(&book), [SWITCHER, None, None]);
        let now = core.settings();
        assert_eq!((now.shortcut, now.tower.shortcut, now.tower.shortcut_keys.as_str()), (true, false, A));
        assert_eq!(Settings::load(&tmp.path().join("settings.json")), now, "and the file");

        // Nothing to settle: nothing is written.
        core.set_tower_shortcut_keys(B.into());
        resume_with(&desk, &book, &core);
        assert!(core.settings().tower.shortcut);
        assert!(desk.holds(&[A, B]));
    }
}
