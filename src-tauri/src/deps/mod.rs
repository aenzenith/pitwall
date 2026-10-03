//! The Dependencies page's checks, per project: whether the installed packages match the lock
//! file (contents compared, never modification times, which a checkout rewrites), whether the
//! active runtimes satisfy what the project asks for, and which migrations its database hasn't
//! run. The checks read files only. The pending migrations come from the migration tool's own
//! read-only status command (`migration_status`): the tool connects to the database, Pitwall
//! never reads its credentials. The network scan (`scan`) runs the package managers' own
//! read-only commands, and only when the user asks.
//!
//! Of `.env`, only `DB_CONNECTION` and `DB_DATABASE` (shown in the migrate confirm) and
//! `APP_ENV` (migrate is refused under production) are ever read: see `EnvValue`.
//!
//! This file has what every language shares: the report's types, the two registries, `inspect`
//! and `report`, and the helpers (JSON, version constraints, `.env`, the login shell's probe,
//! `run_capture`, the scan). Each package ecosystem and each migration tool is a file of its own
//! beside it and knows nothing of the others. When and how often things run is
//! `core/dependencies.rs`.
//!
//! # How to add a package ecosystem
//!
//! 1. Write `deps/<name>.rs`: a unit struct that implements [`Ecosystem`]. `composer.rs` is the
//!    short example, `node.rs` the one with several managers. Only `id`, `manifest`, `detect`,
//!    `watched`, `check` and `needs` are required; leave `install_command` out where installing
//!    has no meaning (packages resolved at build), `scan_plan` where there is no outdated or
//!    audit command, `runtimes` where there is none.
//! 2. Add `mod <name>;` below and one `&<name>::<Struct>,` line to [`ECOSYSTEMS`], each in
//!    alphabetical order. Nothing else in this file, in `core/` or in `commands.rs` changes.
//! 3. A runtime ([`Runtime`]) names its probe (a constant such as `ruby -v`: quick, offline,
//!    without side effects; add `2>&1` when the tool prints its version to stderr), the reader of
//!    what the probe printed ([`version_line`], [`version_in`], or the module's own), how to read
//!    what the project asks for, and the constraint [`Flavor`]. `~>` and `~=` are understood
//!    under every flavor; a bare version is that version (`3.2` is 3.2.x), so a file that means
//!    "at least" hands back `>=1.22`. [`Flavor::Custom`] takes the module's own matcher.
//! 4. Every command line is a `&'static str` constant in the module, picked by what files the
//!    project has (`./gradlew` or `gradle`), never built from text in them.
//!
//! # How to add a migration tool
//!
//! 1. Write `deps/<name>.rs`: a unit struct that implements [`MigrationTool`] (`laravel.rs`).
//!    Required: `id`, `detect`, `variants`, `parse_pending`, `sources` and `ecosystem`; the rest
//!    (`variant`, `watched`, `refuse`, `database`, `framework`, `status_timeout`) has defaults.
//!    `variants` lists every spelling of the tool's two commands, ONE read-only status command
//!    and ONE forward-only migrate command, as constants; `variant` picks the project's by its
//!    files (another module's answer is at hand: `super::ecosystem("npm")?.detect(dir)`). A
//!    prefix that differs per project is spelled out with `concat!`, for instance
//!
//!    ```text
//!    macro_rules! with {
//!        ($python:literal, $needs:expr) => {
//!            Commands { status: concat!($python, " manage.py showmigrations --plan"), migrate: concat!($python, " manage.py migrate"), needs: $needs }
//!        };
//!    }
//!    static VARIANTS: [Commands; 3] = [with!(".venv/bin/python", &[]), with!("uv run python", &["uv"]), with!("python3", &["python3"])];
//!    ```
//! 2. Add `mod <name>;` below and one `&<name>::<Struct>,` line to [`MIGRATION_TOOLS`], each in
//!    alphabetical order.
//! 3. Add the tool's row, with every variant, to `EXPECTED` in the database gate test of
//!    `core/dependencies.rs`. The test fails until the row is there and equal.
//!
//! # Never
//!
//! - A command that touches a database other than those two per tool: no fresh, refresh,
//!   rollback, reset, wipe, drop, seed, flush, force, `schema:load`, `migrate dev`, down, undo,
//!   revert, clean, push or sync, and no shell operators in them. The migrate command runs only
//!   from `Core::deps_migrate`, the user's explicit confirm; a module never runs it.
//! - A command line put together at run time: no `format!`, no project text in it.
//! - A `.env` value other than `EnvValue`'s three, or a new variant there; no credentials, URLs
//!   or DSNs from any other file either.
//! - Running anything from `detect`, `check`, `runtimes`, `refuse`, `database` or `sources`:
//!   they read files. Only `run_capture` (status, scan, probe) and the project's jobs (install,
//!   migrate) start a process.
//! - A new crate (`Cargo.toml` is shared): TOML, YAML and XML are read line by line or with
//!   `regex`, as `node.rs` reads `pnpm-lock.yaml`.
//! - A text of the app's in the code: errors are keys in all `src/locales/*.json`.
//! - A test beyond the two there are (the `.env` one below, the database gate), unless what it
//!   guards breaks silently and for good.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::t;
use crate::registry::now_ms;

mod composer;
mod cpp;
mod dart;
mod django;
mod doctrine;
mod dotnet;
mod efcore;
mod elixir;
mod flyway;
mod go;
mod java;
mod laravel;
mod node;
mod node_exec;
mod prisma;
mod python;
mod rails;
mod ruby;
mod rust;

/// Every package ecosystem, one line each, in alphabetical order. The order is the order of a
/// report's `checks` and `runtimes`.
pub static ECOSYSTEMS: &[&dyn Ecosystem] = &[
    &composer::Composer,
    &cpp::Cpp,
    &dart::Dart,
    &dotnet::Dotnet,
    &elixir::Elixir,
    &go::Go,
    &java::Java,
    &node::Node,
    &python::Python,
    &ruby::Ruby,
    &rust::Rust,
];

/// Every migration tool, one line each, in alphabetical order. Each one's commands are listed
/// again, by hand, in the database gate test of `core/dependencies.rs`.
pub static MIGRATION_TOOLS: &[&dyn MigrationTool] = &[
    &django::Django,
    &doctrine::Doctrine,
    &efcore::EfCore,
    &flyway::Flyway,
    &laravel::Laravel,
    &prisma::Prisma,
    &rails::Rails,
];

