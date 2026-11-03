//! Where Codex stands in each project, as `claude_state.rs` is for Claude: its watch followed or
//! let go as Codex is turned on or off, Pitwall's hook in it and its running sessions. CX1's file.
//!
//! While Codex is off nothing of it is read: no folder of Codex's is listed, no file opened or
//! watched, no process looked for; what its hook may still write into `codex-events/` is swept
//! unread once a day old.

use super::*;
use super::sessions::{lineage, terminal_app, Running};
use crate::codex::Tied;
use crate::codex_hooks::{CodexHook, CodexHookStatus};
use crate::process::ProcessInfo;

/// Codex's running sessions are read again at most this often, unless the hook told of one
/// starting or ending.
const CODEX_READ_EVERY: Duration = Duration::from_secs(30);

/// The sessions of `tied` whose Codex process runs, by thread id: the nearest of the hook's
/// parent and its parents that is a `codex` process. Under `--no-daemon` that is the session's
/// own TUI, whose parents reach the terminal it runs in; else the shared daemon, whose don't.
fn running_of(table: &HashMap<u32, ProcessInfo>, tied: &[Tied]) -> HashMap<String, Running> {
    let mut found = HashMap::new();
    for tie in tied {
        let chain = lineage(table, tie.ppid);
        let Some(at) = chain.iter().position(|pid| table.get(pid).is_some_and(process::is_codex)) else {
            continue;
        };
        let lineage = chain[at..].to_vec();
        let running = Running {
            pid: lineage[0],
            entrypoint: Some(if tie.vscode { "codex-vscode" } else { "cli" }.to_string()),
            cwd: tie.cwd.clone(),
            started_at: tie.started,
            status: None,
            status_at: None,
            app: terminal_app(table, &lineage),
            lineage,
        };
        found.insert(tie.id.clone(), running);
    }
    // A Codex process that isn't the shared server (`codex app-server`, as `is_codex_daemon` in
    // the radio tells it) runs one thread at a time: of those tied to it, the one started last
    // (a `/clear`, `/new` or `/resume` starts its thread anew; the one it left tells no end).
    let serves_many = |pid: u32| table.get(&pid).is_some_and(|process| process.line.split_whitespace().any(|word| word == "app-server"));
    let mut newest: HashMap<u32, Option<u64>> = HashMap::new();
    for running in found.values().filter(|running| !serves_many(running.pid)) {
        let top = newest.entry(running.pid).or_insert(running.started_at);
        *top = (*top).max(running.started_at);
    }
    found.retain(|_, running| newest.get(&running.pid).is_none_or(|top| running.started_at == *top));
    found
}

impl Core {
    /// Pitwall's hook in Codex's own home (`CoreConfig::codex_home`).
    fn codex_hook(&self) -> &CodexHook {
        self.codex_hook.get_or_init(|| CodexHook::new(&self.cfg.codex_home, &self.cfg.registry_dir))
    }

    /// Codex's watch follows the listed projects while Codex is on, and nothing at all while it is
    /// off (`agent_on`): no folder of it is looked into, no file watched.
    pub(super) fn follow_agents(&self) {
        self.follow_codex(&self.listed_paths());
        self.update_codex_hook_status();
        self.refresh_claude();
    }

    /// The Codex watch set to the homes followed now (none while Codex is off), read once more
    /// and its hook's events taken; its folders watched, or let go.
    pub(super) fn follow_codex(&self, paths: &[String]) {
        let homes = self.codex_homes();
        let legacy = self.claude.lock().map(|claude| claude.legacy_seen().to_vec()).unwrap_or_default();
        let on = !homes.is_empty();
        let events_dir = {
            let Ok(mut codex) = self.codex.lock() else {
                return;
            };
            codex.set_homes(homes);
            codex.set_legacy_seen(legacy);
            crate::agent::AgentWatch::set_listed(&mut *codex, paths);
            codex.events_dir().to_path_buf()
        };
        if on {
            // The events first: the rollouts of the sessions they tell of are looked for then.
            self.codex_take_events();
            if let Ok(mut codex) = self.codex.lock() {
                crate::agent::AgentWatch::rescan(&mut *codex);
            }
        } else {
            crate::codex::sweep_events(&events_dir);
            let mut cache = self.sessions_cache();
            cache.codex = None;
            cache.codex_at = None;
            cache.codex_held = None;
        }
        self.watch_codex();
    }

