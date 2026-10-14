//! The mini tower's own settings (`Settings::tower`): each changed by its own command, never
//! with the Settings form. The window itself is the Tauri layer's (`crate::tower`).

use super::*;
use crate::settings::TowerSettings;

impl Core {
    /// One of the tower's switches: `on` (it is shown), `hideStopped`, `allSpaces` or
    /// `shortcut` (its global shortcut).
    pub fn set_tower_option(&self, option: &str, on: bool) -> Result<(), String> {
        self.change_tower(|tower| {
            let switch = match option {
                "on" => &mut tower.on,
                "hideStopped" => &mut tower.hide_stopped,
                "allSpaces" => &mut tower.all_spaces,
                "shortcut" => &mut tower.shortcut,
                _ => return Err(format!("unknown option: {option}")),
            };
            *switch = on;
            Ok(())
        })
    }

    /// A row per project (`list`) or one row of codes (`strip`).
    pub fn set_tower_form(&self, form: &str) -> Result<(), String> {
        if !matches!(form, "list" | "strip") {
            return Err(format!("unknown form: {form}"));
        }
        self.change_tower(|tower| {
            tower.form = form.to_string();
            Ok(())
        })
    }

    /// New keys for the tower's shortcut, which is on from then.
    pub fn set_tower_shortcut_keys(&self, keys: String) {
        let _ = self.change_tower(|tower| {
            tower.shortcut = true;
            tower.shortcut_keys = keys;
            Ok(())
        });
    }

    /// The tower's shortcut: shown becomes hidden and the other way round. What it is now.
    pub fn toggle_tower(&self) -> bool {
        let mut shown = false;
        let _ = self.change_tower(|tower| {
            tower.on = !tower.on;
            shown = tower.on;
            Ok(())
        });
        shown
    }

    /// Where the tower was left. No page draws from it, so no window is told.
    pub fn set_tower_place(&self, place: [i32; 4]) {
        {
            let mut inner = self.lock();
            if inner.settings.tower.place == Some(place) {
                return;
            }
            inner.settings.tower.place = Some(place);
        }
        self.save_settings();
    }

    /// Changes the tower's settings; written and told to the windows only when something moved.
    fn change_tower(&self, change: impl FnOnce(&mut TowerSettings) -> Result<(), String>) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let mut tower = inner.settings.tower.clone();
            change(&mut tower)?;
            if tower == inner.settings.tower {
                return Ok(());
            }
            inner.settings.tower = tower;
        }

        self.save_settings();
        self.notify();
        Ok(())
    }
}
