//! Cargo. Crates are fetched into a cache outside the project and compiled with it, so there is
//! nothing to install and nothing to compare; and Cargo itself has no outdated or audit command
//! (`cargo outdated` and `cargo audit` are installed apart; `cargo update --dry-run` prints prose
//! to stderr about every crate of the lock file, not the project's own). What is left is the
//! runtime, Rust: the version a `rust-toolchain.toml` or `rust-toolchain` pins, else
//! Cargo.toml's `rust-version`. The manifest may sit one folder down (`src-tauri/Cargo.toml`).

use std::fs;
use std::path::{Path, PathBuf};

use super::{parse_version, version_in};
use super::{CheckState, DepPackage, Ecosystem, Flavor, Runtime};

pub struct Rust;

const MANIFEST: &str = "Cargo.toml";
const TOOLCHAIN_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];
/// Folders that are never a crate of the project's own.
const NOT_CRATES: [&str; 3] = ["node_modules", "target", "vendor"];

static RUNTIMES: [Runtime; 1] =
    [Runtime { name: "rust", probe: "rustc --version", version: version_in, required: rust_required, local: Some(rust_local), flavor: Flavor::Custom(at_least) }];

impl Ecosystem for Rust {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        manifest_dir(dir).map(|_| "cargo")
    }

    fn watched(&self) -> &'static [&'static str] {
        // A manifest one folder down is found whatever the folder's name; of those, only Tauri's
        // can be named here.
        &["Cargo.toml", "rust-toolchain.toml", "rust-toolchain", "src-tauri/Cargo.toml", "src-tauri/rust-toolchain.toml"]
    }

    fn check(&self, _dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        (CheckState::Unknown, Vec::new())
    }

    /// Nothing of Cargo's is ever run.
    fn needs(&self, _dir: &Path, _manager: &str) -> &'static [&'static str] {
        &[]
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }
}

/// The folder with the project's Cargo.toml: the project's own, else the first (by name) of its
/// folders that has one. One level only, and no folder is read but the project's.
fn manifest_dir(dir: &Path) -> Option<PathBuf> {
    if dir.join(MANIFEST).is_file() {
        return Some(dir.to_path_buf());
    }
    let mut folders: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.file_name().to_str().is_some_and(|name| !name.starts_with('.') && !NOT_CRATES.contains(&name)))
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    folders.into_iter().find(|folder| folder.join(MANIFEST).is_file())
}

/// A TOML line as key and value, the value without its quotes; `None` for a table header, a
/// comment, or a value that isn't one string.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let value = value.split('#').next().unwrap_or_default().trim();
    let unquoted = value.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')).or_else(|| value.strip_prefix('\'')?.strip_suffix('\''))?;
    Some((key.trim(), unquoted))
}

/// The channel a toolchain file asks for: `channel = "1.84.0"` of the TOML form, or the one
/// line of the plain form (`rust-toolchain` may be either).
fn channel_in(text: &str) -> Option<String> {
    let lines = || text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#'));
    if lines().any(|line| line.starts_with('[')) {
        return lines().filter_map(key_value).find(|(key, _)| *key == "channel").map(|(_, channel)| channel.to_string());
    }
    lines().next().filter(|line| !line.contains('=')).map(str::to_string)
}

/// The project's toolchain channel: the file beside the manifest first (rustup looks upward from
/// where Cargo runs), then the project's own.
fn channel(dir: &Path, manifest: Option<&Path>) -> Option<String> {
    let folders = manifest.into_iter().chain([dir]);
    let mut files = folders.flat_map(|folder| TOOLCHAIN_FILES.iter().map(move |name| folder.join(name)));
    files.find_map(|file| channel_in(&fs::read_to_string(file).ok()?))
}

/// `1.84.0` and `1.84` are versions; `stable`, `beta`, `nightly-2025-01-01` are not.
fn is_version(channel: &str) -> bool {
    parse_version(channel).is_some()
}

/// Cargo.toml's `rust-version`, of `[package]` or (inherited by the members) `[workspace.package]`.
fn rust_version(manifest: &str) -> Option<String> {
    let mut table = "";
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line;
        } else if matches!(table, "[package]" | "[workspace.package]") {
            if let Some((_, version)) = key_value(line).filter(|(key, _)| *key == "rust-version") {
                return Some(version.to_string());
            }
        }
    }
    None
}

/// A project uses Rust with a Cargo.toml (its own or one folder down) or a toolchain file. What
/// it asks for: the version its toolchain file pins; else Cargo.toml's `rust-version`, the least
/// that builds it; else the toolchain file's channel by name.
fn rust_required(dir: &Path) -> Option<Option<String>> {
    let manifest = manifest_dir(dir);
    let channel = channel(dir, manifest.as_deref());
    if manifest.is_none() && channel.is_none() {
        return None;
    }
    if channel.as_deref().is_some_and(is_version) {
        return Some(channel);
    }
    let least = manifest.and_then(|folder| fs::read_to_string(folder.join(MANIFEST)).ok()).and_then(|manifest| rust_version(&manifest));
    Some(least.map(|version| format!(">={version}")).or(channel))
}

/// rustup's folder: `RUSTUP_HOME`, else `.rustup` in the home folder.
fn rustup_home() -> Option<PathBuf> {
    std::env::var_os("RUSTUP_HOME").filter(|home| !home.is_empty()).map(PathBuf::from).or_else(|| Some(dirs::home_dir()?.join(".rustup")))
}

/// The pinned version, when rustup has that toolchain (`toolchains/1.84.0-<host>`): rustup then
/// runs it in the project, whatever the login shell's default toolchain is. Only the folders'
/// names are read.
fn rust_local(dir: &Path) -> Option<String> {
    let pinned = channel(dir, manifest_dir(dir).as_deref()).filter(|channel| is_version(channel))?;
    let prefix = format!("{pinned}-");
    let mut toolchains = fs::read_dir(rustup_home()?.join("toolchains")).ok()?.flatten();
    toolchains.any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix)).then_some(pinned)
}

/// A pinned toolchain (`1.84.0`) and a least version (`>=1.80`) are both met by that version or
/// a later one: a later Rust builds what an earlier one does. A channel's name can't be compared.
fn at_least(required: &str, active: &str) -> Option<bool> {
    let least = parse_version(required.trim().trim_start_matches(">="))?;
    Some(parse_version(active)? >= least)
}
