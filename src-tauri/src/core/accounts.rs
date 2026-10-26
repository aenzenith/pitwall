//! Claude's accounts in the core (`crate::accounts`): their settings, changed through their own
//! commands only; the account a tab put back runs with; and how many Claude sessions run with
//! each.

use super::*;
use crate::accounts::{self, MAIN};
use crate::settings::AccountSettings;

impl Core {
    /// Claude's accounts as the settings hold them.
    pub fn accounts(&self) -> AccountSettings {
        self.lock().settings.accounts.clone()
    }

    /// Changes the accounts through `change` and writes the settings; an error from `change`
    /// leaves them as they were.
    pub fn change_accounts<T>(&self, change: impl FnOnce(&mut AccountSettings) -> Result<T, String>) -> Result<T, String> {
        let value = {
            let mut inner = self.lock();
            let mut accounts = inner.settings.accounts.clone();
            let value = change(&mut accounts)?;
            if accounts == inner.settings.accounts {
                return Ok(value);
            }
            inner.settings.accounts = accounts;
            value
        };
        self.save_settings();
        self.notify();
        Ok(value)
    }

    /// The account a tab of a record (a quit's, an update's) comes back with: the one it ran with
    /// while that one is listed and its folder there, else the main one; never the one in use
    /// for a tab that ran with the main one.
    pub(super) fn account_back(&self, recorded: Option<&str>) -> String {
        let inner = self.lock();
        match recorded.map(|id| accounts::start_with(&inner.settings.accounts, Some(id))) {
            Some(Ok(Some(account))) => account.id.clone(),
            _ => MAIN.to_string(),
        }
    }

    /// How many Claude sessions run with each account now, by its id: a session under a tab of
    /// Pitwall's that runs with another account is that one's, every other the main one's.
    pub fn claudes_by_account(&self) -> HashMap<String, usize> {
        let Some(running) = self.running_cached() else {
            return HashMap::new();
        };
        let shells = self.terminal_shells();
        let tabs: HashMap<u64, Option<String>> = self.lock().terminal_list.iter().map(|view| (view.id, view.account.clone())).collect();

        let mut counts: HashMap<String, usize> = HashMap::new();
        for run in running.values() {
            let account = run
                .lineage
                .iter()
                .find_map(|pid| shells.iter().find(|(shell, _, _)| shell == pid))
                .and_then(|(_, terminal, _)| tabs.get(terminal).cloned().flatten())
                .unwrap_or_else(|| MAIN.to_string());
            *counts.entry(account).or_default() += 1;
        }
        counts
    }
}

impl Core {
    /// Whether Pitwall's hook is in the main account's settings.
    pub fn main_hook_installed(&self) -> bool {
        self.hook.status().installed
    }

    /// Pitwall's hook in the own settings of the account whose folder is `dir`: installed
    /// (`install`) or taken out of them, the script left for the others that run it. Nothing is
    /// done to the settings of an account that are the main one's (a link to them), nor to an
    /// account whose folder is gone (it isn't made again for them): `Ok(false)`.
    pub fn account_hook(&self, dir: &str, install: bool) -> Result<bool, String> {
        if !Path::new(dir).is_dir() || self.shares_main_settings(dir) {
            return Ok(false);
        }
        let hook = crate::hooks::ClaudeHook::new(Path::new(dir).join("settings.json"), &self.cfg.registry_dir);
        if install {
            hook.install()?;
        } else {
            hook.take_out()?;
        }
        Ok(true)
    }

    /// Whether the settings file in the account folder `dir` is the main account's (a link to it).
    pub fn shares_main_settings(&self, dir: &str) -> bool {
        let file = Path::new(dir).join("settings.json");
        let real = |path: &Path| fs::canonicalize(path).ok();
        file.symlink_metadata().is_ok() && real(&file).is_some_and(|file| Some(file) == real(&self.cfg.claude_settings))
    }

    /// The hook in the own settings of every account whose settings aren't the main one's (one
    /// set to share them whose folder had a settings file of its own keeps that one).
    pub(super) fn account_hooks(&self, install: bool) {
        for account in self.accounts().others {
            let _ = self.account_hook(&account.dir, install);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taking the hook out of an account's own settings must leave the script the main account's
    /// settings run: without it every hook event fails, silently, until the hook is set up again.
    #[test]
    fn an_accounts_hook_taken_out_leaves_the_main_ones_script() {
        let tmp = tempfile::tempdir().unwrap();
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude").join("projects"),
                settings_file: tmp.path().join("config").join("settings.json"),
                claude_settings: tmp.path().join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        core.install_claude_hook().unwrap();
        let script = tmp.path().join("registry/hooks/claude-hook.sh");
        let account = tmp.path().join("account");
        fs::create_dir_all(&account).unwrap();
        let account = account.to_str().unwrap();

        assert!(core.account_hook(account, true).unwrap());
        core.account_hook(account, false).unwrap();

        assert!(!fs::read_to_string(tmp.path().join("account/settings.json")).unwrap().contains("claude-hook"));
        assert!(fs::read_to_string(tmp.path().join("claude/settings.json")).unwrap().contains("claude-hook.sh"));
        assert!(script.exists());
    }
}
