//! Codex's accounts: the main one, Codex's own home (`~/.codex`, or `$CODEX_HOME` as Pitwall was
//! started with it), always there, and the others the user added, each a Codex home of its own
//! that the Codex sessions Pitwall starts are given (`CODEX_HOME`). Pitwall keeps an account's
//! name, its folder and two switches (`settings.agents.codex.accounts`); its sign-in stays where
//! Codex keeps it (the home's `auth.json`, or a Keychain item named from the home), never read:
//! only whether it is there is asked (`codex_usage::signed_in`).
//!
//! Unlike Claude's, nothing of the main account's sessions is linked into an account's home: two
//! homes writing one `sessions/` tree would race each other, so Pitwall follows each home's own.
//! "Use the main account's settings" links its `AGENTS.md` and `agents/` alone; `config.toml` is
//! never linked (Codex rewrites it, and it holds the MCP servers and the hooks' trust), nor its
//! `hooks.json` (each account has Pitwall's hook in its own, trusted on its own). Nothing is ever
//! written inside the main account's home, and only Pitwall's own links are ever taken out of
//! another's (`unlink`).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{overlaps, private, real, remove_link, same_folder, shown, slug, symlink, Prepared};
use crate::i18n::t;
use crate::settings::AccountSettings;

/// The variable that gives Codex its home.
pub const ENV: &str = "CODEX_HOME";
/// What an account's home links to the main account's with its settings shared: its instructions
/// and its agents. Skills are Codex's own to share (`~/.agents/skills`).
pub const SHARED: [&str; 2] = ["AGENTS.md", "agents"];

/// The folder a new account named `name` gets: `~/.codex-<name>`, with a number after it while
/// an account listed has that one.
pub fn new_dir(home: &Path, name: &str, accounts: &AccountSettings) -> PathBuf {
    let base = format!(".codex-{}", slug(name));
    (1..)
        .map(|n| home.join(if n == 1 { base.clone() } else { format!("{base}-{n}") }))
        .find(|dir| !accounts.others.iter().any(|account| same_folder(Path::new(&account.dir), dir)))
        .unwrap_or_else(|| home.join(base))
}

/// Whether `dir` can be the home of an account (not `except`): absolute, neither the main
/// account's home (`main`), inside it, nor holding it, and no other account's, nor one of `apart`
/// (Claude's), inside or holding one. `exists`: it must be there already (a folder picked), else
/// only its parent must.
pub fn check_dir(home: &Path, main: &Path, dir: &str, accounts: &AccountSettings, except: Option<&str>, exists: bool, apart: &[PathBuf]) -> Result<(), String> {
    let path = Path::new(dir);
    if !path.is_absolute() || dir.ends_with('/') || dir.ends_with('\\') || dir.chars().any(char::is_control) {
        return Err(t!("core.accounts.folderMissing", folder = dir));
    }
    let (real_dir, main) = (real(path), real(main));
    if real_dir == main {
        return Err(t!("core.accounts.folderMain"));
    }
    if real_dir.starts_with(&main) || main.starts_with(&real_dir) {
        return Err(t!("core.accounts.folderInside"));
    }
    if overlaps(&real_dir, apart) {
        return Err(t!("core.accounts.folderOther"));
    }
    if let Some(other) = accounts.others.iter().find(|account| Some(account.id.as_str()) != except && same_folder(Path::new(&account.dir), path)) {
        return Err(t!("core.accounts.folderTaken", name = other.name));
    }
    let there = if exists { path.is_dir() } else { path.is_dir() || path.parent().is_some_and(Path::is_dir) && !path.exists() };
    if !there {
        return Err(t!("core.accounts.folderMissing", folder = shown(home, dir)));
    }
    Ok(())
}

/// Sets up `dir` as an account's home: made when it isn't there (readable by the user alone:
/// Codex needs it there before it starts), then with `shared` a link to each of the main home's
/// `SHARED` that is there, where `dir` has nothing of that name. Nothing that is there is
/// replaced, and nothing is written into `main`.
pub fn prepare(main: &Path, dir: &Path, shared: bool) -> Result<Prepared, String> {
    let made = match fs::create_dir(dir) {
        Ok(()) => {
            private(dir);
            true
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists && dir.is_dir() => false,
        Err(error) => return Err(t!("core.accounts.folderMake", error = error)),
    };
    let mut prepared = Prepared { made, links: Vec::new() };
    if !shared {
        return Ok(prepared);
    }
    for name in SHARED {
        let (target, link) = (main.join(name), dir.join(name));
        if link.symlink_metadata().is_ok() || target.symlink_metadata().is_err() {
            continue;
        }
        if let Err(error) = symlink(&target, &link) {
            prepared.undo(dir);
            return Err(t!("core.accounts.folderMake", error = error));
        }
        prepared.links.push(link);
    }
    Ok(prepared)
}

/// Takes Pitwall's links out of `dir`: only a link named as one of `SHARED` that leads to the main
/// home's own of that name. Says how many went.
pub fn unlink(main: &Path, dir: &Path) -> usize {
    SHARED
        .iter()
        .filter(|name| {
            let link = dir.join(name);
            let ours = link.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink()) && fs::read_link(&link).is_ok_and(|to| to == main.join(name));
            ours && remove_link(&link).is_ok()
        })
        .count()
}

