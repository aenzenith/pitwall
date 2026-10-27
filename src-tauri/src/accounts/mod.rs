//! Claude's accounts: the main one, Claude Code's own folder (`~/.claude`), always there, and the
//! others the user added, each a Claude Code configuration folder of its own that the Claude
//! sessions Pitwall starts are given (`CLAUDE_CONFIG_DIR`). Pitwall keeps an account's name, its
//! folder and two switches (`settings::AccountSettings`); its sign-in stays where Claude Code keeps
//! it, in a Keychain item of its own, read per request as the main one's is (`usage.rs`), never
//! stored, refreshed or logged.
//!
//! A folder Pitwall sets up holds links to the main account's sessions and logs (`ALWAYS`), so
//! whatever Pitwall follows of Claude it follows for every account, and with "Use the main
//! account's settings" links to its settings too (`SHARED`). Nothing is ever written inside the
//! main account's folder, and only Pitwall's own links are ever taken out of another's (`unlink`).

pub mod app;
pub mod login;
pub mod sync;

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::BuildHasher;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::i18n::t;
use crate::settings::{AccountSettings, ClaudeAccount};
use crate::usage::UsageState;

/// The main account's id, wherever an account is named by one.
pub const MAIN: &str = "main";
/// The variable that gives Claude Code its configuration folder.
pub const ENV: &str = "CLAUDE_CONFIG_DIR";
/// What a folder Pitwall sets up always links to the main account's: Claude Code's running
/// sessions and their logs (with the projects' memory), which Pitwall follows, and the editors'
/// lock files, by which Claude finds the editor it runs beside.
pub const ALWAYS: [&str; 3] = ["projects", "sessions", "ide"];
/// What it links to as well with the main account's settings shared: its instructions, settings
/// (and so the hooks in them), skills, agents, commands, output styles, plugins and keys. Never
/// its sign-in, its `.claude.json` (sign-in and MCP servers) or any running Claude's own state.
pub const SHARED: [&str; 8] = ["CLAUDE.md", "settings.json", "skills", "agents", "commands", "output-styles", "plugins", "keybindings.json"];

/// The Keychain item Claude Code keeps the sign-in of the account in `dir` in (none: the main
/// one). Claude Code names it from the variable's value, as given: `-` and the first 8 hex
/// digits of its SHA-256 after the main one's name. A main account given its folder through the
/// variable would have the suffix too, so the main one is never started with it.
pub fn keychain_service(dir: Option<&str>) -> String {
    const MAIN_SERVICE: &str = "Claude Code-credentials";
    match dir {
        None => MAIN_SERVICE.to_string(),
        Some(dir) => {
            let digest = Sha256::digest(dir.as_bytes());
            let hex: String = digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect();
            format!("{MAIN_SERVICE}-{hex}")
        }
    }
}

/// Where Claude Code keeps an account's sign-in when there is no Keychain (Linux, Windows) or it
/// couldn't use it.
pub fn credentials_file(home: &Path, dir: Option<&str>) -> PathBuf {
    match dir {
        None => main_dir(home).join(".credentials.json"),
        Some(dir) => Path::new(dir).join(".credentials.json"),
    }
}

/// The main account's folder.
pub fn main_dir(home: &Path) -> PathBuf {
    home.join(".claude")
}

/// `path` as the user reads it: the home folder as `~`.
pub fn shown(home: &Path, path: &str) -> String {
    match Path::new(path).strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string(),
    }
}