    /// The listed projects changed: Codex's watch follows them too, while Codex is on.
    pub(super) fn follow_codex_projects(&self, paths: &[String]) {
        if !self.agent_on(Agent::Codex) {
            return;
        }
        if let Ok(mut codex) = self.codex.lock() {
            if crate::agent::AgentWatch::set_listed(&mut *codex, paths) && crate::agent::AgentWatch::ready(&*codex) {
                crate::agent::AgentWatch::rescan(&mut *codex);
            }
        }
    }

    /// Codex's evaluation merged into Claude's (`ScanResult::merge`), while Codex is on and its
    /// watch has read once. Called with the `claude` watch locked; locks `codex`, then `inner`.
    /// The agents it merged.
    pub(super) fn merge_agents(&self, scan: &mut crate::claude::ScanResult) -> Vec<Agent> {
        let Ok(mut codex) = self.codex.lock() else {
            return Vec::new();
        };
        let watch: &mut dyn crate::agent::AgentWatch = &mut *codex;
        if self.agent_on(watch.agent()) && watch.ready() {
            scan.merge(watch.evaluate());
            return vec![watch.agent()];
        }
        Vec::new()
    }

    /// Codex's sessions whose process runs now, by session id, as `running_cached` has Claude's:
    /// its pid and parents (for a radio call's caller, a card's session, a tab), its folder, when
    /// it started. None while Codex is off or nothing could be read. Only the sessions the hook
    /// told of, with its parent, can be told: a session without the hook isn't in it, which says
    /// nothing of whether it runs.
    pub(super) fn codex_running(&self) -> Option<HashMap<String, Running>> {
        if !self.agent_on(Agent::Codex) {
            return None;
        }
        {
            let cache = self.sessions_cache();
            let due = cache.codex_stale || cache.codex_at.is_none_or(|at| at.elapsed() >= CODEX_READ_EVERY);
            if !due {
                return cache.codex.clone();
            }
        }

        let tied = self.codex.lock().map(|codex| codex.tied()).unwrap_or_default();
        let table = process::process_table();
        let mut running = if tied.is_empty() { Some(HashMap::new()) } else { table.as_ref().map(|table| running_of(table, &tied)) };
        // One under the shell of a tab that just went is ending with it: it runs no more.
        let gone = self.gone_shells();
        if let Some(running) = running.as_mut().filter(|_| !gone.is_empty()) {
            running.retain(|_, run| !run.lineage.iter().any(|pid| gone.contains(pid)));
        }
        // Whether a session none of them is may still run, as a click asks it (`codex_liveness`).
        let held = table.as_ref().zip(running.as_ref()).map(|(table, running)| (super::reveal::untied_codex(table, running, true), super::reveal::untied_codex(table, running, false)));

        let mut cache = self.sessions_cache();
        cache.codex = running.clone();
        cache.codex_held = held;
        cache.codex_at = Some(Instant::now());
        cache.codex_stale = false;
        running
    }

    /// The Codex sessions the hook tied to a process since Codex's watch began following them
    /// (this run, Codex on): only these tell by their absence from `codex_running` that they
    /// ended (the board's `stand`). One lost sight of (a restart, Codex off and on) tells nothing.
    pub(super) fn codex_tied_ids(&self) -> HashSet<String> {
        self.codex.lock().map(|codex| codex.tied().into_iter().map(|tie| tie.id).collect()).unwrap_or_default()
    }

    /// Takes the Codex hook's events in (`codex::take_events`); a session that started or ended
    /// has its process looked for again. Returns the listed projects where a turn just ended.
    pub(super) fn codex_take_events(&self) -> Vec<String> {
        let ended = crate::codex::take_events(&self.codex);
        if self.codex.lock().is_ok_and(|mut codex| codex.take_moved()) {
            self.sessions_cache().codex_stale = true;
        }
        ended
    }

