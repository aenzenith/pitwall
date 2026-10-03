//! npm, pnpm, yarn and bun: one ecosystem (`npm`), the manager told by the lock file. Each
//! manager's lock file is compared with its own record of what it installed in `node_modules`.
//! The runtime is Node: `.nvmrc`, `.node-version`, then `engines.node`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use serde_json::Value;

use super::{flag, json_object, none_installed, owned, read_json, severity, text, version_line};
use super::{Advisory, Audit, CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadAudit, ReadOutdated, Runtime, ScanPlan};
use crate::resolve::{detect_package_manager, PackageManager};

pub struct Node;

const LOCKS: [&str; 5] = ["pnpm-lock.yaml", "yarn.lock", "bun.lockb", "bun.lock", "package-lock.json"];

static RUNTIMES: [Runtime; 1] = [Runtime { name: "node", probe: "node -v", version: version_line, required: node_required, local: None, flavor: Flavor::Npm }];

impl Ecosystem for Node {
    fn id(&self) -> &'static str {
        "npm"
    }

    fn manifest(&self) -> &'static str {
        "package.json"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        dir.join("package.json").is_file().then(|| match manager(dir) {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
            PackageManager::Bun => "bun",
        })
    }

    fn watched(&self) -> &'static [&'static str] {
        &[
            "package.json",
            "package-lock.json",
            "node_modules",
            "node_modules/.package-lock.json",
            "pnpm-lock.yaml",
            "node_modules/.pnpm/lock.yaml",
            "yarn.lock",
            "node_modules/.yarn-integrity",
            ".yarn/install-state.gz",
            ".pnp.cjs",
            "bun.lock",
            "bun.lockb",
            ".nvmrc",
            ".node-version",
        ]
    }

    fn check(&self, dir: &Path, manager: &str) -> (CheckState, Vec<DepPackage>) {
        let Some(package) = read_json(&dir.join("package.json")) else {
            return (CheckState::Unknown, Vec::new());
        };
        let declared = declared_node(&package);

        match manager {
            "pnpm" => pnpm_check(dir, &declared),
            "yarn" => yarn_check(dir, &declared),
            "bun" if dir.join("node_modules").is_dir() => missing_from_node_modules(dir, &declared, |_| None),
            "bun" => none_installed(&declared, |_| None),
            _ => npm_check(dir, &declared),
        }
    }

    fn needs(&self, _dir: &Path, manager: &str) -> &'static [&'static str] {
        match manager {
            "pnpm" => &["pnpm"],
            "yarn" => &["yarn"],
            "bun" => &["bun"],
            _ => &["npm"],
        }
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, _dir: &Path, manager: &str) -> Option<&'static str> {
        Some(match manager {
            "pnpm" => "pnpm install",
            "yarn" => "yarn install",
            "bun" => "bun install",
            _ => "npm install",
        })
    }

    fn scan_plan(&self, dir: &Path, manager: &str) -> ScanPlan {
        let (outdated, audit) = match manager {
            "pnpm" => (Some(("pnpm outdated --format json", npm_outdated as ReadOutdated)), Some(("pnpm audit --json", npm_audit as ReadAudit))),
            "yarn" if dir.join(".yarnrc.yml").is_file() => (None, None),
            "yarn" => (Some(("yarn outdated --json", yarn_outdated as ReadOutdated)), Some(("yarn audit --json", yarn_audit as ReadAudit))),
            // npm reads bun's node_modules; bun has no audit report to count.
            "bun" => (Some(("npm outdated --json", npm_outdated as ReadOutdated)), None),
            _ => (Some(("npm outdated --json", npm_outdated as ReadOutdated)), Some(("npm audit --json", npm_audit as ReadAudit))),
        };
        ScanPlan { outdated, audit }
    }
}

/// A package name that stays inside `node_modules` when joined to it.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('/') && !name.contains('\\') && name.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The lock file's manager; without one, `packageManager` in package.json; else npm.
fn manager(dir: &Path) -> PackageManager {
    let files: Vec<String> = LOCKS.iter().filter(|name| dir.join(name).is_file()).map(|name| name.to_string()).collect();
    if !files.is_empty() {
        return detect_package_manager(&files);
    }

    read_json(&dir.join("package.json"))
        .as_ref()
        .and_then(|package| text(package, "packageManager"))
        .and_then(|field| PackageManager::parse(field.split('@').next().unwrap_or_default()))
        .unwrap_or(PackageManager::Npm)
}

