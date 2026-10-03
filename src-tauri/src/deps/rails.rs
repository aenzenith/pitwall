//! Rails' migrations (Active Record): a project with `bin/rails`, `config/application.rb` and,
//! when it has a lock file, `activerecord` in it. The pending ones are the `down` rows of
//! `bin/rails db:migrate:status`; migrate is exactly `bin/rails db:migrate`. Which database
//! they run against is Rails' to say (`config/database.yml` is never read).
//!
//! `db:migrate:status` creates no table: without one it only says so. On SQLite, connecting
//! creates the database's folder and an empty database file where there is none.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use super::ruby::Lock;
use super::{Captured, Commands, DepFramework, MigrationTool, MigrationsError};

pub struct Rails;

/// The read-only status command: which migrations each database has run.
const STATUS: &str = "bin/rails db:migrate:status";
/// The one migrate command there is, run only from the user's explicit confirm.
const MIGRATE: &str = "bin/rails db:migrate";

/// `bin/rails` is a Ruby script of the project's.
static VARIANTS: [Commands; 1] = [Commands { status: STATUS, migrate: MIGRATE, needs: &["ruby"] }];

impl MigrationTool for Rails {
    fn id(&self) -> &'static str {
        "rails"
    }

    /// An engine has `bin/rails` but no `config/application.rb`, and no `db:migrate:status`; an
    /// application without Active Record has no migrations.
    fn detect(&self, dir: &Path) -> bool {
        dir.join("bin").join("rails").is_file()
            && dir.join("config").join("application.rb").is_file()
            && Lock::read(dir).is_none_or(|lock| lock.version("activerecord").is_some())
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_status(output)
    }

    /// `db/migrate`, and a further database's own folder (`db/animals_migrate`).
    fn sources(&self, dir: &Path) -> Vec<PathBuf> {
        let mut sources = vec![PathBuf::from("db/migrate")];
        if let Ok(entries) = fs::read_dir(dir.join("db")) {
            let names = entries.filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()));
            sources.extend(names.filter(|name| name.ends_with("_migrate")).map(|name| Path::new("db").join(name)));
        }
        sources.sort();
        sources
    }

    fn ecosystem(&self) -> &'static str {
        "ruby"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["bin/rails", "config/application.rb", "Gemfile.lock"]
    }

    fn framework(&self, dir: &Path) -> Option<DepFramework> {
        let lock = Lock::read(dir)?;
        let version = lock.version("rails").or_else(|| lock.version("railties"))?;
        Some(DepFramework { name: "rails".into(), version: version.to_string() })
    }

    /// The command boots the whole application first.
    fn status_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }
}

/// A finished `db:migrate:status`: its pending migrations, or why there is no list.
fn read_status(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    let text = format!("{}\n{}", output.stdout, output.stderr);
    if text.contains("Schema migrations table does not exist yet") {
        return Err(MigrationsError::NoTable);
    }
    if !output.success {
        // `env` finds no Ruby, or the version manager hasn't the one the project names.
        const NO_RUBY: [&str; 5] = [
            "env: ruby: no such file",
            "env: 'ruby': no such file",
            "env: ‘ruby’: no such file",
            "is not installed (set by",
            "no version is set for command ruby",
        ];
        const UNREACHABLE: [&str; 16] = [
            "connectionnotestablished",
            "nodatabaseerror",
            "databaseconnectionerror",
            "connectionbad",
            "connectionerror",
            "connection refused",
            "connection to server",
            "could not connect to server",
            "could not translate host name",
            "can't connect to",
            "unknown database",
            "access denied for user",
            "password authentication failed",
            "we could not find your database",
            "there is an issue connecting",
            "unable to open database file",
        ];
        let text = text.to_lowercase();
        return Err(if NO_RUBY.iter().any(|needle| text.contains(needle)) {
            MigrationsError::ToolMissing
        } else if UNREACHABLE.iter().any(|needle| text.contains(needle)) {
            MigrationsError::Unreachable
        } else {
            MigrationsError::Failed
        });
    }
    Ok(pending_migrations(&output.stdout))
}

/// The `down` rows, in order, of every database's table:
///
/// ```text
/// database: storage/development.sqlite3
///
///  Status   Migration ID    Migration Name
/// --------------------------------------------------
///    up     20240101010101  Create users
///   down    20240102020202  Add email to users
/// ```
///
/// A pending migration is its id and name as printed: `20240102020202 Add email to users`.
fn pending_migrations(output: &str) -> Vec<String> {
    static ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(up|down)\s+(\d+)\s*(.*)$").unwrap());
    let mut pending: Vec<String> = Vec::new();
    for line in output.lines() {
        let Some(row) = ROW.captures(line) else {
            continue;
        };
        let name = format!("{} {}", &row[2], row[3].trim()).trim().to_string();
        if &row[1] == "down" && !pending.contains(&name) {
            pending.push(name);
        }
    }
    pending
}