    /// Tells the Codex watch which of its sessions' processes run (`codex_running`), before it is
    /// evaluated. Never with a watch locked: it may read the process table.
    pub(super) fn codex_set_running(&self) {
        if !self.agent_on(Agent::Codex) {
            return;
        }
        let running = self.codex_running().map(|running| running.into_keys().collect());
        if let Ok(mut codex) = self.codex.lock() {
            codex.set_running(running);
        }
    }

    /// When the earliest permission request still open in Codex's sessions of the project at
    /// `path` opened (`mark_seen`); none while Codex is off.
    pub(super) fn codex_open_prompt(&self, path: &str) -> Option<u64> {
        if !self.agent_on(Agent::Codex) {
            return None;
        }
        self.codex.lock().ok()?.open_prompt(path)
    }

    /// What Codex last said in session `id`, read from the end of its rollout when asked; none
    /// while Codex is off.
    pub(super) fn codex_last_message(&self, id: &str) -> Option<String> {
        if !self.agent_on(Agent::Codex) {
            return None;
        }
        let known = self.codex.lock().ok().and_then(|codex| codex.rollout_of(id));
        if let Some(file) = known {
            return crate::codex::message_in(&file);
        }
        self.codex_homes().iter().find_map(|home| crate::codex::last_message(home, id))
    }

    /// Codex's homes to put the hook in or take it out of, at the user's word: Codex's own and
    /// every added account's that is there, whether Codex is on or not (taken out while it is off,
    /// it leaves none behind in an account's home).
    fn codex_hook_homes(&self) -> Vec<PathBuf> {
        let others = self.codex_accounts().others.into_iter().map(|account| PathBuf::from(account.dir)).filter(|dir| dir.is_dir());
        std::iter::once(self.cfg.codex_home.clone()).chain(others).collect()
    }

    /// Installs Pitwall's hook into Codex's `hooks.json` (never `config.toml`, never a trust of
    /// its own: Codex runs it once the user trusted it in `/hooks`), or brings an older one up to
    /// date; in every Codex account's home too.
    pub fn install_codex_hook(&self) -> Result<(), String> {
        let mut result = Ok(());
        for home in self.codex_hook_homes() {
            let done = if home == self.cfg.codex_home { self.codex_hook().install() } else { CodexHook::new(&home, &self.cfg.registry_dir).install() };
            if result.is_ok() {
                result = done;
            }
        }
        self.update_codex_hook_status();
        self.notify();
        result
    }

    /// Asks Codex itself whether it trusts and runs Pitwall's hook in its own home (Settings ›
    /// Agents › Codex, "Check"; `CodexHook::ask_codex`): once it runs every entry, that is written
    /// down and the hook reads at work without waiting for an event. What Codex said, counted.
    pub fn check_codex_hook(&self) -> Result<crate::codex_hooks::CodexTrust, String> {
        if !self.agent_on(Agent::Codex) {
            return Err(t!("core.codex.askFailed"));
        }
        let codex = crate::accounts::codex_sync::codex_program().ok_or_else(|| t!("radio.missing.codex"))?;
        let hook = self.codex_hook();
        let trust = hook.ask_codex(&codex, &[])?;
        if trust.trusted == trust.total {
            hook.note_trusted();
        }
        if self.update_codex_hook_status() {
            self.notify();
        }
        Ok(trust)
    }

    /// Takes Pitwall's entries out of Codex's `hooks.json`, nothing else of it; out of every
    /// Codex account's home too.
    pub fn uninstall_codex_hook(&self) -> Result<(), String> {
        let mut result = Ok(());
        for home in self.codex_hook_homes() {
            let done = if home == self.cfg.codex_home { self.codex_hook().take_out() } else { CodexHook::new(&home, &self.cfg.registry_dir).take_out() };
            if result.is_ok() {
                result = done;
            }
        }
        if result.is_ok() {
            result = self.codex_hook().uninstall();
        }
        self.update_codex_hook_status();
        self.notify();
        result
    }

