//! Python: uv, Poetry, Pipenv and pip, one ecosystem (`python`), the manager told by the lock
//! file. The lock is compared with what the project's own virtual environment has
//! (`.venv/lib/python*/site-packages/*.dist-info/METADATA`), names as PEP 503 spells them. An
//! environment kept elsewhere (Poetry's and Pipenv's default folders, pip's global one) can't be
//! seen without running the tool: the check is `Unknown` then. The runtime is Python:
//! `.python-version`, then pyproject.toml, then the Pipfile; the active one is the project
//! environment's own, where it has one.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::{owned, read_json, text, version_in};
use super::{CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadOutdated, Runtime, ScanPlan};

pub struct Python;

static RUNTIMES: [Runtime; 1] = [Runtime {
    name: "python",
    probe: "python3 --version",
    version: version_in,
    required: python_required,
    local: Some(environment_python),
    flavor: Flavor::Custom(python_satisfies),
}];

/// A folder a project keeps its virtual environment in, and that environment's own commands.
struct Environment {
    folder: &'static str,
    /// Its interpreter, which the commands run (`bin/python`: there is none on Windows).
    python: &'static str,
    install: &'static str,
    outdated: &'static str,
}

macro_rules! environment {
    ($folder:literal) => {
        Environment {
            folder: $folder,
            python: concat!($folder, "/bin/python"),
            install: concat!($folder, "/bin/python -m pip install -r requirements.txt"),
            outdated: concat!($folder, "/bin/python -m pip list --outdated --format=json"),
        }
    };
}

/// uv, Poetry and Pipenv only ever use the first.
static ENVIRONMENTS: [Environment; 3] = [environment!(".venv"), environment!("venv"), environment!("env")];

/// The manifests and locks, and per environment folder what changes when it is made or
/// something is installed into it: `pyvenv.cfg` and `site-packages`, whose path has Python's
/// version in it.
macro_rules! watched {
    ($($folder:literal),*) => {
        &[
            "pyproject.toml",
            "uv.lock",
            "poetry.lock",
            "poetry.toml",
            "Pipfile",
            "Pipfile.lock",
            "requirements.txt",
            ".python-version",
            $(
                concat!($folder, "/pyvenv.cfg"),
                concat!($folder, "/lib/python3.8/site-packages"),
                concat!($folder, "/lib/python3.9/site-packages"),
                concat!($folder, "/lib/python3.10/site-packages"),
                concat!($folder, "/lib/python3.11/site-packages"),
                concat!($folder, "/lib/python3.12/site-packages"),
                concat!($folder, "/lib/python3.13/site-packages"),
                concat!($folder, "/lib/python3.14/site-packages"),
                concat!($folder, "/lib/python3.15/site-packages"),
                concat!($folder, "/Lib/site-packages"),
            )*
        ]
    };
}

impl Ecosystem for Python {
    fn id(&self) -> &'static str {
        "python"
    }

    fn manifest(&self) -> &'static str {
        "requirements.txt"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        let has = |name: &str| dir.join(name).is_file();
        if has("uv.lock") {
            Some("uv")
        } else if has("poetry.lock") || poetry_project(dir) {
            Some("poetry")
        } else if has("Pipfile.lock") {
            Some("pipenv")
        } else if has("requirements.txt") {
            Some("pip")
        } else {
            None
        }
    }

    fn watched(&self) -> &'static [&'static str] {
        watched!(".venv", "venv", "env")
    }

    fn check(&self, dir: &Path, manager: &str) -> (CheckState, Vec<DepPackage>) {
        let unknown = (CheckState::Unknown, Vec::new());
        let Some(locked) = locked(dir, manager) else {
            return unknown;
        };
        // requirements.txt with nothing but options and addresses says nothing to compare.
        if manager == "pip" && locked.is_empty() {
            return unknown;
        }
        let have = match environment(dir, manager) {
            Some(environment) => match site_packages(&dir.join(environment.folder)) {
                Some(folder) => installed(&folder),
                None => return unknown,
            },
            None if in_project(dir, manager) => HashMap::new(),
            None => return unknown,
        };
        (CheckState::Ok, differing(&locked, &have))
    }

    fn needs(&self, _dir: &Path, manager: &str) -> &'static [&'static str] {
        match manager {
            "uv" => &["uv"],
            "poetry" => &["poetry"],
            "pipenv" => &["pipenv"],
            // pip runs from the project's environment.
            _ => &[],
        }
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, dir: &Path, manager: &str) -> Option<&'static str> {
        match manager {
            "uv" => Some("uv sync"),
            "poetry" => Some("poetry install"),
            "pipenv" => Some("pipenv sync"),
            // Only into the project's own environment, never a global one.
            _ => runnable(dir, manager).map(|environment| environment.install),
        }
    }

    /// The outdated packages of the project's environment, by uv or by the environment's pip.
    /// There is no audit: `pip-audit` and `safety` are tools of their own.
    fn scan_plan(&self, dir: &Path, manager: &str) -> ScanPlan {
        let outdated = match manager {
            "uv" => environment(dir, manager).map(|_| "uv pip list --outdated --format=json"),
            _ => runnable(dir, manager).map(|environment| environment.outdated),
        };
        ScanPlan { outdated: outdated.map(|command| (command, pip_outdated as ReadOutdated)), audit: None }
    }
}

