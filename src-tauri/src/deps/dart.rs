//! Dart and Flutter's pub: pubspec.lock against `.dart_tool/package_config.json`, pub's record
//! of what it resolved the project to (the packages themselves sit in a cache outside the
//! project). A Flutter project (`sdk: flutter` in pubspec.yaml) runs pub through `flutter`.
//! The runtime is Dart: pubspec.yaml's `environment.sdk`, else what `.tool-versions` pins.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{count, flag, json_object, none_installed, read_json, text, version_in};
use super::{Advisory, Audit, CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadAudit, ReadOutdated, Runtime, ScanPlan};

pub struct Dart;

const PUBSPEC: &str = "pubspec.yaml";
const CONFIG: &str = ".dart_tool/package_config.json";

const GET: &str = "dart pub get";
const FLUTTER_GET: &str = "flutter pub get";
/// `--show-all`: packages that are up to date are listed too, so one with an advisory against it
/// is never left out.
const OUTDATED: &str = "dart pub outdated --json --show-all";
const FLUTTER_OUTDATED: &str = "flutter pub outdated --json --show-all";

/// pub's ranges (`>=3.5.0 <4.0.0`, `^3.5.0`) read as npm's do. Older SDKs print the version to
/// stderr.
static RUNTIMES: [Runtime; 1] =
    [Runtime { name: "dart", probe: "dart --version 2>&1", version: dart_version, required: dart_required, local: None, flavor: Flavor::Npm }];

impl Ecosystem for Dart {
    fn id(&self) -> &'static str {
        "dart"
    }

    fn manifest(&self) -> &'static str {
        PUBSPEC
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        dir.join(PUBSPEC).is_file().then_some("pub")
    }

    fn watched(&self) -> &'static [&'static str] {
        &[PUBSPEC, "pubspec.lock", CONFIG, ".tool-versions"]
    }

    fn check(&self, dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        let Ok(pubspec) = fs::read_to_string(dir.join(PUBSPEC)) else {
            return (CheckState::Unknown, Vec::new());
        };
        // A workspace's member: the lock file and pub's record are the workspace's, in a folder
        // above.
        if pubspec.lines().any(|line| code(line) == "resolution: workspace") {
            return (CheckState::Unknown, Vec::new());
        }
        check_pub(dir, &pubspec)
    }

    fn needs(&self, dir: &Path, _manager: &str) -> &'static [&'static str] {
        if flutter(dir) {
            &["flutter"]
        } else {
            &["dart"]
        }
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, dir: &Path, _manager: &str) -> Option<&'static str> {
        Some(if flutter(dir) { FLUTTER_GET } else { GET })
    }

    fn scan_plan(&self, dir: &Path, _manager: &str) -> ScanPlan {
        // One command tells both; it is run once for each count.
        let command = if flutter(dir) { FLUTTER_OUTDATED } else { OUTDATED };
        ScanPlan { outdated: Some((command, pub_outdated as ReadOutdated)), audit: Some((command, pub_audit as ReadAudit)) }
    }
}

/// A YAML line without its comment and the space after it. (No `#` occurs in what is read here.)
fn code(line: &str) -> &str {
    line.split(" #").next().unwrap_or_default().trim_end()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The lines under the top-level key `section` of a YAML file, each with its indent, until the
/// next top-level key. Read line by line: of pub's files only block mappings are needed.
fn entries<'a>(yaml: &'a str, section: &'a str) -> impl Iterator<Item = (usize, &'a str)> {
    yaml.lines()
        .map(code)
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .skip_while(move |line| line.strip_prefix(section).map(str::trim_end) != Some(":"))
        .skip(1)
        .take_while(|line| indent(line) > 0)
        .map(|line| (indent(line), line.trim_start()))
}

/// `key: value` of a YAML line, the value without its quotes; a key alone has an empty value.
fn key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim().trim_matches(['"', '\''])))
}

