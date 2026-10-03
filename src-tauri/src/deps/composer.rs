//! Composer: composer.lock against `vendor/composer/installed.json`. The runtime is PHP, as
//! composer.json's `require.php` asks.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::{count, json_object, none_installed, owned, read_json, severity, text, version_line};
use super::{Advisory, Audit, CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadAudit, ReadOutdated, Runtime, ScanPlan};

pub struct Composer;

static RUNTIMES: [Runtime; 1] =
    [Runtime { name: "php", probe: "php -r \"echo PHP_VERSION;\"", version: version_line, required: php_required, local: None, flavor: Flavor::Composer }];

impl Ecosystem for Composer {
    fn id(&self) -> &'static str {
        "composer"
    }

    fn manifest(&self) -> &'static str {
        "composer.json"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        dir.join("composer.json").is_file().then_some("composer")
    }

    fn watched(&self) -> &'static [&'static str] {
        &["composer.json", "composer.lock", "vendor", "vendor/composer/installed.json"]
    }

    fn check(&self, dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        let Some(composer) = read_json(&dir.join("composer.json")) else {
            return (CheckState::Unknown, Vec::new());
        };
        check_composer(dir, &composer, read_json(&dir.join("composer.lock")).as_ref())
    }

    fn needs(&self, _dir: &Path, _manager: &str) -> &'static [&'static str] {
        &["composer"]
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, _dir: &Path, _manager: &str) -> Option<&'static str> {
        Some("composer install")
    }

    fn scan_plan(&self, _dir: &Path, _manager: &str) -> ScanPlan {
        ScanPlan {
            outdated: Some(("composer outdated --direct --format=json --no-interaction", composer_outdated as ReadOutdated)),
            audit: Some(("composer audit --format=json --no-interaction", composer_audit as ReadAudit)),
        }
    }
}

pub(super) struct Locked {
    pub(super) name: String,
    pub(super) version: String,
    reference: Option<String>,
}

fn composer_entry(package: &Value) -> Option<Locked> {
    let reference = package.get("source").and_then(|s| text(s, "reference")).or_else(|| package.get("dist").and_then(|d| text(d, "reference")));
    Some(Locked {
        name: text(package, "name")?.to_lowercase(),
        version: text(package, "version")?.to_string(),
        reference: reference.map(str::to_string),
    })
}

/// composer.lock's `packages`, and `packages-dev` when `dev`.
pub(super) fn locked_packages(lock: &Value, dev: bool) -> Vec<Locked> {
    let sections: &[&str] = if dev { &["packages", "packages-dev"] } else { &["packages"] };
    sections.iter().filter_map(|key| lock.get(*key)?.as_array()).flatten().filter_map(composer_entry).collect()
}

/// A branch install (`dev-main`, `2.x-dev`): the same version name can be another commit.
fn is_branch(version: &str) -> bool {
    version.starts_with("dev-") || version.ends_with("-dev")
}