/// `dependencies` and `devDependencies` (optional ones may rightly be left out).
fn declared_node(package: &Value) -> Vec<String> {
    let mut names: Vec<String> = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|key| package.get(*key)?.as_object())
        .flat_map(|deps| deps.keys().cloned())
        .filter(|name| safe_name(name))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The version in `node_modules/<name>/package.json`.
fn installed_version(dir: &Path, name: &str) -> Option<String> {
    if !safe_name(name) {
        return None;
    }
    text(&read_json(&dir.join("node_modules").join(name).join("package.json"))?, "version").map(str::to_string)
}

/// Declared packages that `node_modules` lacks, or whose version isn't the locked one.
fn missing_from_node_modules(dir: &Path, declared: &[String], locked: impl Fn(&str) -> Option<String>) -> (CheckState, Vec<DepPackage>) {
    let packages: Vec<DepPackage> = declared
        .iter()
        .filter_map(|name| {
            let installed = installed_version(dir, name);
            let locked = locked(name);
            let differs = installed.is_none() || (locked.is_some() && locked != installed);
            differs.then(|| DepPackage { name: name.clone(), locked, installed })
        })
        .collect();
    (CheckState::Ok, packages)
}

/// The version package-lock.json locks a top-level package at (lockfile v2/v3, or v1).
fn npm_locked(lock: &Value, name: &str) -> Option<String> {
    let entry = lock.get("packages").and_then(|p| p.get(format!("node_modules/{name}"))).or_else(|| lock.get("dependencies")?.get(name))?;
    text(entry, "version").map(str::to_string)
}

/// package-lock.json against `node_modules/.package-lock.json`, the record npm keeps of what it
/// installed. Without that record (npm 6, another tool), the declared packages' own versions.
fn npm_check(dir: &Path, declared: &[String]) -> (CheckState, Vec<DepPackage>) {
    let lock = read_json(&dir.join("package-lock.json"));
    let locked = |name: &str| lock.as_ref().and_then(|lock| npm_locked(lock, name));

    if !dir.join("node_modules").is_dir() {
        return none_installed(declared, locked);
    }
    let Some(wanted) = &lock else {
        return missing_from_node_modules(dir, declared, |_| None);
    };
    let Some(hidden) = read_json(&dir.join("node_modules").join(".package-lock.json")) else {
        return missing_from_node_modules(dir, declared, locked);
    };
    let Some(entries) = wanted.get("packages").and_then(Value::as_object) else {
        return missing_from_node_modules(dir, declared, locked);
    };
    let have = hidden.get("packages").and_then(Value::as_object);
    let mut packages = Vec::new();

    for (key, entry) in entries {
        // The root (""), and workspace folders outside node_modules.
        let Some(at) = key.rfind("node_modules/") else {
            continue;
        };
        if flag(entry, "link") {
            continue;
        }
        let Some(version) = text(entry, "version") else {
            continue;
        };
        let installed = have.and_then(|have| have.get(key)).and_then(|entry| text(entry, "version"));
        if installed == Some(version) {
            continue;
        }
        // Optional packages for other platforms (esbuild's, rollup's…) are never installed.
        if installed.is_none() && (flag(entry, "optional") || flag(entry, "devOptional") || flag(entry, "inBundle")) {
            continue;
        }
        packages.push(DepPackage {
            name: key[at + "node_modules/".len()..].to_string(),
            locked: Some(version.to_string()),
            installed: installed.map(str::to_string),
        });
    }

    (CheckState::Ok, packages)
}

/// pnpm-lock.yaml against `node_modules/.pnpm/lock.yaml`, the copy pnpm writes of what it
/// installed: the same text means up to date; otherwise the packages one locks and the other
/// lacks, apart from optional and platform-specific ones pnpm leaves out of its copy, plus
/// declared packages missing from `node_modules`.
fn pnpm_check(dir: &Path, declared: &[String]) -> (CheckState, Vec<DepPackage>) {
    if !dir.join("node_modules").is_dir() {
        return none_installed(declared, |_| None);
    }
    let Ok(wanted) = fs::read_to_string(dir.join("pnpm-lock.yaml")) else {
        return missing_from_node_modules(dir, declared, |_| None);
    };
    let Ok(current) = fs::read_to_string(dir.join("node_modules").join(".pnpm").join("lock.yaml")) else {
        return (CheckState::Unknown, Vec::new());
    };
    if wanted == current {
        return (CheckState::Ok, Vec::new());
    }

    let wanted = PnpmLock::parse(&wanted);
    let current = PnpmLock::parse(&current);
    let installed_versions: HashMap<&str, &str> = current.packages.values().map(|(name, version)| (name.as_str(), version.as_str())).collect();

    let mut packages: Vec<DepPackage> = wanted
        .packages
        .iter()
        .filter(|(key, _)| !current.packages.contains_key(*key) && !wanted.skipped.contains(pnpm_base(key)))
        .map(|(_, (name, version))| DepPackage {
            name: name.clone(),
            locked: Some(version.clone()),
            installed: installed_versions.get(name.as_str()).map(|v| v.to_string()),
        })
        .collect();

    let (_, missing) = missing_from_node_modules(dir, declared, |_| None);
    packages.extend(missing.into_iter().filter(|missing| missing.installed.is_none()));
    (CheckState::Ok, packages)
}