/// pyproject.toml has a `[tool.poetry…]` table.
fn poetry_project(dir: &Path) -> bool {
    fs::read_to_string(dir.join("pyproject.toml")).is_ok_and(|text| text.lines().any(|line| line.trim_start().starts_with("[tool.poetry")))
}

/// The value of `key` under `[table]` (before any table: `""`), read line by line: a string
/// without its quotes, anything else as written; no inline tables or lists. Enough for what is
/// asked of pyproject.toml, poetry.toml, the Pipfile, `pyvenv.cfg` and alembic.ini.
pub(super) fn setting(text: &str, table: &str, key: &str) -> Option<String> {
    let mut inside = table.is_empty();
    for line in text.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            inside = !header.starts_with('[') && header.split(']').next().map(str::trim) == Some(table);
            continue;
        }
        if !inside {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim().trim_matches(['"', '\'']) != key {
            continue;
        }
        let value = value.trim();
        return match value.chars().next()? {
            '{' | '[' => None,
            quote @ ('"' | '\'') => value[1..].split(quote).next().map(str::to_string),
            _ => value.split('#').next().map(|value| value.trim().to_string()),
        };
    }
    None
}

/* ---------- the environment ---------- */

/// The project's own virtual environment: the first of the folders that has a `pyvenv.cfg`.
fn environment(dir: &Path, manager: &str) -> Option<&'static Environment> {
    let known = if manager == "pip" { &ENVIRONMENTS[..] } else { &ENVIRONMENTS[..1] };
    known.iter().find(|environment| dir.join(environment.folder).join("pyvenv.cfg").is_file())
}

/// The project's environment, when its interpreter is where the commands look for it.
fn runnable(dir: &Path, manager: &str) -> Option<&'static Environment> {
    environment(dir, manager).filter(|environment| dir.join(environment.python).exists())
}

/// Whether the manager keeps the environment in the project, so that none there means nothing
/// is installed: uv does, Poetry when poetry.toml says so. Pipenv's and pip's can be anywhere.
fn in_project(dir: &Path, manager: &str) -> bool {
    match manager {
        "uv" => true,
        "poetry" => fs::read_to_string(dir.join("poetry.toml")).ok().and_then(|text| setting(&text, "virtualenvs", "in-project")).as_deref() == Some("true"),
        _ => false,
    }
}

/// The environment's `site-packages`: `lib/python3.12/site-packages`, on Windows
/// `Lib/site-packages`.
fn site_packages(environment: &Path) -> Option<PathBuf> {
    let windows = environment.join("Lib").join("site-packages");
    if windows.is_dir() {
        return Some(windows);
    }
    let versions = fs::read_dir(environment.join("lib")).ok()?;
    versions
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("python"))
        .map(|entry| entry.path().join("site-packages"))
        .find(|folder| folder.is_dir())
}

/// What the environment has, by PEP 503 name: `Name` and `Version` of every
/// `*.dist-info/METADATA`.
fn installed(site_packages: &Path) -> HashMap<String, String> {
    let mut have = HashMap::new();
    for entry in fs::read_dir(site_packages).into_iter().flatten().flatten() {
        let folder = entry.path();
        if folder.extension().is_some_and(|extension| extension == "dist-info") {
            if let Some((name, version)) = metadata(&folder.join("METADATA")) {
                have.insert(normal(&name), version);
            }
        }
    }
    have
}