/// At most this many differing packages are listed per check; `differing` has the total.
const MAX_PACKAGES: usize = 50;
/// How long a status command may take unless its tool says otherwise (it boots the app and
/// queries the database).
const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
/// How long one command of a network scan may take.
const SCAN_TIMEOUT: Duration = Duration::from_secs(90);
/// How long the login shell that reports versions and tools may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// A command's output is kept up to this size.
const CAPTURE_MAX: u64 = 64 * 1024 * 1024;
/// Looked at by the minute's poll for every project, whatever it uses.
const WATCHED: [&str; 2] = [".git/HEAD", ".git/ORIG_HEAD"];

/* ---------- what a module is ---------- */

/// A package ecosystem: one lock file format family and the managers that install from it.
pub trait Ecosystem: Sync {
    /// `npm`, `composer`: the report's `ecosystem`, the scans' key, the install job
    /// `deps:<id>`.
    fn id(&self) -> &'static str;

    /// The file a refusal names when the project isn't part of this ecosystem: `package.json`.
    fn manifest(&self) -> &'static str;

    /// The project's manager (`pnpm`, `poetry`, `gradle`), by its files; `None` when the project
    /// isn't part of this ecosystem. The other methods get it back as `manager`.
    fn detect(&self, dir: &Path) -> Option<&'static str>;

    /// The files and folders, relative to the project, whose size and time the minute's poll
    /// looks at: the manifest, the lock files, the manager's record of what it installed, the
    /// files a runtime's requirement is read from.
    fn watched(&self) -> &'static [&'static str];

    /// The lock file against what is installed, from files only: the state and the differing
    /// packages, in any order and uncapped. `Ok` with packages counts as `Install`.
    fn check(&self, dir: &Path, manager: &str) -> (CheckState, Vec<DepPackage>);

    /// The executables the project's manager needs on the login shell's PATH (`pnpm`); none
    /// when it runs from the project (`./gradlew`). With one missing, the check's `tool` is
    /// `missing` and installs and scans are refused.
    fn needs(&self, dir: &Path, manager: &str) -> &'static [&'static str];

    /// The runtimes this ecosystem implies (`node`). Each is asked of every project, part of
    /// the ecosystem or not (`.nvmrc` alone asks for Node).
    fn runtimes(&self) -> &'static [Runtime] {
        &[]
    }

    /// Exactly what the install job runs, one of the module's constants (`pnpm install`);
    /// `None` where installing has no meaning.
    fn install_command(&self, _dir: &Path, _manager: &str) -> Option<&'static str> {
        None
    }

    /// The read-only outdated and audit commands of the network scan, and the readers of their
    /// output.
    fn scan_plan(&self, _dir: &Path, _manager: &str) -> ScanPlan {
        ScanPlan::default()
    }
}

/// A runtime an ecosystem implies, and how its version is asked and compared.
#[derive(Debug)]
pub struct Runtime {
    /// `node`, `php`: the report's `name`.
    pub name: &'static str,
    /// Prints the active version; run through the login shell in the home folder (so no
    /// project's own version manager file decides).
    pub probe: &'static str,
    /// The version in what `probe` printed.
    pub version: fn(&str) -> Option<String>,
    /// Whether the project uses the runtime (`None`: no) and what it asks for (`Some(None)`:
    /// nothing).
    pub required: fn(&Path) -> Option<Option<String>>,
    /// The project's own copy of the runtime, from its files, where it has one (a virtualenv);
    /// it then stands for the login shell's.
    pub local: Option<fn(&Path) -> Option<String>>,
    pub flavor: Flavor,
}

/// One spelling of a migration tool's two commands. Constants: the database gate test lists
/// every one.
#[derive(Debug, PartialEq, Eq)]
pub struct Commands {
    /// Read-only: which migrations the database has run.
    pub status: &'static str,
    /// Forward-only: runs the pending migrations. Only from the user's explicit confirm.
    pub migrate: &'static str,
    /// The executables both need on the login shell's PATH.
    pub needs: &'static [&'static str],
}

/// A migration tool: what tells a project's pending migrations and runs them.
pub trait MigrationTool: Sync {
    /// `laravel`, `django`: the report's `tool`, the migrate job `deps:migrate:<id>`.
    fn id(&self) -> &'static str;

    /// The project uses this tool, by its files.
    fn detect(&self, dir: &Path) -> bool;

    /// Every spelling of the tool's two commands. Nothing else of this tool ever runs.
    fn variants(&self) -> &'static [Commands];

    /// Which of `variants` the project runs, by its files (a virtualenv, a wrapper script).
    fn variant(&self, _dir: &Path) -> usize {
        0
    }

    /// A finished status command's pending migrations, in order; or why there is no list. The
    /// output is only parsed, never kept or logged.
    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError>;

    /// The migration files, relative to the project: folders (their files, one level) and
    /// single files. When they change the status is read again.
    fn sources(&self, dir: &Path) -> Vec<PathBuf>;

    /// The id of the ecosystem whose install the commands need: after that install ends the
    /// status is read again.
    fn ecosystem(&self) -> &'static str;

    /// The files, relative to the project, whose size and time the minute's poll looks at:
    /// what `detect`, `refuse` and `database` read.
    fn watched(&self) -> &'static [&'static str] {
        &[]
    }

    /// Why migrate must not run here, in the user's language (production).
    fn refuse(&self, _dir: &Path) -> Option<String> {
        None
    }

    /// What a migrate would run against, where that is known without reading credentials.
    fn database(&self, _dir: &Path) -> Option<DepDatabase> {
        None
    }

    /// The framework the tool belongs to and its locked version.
    fn framework(&self, _dir: &Path) -> Option<DepFramework> {
        None
    }

    /// How long the status command may take.
    fn status_timeout(&self) -> Duration {
        STATUS_TIMEOUT
    }
}

pub fn ecosystem(id: &str) -> Option<&'static dyn Ecosystem> {
    ECOSYSTEMS.iter().copied().find(|ecosystem| ecosystem.id() == id)
}

pub fn migration_tool(id: &str) -> Option<&'static dyn MigrationTool> {
    MIGRATION_TOOLS.iter().copied().find(|tool| tool.id() == id)
}