/// A package key without its peer suffix: `vue@3.4.0(typescript@5.4.0)` is `vue@3.4.0`.
fn pnpm_base(key: &str) -> &str {
    key.split('(').next().unwrap_or(key)
}

/// What the check needs of a pnpm lock file, read line by line (no YAML parser): its packages
/// by key with name and version, and those that are optional or for some platforms only.
#[derive(Default)]
struct PnpmLock {
    packages: HashMap<String, (String, String)>,
    skipped: HashSet<String>,
}

impl PnpmLock {
    fn parse(text: &str) -> Self {
        let mut lock = PnpmLock::default();
        let mut major = 9;
        let mut section = String::new();
        let mut entry: Option<String> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let indent = line.len() - line.trim_start().len();

            if indent == 0 {
                let (key, value) = trimmed.split_once(':').unwrap_or((trimmed, ""));
                section = key.to_string();
                entry = None;
                if key == "lockfileVersion" {
                    let digits: String = value.trim().trim_matches(['\'', '"']).chars().take_while(char::is_ascii_digit).collect();
                    major = digits.parse().unwrap_or(9);
                }
                continue;
            }
            if section != "packages" && section != "snapshots" {
                continue;
            }

            if indent == 2 && trimmed.ends_with(':') {
                let key = trimmed.trim_end_matches(':').trim_matches(['\'', '"']).to_string();
                if section == "packages" {
                    if let Some(parsed) = pnpm_key(&key, major) {
                        lock.packages.insert(key.clone(), parsed);
                    }
                }
                entry = Some(pnpm_base(&key).to_string());
            } else if indent == 4 {
                let platform = ["os:", "cpu:", "libc:"].iter().any(|prefix| trimmed.starts_with(prefix));
                if platform || trimmed == "optional: true" {
                    if let Some(key) = &entry {
                        lock.skipped.insert(key.clone());
                    }
                }
            }
        }

        lock
    }
}

/// Name and version from a pnpm package key: `'@vue/shared@3.4.0'` (v9), `/vue@3.4.0(…)` (v6),
/// `/vue/3.4.0` (v5).
fn pnpm_key(key: &str, major: u32) -> Option<(String, String)> {
    let key = pnpm_base(key.strip_prefix('/').unwrap_or(key));
    if major <= 5 {
        let (name, version) = key.rsplit_once('/')?;
        return Some((name.to_string(), version.split('_').next()?.to_string()));
    }
    let at = key.get(1..)?.rfind('@')? + 1;
    Some((key[..at].to_string(), key[at + 1..].to_string()))
}

fn modified(file: &Path) -> Option<SystemTime> {
    fs::metadata(file).ok()?.modified().ok()
}

/// Yarn: classic keeps the lock entries it installed in `node_modules/.yarn-integrity`, which is
/// compared with yarn.lock entry by entry. Berry's install state is compressed: there, as best
/// effort, it counts as current when it is no older than yarn.lock.
fn yarn_check(dir: &Path, declared: &[String]) -> (CheckState, Vec<DepPackage>) {
    let lock = fs::read_to_string(dir.join("yarn.lock")).ok();
    let berry = dir.join(".yarnrc.yml").is_file() || lock.as_deref().is_some_and(|lock| lock.contains("__metadata:"));
    let lock_time = modified(&dir.join("yarn.lock"));
    let newer = |state: Option<SystemTime>| match (lock_time, state) {
        (Some(lock), Some(state)) => lock > state,
        _ => false,
    };

    if berry {
        let state = dir.join(".yarn").join("install-state.gz");
        if !state.is_file() {
            return none_installed(declared, |_| None);
        }
        return (if newer(modified(&state)) { CheckState::Install } else { CheckState::Ok }, Vec::new());
    }

    if !dir.join("node_modules").is_dir() {
        return none_installed(declared, |_| None);
    }
    let Some(lock) = lock else {
        return missing_from_node_modules(dir, declared, |_| None);
    };
    let integrity_file = dir.join("node_modules").join(".yarn-integrity");
    let Some(integrity) = read_json(&integrity_file) else {
        return (CheckState::Unknown, Vec::new());
    };
    let Some(installed) = integrity.get("lockfileEntries").and_then(Value::as_object) else {
        return (if newer(modified(&integrity_file)) { CheckState::Install } else { CheckState::Ok }, Vec::new());
    };

    let mut packages: Vec<DepPackage> = Vec::new();
    for entry in parse_yarn_lock(&lock) {
        let Some(resolved) = &entry.resolved else {
            continue;
        };
        if entry.patterns.iter().all(|pattern| installed.get(pattern).and_then(Value::as_str) == Some(resolved.as_str())) {
            continue;
        }
        let Some(name) = entry.patterns.first().map(|pattern| yarn_pattern_name(pattern)) else {
            continue;
        };
        if !packages.iter().any(|p| p.name == name && p.locked == entry.version) {
            packages.push(DepPackage { name, locked: entry.version.clone(), installed: None });
        }
    }
    for package in &mut packages {
        package.installed = installed_version(dir, &package.name);
    }

    (CheckState::Ok, packages)
}

