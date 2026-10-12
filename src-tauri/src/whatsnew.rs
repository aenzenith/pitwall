//! What changed in this version, shown once after an update. The notes are written by hand for
//! each release, in every language Pitwall speaks, in `src/whats-new.json`. That file holds the
//! next release's notes only, and its `since` names the release they come after. Once the
//! version is no longer `since`, a release went out: the notes are emptied, `since` is set to
//! that version, and the next release's notes start again. The release workflow cuts no
//! release while the file is still an earlier one's (`release.yml`).
//!
//! The changelog release-please writes (`CHANGELOG.md`) is read only for which version came
//! before this one and for this one's date; none of its sentences is ever shown.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const NOTES: &str = include_str!("../../src/whats-new.json");
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");

/// The version this build is.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// `src/whats-new.json`.
#[derive(Debug, Deserialize)]
struct File {
    /// The release these notes come after.
    since: String,
    notes: Vec<Written>,
}

/// A note as it is written: its texts by language code.
#[derive(Debug, Deserialize)]
struct Written {
    /// `new` | `fix`
    kind: String,
    title: HashMap<String, String>,
    #[serde(default)]
    body: HashMap<String, String>,
}

/// One thing the release brought, in the language shown.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Note {
    /// `new` | `fix`
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
}

/// The notes to show. `pending`: they haven't been shown yet; `from`: the version updated from,
/// when known; `date`: this version's release day (`YYYY-MM-DD`), when it has one. No `notes`:
/// there are none for this build, and nothing is shown.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WhatsNew {
    pub current: String,
    pub date: Option<String>,
    pub pending: bool,
    pub from: Option<String>,
    pub notes: Vec<Note>,
}

/// `1.9.0` as numbers; None for anything that isn't three of them.
fn parts(version: &str) -> Option<(u64, u64, u64)> {
    let mut numbers = version.trim().trim_start_matches('v').splitn(3, '.').map(|part| part.parse::<u64>().ok());
    Some((numbers.next()??, numbers.next()??, numbers.next()??))
}

/// Whether `version` is newer than `than`. A version that doesn't read as one is never newer.
fn newer(version: &str, than: &str) -> bool {
    match (parts(version), parts(than)) {
        (Some(version), Some(than)) => version > than,
        _ => false,
    }
}

/// A changelog heading's version and date: `## [1.9.0](https://…) (2026-10-12)`.
fn heading(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.strip_prefix("## ")?.trim();
    let version: String = rest.trim_start_matches('[').chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    parts(&version)?;

    let date = rest.rsplit_once('(').map(|(_, date)| date.trim_end_matches(')').trim().to_string()).filter(|date| {
        date.len() == 10 && date.chars().enumerate().all(|(at, c)| if at == 4 || at == 7 { c == '-' } else { c.is_ascii_digit() })
    });
    Some((version, date))
}

/// Every release the changelog names, the newest first.
fn releases(changelog: &str) -> Vec<(String, Option<String>)> {
    changelog.lines().filter_map(heading).collect()
}

/// Whether notes written after `since` are this build's: `since` is the release right before
/// this version, or this very version (a build of work not released yet).
fn ours(since: &str, changelog: &str, current: &str) -> bool {
    let releases = releases(changelog);
    let before = releases.iter().position(|(version, _)| version == current).and_then(|at| releases.get(at + 1));
    since == current || before.is_some_and(|(version, _)| version == since)
}

/// The text in `language`, else in English; None when it has neither.
fn worded(texts: &HashMap<String, String>, language: &str) -> Option<String> {
    [language, "en"].iter().find_map(|code| texts.get(*code).map(|text| text.trim()).filter(|text| !text.is_empty())).map(str::to_string)
}

/// This build's notes in `language`; none when the file is another release's or doesn't parse.
fn notes_of(file: &str, changelog: &str, current: &str, language: &str) -> Vec<Note> {
    let Ok(file) = serde_json::from_str::<File>(file) else {
        return Vec::new();
    };
    if !ours(&file.since, changelog, current) {
        return Vec::new();
    }

    file.notes
        .iter()
        .filter_map(|note| {
            let kind = if note.kind == "fix" { "fix" } else { "new" };
            Some(Note { kind: kind.to_string(), title: worded(&note.title, language)?, body: worded(&note.body, language) })
        })
        .collect()
}

/// Whether this build has notes still to be shown: it has some, and it is newer than the
/// version whose notes were shown last (`seen` None: an install from before there were notes).
pub fn is_pending(seen: Option<&str>) -> bool {
    seen.is_none_or(|seen| newer(CURRENT, seen)) && !notes_of(NOTES, CHANGELOG, CURRENT, "en").is_empty()
}

/// What to show for this build in `language`. `pending`: as the core holds it since the start.
pub fn view(seen: Option<&str>, pending: bool, language: &str) -> WhatsNew {
    let notes = notes_of(NOTES, CHANGELOG, CURRENT, language);
    // Notes written after this very version are a release's to come: it has no day yet.
    let to_come = serde_json::from_str::<File>(NOTES).is_ok_and(|file| file.since == CURRENT);
    let date = releases(CHANGELOG).into_iter().find(|(version, _)| !to_come && version == CURRENT).and_then(|(_, date)| date);

    WhatsNew {
        current: CURRENT.to_string(),
        date,
        pending: pending && !notes.is_empty(),
        from: seen.filter(|seen| pending && *seen != CURRENT).map(str::to_string),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release that goes out with a note one language lacks, or with a file that doesn't
    /// parse, can't be taken back, and nothing else would say so. (Whether the notes were
    /// written again since the last release is the release workflow's to check: the tree it
    /// tests is still at the version before.)
    #[test]
    fn the_notes_are_this_versions_and_in_every_language() {
        let file: File = serde_json::from_str(NOTES).expect("src/whats-new.json parses");

        assert!(
            ours(&file.since, CHANGELOG, CURRENT),
            "src/whats-new.json holds the notes written after {}, which isn't the release before {CURRENT}: empty it, set `since`, and write this release's notes",
            file.since
        );
        assert!(!file.notes.is_empty(), "src/whats-new.json has no notes for the release after {}", file.since);

        for (at, note) in file.notes.iter().enumerate() {
            assert!(matches!(note.kind.as_str(), "new" | "fix"), "note {at}: kind is `{}`", note.kind);
            for language in crate::i18n::LANGUAGES {
                let has = |texts: &HashMap<String, String>| texts.get(language).is_some_and(|text| !text.trim().is_empty());
                assert!(has(&note.title), "note {at}: no title in {language}");
                assert!(note.body.is_empty() || has(&note.body), "note {at}: no body in {language}");
            }
        }

        // What the app would show is the whole file, in a language of its own.
        assert_eq!(notes_of(NOTES, CHANGELOG, CURRENT, "tr").len(), file.notes.len());
    }

    /// The same file under the next release but one: it is the release before's, and showing
    /// it would tell the user about changes they already have. Nothing is shown instead.
    #[test]
    fn another_releases_notes_are_never_shown() {
        let notes = r#"{"since":"1.0.0","notes":[{"kind":"new","title":{"en":"Old"}}]}"#;
        let changelog = "## [1.2.0](x) (2026-01-03)\n\n## [1.1.0](x) (2026-01-02)\n\n## 1.0.0 (2026-01-01)\n";

        assert_eq!(notes_of(notes, changelog, "1.1.0", "en").len(), 1, "the release right after `since`");
        assert_eq!(notes_of(notes, changelog, "1.0.0", "en").len(), 1, "work not released yet");
        assert!(notes_of(notes, changelog, "1.2.0", "en").is_empty(), "a later release");
    }
}