/// Whether the account whose home is `dir` shares the main account's settings: a link of
/// Pitwall's is there.
pub fn shares_main(main: &Path, dir: &Path) -> bool {
    SHARED.iter().any(|name| fs::read_link(dir.join(name)).is_ok_and(|to| to == main.join(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ClaudeAccount;

    /// An account's home is written into alone: the main account's home is as it was, its
    /// `sessions/` (and `config.toml`, `hooks.json`, `auth.json`) never reach the account, and
    /// taking the account's links out leaves the user's own. A link to the main sessions would
    /// have two Codex homes write one tree; one to `config.toml` would let Codex write the main
    /// account's MCP servers and trust from the other.
    #[test]
    fn a_codex_account_never_writes_into_the_main_home_or_gets_its_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let main = home.join(".codex");
        for dir in ["sessions/2026/11/02", "archived_sessions", "agents", "log"] {
            fs::create_dir_all(main.join(dir)).unwrap();
        }
        for file in ["config.toml", "hooks.json", "auth.json", "AGENTS.md", "session_index.jsonl", "sessions/2026/11/02/rollout-a.jsonl"] {
            fs::write(main.join(file), file).unwrap();
        }
        let before = listing(&main);
        let none = AccountSettings::default();
        let s = |path: &Path| path.to_string_lossy().into_owned();

        // Not the main home, nor inside it, nor holding it; nor Claude's folders (its main one, an
        // account's), where Codex would write its sign-in and sessions.
        let claude = [home.join(".claude"), home.join("claude-work")];
        fs::create_dir_all(home.join("claude-work/inner")).unwrap();
        for refused in [s(&main), s(&main.join("sessions")), s(&home), s(tmp.path()), "relative".into(), s(&home.join(".claude")), s(&home.join("claude-work/inner"))] {
            assert!(check_dir(&home, &main, &refused, &none, None, false, &claude).is_err(), "{refused}");
        }
        let dir = new_dir(&home, "İş", &none);
        assert_eq!(dir, home.join(".codex-is"));
        check_dir(&home, &main, &s(&dir), &none, None, false, &claude).unwrap();

        let made = prepare(&main, &dir, true).unwrap();
        assert!(made.made);
        assert_eq!(names(&dir), ["AGENTS.md", "agents"], "its instructions and agents only");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
        assert!(shares_main(&main, &dir));

        // Codex writes its own; the user puts a link of their own named as one of Pitwall's.
        fs::create_dir_all(dir.join("sessions")).unwrap();
        fs::write(dir.join("config.toml"), "mine").unwrap();
        assert_eq!(unlink(&main, &dir), 2);
        let elsewhere = home.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        symlink(&elsewhere, &dir.join("agents")).unwrap();
        assert_eq!(unlink(&main, &dir), 0);
        assert_eq!(names(&dir), ["agents", "config.toml", "sessions"]);
        assert!(!fs::symlink_metadata(dir.join("sessions")).unwrap().file_type().is_symlink());

        // Not shared: nothing of the main home at all.
        let plain = home.join(".codex-plain");
        prepare(&main, &plain, false).unwrap();
        assert!(names(&plain).is_empty());

        let taken = AccountSettings { others: vec![ClaudeAccount { id: "a".into(), name: "İş".into(), dir: s(&dir), shared: true, sync: true }], ..AccountSettings::default() };
        assert!(check_dir(&home, &main, &s(&dir), &taken, None, true, &[]).is_err(), "another account's");
        assert_eq!(new_dir(&home, "is", &taken), home.join(".codex-is-2"));
        assert_eq!(listing(&main), before, "the main account's home is as it was");
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    fn listing(dir: &Path) -> Vec<(String, bool, Option<String>)> {
        let mut all = Vec::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(at) = todo.pop() {
            for entry in fs::read_dir(&at).unwrap().flatten() {
                let path = entry.path();
                let link = path.symlink_metadata().unwrap().file_type().is_symlink();
                if path.is_dir() && !link {
                    todo.push(path.clone());
                }
                all.push((path.to_string_lossy().into_owned(), link, fs::read_to_string(&path).ok()));
            }
        }
        all.sort();
        all
    }
}