struct YarnEntry {
    patterns: Vec<String>,
    version: Option<String>,
    resolved: Option<String>,
}

/// The entries of a classic yarn.lock: their patterns, version and resolved address.
fn parse_yarn_lock(text: &str) -> Vec<YarnEntry> {
    let mut entries: Vec<YarnEntry> = Vec::new();

    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            if let Some(header) = line.strip_suffix(':') {
                let patterns = header.split(", ").map(|pattern| pattern.trim().trim_matches('"').to_string()).collect();
                entries.push(YarnEntry { patterns, version: None, resolved: None });
            }
            continue;
        }
        if line.starts_with("   ") {
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        let property = line.trim();
        if let Some(version) = property.strip_prefix("version ") {
            entry.version = Some(version.trim().trim_matches('"').to_string());
        } else if let Some(resolved) = property.strip_prefix("resolved ") {
            entry.resolved = Some(resolved.trim().trim_matches('"').to_string());
        }
    }

    entries
}

/// `@babel/core@^7.0.0` is `@babel/core`.
fn yarn_pattern_name(pattern: &str) -> String {
    match pattern.get(1..).and_then(|rest| rest.find('@')) {
        Some(at) => pattern[..at + 1].to_string(),
        None => pattern.to_string(),
    }
}

/* ---------- the runtime ---------- */

/// A project uses Node with a package.json, an `.nvmrc` or a `.node-version`. What it asks for:
/// `.nvmrc`, `.node-version`, then `engines.node`.
fn node_required(dir: &Path) -> Option<Option<String>> {
    let package = dir.join("package.json");
    if !package.is_file() && !dir.join(".nvmrc").is_file() && !dir.join(".node-version").is_file() {
        return None;
    }
    for name in [".nvmrc", ".node-version"] {
        if let Ok(content) = fs::read_to_string(dir.join(name)) {
            let line = content.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#'));
            if let Some(line) = line {
                return Some(Some(line.to_string()));
            }
        }
    }
    let package = read_json(&package);
    Some(package.as_ref().and_then(|p| p.get("engines")?.get("node")?.as_str()).map(|range| range.trim().to_string()).filter(|range| !range.is_empty()))
}

/* ---------- the network scan ---------- */

/// `npm outdated --json` / `pnpm outdated --format json`: an object keyed by package (a list
/// under a name several workspaces have: the first one's versions); nothing printed means none.
fn npm_outdated(output: &str) -> Option<Vec<Outdated>> {
    if output.trim().is_empty() {
        return Some(Vec::new());
    }
    let value = json_object(output)?;
    if value.get("error").is_some() {
        return None;
    }
    let packages = value.as_object()?.iter().map(|(name, found)| {
        let found = found.as_array().and_then(|list| list.first()).unwrap_or(found);
        Outdated { name: name.clone(), installed: owned(found, "current"), wanted: owned(found, "wanted"), latest: owned(found, "latest") }
    });
    Some(packages.collect())
}