/// `Name` and `Version` among a METADATA file's headers; the description after them isn't read.
fn metadata(file: &Path) -> Option<(String, String)> {
    let mut head = Vec::new();
    fs::File::open(file).ok()?.take(8 * 1024).read_to_end(&mut head).ok()?;
    let head = String::from_utf8_lossy(&head);
    let header =
        |key: &str| head.lines().take_while(|line| !line.trim().is_empty()).find_map(|line| line.strip_prefix(key)).map(|value| value.trim().to_string());
    Some((header("Name:")?, header("Version:")?))
}

/// A package name as PEP 503 compares it: `Typing_Extensions` and `typing.extensions` are
/// `typing-extensions`.
fn normal(name: &str) -> String {
    let mut normal = String::with_capacity(name.len());
    for c in name.trim().chars() {
        if !matches!(c, '-' | '_' | '.') {
            normal.push(c.to_ascii_lowercase());
        } else if !normal.ends_with('-') {
            normal.push('-');
        }
    }
    normal
}

/// The same version, as PEP 440 sees it for plain releases: `1.0` is `1.0.0`.
fn same(locked: &str, installed: &str) -> bool {
    let plain = |version: &str| {
        let version = version.trim().trim_start_matches(['v', 'V']).to_ascii_lowercase();
        if version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            version.trim_end_matches(".0").to_string()
        } else {
            version
        }
    };
    plain(locked) == plain(installed)
}

/// The version of `name` (PEP 503) in the project's environment.
pub(super) fn installed_version(dir: &Path, name: &str) -> Option<String> {
    installed(&site_packages(&dir.join(environment(dir, "pip")?.folder))?).remove(name)
}

/* ---------- the lock files ---------- */

/// A package a lock file names.
struct Locked {
    /// As PEP 503 spells it.
    name: String,
    /// `None`: asked for without an exact version (requirements.txt).
    version: Option<String>,
    /// Installed on every platform and Python: not optional, behind no environment marker. One
    /// that isn't may rightly be absent, so only the version of an installed one is compared.
    always: bool,
}

fn locked(dir: &Path, manager: &str) -> Option<Vec<Locked>> {
    let read = |name: &str| fs::read_to_string(dir.join(name)).ok();
    let project = || read("pyproject.toml").unwrap_or_default();
    Some(match manager {
        // `uv sync` installs the `dev` group, unless the project names its own default ones.
        "uv" => {
            let dev = !project().contains("default-groups");
            toml_locked(&read("uv.lock")?, &|group| dev && group == "dev", &HashSet::new())
        }
        "poetry" => {
            let (groups, names) = poetry_optional(&project());
            toml_locked(&read("poetry.lock")?, &|group| !groups.contains(group), &names)
        }
        "pipenv" => pipfile_locked(&read_json(&dir.join("Pipfile.lock"))?),
        _ => requirements(&read("requirements.txt")?),
    })
}

/// Whether the project's lock names `name` (PEP 503), and the version it pins it at, if any.
pub(super) fn locked_version(dir: &Path, name: &str) -> Option<Option<String>> {
    locked(dir, Python.detect(dir)?)?.into_iter().find(|package| package.name == name).map(|package| package.version)
}

/// The locked packages the environment has at another version, and those it lacks though they
/// are always installed. A name locked at several versions (one per Python) is fine at any.
fn differing(locked: &[Locked], have: &HashMap<String, String>) -> Vec<DepPackage> {
    let mut by_name: BTreeMap<&str, Vec<&Locked>> = BTreeMap::new();
    for package in locked {
        by_name.entry(&package.name).or_default().push(package);
    }
    by_name
        .into_iter()
        .filter_map(|(name, wanted)| {
            let installed = have.get(name);
            let fine = match installed {
                Some(installed) => wanted.iter().any(|package| package.version.as_deref().is_none_or(|version| same(version, installed))),
                None => !wanted.iter().any(|package| package.always),
            };
            (!fine).then(|| DepPackage { name: name.to_string(), locked: wanted[0].version.clone(), installed: installed.cloned() })
        })
        .collect()
}

