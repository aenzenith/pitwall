//! Pitwall's own files about chats and sessions, beside the settings and readable by the user
//! alone: the bookmarks of chats (`chat-bookmarks.json`, O11: a session's id to the uuids of its
//! marked messages, no text) and the Sessions page's archive, groups and unread marks
//! (`sessions-meta.json`, O10: session ids and the names the user gave the groups; in backups).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::i18n::t;
use crate::start::{is_uuid, new_uuid};

pub const BOOKMARKS: &str = "chat-bookmarks.json";
pub const SESSIONS_META: &str = "sessions-meta.json";

/// One change at a time to either file: read, changed and written whole.
static WRITING: Mutex<()> = Mutex::new(());

/// The most bookmarks a session keeps, groups the page keeps, and characters a group's name has.
const BOOKMARKS_MAX: usize = 500;
const GROUPS_MAX: usize = 200;
const NAME_MAX: usize = 100;

fn refused() -> String {
    t!("chatCore.error.refused")
}

fn write(file: &Path, json: &str) -> Result<(), String> {
    crate::core::write_private(file, json).map_err(|error| t!("chatCore.error.write", error = error))
}

/// The bookmarks file as it is, its entries that aren't ids left out.
fn read_bookmarks(file: &Path) -> BTreeMap<String, Vec<String>> {
    let read: BTreeMap<String, Vec<String>> = fs::read(file).ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default();
    read.into_iter()
        .filter(|(session, _)| is_uuid(session))
        .map(|(session, messages)| (session, messages.into_iter().filter(|message| is_uuid(message)).collect::<Vec<_>>()))
        .filter(|(_, messages)| !messages.is_empty())
        .collect()
}

/// The messages of `session` the user marked, in the order they were marked.
pub fn bookmarks(file: &Path, session: &str) -> Vec<String> {
    if !is_uuid(session) {
        return Vec::new();
    }
    read_bookmarks(file).remove(session).unwrap_or_default()
}

/// Marks message `message` of `session`, or takes the mark off.
pub fn set_bookmark(file: &Path, session: &str, message: &str, on: bool) -> Result<(), String> {
    if !is_uuid(session) || !is_uuid(message) {
        return Err(refused());
    }
    let _writing = WRITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut all = read_bookmarks(file);
    let marks = all.entry(session.to_string()).or_default();
    let had = marks.iter().any(|mark| mark == message);
    match (on, had) {
        (true, false) if marks.len() < BOOKMARKS_MAX => marks.push(message.to_string()),
        (true, false) => return Err(refused()),
        (false, true) => marks.retain(|mark| mark != message),
        _ => return Ok(()),
    }
    all.retain(|_, marks| !marks.is_empty());
    let json = serde_json::to_string(&all).map_err(|error| error.to_string())?;
    write(file, &json)
}

/// The Sessions page's marks (`wire.ts` `SessionsMeta`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionsMeta {
    pub archived: Vec<String>,
    pub groups: Vec<Group>,
    pub unread: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub sessions: Vec<String>,
}