/// Every file the minute's poll looks at, relative to a project: the registries' and Git's.
pub fn watched() -> &'static [&'static str] {
    static ALL: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
        let mut all: Vec<&'static str> = Vec::new();
        let ecosystems = ECOSYSTEMS.iter().flat_map(|ecosystem| ecosystem.watched());
        let tools = MIGRATION_TOOLS.iter().flat_map(|tool| tool.watched());
        for name in ecosystems.chain(tools).chain(&WATCHED) {
            if !all.contains(name) {
                all.push(*name);
            }
        }
        all
    });
    &ALL
}

/* ---------- what the page gets ---------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Ok,
    /// What is installed differs from the lock file, or nothing is installed.
    Install,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepPackage {
    pub name: String,
    pub locked: Option<String>,
    /// `None`: not installed.
    pub installed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolState {
    Ok,
    /// The login shell has no such executable: installs and scans are refused.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepCheck {
    pub ecosystem: &'static str,
    pub manager: &'static str,
    pub state: CheckState,
    /// The differing packages, at most `MAX_PACKAGES`.
    pub packages: Vec<DepPackage>,
    /// How many differ in all.
    pub differing: usize,
    /// Exactly what the install job runs; `None` where installing has no meaning.
    pub install_command: Option<&'static str>,
    /// There is an outdated or audit scan for this manager.
    pub can_scan: bool,
    pub tool: ToolState,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepRuntime {
    /// `node`, `php`
    pub name: &'static str,
    pub required: Option<String>,
    pub active: Option<String>,
    /// `None` when either side is unknown or the requirement can't be read.
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanError {
    Offline,
    Timeout,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepScan {
    pub at: u64,
    pub outdated: Option<u32>,
    pub vulnerable: Option<u32>,
    pub error: Option<ScanError>,
    #[serde(default)]
    pub running: bool,
    /// What the counts are of. Kept with the scan and handed over when asked for; never part
    /// of a report (`counts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ScanDetails>,
}

impl DepScan {
    /// The scan as a report carries it: its counts, without what they are of.
    pub fn counts(&self) -> DepScan {
        DepScan { at: self.at, outdated: self.outdated, vulnerable: self.vulnerable, error: self.error, running: self.running, details: None }
    }
}

/// A package an outdated report lists: the version installed, the newest the project's
/// constraint allows (where the manager tells) and the newest there is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outdated {
    pub name: String,
    pub installed: Option<String>,
    pub wanted: Option<String>,
    pub latest: Option<String>,
}

/// One advisory against a package, as far as the manager's audit tells.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advisory {
    pub package: String,
    pub installed: Option<String>,
    /// `critical`, `high`, `moderate`, `low` or `info`.
    pub severity: Option<String>,
    /// The project asks for the package itself; `false`: it comes with another.
    pub direct: Option<bool>,
    pub title: Option<String>,
    pub url: Option<String>,
    /// The versions it is in, as the advisory writes them (`<1.8.2`).
    pub affected: Option<String>,
    /// The vulnerable packages it comes with, for one that has no advisory of its own.
    pub via: Option<String>,
    /// What fixes it, as the audit writes it: the patched versions (`>=1.8.2`) or the package to
    /// install (`vite@6.2.0`).
    pub fix: Option<String>,
    /// The manager can fix it itself; `None` where the audit doesn't say.
    pub fixable: Option<bool>,
}

/// What an audit found: how many packages are vulnerable (the page's count) and the advisories
/// against them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Audit {
    pub packages: u32,
    pub advisories: Vec<Advisory>,
}

/// What a scan's counts are of. A part is `None` when its command didn't run or gave no list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanDetails {
    pub outdated: Option<Vec<Outdated>>,
    pub advisories: Option<Vec<Advisory>>,
}

/// A project's last scans, by ecosystem id.
pub type DepScans = BTreeMap<String, DepScan>;

/// Why the last status read gave no list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationsError {
    /// The database refused or couldn't be reached.
    Unreachable,
    /// The database has no migrations table yet.
    NoTable,
    Timeout,
    Failed,
    /// The login shell lacks an executable the status command needs: it wasn't run.
    ToolMissing,
}

/// What the last status read of one of a project's migration tools gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationRead {
    /// The pending migrations, in order. Kept from the last read that gave a list when this one
    /// failed.
    pub pending: Vec<String>,
    pub error: Option<MigrationsError>,
    /// When the last read began (ms); 0: never read.
    pub at: u64,
    /// A read is under way (or asked for).
    pub running: bool,
}

/// A project's pending migrations, as its database says through one migration tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepMigrations {
    pub tool: &'static str,
    /// The forward-only migrate command the confirm shows.
    pub command: &'static str,
    pub pending: Vec<String>,
    pub error: Option<MigrationsError>,
    pub at: u64,
    pub running: bool,
    pub database: Option<DepDatabase>,
}

/// What a migrate would run against: for Laravel, `DB_CONNECTION` and `DB_DATABASE` of `.env`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepDatabase {
    pub connection: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepFramework {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepReport {
    pub path: String,
    pub checks: Vec<DepCheck>,
    pub runtimes: Vec<DepRuntime>,
    /// One per migration tool the project uses.
    pub migrations: Vec<DepMigrations>,
    pub framework: Option<DepFramework>,
    pub scans: DepScans,
    pub checked_at: u64,
    /// An install job of this project runs: its ecosystem.
    pub installing: Option<&'static str>,
}

/* ---------- one project's files ---------- */

/// What the files of a project say, as of `checked_at`; the active runtimes, the tools on PATH,
/// scans, pending migrations and jobs are added when a report is made (`report`).
#[derive(Debug, Clone)]
pub struct Inspection {
    pub checks: Vec<Checked>,
    pub runtimes: Vec<Asked>,
    pub migrations: Vec<Found>,
    pub framework: Option<DepFramework>,
    pub checked_at: u64,
}

/// An ecosystem's check, with the executables its manager needs.
#[derive(Debug, Clone)]
pub struct Checked {
    /// `tool` is set when the report is made.
    pub check: DepCheck,
    pub needs: &'static [&'static str],
}

/// A runtime the project uses, and what it asks for, if anything.
#[derive(Debug, Clone)]
pub struct Asked {
    pub runtime: &'static Runtime,
    pub required: Option<String>,
    /// The project's own copy's version (`Runtime::local`).
    pub local: Option<String>,
}