/// A `[[package]]` of uv.lock or poetry.lock, as far as the check needs it.
#[derive(Default)]
struct Entry {
    name: String,
    version: String,
    /// The project itself or a folder of it (uv's `editable`, `virtual`, `directory`): not
    /// compared.
    local: bool,
    /// Poetry's `optional = true`, `markers` of its own, or only in groups its install leaves
    /// out.
    conditional: bool,
    /// The packages it needs, and whether always: without a marker, not for an extra.
    needs: Vec<(String, bool)>,
}

/// uv.lock and poetry.lock, read line by line (no TOML parser) for the name and version of each
/// `[[package]]`, and for what tells whether it is installed everywhere.
///
/// uv: `dependencies = [` with one `{ name = "…", marker = "…" }` a line, and the lists of
/// `[package.dev-dependencies]` (dependency groups) and `[package.optional-dependencies]`
/// (extras: never). Poetry: `optional`, `markers` and `groups` (lock 2.1) on the package, and
/// `[package.dependencies]` with `name = {…, markers = "…"}`.
///
/// `installs` says whether the manager's install takes a dependency group; `apart` are the
/// packages of Poetry's optional groups, which an older lock doesn't tell from the others.
fn toml_locked(text: &str, installs: &dyn Fn(&str) -> bool, apart: &HashSet<String>) -> Vec<Locked> {
    enum Table {
        Package,
        Dev,
        Extras,
        Needs,
        Other,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut table = Table::Other;
    // The uv list being read, and whether its packages are always installed.
    let mut list: Option<bool> = None;
    // Poetry's lock 2.1 says per package where it is installed.
    let mut settled = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if line.starts_with('[') {
            list = None;
            table = match trimmed {
                "[[package]]" => {
                    entries.push(Entry::default());
                    Table::Package
                }
                "[package.dev-dependencies]" => Table::Dev,
                "[package.optional-dependencies]" => Table::Extras,
                "[package.dependencies]" => Table::Needs,
                _ => Table::Other,
            };
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        if let Some(always) = list {
            if trimmed.starts_with(']') {
                list = None;
            } else {
                needs(trimmed, always, entry);
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().trim_matches('"'), value.trim());
        let quoted = || value.split('"').nth(1).unwrap_or_default().to_string();
        let mut open = |always: bool, entry: &mut Entry| {
            needs(value, always, entry);
            list = (!value.ends_with(']')).then_some(always);
        };

        match (&table, key) {
            (Table::Package, "name") => entry.name = normal(&quoted()),
            (Table::Package, "version") => entry.version = quoted(),
            (Table::Package, "source") => {
                let kind = value.trim_start_matches('{').trim_start();
                entry.local = ["editable", "virtual", "directory"].iter().any(|local| kind.starts_with(local));
            }
            (Table::Package, "optional") => entry.conditional |= value.starts_with("true"),
            (Table::Package, "markers") => entry.conditional = true,
            (Table::Package, "groups") => {
                settled = true;
                entry.conditional |= !value.split('"').skip(1).step_by(2).any(installs);
            }
            (Table::Package, "dependencies") if value.starts_with('[') => open(true, entry),
            (Table::Dev, group) if value.starts_with('[') => open(installs(group), entry),
            (Table::Extras, _) if value.starts_with('[') => open(false, entry),
            // A list of `{version = "…", markers = "…"}` is one need per marker.
            (Table::Needs, name) if !name.starts_with(['{', ']']) => {
                let always = value != "[" && !value.contains("markers") && !value.contains("optional = true");
                entry.needs.push((normal(name), always));
            }
            _ => {}
        }
    }

    let always = if settled {
        entries.iter().filter(|entry| !entry.conditional).map(|entry| entry.name.clone()).collect()
    } else {
        for entry in &mut entries {
            entry.conditional |= apart.contains(&entry.name);
        }
        reached(&entries)
    };
    entries
        .into_iter()
        .filter(|entry| !entry.local && !entry.name.is_empty() && !entry.version.is_empty())
        .map(|entry| Locked { always: always.contains(&entry.name), name: entry.name, version: Some(entry.version) })
        .collect()
}

/// The dependency groups `poetry install` leaves out (`[tool.poetry.group.docs]` with
/// `optional = true` in pyproject.toml), and the packages those name.
fn poetry_optional(pyproject: &str) -> (HashSet<String>, HashSet<String>) {
    let mut groups: HashSet<String> = HashSet::new();
    let mut named: Vec<(&str, String)> = Vec::new();
    let mut table = "";
    for line in pyproject.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line.trim_matches(['[', ']']).trim().strip_prefix("tool.poetry.group.").unwrap_or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        if let Some(group) = table.strip_suffix(".dependencies") {
            named.push((group, normal(key)));
        } else if !table.is_empty() && key == "optional" && value.trim().starts_with("true") {
            groups.insert(table.to_string());
        }
    }
    let names = named.into_iter().filter(|(group, _)| groups.contains(*group)).map(|(_, name)| name).collect();
    (groups, names)
}

/// uv's `{ name = "tzdata", marker = "sys_platform == 'win32'" }`, any number on a line.
fn needs(line: &str, always: bool, entry: &mut Entry) {
    for need in line.split("name = \"").skip(1) {
        let name = need.split('"').next().unwrap_or_default();
        entry.needs.push((normal(name), always && !need.contains("marker = ")));
    }
}

/// The names installed everywhere: from the project itself (uv) and the packages no other asks
/// for (Poetry's lock leaves the project out: its direct ones), along every need that is always
/// one. What only a marker or an extra leads to may be absent.
fn reached(entries: &[Entry]) -> HashSet<String> {
    let mut by_name: HashMap<&str, Vec<&Entry>> = HashMap::new();
    for entry in entries {
        by_name.entry(&entry.name).or_default().push(entry);
    }
    let asked: HashSet<&str> = entries.iter().flat_map(|entry| &entry.needs).map(|(name, _)| name.as_str()).collect();
    let mut queue: Vec<&str> = entries.iter().filter(|entry| entry.local || !asked.contains(entry.name.as_str())).map(|entry| entry.name.as_str()).collect();
    let mut reached = HashSet::new();

    while let Some(name) = queue.pop() {
        let Some(found) = by_name.get(name) else {
            continue;
        };
        if found.iter().all(|entry| entry.conditional) || !reached.insert(name.to_string()) {
            continue;
        }
        let always = found.iter().flat_map(|entry| &entry.needs).filter(|(_, always)| *always);
        queue.extend(always.map(|(name, _)| name.as_str()));
    }
    reached
}

/// A marker that only sets Python's lowest version (`python_version >= '3.8'`), as Pipenv
/// writes for nearly every package: the environment meets it, the package is installed.
fn lowest_python_only(marker: &str) -> bool {
    static CLAUSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\(?\s*python(_full)?_version\s*>=?\s*['"][\d.]+['"]\s*\)?$"#).unwrap());
    marker.split(" and ").all(|clause| CLAUSE.is_match(clause.trim()))
}

/// Pipfile.lock: `default` and `develop`, each package's `"version": "==1.2.3"`. `pipenv sync`
/// leaves `develop` out, and a package with a marker may be absent.
fn pipfile_locked(lock: &Value) -> Vec<Locked> {
    let mut locked = Vec::new();
    for (section, synced) in [("default", true), ("develop", false)] {
        for (name, package) in lock.get(section).and_then(Value::as_object).into_iter().flatten() {
            // Editable and VCS packages have no version.
            let Some(version) = text(package, "version").and_then(|version| version.strip_prefix("==")) else {
                continue;
            };
            let always = synced && text(package, "markers").is_none_or(lowest_python_only);
            locked.push(Locked { name: normal(name), version: Some(version.to_string()), always });
        }
    }
    locked
}

/// requirements.txt: only an exact `name==1.2.3` is a locked version; any other requirement is
/// only looked for. Options (`-r`, `-e`, `--hash`), paths and addresses are passed over; a
/// requirement with a marker after `;` may be absent.
fn requirements(text: &str) -> Vec<Locked> {
    let requirement = |line: &str| {
        let line = line.split(" #").next().unwrap_or_default().trim().trim_end_matches('\\');
        let line = line.split(" --").next().unwrap_or_default().trim();
        if line.is_empty() || line.starts_with(['#', '-', '.', '/']) || line.contains("://") || line.contains('@') {
            return None;
        }
        let (asked, marker) = match line.split_once(';') {
            Some((asked, marker)) => (asked.trim(), Some(marker.trim())),
            None => (line, None),
        };
        let end = asked.find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_' | '.')).unwrap_or(asked.len());
        let (name, rest) = asked.split_at(end);
        // Past the extras of `name[extra]==1.2.3`.
        let rest = if rest.starts_with('[') { rest.split_once(']').map_or("", |(_, rest)| rest) } else { rest }.trim();
        let pinned = rest.strip_prefix("===").or_else(|| rest.strip_prefix("==")).map(str::trim);
        let version = pinned.filter(|version| !version.is_empty() && !version.contains(['*', ',', ' ']));
        (!name.is_empty()).then(|| Locked { name: normal(name), version: version.map(str::to_string), always: marker.is_none_or(lowest_python_only) })
    };
    text.lines().filter_map(requirement).collect()
}

/* ---------- the runtime ---------- */

/// A project uses Python with any of the managers' files or a `.python-version`. What it asks
/// for: `.python-version`, then pyproject.toml's `requires-python` (or Poetry's `python`), then
/// the Pipfile's `python_full_version` or `python_version`.
fn python_required(dir: &Path) -> Option<Option<String>> {
    const USES: [&str; 7] = [".python-version", "pyproject.toml", "uv.lock", "poetry.lock", "Pipfile", "Pipfile.lock", "requirements.txt"];
    if !USES.iter().any(|name| dir.join(name).is_file()) {
        return None;
    }
    let read = |name: &str| fs::read_to_string(dir.join(name)).ok();
    let pinned = || {
        let text = read(".python-version")?;
        text.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_string)
    };
    let project = || {
        let text = read("pyproject.toml")?;
        setting(&text, "project", "requires-python").or_else(|| setting(&text, "tool.poetry.dependencies", "python"))
    };
    let pipfile = || {
        let text = read("Pipfile")?;
        setting(&text, "requires", "python_full_version").or_else(|| setting(&text, "requires", "python_version"))
    };
    Some(pinned().or_else(project).or_else(pipfile).filter(|asked| !asked.is_empty()))
}

/// The version of the project environment's Python: `version` or `version_info` of its
/// `pyvenv.cfg` (`3.12.4`, `3.12.4.final.0`).
fn environment_python(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(environment(dir, "pip")?.folder).join("pyvenv.cfg")).ok()?;
    ["version", "version_info"].iter().find_map(|key| setting(&text, "", key)).and_then(|version| version_in(&version))
}

/// A version's numbers, and whether a wildcard ends it: `3.12.*` is `([3, 12], true)`. What
/// follows the numbers (`rc1`, `t`, `+local`) is dropped. `None`: no version (`pypy3.10`).
fn release(version: &str) -> Option<(Vec<u64>, bool)> {
    let mut numbers = Vec::new();
    for part in version.trim().trim_start_matches(['v', 'V']).split('.') {
        if part == "*" {
            return Some((numbers, true));
        }
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            break;
        }
        numbers.push(digits.parse().ok()?);
        if digits.len() < part.len() {
            break;
        }
    }
    (!numbers.is_empty()).then_some((numbers, false))
}

