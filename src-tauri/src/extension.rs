//! Pitwall for VS Code, the editor extension: whether the chosen editor has it, and where to get
//! it. With it, Pitwall and the editor's windows share servers, their output and Claude's state.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;

pub const ID: &str = "aenzenith.pitwall-vscode";
/// Cursor and Windsurf install from Open VSX, where it isn't; they take the release's `.vsix`.
const RELEASES: &str = "https://github.com/aenzenith/pitwall-vscode/releases/latest";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatus {
    /// The version installed in the chosen editor, when it is.
    pub version: Option<String>,
    /// The chosen editor installs it from the VS Code Marketplace; otherwise from a `.vsix`.
    pub marketplace: bool,
}

/// `~/.vscode/extensions` and its likes.
fn extensions_dir(editor: &str) -> Option<PathBuf> {
    let folder = match editor {
        "vscode-insiders" => ".vscode-insiders",
        "cursor" => ".cursor",
        "windsurf" => ".windsurf",
        _ => ".vscode",
    };
    dirs::home_dir().map(|home| home.join(folder).join("extensions"))
}

/// The installed version: from `extensions.json`, the list the editor keeps; without it, from a
/// folder named `<id>-<version>` that isn't marked as removed.
fn installed_version(editor: &str) -> Option<String> {
    let dir = extensions_dir(editor)?;

    if let Ok(text) = fs::read_to_string(dir.join("extensions.json")) {
        if let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            return list
                .iter()
                .find(|entry| entry["identifier"]["id"].as_str().is_some_and(|id| id.eq_ignore_ascii_case(ID)))
                .and_then(|entry| entry["version"].as_str().map(str::to_string));
        }
    }

    let removed: HashMap<String, bool> = fs::read_to_string(dir.join(".obsolete")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let prefix = format!("{ID}-");
    fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !removed.contains_key(name))
        .find_map(|name| name.to_lowercase().strip_prefix(&prefix).map(str::to_string))
}

pub fn status(editor: &str) -> ExtensionStatus {
    ExtensionStatus { version: installed_version(editor), marketplace: matches!(editor, "vscode" | "vscode-insiders") }
}

/// Where to get it: its page in VS Code itself (one click installs), else the latest release.
pub fn page(editor: &str) -> String {
    match editor {
        "vscode" | "vscode-insiders" => format!("{editor}:extension/{ID}"),
        _ => RELEASES.into(),
    }
}