/// The project depends on the Flutter SDK (`flutter:` with `sdk: flutter` under it), so pub is
/// run through `flutter`: plain `dart` doesn't know that SDK's packages.
fn flutter(dir: &Path) -> bool {
    fs::read_to_string(dir.join(PUBSPEC)).is_ok_and(|pubspec| pubspec.lines().any(|line| code(line).trim_start() == "sdk: flutter"))
}

/// A package of pubspec.lock.
struct Locked {
    name: String,
    version: String,
    /// `hosted`, `git`, `path`, `sdk`
    source: String,
    /// A git package's commit.
    commit: Option<String>,
}

/// pubspec.lock's `packages`: a name per two-space key, with its `source` and `version` (and,
/// under `description`, a git package's `resolved-ref`).
fn locked(lock: &str) -> Vec<Locked> {
    let mut packages: Vec<Locked> = Vec::new();
    for (depth, line) in entries(lock, "packages") {
        let Some((key, value)) = key_value(line) else {
            continue;
        };
        if depth == 2 {
            packages.push(Locked { name: key.to_string(), version: String::new(), source: String::new(), commit: None });
            continue;
        }
        let Some(package) = packages.last_mut() else {
            continue;
        };
        match (depth, key) {
            (4, "version") => package.version = value.to_string(),
            (4, "source") => package.source = value.to_string(),
            (6, "resolved-ref") => package.commit = Some(value.to_string()),
            _ => {}
        }
    }
    packages
}

/// The packages pubspec.yaml asks for itself: the keys of `dependencies` and `dev_dependencies`
/// (the least indented lines there, however far a hand indents).
fn declared(pubspec: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for section in ["dependencies", "dev_dependencies"] {
        let keys = entries(pubspec, section).map(|(depth, _)| depth).min();
        names.extend(entries(pubspec, section).filter(|(depth, _)| Some(*depth) == keys).filter_map(|(_, line)| key_value(line).map(|(name, _)| name.to_string())));
    }
    names.sort();
    names.dedup();
    names
}

/// pubspec.lock against `.dart_tool/package_config.json`. A hosted package's folder there ends
/// in `<name>-<version>`, a git one's has its commit; of a path or SDK package only the presence
/// tells. Without the record, `pub get` never ran here; without a lock file, the packages
/// pubspec.yaml names are looked for.
fn check_pub(dir: &Path, pubspec: &str) -> (CheckState, Vec<DepPackage>) {
    let lock = fs::read_to_string(dir.join("pubspec.lock")).ok().map(|lock| locked(&lock));
    let Some(config) = read_json(&dir.join(CONFIG)) else {
        return match lock {
            Some(lock) => {
                let all: Vec<DepPackage> = lock.into_iter().map(|p| DepPackage { name: p.name, locked: Some(p.version), installed: None }).collect();
                (if all.is_empty() { CheckState::Ok } else { CheckState::Install }, all)
            }
            None => none_installed(&declared(pubspec), |_| None),
        };
    };
    let have: HashMap<&str, &str> = config
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| Some((text(package, "name")?, text(package, "rootUri")?)))
        .collect();

    let packages: Vec<DepPackage> = match lock {
        Some(lock) => lock
            .into_iter()
            .filter_map(|wanted| {
                let root = have.get(wanted.name.as_str());
                let installed = match (root, wanted.source.as_str()) {
                    (None, _) => None,
                    (Some(root), "hosted") => {
                        let folder = root.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
                        Some(folder.strip_prefix(wanted.name.as_str()).and_then(|rest| rest.strip_prefix('-')).unwrap_or_default().to_string())
                    }
                    (Some(root), "git") if wanted.commit.as_deref().is_some_and(|commit| !root.contains(commit)) => Some(String::new()),
                    (Some(_), _) => Some(wanted.version.clone()),
                };
                let same = installed.as_deref() == Some(wanted.version.as_str());
                (!same).then(|| DepPackage { name: wanted.name, locked: Some(wanted.version), installed: installed.filter(|version| !version.is_empty()) })
            })
            .collect(),
        None => declared(pubspec)
            .into_iter()
            .filter(|name| !have.contains_key(name.as_str()))
            .map(|name| DepPackage { name, locked: None, installed: None })
            .collect(),
    };
    (CheckState::Ok, packages)
}

