//! Laravel's migrations: a project with `artisan` and `laravel/framework` in composer.lock. The
//! pending ones come from `php artisan migrate:status`; migrate is exactly `php artisan migrate`
//! and refused when `.env` says production or sets no `APP_ENV` (Laravel then counts as
//! production). `DB_CONNECTION` and `DB_DATABASE` of `.env` name what a migrate runs against.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::composer::locked_packages;
use super::{env_value, read_json, Captured, Commands, DepDatabase, DepFramework, EnvValue, MigrationTool, MigrationsError};
use crate::i18n::t;

pub struct Laravel;

/// The read-only status command: which migrations the database has run. The only other artisan
/// command there is.
const STATUS: &str = "php artisan migrate:status --no-ansi --no-interaction";
/// The one migrate command there is (no `--force`, never fresh, refresh, rollback, reset, wipe
/// or seed), run only from the user's explicit confirm.
const MIGRATE: &str = "php artisan migrate";

static VARIANTS: [Commands; 1] = [Commands { status: STATUS, migrate: MIGRATE, needs: &["php"] }];

const FRAMEWORK: &str = "laravel/framework";

impl MigrationTool for Laravel {
    fn id(&self) -> &'static str {
        "laravel"
    }

    fn detect(&self, dir: &Path) -> bool {
        framework_version(dir).is_some()
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_status(output)
    }

    fn sources(&self, _dir: &Path) -> Vec<PathBuf> {
        vec![PathBuf::from("database/migrations")]
    }

    fn ecosystem(&self) -> &'static str {
        "composer"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["artisan", ".env"]
    }

    fn refuse(&self, dir: &Path) -> Option<String> {
        let env = fs::read_to_string(dir.join(".env")).ok();
        match env.as_deref().and_then(|env| env_value(env, EnvValue::AppEnv)) {
            None => Some(t!("core.error.depsNoAppEnv")),
            Some(app_env) if app_env.trim().eq_ignore_ascii_case("production") => Some(t!("core.error.depsProduction")),
            Some(_) => None,
        }
    }

    fn database(&self, dir: &Path) -> Option<DepDatabase> {
        let env = fs::read_to_string(dir.join(".env")).ok();
        Some(DepDatabase {
            connection: env.as_deref().and_then(|env| env_value(env, EnvValue::DbConnection)),
            name: env.as_deref().and_then(|env| env_value(env, EnvValue::DbDatabase)),
        })
    }

    fn framework(&self, dir: &Path) -> Option<DepFramework> {
        framework_version(dir).map(|version| DepFramework { name: FRAMEWORK.into(), version: version.trim_start_matches('v').to_string() })
    }
}

/// The version composer.lock locks `laravel/framework` at, in a project with `artisan` and a
/// composer.json; `None`: not a Laravel project.
fn framework_version(dir: &Path) -> Option<String> {
    if !dir.join("artisan").is_file() || !dir.join("composer.json").is_file() {
        return None;
    }
    let lock = read_json(&dir.join("composer.lock"))?;
    locked_packages(&lock, true).into_iter().find(|p| p.name == FRAMEWORK).map(|p| p.version)
}

/// A finished `migrate:status`: its pending migrations, or why there is no list.
fn read_status(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    let text = format!("{}\n{}", output.stdout, output.stderr);
    if text.contains("Migration table not found") {
        return Err(MigrationsError::NoTable);
    }
    if !output.success {
        const UNREACHABLE: [&str; 7] = [
            "sqlstate[hy000] [2002]",
            "connection refused",
            "could not connect",
            "access denied",
            "sqlstate[08006]",
            "unable to open database file",
            "database file at path",
        ];
        let text = text.to_lowercase();
        let unreachable =
            UNREACHABLE.iter().any(|needle| text.contains(needle)) || (text.contains("sqlstate") && text.contains("no such file or directory"));
        return Err(if unreachable { MigrationsError::Unreachable } else { MigrationsError::Failed });
    }
    Ok(pending_migrations(&output.stdout))
}

/// A migration's name: `2026_09_01_120000_add_x_table`.
static MIGRATION_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}_\d{2}_\d{2}_\d{6}_\S+$").unwrap());

/// The Pending rows of `migrate:status`, in order: Laravel 10+'s
/// `  2026_09_01_120000_add_x_table ........ Pending` (`… [1] Ran` when run), or the older
/// table's `| No   | 2014_10_12_000000_create_users_table |`.
fn pending_migrations(output: &str) -> Vec<String> {
    let mut pending: Vec<String> = Vec::new();
    for line in output.lines().map(str::trim) {
        let Some(name) = line.split(|c: char| c.is_whitespace() || c == '|').find(|word| MIGRATION_NAME.is_match(word)) else {
            continue;
        };
        let is_pending = if line.starts_with('|') {
            matches!(line.split('|').map(str::trim).find(|cell| !cell.is_empty()), Some("No" | "N"))
        } else {
            line.split_whitespace().last() == Some("Pending")
        };
        if is_pending && !pending.iter().any(|known| known == name) {
            pending.push(name.to_string());
        }
    }
    pending
}