/// A migration tool the project uses, and the spelling of its commands here.
#[derive(Debug, Clone)]
pub struct Found {
    pub tool: &'static str,
    pub commands: &'static Commands,
    pub database: Option<DepDatabase>,
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(file).ok()?).ok()
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

/// A text field, owned; an empty one counts as missing.
fn owned(value: &Value, key: &str) -> Option<String> {
    text(value, key).filter(|text| !text.is_empty()).map(str::to_string)
}

/// A severity as the page names it, whatever the audit calls it (`Medium`, `HIGH`).
fn severity(text: Option<&str>) -> Option<String> {
    let known = match text?.trim().to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "high" => "high",
        "moderate" | "medium" => "moderate",
        "low" => "low",
        "info" => "info",
        _ => return None,
    };
    Some(known.to_string())
}

/// Reads everything the report needs from the project's files.
pub fn inspect(dir: &Path) -> Inspection {
    let checked_at = now_ms();
    let mut checks = Vec::new();
    let mut runtimes: Vec<Asked> = Vec::new();

    for ecosystem in ECOSYSTEMS {
        if let Some(manager) = ecosystem.detect(dir) {
            let (state, packages) = ecosystem.check(dir, manager);
            let install = ecosystem.install_command(dir, manager);
            let plan = ecosystem.scan_plan(dir, manager);
            checks.push(Checked {
                check: finish(ecosystem.id(), manager, state, packages, install, plan.any()),
                needs: ecosystem.needs(dir, manager),
            });
        }
        for runtime in ecosystem.runtimes() {
            if runtimes.iter().any(|asked| asked.runtime.name == runtime.name) {
                continue;
            }
            if let Some(required) = (runtime.required)(dir) {
                runtimes.push(Asked { runtime, required, local: runtime.local.and_then(|local| local(dir)) });
            }
        }
    }

    let mut migrations = Vec::new();
    let mut framework = None;
    for tool in MIGRATION_TOOLS {
        let Some(commands) = commands(*tool, dir) else {
            continue;
        };
        migrations.push(Found { tool: tool.id(), commands, database: tool.database(dir) });
        if framework.is_none() {
            framework = tool.framework(dir);
        }
    }

    Inspection { checks, runtimes, migrations, framework, checked_at }
}

/// The report the page shows: the project's files with the active runtimes, the tools the login
/// shell has, its scans, its last status reads and whether an install runs.
pub fn report(
    path: &str,
    inspection: &Inspection,
    probed: &Probed,
    scans: DepScans,
    reads: Option<&HashMap<&'static str, MigrationRead>>,
    installing: Option<&'static str>,
) -> DepReport {
    let checks = inspection
        .checks
        .iter()
        .map(|checked| {
            let missing = checked.needs.iter().any(|tool| probed.lacks(tool));
            DepCheck { tool: if missing { ToolState::Missing } else { ToolState::Ok }, ..checked.check.clone() }
        })
        .collect();

    let runtimes = inspection
        .runtimes
        .iter()
        .map(|asked| {
            let active = asked.local.clone().or_else(|| probed.version(asked.runtime.name));
            let ok = match (&asked.required, &active) {
                (Some(required), Some(active)) => satisfies(required, active, asked.runtime.flavor),
                _ => None,
            };
            DepRuntime { name: asked.runtime.name, required: asked.required.clone(), active, ok }
        })
        .collect();

    let migrations = inspection
        .migrations
        .iter()
        .map(|found| {
            let read = reads.and_then(|reads| reads.get(found.tool)).cloned().unwrap_or_default();
            DepMigrations {
                tool: found.tool,
                command: found.commands.migrate,
                pending: read.pending,
                error: read.error,
                at: read.at,
                running: read.running,
                database: found.database.clone(),
            }
        })
        .collect();

    DepReport {
        path: path.to_string(),
        checks,
        runtimes,
        migrations,
        framework: inspection.framework.clone(),
        scans,
        checked_at: inspection.checked_at,
        installing,
    }
}

/// A check's result: sorted, the list capped, the total kept.
fn finish(
    ecosystem: &'static str,
    manager: &'static str,
    state: CheckState,
    mut packages: Vec<DepPackage>,
    install_command: Option<&'static str>,
    can_scan: bool,
) -> DepCheck {
    packages.sort();
    packages.dedup();
    let differing = packages.len();
    let state = if state == CheckState::Ok && differing > 0 { CheckState::Install } else { state };
    packages.truncate(MAX_PACKAGES);
    DepCheck { ecosystem, manager, state, packages, differing, install_command, can_scan, tool: ToolState::Ok }
}

/// Every listed package as missing, or `Ok` when there is none.
fn none_installed(names: &[String], locked: impl Fn(&str) -> Option<String>) -> (CheckState, Vec<DepPackage>) {
    let packages: Vec<DepPackage> = names.iter().map(|name| DepPackage { name: name.clone(), locked: locked(name), installed: None }).collect();
    (if packages.is_empty() { CheckState::Ok } else { CheckState::Install }, packages)
}

/* ---------- version constraints ---------- */

/// Whose rules a constraint follows: they differ for `~1.2` (npm: below 1.3, Composer: below 2).
#[derive(Debug, Clone, Copy)]
pub enum Flavor {
    Npm,
    Composer,
    /// The module's own reading of (constraint, version); `None` when either can't be read.
    #[allow(dead_code)] // For the modules whose constraints are neither.
    Custom(fn(&str, &str) -> Option<bool>),
}

type Version = [u64; 3];

/// `v22.3.0`, `8.4.0RC1`, `20`: the numbers, missing ones as 0.
fn parse_version(value: &str) -> Option<Version> {
    let partial = parse_partial(value.trim().trim_start_matches(['v', 'V']))?;
    (!partial.is_empty()).then(|| floor(&partial))
}

/// The numbers given before any wildcard: `8.2.*` is `[8, 2]`, `*` is `[]`. A pre-release or
/// build suffix on the last number is ignored. `None` when it isn't a version.
fn parse_partial(value: &str) -> Option<Vec<u64>> {
    if value.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for part in value.split('.') {
        if matches!(part, "*" | "x" | "X") {
            break;
        }
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() || parts.len() == 3 {
            return None;
        }
        let rest = &part[digits.len()..];
        if !rest.is_empty() && !rest.starts_with(['-', '+']) && !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        parts.push(digits.parse().ok()?);
        if !rest.is_empty() {
            break;
        }
    }
    Some(parts)
}

