//! Django's migrations: a project with a `manage.py` and Django, named by its lock file or by
//! `manage.py` itself (requirements often live in a folder of their own). The pending ones come
//! from `manage.py showmigrations --plan`, which only reads; migrate is exactly
//! `manage.py migrate --no-input`. Both run with the project environment's interpreter, else
//! through the project's manager, else with `python3` (`python::runner`).
//!
//! Django's settings are code: what a migrate would run against isn't known here, and nothing
//! in the files tells production apart, so this module never refuses.

use std::fs;
use std::path::{Path, PathBuf};

use super::python::{installed_version, locked_version, runner};
use super::{Captured, Commands, DepFramework, MigrationTool, MigrationsError};

pub struct Django;

/// The read-only status command and the one migrate command there is (no target, so never
/// backwards; no `flush`, `--fake` or `--run-syncdb`), behind `$python`.
macro_rules! with {
    ($python:literal, $needs:expr) => {
        Commands { status: concat!($python, " manage.py showmigrations --plan"), migrate: concat!($python, " manage.py migrate --no-input"), needs: $needs }
    };
}

/// In `python::runner`'s order.
static VARIANTS: [Commands; 6] = [
    with!(".venv/bin/python", &[]),
    with!("venv/bin/python", &[]),
    with!("uv run python", &["uv"]),
    with!("poetry run python", &["poetry"]),
    with!("pipenv run python", &["pipenv"]),
    with!("python3", &["python3"]),
];

const FRAMEWORK: &str = "django";

impl MigrationTool for Django {
    fn id(&self) -> &'static str {
        "django"
    }

    fn detect(&self, dir: &Path) -> bool {
        let Ok(manage) = fs::read_to_string(dir.join("manage.py")) else {
            return false;
        };
        manage.contains("django.core.management") || locked_version(dir, FRAMEWORK).is_some()
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn variant(&self, dir: &Path) -> usize {
        runner(dir, "python")
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_plan(output)
    }

    /// Every app's `migrations` folder, an app being a folder of the project or of one of its
    /// folders (`apps/orders`).
    fn sources(&self, dir: &Path) -> Vec<PathBuf> {
        let mut sources = Vec::new();
        for outer in folders(dir) {
            for inner in folders(&dir.join(&outer)) {
                let folder = Path::new(&outer).join(&inner);
                if inner == "migrations" {
                    sources.push(folder);
                } else if dir.join(&folder).join("migrations").is_dir() {
                    sources.push(folder.join("migrations"));
                }
            }
        }
        sources
    }

    fn ecosystem(&self) -> &'static str {
        "python"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["manage.py"]
    }

    /// Django at the version the lock pins it at, else the one the project's environment has.
    fn framework(&self, dir: &Path) -> Option<DepFramework> {
        let version = locked_version(dir, FRAMEWORK).flatten().or_else(|| installed_version(dir, FRAMEWORK))?;
        Some(DepFramework { name: FRAMEWORK.into(), version })
    }
}

/// The folders of `dir` an app can be in, by name: no hidden ones, no environments, no
/// `node_modules`.
fn folders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.') && !matches!(name.as_str(), "node_modules" | "venv" | "env" | "__pycache__"))
        .collect();
    names.sort();
    names
}

/// A finished `showmigrations --plan`: its pending migrations, or why there is no list. A
/// database without Django's migrations table is no error to Django: every migration is
/// pending then.
fn read_plan(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    if output.success {
        return Ok(pending_migrations(&output.stdout));
    }
    // A query an app makes while it loads, on a table no migration has made yet: SQLite's,
    // MySQL's and PostgreSQL's words for it.
    let no_table =
        |line: &str| line.contains("no such table") || line.contains("doesn't exist") || (line.contains("relation") && line.contains("does not exist"));
    const UNREACHABLE: [&str; 7] =
        ["operationalerror", "interfaceerror", "connection refused", "could not connect", "can't connect", "access denied", "unable to open database file"];
    let text = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    Err(if text.lines().any(no_table) {
        MigrationsError::NoTable
    } else if UNREACHABLE.iter().any(|needle| text.contains(needle)) {
        MigrationsError::Unreachable
    } else {
        MigrationsError::Failed
    })
}

/// The `[ ]` rows of the plan, in its order: `[ ]  orders.0002_add_total` is pending,
/// `[X]  orders.0001_initial` has run.
fn pending_migrations(output: &str) -> Vec<String> {
    let mut pending: Vec<String> = Vec::new();
    for line in output.lines() {
        let Some(name) = line.trim_start().strip_prefix("[ ]").and_then(|rest| rest.split_whitespace().next()) else {
            continue;
        };
        if name.contains('.') && !pending.iter().any(|known| known == name) {
            pending.push(name.to_string());
        }
    }
    pending
}
