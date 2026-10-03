//! C++: vcpkg (a `vcpkg.json` manifest) and Conan (a conanfile).
//!
//! vcpkg in manifest mode installs into `vcpkg_installed` beside the manifest and keeps a record
//! there (`vcpkg_installed/vcpkg/status` and the `updates` beside it). The manifest's packages
//! are looked for in that record: a missing one is to install. Which versions the baseline and
//! the overrides resolve to isn't knowable from files, so versions aren't compared. A project
//! built through CMake has its `vcpkg_installed` in the build folder, wherever that is: without
//! one beside the manifest nothing is said.
//!
//! Conan keeps its packages in a cache outside the project: nothing to compare. Its install
//! (`conan install . --build=missing`) isn't offered: it compiles what has no binary, which can
//! take an hour, needs a profile chosen first, and writes its generated files into the folder it
//! runs in unless told where; none of that is a constant.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{read_json, text};
use super::{CheckState, DepPackage, Ecosystem};

pub struct Cpp;

const VCPKG_MANIFEST: &str = "vcpkg.json";
/// vcpkg's record of what it installed for the manifest.
const VCPKG_RECORD: &str = "vcpkg_installed/vcpkg";
const CONANFILES: [&str; 2] = ["conanfile.txt", "conanfile.py"];

impl Ecosystem for Cpp {
    fn id(&self) -> &'static str {
        "cpp"
    }

    fn manifest(&self) -> &'static str {
        VCPKG_MANIFEST
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        if dir.join(VCPKG_MANIFEST).is_file() {
            Some("vcpkg")
        } else {
            CONANFILES.iter().any(|name| dir.join(name).is_file()).then_some("conan")
        }
    }

    fn watched(&self) -> &'static [&'static str] {
        &["vcpkg.json", "vcpkg_installed/vcpkg/status", "vcpkg_installed/vcpkg/updates", "conanfile.txt", "conanfile.py"]
    }

    fn check(&self, dir: &Path, manager: &str) -> (CheckState, Vec<DepPackage>) {
        match manager {
            "vcpkg" => check_vcpkg(dir),
            _ => (CheckState::Unknown, Vec::new()),
        }
    }

    fn needs(&self, _dir: &Path, manager: &str) -> &'static [&'static str] {
        match manager {
            "vcpkg" => &["vcpkg"],
            // Nothing of Conan's is ever run.
            _ => &[],
        }
    }

    fn install_command(&self, _dir: &Path, manager: &str) -> Option<&'static str> {
        (manager == "vcpkg").then_some("vcpkg install")
    }
}

/// The packages the manifest asks for on every platform: the names in `dependencies`. One with a
/// `platform` condition may rightly be left out here.
fn declared(manifest: &Value) -> Vec<String> {
    let dependencies = manifest.get("dependencies").and_then(Value::as_array);
    let mut names: Vec<String> = dependencies
        .into_iter()
        .flatten()
        .filter_map(|dependency| match dependency {
            Value::String(name) => Some(name.as_str()),
            Value::Object(_) if dependency.get("platform").is_none() => text(dependency, "name"),
            _ => None,
        })
        .map(str::to_lowercase)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The packages vcpkg's record has as installed, for any triplet; `None` without a record. The
/// record is `status` and then each file of `updates` in name order, all of them paragraphs of
/// `Key: value` lines; a later paragraph of the same package and triplet replaces an earlier one
/// (`Status: install ok installed`, or `purge ok not-installed` once removed). A paragraph with
/// a `Feature` is a part of a package, not the package.
fn installed(dir: &Path) -> Option<HashSet<String>> {
    let record = dir.join(VCPKG_RECORD);
    let status = fs::read_to_string(record.join("status")).ok();
    let mut updates: Vec<_> = fs::read_dir(record.join("updates")).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    if status.is_none() && updates.is_empty() {
        return None;
    }
    updates.sort();
    let texts = status.into_iter().chain(updates.iter().filter_map(|file| fs::read_to_string(file).ok()));

    let mut states: HashMap<(String, String), bool> = HashMap::new();
    for text in texts {
        for paragraph in text.replace("\r\n", "\n").split("\n\n") {
            let field = |name: &str| paragraph.lines().find_map(|line| line.strip_prefix(name)?.strip_prefix(':')).map(str::trim);
            let (Some(package), None) = (field("Package"), field("Feature")) else {
                continue;
            };
            let triplet = field("Architecture").unwrap_or_default();
            states.insert((package.to_lowercase(), triplet.to_string()), field("Status") == Some("install ok installed"));
        }
    }
    Some(states.into_iter().filter(|(_, installed)| *installed).map(|((package, _), _)| package).collect())
}

/// vcpkg.json's dependencies against vcpkg's record of what it installed for it.
fn check_vcpkg(dir: &Path) -> (CheckState, Vec<DepPackage>) {
    let (Some(manifest), Some(installed)) = (read_json(&dir.join(VCPKG_MANIFEST)), installed(dir)) else {
        return (CheckState::Unknown, Vec::new());
    };
    let packages = declared(&manifest)
        .into_iter()
        .filter(|name| !installed.contains(name))
        .map(|name| DepPackage { name, locked: None, installed: None })
        .collect();
    (CheckState::Ok, packages)
}