fn floor(partial: &[u64]) -> Version {
    let mut version = [0; 3];
    for (slot, value) in version.iter_mut().zip(partial) {
        *slot = *value;
    }
    version
}

/// The first version past a partial: `[20]` is 21.0.0, `[20, 11]` is 20.12.0.
fn next(partial: &[u64]) -> Option<Version> {
    let mut version = floor(partial);
    let last = partial.len().checked_sub(1)?;
    version[last] += 1;
    Some(version)
}

/// Whether `version` meets `constraint`: `^`, `~`, `~>`, `~=`, `>=`, `>`, `<=`, `<`, `=`, `==`,
/// `!=`, `*`, `x`, partial versions, hyphen ranges, `||` (and Composer's `|`), `,` or spaces for
/// "and". `None` when either can't be read (`lts/*`, `node`, …).
pub fn satisfies(constraint: &str, version: &str, flavor: Flavor) -> Option<bool> {
    if let Flavor::Custom(holds) = flavor {
        return holds(constraint, version);
    }
    let version = parse_version(version)?;
    let constraint = constraint.trim();
    if constraint.is_empty() {
        return None;
    }
    let mut any = false;
    for alternative in constraint.replace("||", "|").split('|') {
        any |= range_holds(alternative.trim(), version, flavor)?;
    }
    Some(any)
}

fn range_holds(range: &str, version: Version, flavor: Flavor) -> Option<bool> {
    if range.is_empty() || matches!(range, "*" | "x" | "X") {
        return Some(true);
    }
    if let Some((low, high)) = range.split_once(" - ") {
        let low = parse_partial(low.trim().trim_start_matches('v'))?;
        let high = parse_partial(high.trim().trim_start_matches('v'))?;
        let below = if high.len() == 3 { version <= floor(&high) } else { next(&high).is_none_or(|next| version < next) };
        return Some(version >= floor(&low) && below);
    }

    // `>= 8.1` is one comparator; `,` means "and" in Composer.
    let mut tokens: Vec<String> = Vec::new();
    let mut operator = String::new();
    for word in range.replace(',', " ").split_whitespace() {
        if word.chars().all(|c| matches!(c, '<' | '>' | '=' | '^' | '~' | '!')) {
            operator.push_str(word);
        } else {
            tokens.push(format!("{operator}{word}"));
            operator.clear();
        }
    }
    if !operator.is_empty() {
        return None;
    }

    let mut all = true;
    for token in tokens {
        all &= comparator_holds(&token, version, flavor)?;
    }
    Some(all)
}

fn comparator_holds(token: &str, version: Version, flavor: Flavor) -> Option<bool> {
    // Composer's stability flags: `^8.2@dev`.
    let token = token.split('@').next().unwrap_or(token);
    let operators = [">=", "<=", "!=", "==", "~>", "~=", ">", "<", "=", "^", "~"];
    let (operator, rest) = operators.iter().find_map(|op| token.strip_prefix(op).map(|rest| (*op, rest))).unwrap_or(("", token));
    let rest = rest.trim().trim_start_matches(['v', 'V']);

    if matches!(rest, "*" | "x" | "X") && matches!(operator, "" | "=" | "==" | ">=") {
        return Some(true);
    }
    let partial = parse_partial(rest)?;
    if partial.is_empty() {
        return None;
    }
    let low = floor(&partial);
    let exact = partial.len() == 3;
    let upper = next(&partial)?;
    let in_range = |high: Version| version >= low && version < high;

    Some(match operator {
        "" | "=" | "==" => {
            if exact {
                version == low
            } else {
                in_range(upper)
            }
        }
        "!=" => {
            if exact {
                version != low
            } else {
                !in_range(upper)
            }
        }
        ">=" => version >= low,
        ">" => {
            if exact {
                version > low
            } else {
                version >= upper
            }
        }
        "<" => version < low,
        "<=" => {
            if exact {
                version <= low
            } else {
                version < upper
            }
        }
        // `~>` (Ruby, Elixir) and `~=` (Python) let the last number given rise, as Composer's
        // `~` does: `~> 3.2` is below 4, `~> 3.2.1` below 3.3.
        "~" | "~>" | "~=" => {
            let last_rises = operator != "~" || matches!(flavor, Flavor::Composer);
            let high = match partial.len() {
                1 => [low[0] + 1, 0, 0],
                2 if last_rises => [low[0] + 1, 0, 0],
                _ => [low[0], low[1] + 1, 0],
            };
            in_range(high)
        }
        "^" => {
            let high = if low[0] > 0 || partial.len() == 1 {
                [low[0] + 1, 0, 0]
            } else if low[1] > 0 || partial.len() == 2 {
                [0, low[1] + 1, 0]
            } else {
                [0, 0, low[2] + 1]
            };
            in_range(high)
        }
        _ => return None,
    })
}

/* ---------- the login shell's runtimes and tools ---------- */

/// What the login shell answered, as last asked: the runtimes' active versions and which
/// executables it has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probed {
    /// By runtime name; `None`: asked, none found.
    versions: HashMap<&'static str, Option<String>>,
    /// By executable; `None`: asked, no answer (the shell failed or took too long).
    tools: HashMap<&'static str, Option<bool>>,
}

impl Probed {
    fn version(&self, runtime: &str) -> Option<String> {
        self.versions.get(runtime).cloned().flatten()
    }

    /// The login shell was asked and has no such executable. One never asked about, or without
    /// an answer, counts as there.
    pub fn lacks(&self, tool: &str) -> bool {
        self.tools.get(tool) == Some(&Some(false))
    }

    /// Takes over what a later probe answered.
    pub fn merge(&mut self, later: Probed) {
        self.versions.extend(later.versions);
        self.tools.extend(later.tools);
    }

    /// What of `wanted` was never asked.
    pub fn unasked(&self, mut wanted: Wanted) -> Wanted {
        wanted.runtimes.retain(|runtime| !self.versions.contains_key(runtime.name));
        wanted.tools.retain(|tool| !self.tools.contains_key(tool));
        wanted
    }
}

/// What a probe asks the login shell: runtimes' versions, and whether executables exist.
#[derive(Debug, Default)]
pub struct Wanted {
    runtimes: Vec<&'static Runtime>,
    tools: Vec<&'static str>,
}