/// Numbers against numbers, the shorter padded with zeros as PEP 440 does: 3.12 is 3.12.0.
fn compare(version: &[u64], other: &[u64]) -> Ordering {
    let at = |numbers: &[u64], index: usize| numbers.get(index).copied().unwrap_or(0);
    (0..version.len().max(other.len())).map(|index| at(version, index).cmp(&at(other, index))).find(|order| order.is_ne()).unwrap_or(Ordering::Equal)
}

/// Whether Python `version` meets what the project asks for: PEP 440's specifiers (`>=3.11`,
/// `~=3.12`, `<4`, `==3.13.*`, `!=3.9.1`, joined by commas), a bare version as `.python-version`
/// and the Pipfile have it (`3.12` is any 3.12.x), and Poetry's `^3.11`, `~3.11` and `||`. As in
/// PEP 440, `<=3.12` stops at 3.12.0 and `==3.12` is only that. `None` when either can't be read
/// (`pypy3.10`, `system`).
fn python_satisfies(constraint: &str, version: &str) -> Option<bool> {
    let (version, _) = release(version)?;
    let mut any = false;
    for alternative in constraint.replace("||", "|").split('|') {
        // `>= 3.11` is one clause.
        let mut clauses: Vec<String> = Vec::new();
        let mut operator = String::new();
        for word in alternative.split([',', ' ']).filter(|word| !word.is_empty()) {
            if word.chars().all(|c| matches!(c, '<' | '>' | '=' | '!' | '~' | '^')) {
                operator.push_str(word);
            } else {
                clauses.push(format!("{operator}{word}"));
                operator.clear();
            }
        }
        if clauses.is_empty() || !operator.is_empty() {
            return None;
        }
        let mut all = true;
        for clause in clauses {
            all &= clause_holds(&clause, &version)?;
        }
        any |= all;
    }
    Some(any)
}