    /// Where Pitwall's hook stands in Codex's own home, for Settings: not installed, installed and
    /// waiting for the user's trust in Codex, or at work (`CodexWatch::hook_state`). Read only
    /// while Codex is on; off, it stays as last read.
    pub(super) fn codex_hook_status(&self) -> Option<CodexHookStatus> {
        if !self.agent_on(Agent::Codex) {
            return None;
        }
        let hook = self.codex_hook();
        let mut status = hook.status();
        if status.state != HookState::Missing {
            let changed = hook.changed_at();
            let heard_before = hook.heard_since_changed();
            status.state = if hook.trusted_since_changed() {
                // Codex itself said it runs every entry, whatever sessions began before that.
                HookState::Working
            } else {
                self.codex.lock().map(|codex| codex.hook_state(changed, heard_before)).unwrap_or(status.state)
            };
            if status.state == HookState::Working && !heard_before {
                hook.note_heard();
            }
        }
        Some(status)
    }

    /// The hook's state into what the snapshot carries; true when it changed.
    pub(super) fn update_codex_hook_status(&self) -> bool {
        let Some(status) = self.codex_hook_status() else {
            return false;
        };
        let heard = self.codex.lock().ok().and_then(|codex| codex.heard());
        let mut inner = self.lock();
        let changed = inner.codex_hook != status.state || inner.codex_hook_outdated != status.outdated || inner.codex_heard != heard;
        inner.codex_hook = status.state;
        inner.codex_hook_outdated = status.outdated;
        inner.codex_heard = heard;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::tests::{marked, write_rollouts, CLI, CLI_ID, LATEST, LATEST_ID, PROJECT};

    /// Every look into the Codex home under `root` (`codex::LOOKS`), from any test's thread.
    fn looks_under(root: &Path) -> Vec<PathBuf> {
        crate::codex::LOOKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().filter(|path| path.starts_with(root)).cloned().collect()
    }

    /// A user who turned Codex off has said Pitwall is not to read it: its rollouts, its names,
    /// its hook's file and its processes are neither read nor watched, whatever Pitwall does
    /// meanwhile, until it is turned on.
    #[test]
    fn an_agent_that_is_off_is_read_for_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex");
        write_rollouts(&home, &[(LATEST_ID, LATEST), (CLI_ID, CLI)]);
        fs::write(home.join("session_index.jsonl"), format!("{{\"id\":\"{CLI_ID}\",\"thread_name\":\"Login\"}}\n")).unwrap();
        fs::write(home.join("hooks.json"), "{}").unwrap();
        let events = tmp.path().join("registry").join("codex-events");
        fs::create_dir_all(&events).unwrap();
        crate::hooks::write_event(&events, &format!("event=SessionStart\nsession_id={CLI_ID}\ncwd={PROJECT}\nppid=4242\n")).unwrap();
        assert!(marked(LATEST).contains(crate::codex::tests::SECRET));

        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude").join("projects"),
                codex_home: home.clone(),
                settings_file: tmp.path().join("config").join("settings.json"),
                claude_settings: tmp.path().join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        core.lock().settings.agents.of_mut(Agent::Codex).on = Some(false);

        core.poll_files();
        core.look_for_agents();
        core.follow_agents();
        core.refresh_claude();
        core.mark_seen(PROJECT);
        let view = core.sessions_view(&HashMap::new());
        assert_eq!(core.last_message(CLI_ID), None);
        assert_eq!(core.codex_running(), None);
        assert!(!core.update_codex_hook_status());

        assert!(!core.agent_on(Agent::Codex));
        assert!(view.sessions.iter().all(|row| row.agent == Agent::Claude));
        assert!(core.lock().scanned.iter().all(|session| session.agent == Agent::Claude));
        assert_eq!(looks_under(&home), Vec::<PathBuf>::new());

        // On: read and followed.
        core.lock().settings.agents.of_mut(Agent::Codex).on = Some(true);
        core.agents_changed();
        assert!(!looks_under(&home).is_empty());
        let scanned: Vec<String> = core.lock().scanned.iter().filter(|session| session.agent == Agent::Codex).map(|session| session.id.clone()).collect();
        assert!(scanned.contains(&CLI_ID.to_string()) && scanned.contains(&LATEST_ID.to_string()), "{scanned:?}");
    }
}