impl Wanted {
    /// Everything these projects' reports need: the runtimes they use, the executables their
    /// managers and migration tools need.
    pub fn of<'a>(inspections: impl IntoIterator<Item = &'a Inspection>) -> Self {
        let mut wanted = Wanted::default();
        for inspection in inspections {
            for asked in &inspection.runtimes {
                if !wanted.runtimes.iter().any(|runtime| runtime.name == asked.runtime.name) {
                    wanted.runtimes.push(asked.runtime);
                }
            }
            let needs = inspection.checks.iter().map(|checked| checked.needs).chain(inspection.migrations.iter().map(|found| found.commands.needs));
            for tool in needs.flatten() {
                if !wanted.tools.contains(tool) {
                    wanted.tools.push(*tool);
                }
            }
        }
        wanted.runtimes.sort_by_key(|runtime| runtime.name);
        wanted.tools.sort_unstable();
        wanted
    }

    pub fn tools(tools: &[&'static str]) -> Self {
        Wanted { runtimes: Vec::new(), tools: tools.to_vec() }
    }

    pub fn is_empty(&self) -> bool {
        self.runtimes.is_empty() && self.tools.is_empty()
    }
}

const RUNTIME_MARK: &str = "pitwall-runtime-";
const TOOL_MARK: &str = "pitwall-tool-";
const END_MARK: &str = "pitwall-end";

/// A name that is safe on a command line and in a marker.
fn plain(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

/// Asks the login shell for everything in `wanted` in one call, in the home folder (so no
/// project's own version manager file decides). Every answer comes after a marker line of its
/// own, so a missing tool leaves its own answer empty and no other's. Blocks for as long as the
/// shell runs; what it didn't answer stays asked, without an answer.
pub fn probe(wanted: &Wanted) -> Probed {
    let mut probed = Probed::default();
    probed.versions.extend(wanted.runtimes.iter().map(|runtime| (runtime.name, None)));
    probed.tools.extend(wanted.tools.iter().map(|tool| (*tool, None)));
    // Tests never start the login shell.
    if wanted.is_empty() || cfg!(test) {
        return probed;
    }

    let home = dirs::home_dir().map(|home| home.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
    if let Ok(output) = run_capture(&probe_command(wanted), &home, PROBE_TIMEOUT) {
        read_probe(&output.stdout, wanted, &mut probed);
    }
    probed
}

/// `echo <marker>` before each runtime's probe and each tool's lookup, and one at the end. The
/// empty `echo` first ends a line the command before left open (`php -r` prints no newline).
fn probe_command(wanted: &Wanted) -> String {
    let (joiner, blank, lookup) = if cfg!(windows) { (" & ", "echo.", "where") } else { ("; ", "echo", "command -v") };
    let mut parts: Vec<String> = Vec::new();
    let mark = |parts: &mut Vec<String>, marker: String| {
        parts.push(blank.to_string());
        parts.push(format!("echo {marker}"));
    };

    for runtime in wanted.runtimes.iter().filter(|runtime| plain(runtime.name)) {
        mark(&mut parts, format!("{RUNTIME_MARK}{}", runtime.name));
        parts.push(runtime.probe.to_string());
    }
    for tool in wanted.tools.iter().filter(|tool| plain(tool)) {
        mark(&mut parts, format!("{TOOL_MARK}{tool}"));
        parts.push(format!("{lookup} {tool}"));
    }
    mark(&mut parts, END_MARK.to_string());
    parts.join(joiner)
}

/// The answers between the markers; what the shell printed before the first is start-up noise.
/// An answer counts once the next marker closes it.
fn read_probe(output: &str, wanted: &Wanted, probed: &mut Probed) {
    enum Slot {
        Runtime(&'static Runtime),
        Tool(&'static str),
    }
    let mut open: Option<(Slot, Vec<&str>)> = None;

    for line in output.lines().map(str::trim) {
        let marker = if line == END_MARK {
            Some(None)
        } else if let Some(name) = line.strip_prefix(RUNTIME_MARK) {
            wanted.runtimes.iter().find(|runtime| runtime.name == name).map(|runtime| Some(Slot::Runtime(runtime)))
        } else if let Some(name) = line.strip_prefix(TOOL_MARK) {
            wanted.tools.iter().find(|tool| **tool == name).map(|tool| Some(Slot::Tool(tool)))
        } else {
            None
        };

        let Some(next) = marker else {
            if let Some((_, lines)) = &mut open {
                lines.push(line);
            }
            continue;
        };
        match open.take() {
            Some((Slot::Runtime(runtime), lines)) => {
                probed.versions.insert(runtime.name, (runtime.version)(&lines.join("\n")));
            }
            Some((Slot::Tool(tool), lines)) => {
                probed.tools.insert(tool, Some(lines.iter().any(|line| !line.is_empty())));
            }
            None => {}
        }
        open = next.map(|slot| (slot, Vec::new()));
    }
}

/// The first line that starts with a version of three numbers (`v22.3.0`, `8.3.9`), as
/// `22.3.0`.
pub fn version_line(output: &str) -> Option<String> {
    output.lines().map(str::trim).find_map(version_token)
}

/// The first version anywhere in the output: `Python 3.12.1`, `go version go1.22.3 darwin/arm64`
/// and `ruby 3.3.0 (2023-12-25…)` are `3.12.1`, `1.22.3` and `3.3.0`.
#[allow(dead_code)] // For the runtimes that print more than the version.
pub fn version_in(output: &str) -> Option<String> {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+(?:\.\d+)?").unwrap());
    VERSION.find(output).map(|found| found.as_str().to_string())
}

/// `v22.3.0` is `22.3.0`; a line that doesn't start with a version is `None`.
fn version_token(line: &str) -> Option<String> {
    let rest = line.strip_prefix('v').unwrap_or(line);
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
    let version = &rest[..end];
    (version.split('.').count() == 3 && version.split('.').all(|part| !part.is_empty())).then(|| version.to_string())
}

/* ---------- .env ---------- */

/// The only `.env` values this app ever reads. Never a fourth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvValue {
    DbConnection,
    DbDatabase,
    AppEnv,
}

impl EnvValue {
    fn key(self) -> &'static str {
        match self {
            Self::DbConnection => "DB_CONNECTION",
            Self::DbDatabase => "DB_DATABASE",
            Self::AppEnv => "APP_ENV",
        }
    }
}

/// Each `KEY=…` of a dotenv file with its value as written on that line; a quoted value that
/// runs over several lines is skipped whole (its value `None`), so none of its lines passes for
/// a key.
fn scan_env(text: &str, mut each: impl FnMut(&str, Option<&str>)) {
    let mut lines = text.lines();

    while let Some(line) = lines.next() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = rest.trim_start();

        let mut whole = true;
        if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
            if !closes(&value[1..], quote) {
                whole = false;
                for next in lines.by_ref() {
                    if closes(next, quote) {
                        break;
                    }
                }
            }
        }

        let valid = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
        if valid {
            each(key, whole.then_some(value));
        }
    }
}

