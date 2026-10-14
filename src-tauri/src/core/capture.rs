//! A card written from the quick switcher (`+`): which project it most likely belongs to.
//! Nothing new is read for it: the answer is the seen book's (`claude-seen.json`), which the
//! app and the extension already write whenever a project is looked at.

use super::*;

impl Core {
    /// The listed project looked at last (opened from Pitwall, its Claude turn marked seen, its
    /// window brought up by the extension); none when no listed project was ever looked at. It
    /// reads the seen book's file, so not on the main thread.
    pub fn last_looked(&self) -> Option<String> {
        let paths = self.listed_paths();
        self.claude.lock().ok()?.last_seen(&paths)
    }
}