/// A name as a folder's: lower-case letters and digits, the rest dashes, a letter with a mark
/// as the letter alone (`Kişisel` → `kisisel`). `account` when nothing is left.
fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'a'..='z' | '0'..='9' => Some(c),
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => Some('a'),
            'ç' | 'ć' | 'č' => Some('c'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => Some('e'),
            'ğ' => Some('g'),
            'ì' | 'í' | 'î' | 'ï' | 'ı' | 'ī' => Some('i'),
            'ñ' | 'ń' | 'ň' => Some('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => Some('o'),
            'ś' | 'š' | 'ş' | 'ß' => Some('s'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => Some('u'),
            'ý' | 'ÿ' => Some('y'),
            'ź' | 'ż' | 'ž' => Some('z'),
            'ł' => Some('l'),
            'ř' => Some('r'),
            'ť' => Some('t'),
            'ď' => Some('d'),
            // A mark that came apart from its letter (`İ` lower-cased is `i` and a dot above).
            '\u{300}'..='\u{36f}' => continue,
            _ => None,
        };
        match plain {
            Some(c) => slug.push(c),
            None if !slug.is_empty() && !slug.ends_with('-') => slug.push('-'),
            None => {}
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "account".into()
    } else {
        slug.chars().take(40).collect::<String>().trim_end_matches('-').to_string()
    }
}

/// The folder a new account named `name` gets: `~/.claude-<name>`, with a number after it while
/// an account listed has that one.
pub fn new_dir(home: &Path, name: &str, accounts: &AccountSettings) -> PathBuf {
    let base = format!(".claude-{}", slug(name));
    (1..)
        .map(|n| home.join(if n == 1 { base.clone() } else { format!("{base}-{n}") }))
        .find(|dir| !accounts.others.iter().any(|account| same_folder(Path::new(&account.dir), dir)))
        .unwrap_or_else(|| home.join(base))
}

/// Whether two folders are one, links followed where they are there.
fn same_folder(a: &Path, b: &Path) -> bool {
    real(a) == real(b)
}

/// `path` with links and `..` followed as far as it is there.
fn real(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `dir` can be the folder of an account (not `except`): absolute and written in plain
/// ASCII (Claude Code names the Keychain item from the normalized characters, which only plain
/// ASCII leaves as they are), neither the main account's folder, inside it, nor holding it (the
/// home folder would make the main account's `~/.claude.json` this one's), and no other account's.
/// `exists`: it must be there already (a folder picked), else only its parent must.
pub fn check_dir(home: &Path, dir: &str, accounts: &AccountSettings, except: Option<&str>, exists: bool) -> Result<(), String> {
    let path = Path::new(dir);
    if !path.is_absolute() || dir.ends_with('/') || dir.ends_with('\\') {
        return Err(t!("core.accounts.folderMissing", folder = dir));
    }
    // On macOS Claude Code names the account's Keychain item from the path's letters, which the
    // file system may hand back composed otherwise.
    if (cfg!(target_os = "macos") && !dir.is_ascii()) || dir.chars().any(char::is_control) {
        return Err(t!("core.accounts.folderAscii"));
    }
    let (real_dir, main) = (real(path), real(&main_dir(home)));
    if real_dir == main {
        return Err(t!("core.accounts.folderMain"));
    }
    if real_dir.starts_with(&main) || main.starts_with(&real_dir) {
        return Err(t!("core.accounts.folderInside"));
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

/// A name for an account: trimmed, not empty, no other account's (any letter case).
pub fn check_name(name: &str, accounts: &AccountSettings, except: Option<&str>) -> Result<String, String> {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(60).collect();
    if name.is_empty() {
        return Err(t!("core.accounts.nameEmpty"));
    }
    let taken = |other: &str| other.trim().to_lowercase() == name.to_lowercase();
    let main_taken = except != Some(MAIN) && accounts.main_name.as_deref().is_some_and(taken);
    if main_taken || accounts.others.iter().any(|account| Some(account.id.as_str()) != except && taken(&account.name)) {
        return Err(t!("core.accounts.nameTaken", name = name));
    }
    Ok(name)
}

/// A new account's id: 8 hex digits no account has.
pub fn new_id(accounts: &AccountSettings) -> String {
    loop {
        let id = format!("{:08x}", RandomState::new().hash_one(std::time::SystemTime::now()) as u32);
        if id != MAIN && !accounts.others.iter().any(|account| account.id == id) {
            return id;
        }
    }
}

/// What `prepare` made, for taking it back when the account isn't added after all.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Prepared {
    /// The folder was made.
    pub made: bool,
    /// The links made in it.
    pub links: Vec<PathBuf>,
}

impl Prepared {
    /// Takes back what was made: the links (while they are links still), then the folder if it
    /// is empty. Anything Claude Code wrote there meanwhile stays, and so the folder.
    pub fn undo(&self, dir: &Path) {
        for link in &self.links {
            if link.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink()) {
                let _ = remove_link(link);
            }
        }
        if self.made {
            let _ = fs::remove_dir(dir);
        }
    }
}

/// Sets up `dir` for an account: made when it isn't there (readable by the user alone), then a
/// link to each of the main account's `ALWAYS` (and with `shared` its `SHARED`) that is there,
/// where `dir` has nothing of that name. Nothing that is there is replaced.
pub fn prepare(home: &Path, dir: &Path, shared: bool) -> Result<Prepared, String> {
    let made = match fs::create_dir(dir) {
        Ok(()) => {
            private(dir);
            true
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists && dir.is_dir() => false,
        Err(error) => return Err(t!("core.accounts.folderMake", error = error)),
    };
    let mut prepared = Prepared { made, links: Vec::new() };
    let main = main_dir(home);
    let wanted = ALWAYS.iter().chain(if shared { SHARED.iter() } else { [].iter() });
    for name in wanted {
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

/// Takes Pitwall's links out of `dir`: only a link named as one of `ALWAYS` or `SHARED` that
/// leads to the main account's own of that name. Says how many went.
pub fn unlink(home: &Path, dir: &Path) -> usize {
    let main = main_dir(home);
    ALWAYS
        .iter()
        .chain(SHARED.iter())
        .filter(|name| {
            let link = dir.join(name);
            let ours = link.symlink_metadata().is_ok_and(|meta| meta.file_type().is_symlink()) && fs::read_link(&link).is_ok_and(|to| to == main.join(name));
            ours && remove_link(&link).is_ok()
        })
        .count()
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(unix)]
fn remove_link(link: &Path) -> std::io::Result<()> {
    fs::remove_file(link)
}

/// A link to a folder is a folder's on Windows.
#[cfg(windows)]
fn remove_link(link: &Path) -> std::io::Result<()> {
    fs::remove_file(link).or_else(|_| fs::remove_dir(link))
}

#[cfg(unix)]
fn private(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn private(_dir: &Path) {}

/// The account `id` names: none for the main one, an id not listed, or one whose folder isn't
/// there.
pub fn listed<'a>(accounts: &'a AccountSettings, id: &str) -> Option<&'a ClaudeAccount> {
    accounts.others.iter().find(|account| account.id == id)
}

/// The account a Claude session starts with: `id`, else the one in use. `Ok(None)`: the main one.
/// The one in use whose folder has gone is the main one (it can't be told apart from the main
/// account then, which its tab shows); `id` named for it is refused.
pub fn start_with<'a>(accounts: &'a AccountSettings, id: Option<&str>) -> Result<Option<&'a ClaudeAccount>, String> {
    match id {
        Some(MAIN) => Ok(None),
        Some(id) => {
            let account = listed(accounts, id).ok_or_else(|| t!("core.accounts.unknown"))?;
            if !Path::new(&account.dir).is_dir() {
                return Err(t!("core.accounts.missing", name = account.name));
            }
            Ok(Some(account))
        }
        None => Ok(accounts.in_use.as_deref().and_then(|id| listed(accounts, id)).filter(|account| Path::new(&account.dir).is_dir())),
    }
}

/// The main account's name as shown.
pub fn main_name(accounts: &AccountSettings) -> String {
    accounts.main_name.clone().unwrap_or_else(|| t!("accounts.main"))
}

/// The name of the account `id` (none: the main one) as shown.
pub fn name_of(accounts: &AccountSettings, id: Option<&str>) -> String {
    match id.and_then(|id| listed(accounts, id)) {
        Some(account) => account.name.clone(),
        None => main_name(accounts),
    }
}

/// An account as Settings and the Fuel page show it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    /// None: the main one with no name of the user's.
    pub name: Option<String>,
    pub main: bool,
    pub dir: String,
    pub shared: bool,
    pub sync: bool,
    pub in_use: bool,
    pub missing: bool,
    pub limits: UsageState,
    pub running: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts(others: &[(&str, &str, &str)]) -> AccountSettings {
        AccountSettings {
            main_name: None,
            others: others.iter().map(|(id, name, dir)| ClaudeAccount { id: (*id).into(), name: (*name).into(), dir: (*dir).into(), shared: true, sync: true }).collect(),
            in_use: None,
        }
    }

    /// Claude Code 2.1 names an account's Keychain item `Claude Code-credentials-` and the first
    /// 8 hex digits of the SHA-256 of `CLAUDE_CONFIG_DIR` as given; the main one, with no
    /// variable, has no suffix. A wrong name reads another account's sign-in or none: the values
    /// are the digest's, worked out apart (`printf %s DIR | shasum -a 256`).
    #[test]
    fn the_keychain_item_is_named_as_claude_code_names_it() {
        assert_eq!(keychain_service(None), "Claude Code-credentials");
        assert_eq!(keychain_service(Some("/Users/me/.claude-work")), "Claude Code-credentials-1e91dd84");
        assert_eq!(keychain_service(Some("/Users/me/.claude")), "Claude Code-credentials-0bd83d07");
        // The value as given: a trailing slash is another item.
        assert_ne!(keychain_service(Some("/Users/me/.claude-work/")), keychain_service(Some("/Users/me/.claude-work")));
    }

    #[test]
    fn a_new_accounts_folder_is_named_from_its_name() {
        let home = Path::new("/Users/me");
        let none = accounts(&[]);
        assert_eq!(new_dir(home, "Kişisel", &none), home.join(".claude-kisisel"));
        assert_eq!(new_dir(home, "  Müşteri İş  ", &none), home.join(".claude-musteri-is"));
        assert_eq!(new_dir(home, "Работа", &none), home.join(".claude-account"));
        let taken = accounts(&[("a", "İş", "/Users/me/.claude-is")]);
        assert_eq!(new_dir(home, "is", &taken), home.join(".claude-is-2"));
    }

    /// An account's folder given as the home folder makes Claude Code write the main account's
    /// own `~/.claude.json` (sign-in and MCP servers); one inside `~/.claude` mixes it with the
    /// main one's files. Both are refused, as is a folder Claude would name another Keychain item
    /// for than Pitwall reads.
    #[test]
    fn a_folder_that_would_mix_with_the_main_accounts_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join(".claude/inner")).unwrap();
        fs::create_dir_all(home.join("work")).unwrap();
        let mine = accounts(&[("a", "Work", &home.join("work").to_string_lossy())]);
        let s = |p: &Path| p.to_string_lossy().into_owned();

        for refused in [s(&home), s(&home.join(".claude")), s(&home.join(".claude/inner")), s(tmp.path()), s(&home.join("work")), "relative/dir".into(), format!("{}/", s(&home.join("other")))] {
            assert!(check_dir(&home, &refused, &mine, None, false).is_err(), "{refused}");
        }
        assert_eq!(check_dir(&home, &s(&home.join("işler")), &mine, None, false).is_err(), cfg!(target_os = "macos"), "Keychain item named from the letters");
        assert!(check_dir(&home, &s(&home.join(".claude-new")), &mine, None, false).is_ok());
        assert!(check_dir(&home, &s(&home.join(".claude-new")), &mine, None, true).is_err(), "picked, it must be there");
        assert!(check_dir(&home, &s(&home.join("work")), &mine, Some("a"), true).is_ok(), "its own folder");
    }

    /// Setting a folder up writes only into it, never into the main account's; what is there is
    /// never replaced; taking the links out leaves everything but Pitwall's own links.
    #[test]
    fn links_go_into_the_accounts_folder_and_come_out_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let main = main_dir(home);
        fs::create_dir_all(main.join("projects")).unwrap();
        fs::create_dir_all(main.join("sessions")).unwrap();
        fs::create_dir_all(main.join("skills")).unwrap();
        fs::write(main.join("settings.json"), "{}").unwrap();
        fs::write(main.join("CLAUDE.md"), "main").unwrap();
        let before = listing(&main);

        let dir = home.join(".claude-work");
        assert!(prepare(home, &dir, false).unwrap().made);
        assert_eq!(names(&dir), ["projects", "sessions"], "only what is always shared");
        unlink(home, &dir);

        // The user's own CLAUDE.md stays theirs; the rest of the settings link.
        fs::write(dir.join("CLAUDE.md"), "mine").unwrap();
        assert!(!prepare(home, &dir, true).unwrap().made);
        assert_eq!(names(&dir), ["CLAUDE.md", "projects", "sessions", "settings.json", "skills"]);
        assert_eq!(fs::read_to_string(dir.join("CLAUDE.md")).unwrap(), "mine");
        assert!(fs::symlink_metadata(dir.join("settings.json")).unwrap().file_type().is_symlink());

        // A link of the user's own, to elsewhere, stays, though it is named as one of Pitwall's.
        // Made under a name nothing had: on Windows a name just taken out may still be held a
        // moment (a link being looked at by Defender), and no new link takes it meanwhile.
        let elsewhere = home.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        symlink(&elsewhere, &dir.join("agents")).unwrap();
        assert_eq!(unlink(home, &dir), 4);
        assert_eq!(names(&dir), ["CLAUDE.md", "agents"]);
        assert_eq!(listing(&main), before, "the main account's folder is as it was");
    }

    /// A sign-in that didn't end well takes back what was made for it, and only that.
    #[test]
    fn an_account_not_added_leaves_its_folder_as_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        fs::create_dir_all(main_dir(home).join("projects")).unwrap();
        fs::create_dir_all(main_dir(home).join("sessions")).unwrap();

        let new = home.join(".claude-new");
        prepare(home, &new, true).unwrap().undo(&new);
        assert!(!new.exists(), "a folder made for it goes");

        let theirs = home.join("theirs");
        fs::create_dir_all(theirs.join("sessions")).unwrap();
        fs::write(theirs.join(".claude.json"), "{}").unwrap();
        prepare(home, &theirs, true).unwrap().undo(&theirs);
        assert_eq!(names(&theirs), [".claude.json", "sessions"], "a folder picked is as it was");
    }

    #[test]
    fn the_account_a_claude_starts_with() {
        let tmp = tempfile::tempdir().unwrap();
        let there = tmp.path().to_string_lossy().into_owned();
        let mut set = accounts(&[("a", "Work", &there), ("b", "Gone", "/no/such/folder")]);
        assert_eq!(start_with(&set, None).unwrap(), None);
        set.in_use = Some("a".into());
        assert_eq!(start_with(&set, None).unwrap().map(|a| a.id.as_str()), Some("a"));
        assert_eq!(start_with(&set, Some(MAIN)).unwrap(), None);
        set.in_use = Some("b".into());
        assert_eq!(start_with(&set, None).unwrap(), None, "in use with its folder gone: the main one");
        assert!(start_with(&set, Some("b")).is_err());
        assert!(start_with(&set, Some("zz")).is_err());
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    fn listing(dir: &Path) -> Vec<(String, Option<String>)> {
        let mut all = Vec::new();
        for entry in walk(dir) {
            let text = fs::read_to_string(&entry).ok();
            all.push((entry.to_string_lossy().into_owned(), text));
        }
        all.sort();
        all
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut found = vec![dir.to_path_buf()];
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            } else {
                found.push(path);
            }
        }
        found
    }
}