/// `text` holds the closing `quote` (unescaped, for double quotes).
fn closes(text: &str, quote: char) -> bool {
    let mut escaped = false;
    for c in text.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote == '"' {
            escaped = true;
        } else if c == quote {
            return true;
        }
    }
    false
}

/// One of the allowed values (the last assignment wins, as in dotenv), unquoted, without a
/// trailing comment; empty counts as unset.
pub fn env_value(text: &str, which: EnvValue) -> Option<String> {
    let mut found = None;
    scan_env(text, |key, value| {
        if key == which.key() {
            found = value.map(unquote);
        }
    });
    found.filter(|value| !value.is_empty())
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
        if let Some(end) = value[1..].find(quote) {
            return value[1..1 + end].to_string();
        }
    }
    let end = value.find(" #").or_else(|| value.find("\t#")).unwrap_or(value.len());
    let value = &value[..end];
    if value.starts_with('#') {
        String::new()
    } else {
        value.trim().to_string()
    }
}

/* ---------- jobs ---------- */

/// The install job's command for `ecosystem`, by the project's manager, and the executables it
/// needs; refused when the project isn't part of the ecosystem or there is nothing to install.
/// Fixed command lines: nothing from the project reaches them.
pub fn install_command(ecosystem: &dyn Ecosystem, dir: &Path) -> Result<(&'static str, &'static [&'static str]), String> {
    let manager = ecosystem.detect(dir).ok_or_else(|| t!("core.error.depsNothing", file = ecosystem.manifest()))?;
    let command = ecosystem.install_command(dir, manager).ok_or_else(|| t!("core.error.depsNoInstall", ecosystem = ecosystem.id()))?;
    Ok((command, ecosystem.needs(dir, manager)))
}

/// The spelling of `tool`'s commands in this project: one of its `variants`, never anything
/// else. `None` when the project doesn't use the tool.
fn commands(tool: &dyn MigrationTool, dir: &Path) -> Option<&'static Commands> {
    if !tool.detect(dir) {
        return None;
    }
    tool.variants().get(tool.variant(dir))
}

/// The migrate job's commands: `migrate` is exactly one of the tool's forward-only constants.
/// Refused when the project doesn't use the tool, and when the tool refuses (Laravel: `.env`
/// says production or sets no `APP_ENV`).
pub fn migrate_command(tool: &dyn MigrationTool, dir: &Path) -> Result<&'static Commands, String> {
    let commands = commands(tool, dir).ok_or_else(|| t!("core.error.depsNotUsed", tool = tool.id()))?;
    match tool.refuse(dir) {
        Some(why) => Err(why),
        None => Ok(commands),
    }
}

/// The migrations the project's database hasn't run, from the tool's read-only status command
/// in the project folder through the login shell. Blocks for as long as it runs. Its output
/// (which can name the connection) is only parsed, never kept or logged.
pub fn migration_status(tool: &dyn MigrationTool, dir: &Path) -> Result<Vec<String>, MigrationsError> {
    let commands = commands(tool, dir).ok_or(MigrationsError::Failed)?;
    match run_capture(commands.status, &dir.to_string_lossy(), tool.status_timeout()) {
        Err(CaptureError::Timeout) => Err(MigrationsError::Timeout),
        Err(CaptureError::Spawn) => Err(MigrationsError::Failed),
        Ok(output) => tool.parse_pending(&output),
    }
}

/* ---------- running a command for its output ---------- */

pub struct Captured {
    pub stdout: String,
    pub stderr: String,
    /// It exited with 0.
    pub success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    Spawn,
    Timeout,
}

/// Runs `command_line` in `cwd` through the login shell (`process::spawn_shell`) and gathers its
/// output; after `timeout` its whole process tree is stopped.
pub fn run_capture(command_line: &str, cwd: &str, timeout: Duration) -> Result<Captured, CaptureError> {
    let mut child = crate::process::spawn_shell(command_line, cwd).map_err(|_| CaptureError::Spawn)?;
    let pid = child.id();
    let gather = |stream: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(stream) = stream {
                let _ = stream.take(CAPTURE_MAX).read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let stdout = gather(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let stderr = gather(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));

    let deadline = Instant::now() + timeout;
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break false,
            Ok(None) if Instant::now() >= deadline => break true,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
        }
    };

    // Whatever of its tree is left (or all of it, on a timeout) goes, so the output ends.
    if timed_out || crate::process::group_alive(pid) {
        crate::process::stop_groups(&[pid], Duration::from_millis(600), Duration::from_secs(3));
    }
    let success = child.wait().is_ok_and(|status| status.success());
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();

    if timed_out {
        return Err(CaptureError::Timeout);
    }
    Ok(Captured { stdout, stderr, success })
}

/* ---------- the network scan ---------- */