/// One change of them (`wire.ts` `SessionsMetaPatch`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum SessionsMetaPatch {
    Archive { archive: Vec<String>, on: bool },
    Unread { unread: Vec<String>, on: bool },
    Group { group: GroupName },
    Ungroup { ungroup: String },
    Move {
        #[serde(rename = "move")]
        sessions: Vec<String>,
        #[serde(default)]
        to: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GroupName {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
}

/// A group's id as Pitwall makes them: `g_` and 8 hex digits.
fn is_group_id(id: &str) -> bool {
    id.strip_prefix("g_").is_some_and(|hex| hex.len() == 8 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// A group's name as the user wrote it: on one line, trimmed, `NAME_MAX` characters at most.
fn group_name(name: &str) -> Option<String> {
    let name: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let name: String = name.trim().chars().take(NAME_MAX).collect();
    (!name.is_empty()).then_some(name)
}

/// Each id once, in its first place.
fn unique(ids: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
}

impl SessionsMeta {
    /// Only what such a file may hold: session ids, groups of Pitwall's ids with a name, each
    /// session in one group at most.
    pub fn checked(mut self) -> Self {
        self.archived.retain(|id| is_uuid(id));
        unique(&mut self.archived);
        self.unread.retain(|id| is_uuid(id));
        unique(&mut self.unread);
        let mut placed = std::collections::HashSet::new();
        let mut groups = Vec::new();
        for mut group in self.groups {
            let Some(name) = group_name(&group.name) else {
                continue;
            };
            if !is_group_id(&group.id) || groups.iter().any(|known: &Group| known.id == group.id) || groups.len() == GROUPS_MAX {
                continue;
            }
            group.name = name;
            group.sessions.retain(|id| is_uuid(id) && placed.insert(id.clone()));
            groups.push(group);
        }
        self.groups = groups;
        self
    }

    /// `other`'s marks with these (a backup's, imported): archived and unread together, its groups
    /// in the place of those with their ids, the others kept, a session in the group the import
    /// puts it in.
    pub fn merged(mut self, other: SessionsMeta) -> Self {
        let other = other.checked();
        self.archived.extend(other.archived);
        self.unread.extend(other.unread);
        let moved: Vec<String> = other.groups.iter().flat_map(|group| group.sessions.clone()).collect();
        for group in &mut self.groups {
            group.sessions.retain(|id| !moved.contains(id));
        }
        for group in other.groups {
            match self.groups.iter_mut().find(|known| known.id == group.id) {
                Some(known) => *known = group,
                None => self.groups.push(group),
            }
        }
        self.checked()
    }

    fn apply(&mut self, patch: SessionsMetaPatch) -> Result<(), String> {
        let ids = |ids: &[String]| -> Result<(), String> { if ids.iter().all(|id| is_uuid(id)) { Ok(()) } else { Err(refused()) } };
        match patch {
            SessionsMetaPatch::Archive { archive, on } => {
                ids(&archive)?;
                set(&mut self.archived, archive, on);
            }
            SessionsMetaPatch::Unread { unread, on } => {
                ids(&unread)?;
                set(&mut self.unread, unread, on);
            }
            SessionsMetaPatch::Group { group } => {
                let name = group_name(&group.name).ok_or_else(refused)?;
                match group.id {
                    Some(id) => {
                        let known = self.groups.iter_mut().find(|known| known.id == id).ok_or_else(refused)?;
                        known.name = name;
                    }
                    None => {
                        if self.groups.len() >= GROUPS_MAX {
                            return Err(refused());
                        }
                        let id = format!("g_{}", &new_uuid()[..8]);
                        self.groups.push(Group { id, name, sessions: Vec::new() });
                    }
                }
            }
            SessionsMetaPatch::Ungroup { ungroup } => {
                let before = self.groups.len();
                self.groups.retain(|group| group.id != ungroup);
                if self.groups.len() == before {
                    return Err(refused());
                }
            }
            SessionsMetaPatch::Move { sessions, to } => {
                ids(&sessions)?;
                if to.as_ref().is_some_and(|to| !self.groups.iter().any(|group| &group.id == to)) {
                    return Err(refused());
                }
                for group in &mut self.groups {
                    group.sessions.retain(|id| !sessions.contains(id));
                }
                if let Some(group) = to.and_then(|to| self.groups.iter_mut().find(|group| group.id == to)) {
                    group.sessions.extend(sessions);
                    unique(&mut group.sessions);
                }
            }
        }
        Ok(())
    }
}

fn set(list: &mut Vec<String>, ids: Vec<String>, on: bool) {
    if on {
        list.extend(ids);
        unique(list);
    } else {
        list.retain(|id| !ids.contains(id));
    }
}

/// The marks as the file has them; none when there is no file or it doesn't parse.
pub fn sessions_meta(file: &Path) -> SessionsMeta {
    fs::read(file).ok().and_then(|raw| serde_json::from_slice::<SessionsMeta>(&raw).ok()).unwrap_or_default().checked()
}

/// Writes `meta` whole; the file goes once it holds nothing.
pub fn write_sessions_meta(file: &Path, meta: &SessionsMeta) -> Result<(), String> {
    if *meta == SessionsMeta::default() {
        return match fs::remove_file(file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(t!("chatCore.error.write", error = error)),
            _ => Ok(()),
        };
    }
    let json = serde_json::to_string(meta).map_err(|error| error.to_string())?;
    write(file, &json)
}

/// One change, read, made and written under the file's lock; the marks as they are now.
pub fn change_sessions_meta(file: &Path, patch: SessionsMetaPatch) -> Result<SessionsMeta, String> {
    let _writing = WRITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut meta = sessions_meta(file);
    meta.apply(patch)?;
    let meta = meta.checked();
    write_sessions_meta(file, &meta)?;
    Ok(meta)
}

/// A backup's marks merged into the file's (`SessionsMeta::merged`).
pub fn import_sessions_meta(file: &Path, theirs: SessionsMeta) -> Result<(), String> {
    let _writing = WRITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let meta = sessions_meta(file).merged(theirs);
    write_sessions_meta(file, &meta)
}