/// `npm audit --json` / `pnpm audit --json`: the vulnerable packages' total in `metadata`, and
/// the advisories against them. npm lists `vulnerabilities` by package, each with what it comes
/// `via` (an advisory of its own, or the name of the vulnerable package it depends on); pnpm
/// lists `advisories` by id, as the registry writes them.
fn npm_audit(output: &str) -> Option<Audit> {
    let value = json_object(output)?;
    if value.get("error").is_some() {
        return None;
    }
    let severities = value.get("metadata")?.get("vulnerabilities")?.as_object()?;
    let total = match severities.get("total").and_then(Value::as_u64) {
        Some(total) => total,
        None => severities.values().filter_map(Value::as_u64).sum(),
    };

    let mut advisories = Vec::new();
    for (name, package) in value.get("vulnerabilities").and_then(Value::as_object).into_iter().flatten() {
        let via = package.get("via").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        // `fixAvailable`: whether `npm audit fix` has one, or the package it would install.
        let (fix, fixable) = match package.get("fixAvailable") {
            Some(Value::Bool(fixable)) => (None, Some(*fixable)),
            Some(to) if to.is_object() => (text(to, "name").zip(text(to, "version")).map(|(name, version)| format!("{name}@{version}")), Some(true)),
            _ => (None, None),
        };
        let of_package = |advisory: Advisory| Advisory {
            package: name.clone(),
            direct: package.get("isDirect").and_then(Value::as_bool),
            fix: fix.clone(),
            fixable,
            ..advisory
        };
        let own: Vec<&Value> = via.iter().filter(|source| source.is_object()).collect();
        if own.is_empty() {
            let with: Vec<&str> = via.iter().filter_map(Value::as_str).collect();
            advisories.push(of_package(Advisory {
                severity: severity(text(package, "severity")),
                affected: owned(package, "range"),
                via: (!with.is_empty()).then(|| with.join(", ")),
                ..Advisory::default()
            }));
        }
        for source in own {
            advisories.push(of_package(Advisory {
                severity: severity(text(source, "severity").or_else(|| text(package, "severity"))),
                title: owned(source, "title"),
                url: owned(source, "url"),
                affected: owned(source, "range"),
                ..Advisory::default()
            }));
        }
    }
    advisories.extend(value.get("advisories").and_then(Value::as_object).into_iter().flatten().map(|(_, advisory)| registry_advisory(advisory)));

    Some(Audit { packages: u32::try_from(total).unwrap_or(u32::MAX), advisories })
}

/// An advisory as the npm registry writes it (pnpm's and Yarn classic's audits): the module,
/// the versions it is in and the ones that patch it, and the version found in the project.
fn registry_advisory(advisory: &Value) -> Advisory {
    let found = advisory.get("findings").and_then(Value::as_array).and_then(|findings| findings.first());
    Advisory {
        package: text(advisory, "module_name").unwrap_or_default().to_string(),
        installed: found.and_then(|finding| owned(finding, "version")),
        severity: severity(text(advisory, "severity")),
        title: owned(advisory, "title"),
        url: owned(advisory, "url"),
        affected: owned(advisory, "vulnerable_versions"),
        // `<0.0.0`: the registry's way of saying no version patches it.
        fix: owned(advisory, "patched_versions").filter(|versions| versions != "<0.0.0"),
        ..Advisory::default()
    }
}

/// Yarn classic prints one JSON object per line.
fn json_lines(output: &str) -> impl Iterator<Item = Value> + '_ {
    output.lines().filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
}

/// `yarn outdated --json`: the rows of its `table` line (package, current, wanted, latest, …);
/// none when there is no table.
fn yarn_outdated(output: &str) -> Option<Vec<Outdated>> {
    let mut failed = false;
    for line in json_lines(output) {
        match text(&line, "type") {
            Some("table") => {
                let cell = |row: &Value, at: usize| row.get(at).and_then(Value::as_str).filter(|cell| !cell.is_empty()).map(str::to_string);
                let rows = line.get("data")?.get("body")?.as_array()?.iter();
                return Some(rows.map(|row| Outdated { name: cell(row, 0).unwrap_or_default(), installed: cell(row, 1), wanted: cell(row, 2), latest: cell(row, 3) }).collect());
            }
            Some("error") => failed = true,
            _ => {}
        }
    }
    (!failed).then(Vec::new)
}

/// `yarn audit --json`: the counts of its `auditSummary` line, and the advisories of its
/// `auditAdvisory` lines (one a path to the package: each advisory once).
fn yarn_audit(output: &str) -> Option<Audit> {
    let mut total: Option<u64> = None;
    let mut advisories: Vec<Advisory> = Vec::new();
    for line in json_lines(output) {
        match text(&line, "type") {
            Some("auditSummary") => total = Some(line.get("data")?.get("vulnerabilities")?.as_object()?.values().filter_map(Value::as_u64).sum()),
            Some("auditAdvisory") => {
                if let Some(advisory) = line.get("data").and_then(|data| data.get("advisory")).map(registry_advisory) {
                    if !advisories.contains(&advisory) {
                        advisories.push(advisory);
                    }
                }
            }
            _ => {}
        }
    }
    Some(Audit { packages: u32::try_from(total?).unwrap_or(u32::MAX), advisories })
}