/// Reads the outdated packages off a command's output.
pub type ReadOutdated = fn(&str) -> Option<Vec<Outdated>>;
/// Reads the audit off a command's output.
pub type ReadAudit = fn(&str) -> Option<Audit>;
/// A command of a scan and the reader of its output.
pub type ScanPart<T> = Option<(&'static str, fn(&str) -> Option<T>)>;

/// What a scan runs for a project's manager: the outdated and the audit command, both
/// read-only constants. Neither: there is no scan for it.
#[derive(Clone, Copy, Default)]
pub struct ScanPlan {
    pub outdated: ScanPart<Vec<Outdated>>,
    pub audit: ScanPart<Audit>,
}

impl ScanPlan {
    fn any(&self) -> bool {
        self.outdated.is_some() || self.audit.is_some()
    }
}

/// The most a scan keeps of each list; its counts stay whole.
const MAX_LISTED: usize = 300;

/// A part of a scan: its command's output, read; why it gave nothing is kept in `error`.
fn scan_part<T>(part: ScanPart<T>, cwd: &str, error: &mut Option<ScanError>) -> Option<T> {
    let (command, read) = part?;
    match run_capture(command, cwd, SCAN_TIMEOUT) {
        Err(CaptureError::Timeout) => {
            error.get_or_insert(ScanError::Timeout);
            None
        }
        Err(CaptureError::Spawn) => {
            error.get_or_insert(ScanError::Failed);
            None
        }
        Ok(output) => read(&output.stdout).or_else(|| {
            error.get_or_insert(failure_kind(&output.stdout, &output.stderr));
            None
        }),
    }
}

/// The most severe first, then by package; one with no severity told comes last.
fn by_severity(advisories: &mut [Advisory]) {
    let rank = |advisory: &Advisory| match advisory.severity.as_deref() {
        Some("critical") => 0,
        Some("high") => 1,
        Some("moderate") => 2,
        Some("low") => 3,
        Some("info") => 4,
        _ => 5,
    };
    advisories.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.package.to_lowercase().cmp(&b.package.to_lowercase())));
}

/// The network scan: the direct packages that are outdated and the vulnerable ones, counted and
/// listed, with the package manager's read-only commands in the project folder. Blocks for as
/// long as they run.
pub fn scan(ecosystem: &dyn Ecosystem, dir: &Path) -> DepScan {
    let cwd = dir.to_string_lossy().into_owned();
    let mut error: Option<ScanError> = None;

    let plan = ecosystem.detect(dir).map(|manager| ecosystem.scan_plan(dir, manager));
    let (outdated, audit) = match plan {
        Some(plan) if plan.any() => (scan_part(plan.outdated, &cwd, &mut error), scan_part(plan.audit, &cwd, &mut error)),
        _ => {
            error = Some(ScanError::Failed);
            (None, None)
        }
    };

    let counted = outdated.as_ref().map(|list| count(list.len()));
    let vulnerable = audit.as_ref().map(|audit| audit.packages);
    let details = (outdated.is_some() || audit.is_some()).then(|| ScanDetails {
        outdated: outdated.map(|mut list| {
            list.sort_by_key(|package| package.name.to_lowercase());
            list.truncate(MAX_LISTED);
            list
        }),
        advisories: audit.map(|mut audit| {
            by_severity(&mut audit.advisories);
            audit.advisories.truncate(MAX_LISTED);
            audit.advisories
        }),
    });
    DepScan { at: now_ms(), outdated: counted, vulnerable, error, running: false, details }
}

/// A failed command's kind: no network, or anything else.
fn failure_kind(stdout: &str, stderr: &str) -> ScanError {
    const OFFLINE: [&str; 21] = [
        "enotfound",
        "eai_again",
        "econnrefused",
        "econnreset",
        "etimedout",
        "enetunreach",
        "getaddrinfo",
        "could not resolve host",
        "network is unreachable",
        "curl error 6",
        "curl error 7",
        "php_network_getaddresses",
        "err_pnpm_meta_fetch_fail",
        "transportexception",
        "temporary failure in name resolution",
        "name or service not known",
        "nodename nor servname provided",
        "no such host",
        "failed to lookup address",
        "dns error",
        "connection timed out",
    ];
    let text = format!("{stdout}\n{stderr}").to_lowercase();
    if OFFLINE.iter().any(|needle| text.contains(needle)) {
        ScanError::Offline
    } else {
        ScanError::Failed
    }
}

/// The JSON object in a command's output, past any noise the shell printed first.
fn json_object(output: &str) -> Option<Value> {
    let trimmed = output.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value.is_object().then_some(value);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str::<Value>(trimmed.get(start..=end)?).ok().filter(Value::is_object)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        let file = dir.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    /// `.env` secrets never leave the project: of its values, a report carries only
    /// `DB_CONNECTION` and `DB_DATABASE`.
    #[test]
    fn env_values_other_than_the_database_never_reach_a_report() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(dir, "artisan", "<?php\n");
        write(dir, "package.json", r#"{"dependencies":{"vue":"^3.5.0"}}"#);
        write(dir, "composer.json", r#"{"require":{"php":"^8.2","laravel/framework":"^13.0"}}"#);
        write(dir, "composer.lock", r#"{"packages":[{"name":"laravel/framework","version":"v13.1.0"}],"packages-dev":[]}"#);
        write(
            dir,
            ".env",
            concat!(
                "APP_ENV=local\n",
                "export APP_KEY=base64:xyz\n",
                "DB_CONNECTION=mysql\n",
                "DB_DATABASE=shop # the shop\n",
                "DB_DATABASE_URL=mysql://root:hunter2@127.0.0.1/shop\n",
                "DB_PASSWORD=supersecret\n",
                "MAIL_PASSWORD=\"mail-pass-123\"\n",
                "PRIVATE_KEY=\"-----BEGIN KEY-----\n",
                "STRIPE_SECRET=sk_live_hidden\n",
                "-----END KEY-----\"\n",
            ),
        );

        let inspection = inspect(dir);
        let mut probed = Probed::default();
        probed.versions.insert("node", Some("22.3.0".into()));
        probed.versions.insert("php", Some("8.3.9".into()));
        let read = MigrationRead { pending: vec!["2026_09_01_120000_add_x_table".into()], error: None, at: 1, running: false };
        let reads: HashMap<&'static str, MigrationRead> = inspection.migrations.iter().map(|found| (found.tool, read.clone())).collect();
        let scans: DepScans = ECOSYSTEMS.iter().map(|ecosystem| (ecosystem.id().to_string(), DepScan::default())).collect();
        let json = serde_json::to_string(&report("/p", &inspection, &probed, scans, Some(&reads), None)).unwrap();

        for secret in ["supersecret", "base64:xyz", "xyz", "mail-pass-123", "hunter2", "sk_live_hidden", "BEGIN KEY", "local", "the shop"] {
            assert!(!json.contains(secret), "{secret} leaked into {json}");
        }
        let databases: Vec<Option<DepDatabase>> = inspection.migrations.iter().map(|found| found.database.clone()).collect();
        assert_eq!(databases, [Some(DepDatabase { connection: Some("mysql".into()), name: Some("shop".into()) })]);
        assert_eq!(inspection.framework.as_ref().map(|f| f.version.as_str()), Some("13.1.0"));
    }
}