/* ---------- the runtime ---------- */

/// The version `.tool-versions` (asdf, mise) names for `tool`: the first on its line.
fn tool_version(dir: &Path, tool: &str) -> Option<String> {
    let text = fs::read_to_string(dir.join(".tool-versions")).ok()?;
    text.lines().find_map(|line| {
        let mut words = line.split('#').next().unwrap_or_default().split_whitespace();
        (words.next()? == tool).then(|| words.next().map(str::to_string)).flatten()
    })
}

/// `Dart SDK version: 3.7.3 (stable) …`: the version after that label, whatever a wrapper
/// (Flutter's `dart`) printed before it.
fn dart_version(output: &str) -> Option<String> {
    version_in(output.split_once("Dart SDK version:")?.1)
}

/// A project uses Dart with a pubspec.yaml, or with a `.tool-versions` that names it. What it
/// asks for: `sdk` under pubspec.yaml's `environment`, else what `.tool-versions` pins.
fn dart_required(dir: &Path) -> Option<Option<String>> {
    let pubspec = fs::read_to_string(dir.join(PUBSPEC)).ok();
    let pinned = tool_version(dir, "dart");
    if pubspec.is_none() && pinned.is_none() {
        return None;
    }
    let sdk = pubspec.as_deref().and_then(|pubspec| {
        let mut keys = entries(pubspec, "environment").filter_map(|(_, line)| key_value(line));
        keys.find(|(key, value)| *key == "sdk" && !value.is_empty()).map(|(_, range)| range.to_string())
    });
    Some(sdk.or(pinned))
}

/* ---------- the network scan ---------- */

/// The `packages` of `pub outdated --json`.
fn outdated_packages(output: &str) -> Option<Vec<Value>> {
    match json_object(output)?.get_mut("packages")?.take() {
        Value::Array(packages) => Some(packages),
        _ => None,
    }
}

fn version_of<'a>(package: &'a Value, which: &str) -> Option<&'a str> {
    text(package.get(which)?, "version")
}

fn owned_version(package: &Value, which: &str) -> Option<String> {
    version_of(package, which).map(str::to_string)
}

/// The packages the project asks for itself (`direct`, `dev`) whose resolved version isn't the
/// latest; `upgradable` is the newest its constraint allows.
fn pub_outdated(output: &str) -> Option<Vec<Outdated>> {
    let packages = outdated_packages(output)?;
    let behind = packages.iter().filter(|package| {
        let own = matches!(text(package, "kind"), Some("direct" | "dev"));
        own && matches!((version_of(package, "current"), version_of(package, "latest")), (Some(current), Some(latest)) if current != latest)
    });
    let listed = behind.map(|package| Outdated {
        name: text(package, "package").unwrap_or_default().to_string(),
        installed: owned_version(package, "current"),
        wanted: owned_version(package, "upgradable"),
        latest: owned_version(package, "latest"),
    });
    Some(listed.collect())
}

/// The resolved packages with a security advisory against their version: pub tells that there
/// is one, not what it says. An SDK too old to tell (no `isCurrentAffectedByAdvisory`) gives no
/// count.
fn pub_audit(output: &str) -> Option<Audit> {
    let packages = outdated_packages(output)?;
    if !packages.is_empty() && packages.iter().all(|package| package.get("isCurrentAffectedByAdvisory").is_none()) {
        return None;
    }
    let affected = packages.iter().filter(|package| !package["current"].is_null() && flag(package, "isCurrentAffectedByAdvisory"));
    let advisories: Vec<Advisory> =
        affected.map(|package| Advisory { package: text(package, "package").unwrap_or_default().to_string(), installed: owned_version(package, "current"), ..Advisory::default() }).collect();
    Some(Audit { packages: count(advisories.len()), advisories })
}