/// composer.lock against `vendor/composer/installed.json`, Composer's record of what it
/// installed (Composer 2's object or Composer 1's list). An install without dev packages
/// (`dev: false`) isn't asked for them.
fn check_composer(dir: &Path, composer: &Value, lock: Option<&Value>) -> (CheckState, Vec<DepPackage>) {
    // Platform requirements (php, ext-*, lib-*, composer-*) have no slash.
    let declared = |dev: bool| -> Vec<String> {
        let sections: &[&str] = if dev { &["require", "require-dev"] } else { &["require"] };
        let mut names: Vec<String> = sections
            .iter()
            .filter_map(|key| composer.get(*key)?.as_object())
            .flat_map(|require| require.keys().map(|name| name.to_lowercase()))
            .filter(|name| name.contains('/'))
            .collect();
        names.sort();
        names.dedup();
        names
    };

    let installed = dir.join("vendor").is_dir().then(|| read_json(&dir.join("vendor").join("composer").join("installed.json"))).flatten();
    let Some(installed) = installed else {
        return match lock {
            Some(lock) => {
                let all: Vec<DepPackage> =
                    locked_packages(lock, true).into_iter().map(|p| DepPackage { name: p.name, locked: Some(p.version), installed: None }).collect();
                (if all.is_empty() { CheckState::Ok } else { CheckState::Install }, all)
            }
            None => none_installed(&declared(true), |_| None),
        };
    };

    let (list, dev) = match &installed {
        Value::Array(list) => (Some(list), true),
        Value::Object(record) => (record.get("packages").and_then(Value::as_array), record.get("dev").and_then(Value::as_bool).unwrap_or(true)),
        _ => (None, true),
    };
    let have: HashMap<String, Locked> = list.into_iter().flatten().filter_map(composer_entry).map(|p| (p.name.clone(), p)).collect();

    let packages: Vec<DepPackage> = match lock {
        Some(lock) => locked_packages(lock, dev)
            .into_iter()
            .filter_map(|wanted| {
                let found = have.get(&wanted.name);
                let same = found.is_some_and(|found| found.version == wanted.version && (!is_branch(&wanted.version) || found.reference == wanted.reference));
                (!same).then(|| DepPackage { name: wanted.name, locked: Some(wanted.version), installed: found.map(|f| f.version.clone()) })
            })
            .collect(),
        None => declared(dev)
            .into_iter()
            .filter(|name| !have.contains_key(name))
            .map(|name| DepPackage { name, locked: None, installed: None })
            .collect(),
    };

    (CheckState::Ok, packages)
}

/* ---------- the runtime ---------- */

/// A project with a composer.json uses PHP; what it asks for is `require.php`.
fn php_required(dir: &Path) -> Option<Option<String>> {
    let file = dir.join("composer.json");
    file.is_file().then(|| read_json(&file).as_ref().and_then(|composer| composer.get("require")?.get("php")?.as_str()).map(str::to_string))
}

/* ---------- the network scan ---------- */

/// `composer outdated --direct --format=json`: the listed packages that aren't up to date. One
/// whose newest version the constraint allows (`semver-safe-update`) has it as `wanted` too.
fn composer_outdated(output: &str) -> Option<Vec<Outdated>> {
    let value = json_object(output)?;
    let installed = match value.get("installed") {
        Some(Value::Array(list)) => list,
        Some(_) => return None,
        None => return Some(Vec::new()),
    };
    let behind = installed.iter().filter(|package| text(package, "latest-status") != Some("up-to-date"));
    let packages = behind.map(|package| {
        let latest = owned(package, "latest");
        let wanted = if text(package, "latest-status") == Some("semver-safe-update") { latest.clone() } else { None };
        Outdated { name: text(package, "name").unwrap_or_default().to_string(), installed: owned(package, "version"), wanted, latest }
    });
    Some(packages.collect())
}

/// `composer audit --format=json`: the packages with advisories (`advisories` is `[]` when
/// there are none), and each one's advisories: a list, or an object by index.
fn composer_audit(output: &str) -> Option<Audit> {
    let value = json_object(output)?;
    let (packages, lists): (usize, Vec<&Value>) = match value.get("advisories")? {
        Value::Object(map) => (map.values().filter(|list| list.as_array().is_none_or(|list| !list.is_empty())).count(), map.values().collect()),
        Value::Array(list) => (list.len(), list.iter().collect()),
        _ => return None,
    };
    let each = |list: &'_ Value| -> Vec<Value> {
        match list {
            Value::Array(items) => items.clone(),
            Value::Object(items) if items.contains_key("packageName") => vec![list.clone()],
            Value::Object(items) => items.values().cloned().collect(),
            _ => Vec::new(),
        }
    };
    let advisories = lists.into_iter().flat_map(each).filter_map(|advisory| {
        Some(Advisory {
            package: text(&advisory, "packageName")?.to_string(),
            severity: severity(text(&advisory, "severity")),
            title: owned(&advisory, "title"),
            url: owned(&advisory, "link"),
            affected: owned(&advisory, "affectedVersions"),
            ..Advisory::default()
        })
    });
    Some(Audit { packages: count(packages), advisories: advisories.collect() })
}