fn clause_holds(clause: &str, version: &[u64]) -> Option<bool> {
    const OPERATORS: [&str; 10] = ["===", "==", "!=", "~=", ">=", "<=", ">", "<", "^", "~"];
    let (operator, rest) = OPERATORS.iter().find_map(|operator| clause.strip_prefix(operator).map(|rest| (*operator, rest))).unwrap_or(("", clause));
    let (numbers, wildcard) = release(rest)?;
    let order = compare(version, &numbers);
    let starts = |numbers: &[u64]| numbers.iter().enumerate().all(|(index, number)| version.get(index).copied().unwrap_or(0) == *number);
    // Below the first version past `numbers` when the number at `index` rises.
    let below = |index: usize| {
        let mut next = numbers[..=index].to_vec();
        next[index] += 1;
        compare(version, &next).is_lt()
    };

    Some(match operator {
        "" => starts(&numbers),
        "==" | "===" if wildcard => starts(&numbers),
        "==" | "===" => order.is_eq(),
        "!=" if wildcard => !starts(&numbers),
        "!=" => order.is_ne(),
        _ if wildcard || numbers.is_empty() => return None,
        ">=" => order.is_ge(),
        "<=" => order.is_le(),
        ">" => order.is_gt(),
        "<" => order.is_lt(),
        // `~=3.12` is 3.12 and up, below 4; `~=3.12.1` below 3.13.
        "~=" if numbers.len() < 2 => return None,
        "~=" => order.is_ge() && starts(&numbers[..numbers.len() - 1]),
        // Poetry: `^3.11` is below 4, `^0.3` below 0.4; `~3.11` below 3.12, `~3` below 4.
        "^" => order.is_ge() && below(numbers.iter().position(|number| *number > 0).unwrap_or(numbers.len() - 1)),
        _ => order.is_ge() && below(numbers.len().min(2) - 1),
    })
}

