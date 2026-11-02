//! Codex's accounts, as `core/accounts.rs` is Claude's: each a `CODEX_HOME` of its own
//! (`settings.agents.codex.accounts`), the main one Codex's own home (`CoreConfig::codex_home`),
//! which never gets the variable. CX5's file.

use super::*;
use crate::accounts::{self, MAIN};
use crate::codex_hooks::CodexHook;
use crate::settings::AccountSettings;

impl Core {
    /// Every Codex home to follow, the main one first, then each added account's that is there:
    /// what CX1's watch reads and where Pitwall's hook goes. None while Codex is off.
    pub(super) fn codex_homes(&self) -> Vec<PathBuf> {
        self.codex_account_homes().into_iter().map(|(_, home)| home).collect()
    }

    /// `codex_homes` with each one's account id (`accounts::MAIN` for the main one): what the
    /// Fuel page reads the limits and tokens of. None while Codex is off.
    pub fn codex_account_homes(&self) -> Vec<(String, PathBuf)> {
        if !self.agent_on(Agent::Codex) {
            return Vec::new();
        }
        let others = self.codex_accounts().others.into_iter().map(|account| (account.id, PathBuf::from(account.dir))).filter(|(_, dir)| dir.is_dir());
        std::iter::once((MAIN.to_string(), self.cfg.codex_home.clone())).chain(others).collect()
    }

    /// The main account's home.
    pub fn codex_main_home(&self) -> PathBuf {
        self.cfg.codex_home.clone()
    }

    /// Codex's folders while Codex is on, its main home first: no Claude account may have one,
    /// be inside one or hold one (`accounts::check_dir`). None while it is off, as before Codex.
    pub fn codex_folders(&self) -> Vec<PathBuf> {
        if !self.agent_on(Agent::Codex) {
            return Vec::new();
        }
        accounts::folders_of(self.cfg.codex_home.clone(), &self.codex_accounts())
    }

    /// The Codex account a session starts with, as `accounts::start_with` is for Claude: the one
    /// `account` names (`accounts::MAIN` for the main one), else the one in use; its id and the
    /// folder its tab is given as `CODEX_HOME`, none for the main one (which never gets it).
    pub(super) fn codex_start_with(&self, account: Option<&str>) -> Result<Option<(String, PathBuf)>, String> {
        let accounts = self.codex_accounts();
        Ok(accounts::start_with(&accounts, account)?.map(|account| (account.id.clone(), PathBuf::from(&account.dir))))
    }

    /// Codex's accounts as the settings hold them.
    pub fn codex_accounts(&self) -> AccountSettings {
        self.lock().settings.agents.codex.accounts.clone()
    }

    /// Changes Codex's accounts through `change` and writes the settings; an error from `change`
    /// leaves them as they were.
    pub fn change_codex_accounts<T>(&self, change: impl FnOnce(&mut AccountSettings) -> Result<T, String>) -> Result<T, String> {
        let (value, moved) = {
            let mut inner = self.lock();
            let mut accounts = inner.settings.agents.codex.accounts.clone();
            let value = change(&mut accounts)?;
            if accounts == inner.settings.agents.codex.accounts {
                return Ok(value);
            }
            let dirs = |set: &AccountSettings| set.others.iter().map(|account| account.dir.clone()).collect::<Vec<_>>();
            let moved = dirs(&accounts) != dirs(&inner.settings.agents.codex.accounts);
            inner.settings.agents.codex.accounts = accounts;
            (value, moved)
        };
        self.save_settings();
        // An account added or removed is followed, or let go, at once (`codex_homes`).
        if moved {
            self.follow_codex(&self.listed_paths());
        }
        self.notify();
        Ok(value)
    }

    /// How many Codex sessions run with each account now, by its id: a session under a tab of
    /// Pitwall's that runs with another account is that one's, every other the main one's.
    pub fn codex_by_account(&self) -> HashMap<String, usize> {
        let Some(running) = self.codex_running() else {
            return HashMap::new();
        };
        let shells = self.terminal_shells();
        let tabs: HashMap<u64, Option<String>> = self.lock().terminal_list.iter().filter(|view| view.kind == Agent::Codex.tab_kind()).map(|view| (view.id, view.account.clone())).collect();

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

    /// Whether Codex may run with the account `id` now: a session of it the hook told of
    /// (`codex_by_account`), or a tab of Pitwall's started with it where Codex may run
    /// (`agent_may_run_in`), which alone tells of one while the account's hook isn't trusted yet
    /// or isn't there.
    pub fn codex_account_runs(&self, id: &str) -> bool {
        if self.codex_by_account().get(id).is_some_and(|count| *count > 0) {
            return true;
        }
        let tabs: Vec<u64> = self.lock().terminal_list.iter().filter(|view| view.kind == Agent::Codex.tab_kind() && view.account.as_deref() == Some(id)).map(|view| view.id).collect();
        tabs.into_iter().any(|tab| self.agent_may_run_in(tab))
    }

    /// Whether Pitwall's hook is in the main Codex home's `hooks.json`.
    pub fn codex_main_hook_installed(&self) -> bool {
        CodexHook::new(&self.cfg.codex_home, &self.cfg.registry_dir).status().state != HookState::Missing
    }

    /// Pitwall's hook in the `hooks.json` of the Codex account whose home is `dir`: installed or
    /// taken out (Pitwall's entries only; the script stays, the main home's and the others' hook
    /// run it too). Codex runs it once the user trusted it in that account's `/hooks`. Nothing is
    /// done to a home that is gone (it isn't made again for it): `Ok(false)`.
    pub fn codex_account_hook(&self, dir: &str, install: bool) -> Result<bool, String> {
        if !Path::new(dir).is_dir() {
            return Ok(false);
        }
        let hook = CodexHook::new(Path::new(dir), &self.cfg.registry_dir);
        if install {
            hook.install()?;
        } else {
            hook.take_out()?;
        }
        Ok(true)
    }
}