/* ---------- running a Python tool ---------- */

/// How a project runs one of Python's tools, as the place of its spelling among the six a
/// migration tool lists in this order: `.venv/bin/<tool>`, `venv/bin/<tool>`, `uv run <tool>`,
/// `poetry run <tool>`, `pipenv run <tool>`, `<bare>`. The project environment's own comes
/// first; else the manager's runner, which finds an environment kept elsewhere.
///
/// `uv run` without an environment would make one, lock and install on its own, which a status
/// read must not do: it is used only where there is an environment without `bin/` (Windows).
/// Until the install, such a project falls to the bare spelling, which fails.
pub(super) fn runner(dir: &Path, executable: &str) -> usize {
    if let Some(at) = ENVIRONMENTS[..2].iter().position(|environment| dir.join(environment.folder).join("bin").join(executable).exists()) {
        return at;
    }
    match Python.detect(dir) {
        Some("uv") if environment(dir, "uv").is_some() => 2,
        Some("poetry") => 3,
        Some("pipenv") => 4,
        _ => 5,
    }
}

/* ---------- the network scan ---------- */

/// `pip list --outdated --format=json` and `uv pip list --outdated --format=json`: a list of
/// `{"name", "version", "latest_version"}`, past any noise the shell printed first. The
/// environment's own tools (pip, setuptools, wheel) aren't the project's.
fn pip_outdated(output: &str) -> Option<Vec<Outdated>> {
    let trimmed = output.trim();
    let list: Value = serde_json::from_str(trimmed).ok().or_else(|| serde_json::from_str(trimmed.get(trimmed.find('[')?..=trimmed.rfind(']')?)?).ok())?;
    let own = |package: &&Value| text(package, "name").is_some_and(|name| !matches!(normal(name).as_str(), "pip" | "setuptools" | "wheel"));
    let packages = list.as_array()?.iter().filter(own);
    Some(packages.map(|package| Outdated { name: text(package, "name").unwrap_or_default().to_string(), installed: owned(package, "version"), wanted: None, latest: owned(package, "latest_version") }).collect())
}
