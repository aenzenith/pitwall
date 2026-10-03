//! Doctrine Migrations in a Symfony project: `bin/console` and `doctrine/doctrine-migrations-bundle`
//! 3 or later in composer.lock (the 2.x bundle has no `list`). The pending ones come from
//! `doctrine:migrations:list`, which only reads: it doesn't create the versions table (SQLite
//! itself leaves an empty file where the database file was missing). Migrate is
//! `doctrine:migrations:migrate` without a version, which goes up to the latest and never back.
//! It is refused when `APP_ENV` is `prod` (`.env.local`, then `.env`) or the environment was
//! dumped for production (`.env.local.php`). `DATABASE_URL` is never read.

use std::fs;
use std::path::{Path, PathBuf};

use super::composer::locked_packages;
use super::{env_value, read_json, Captured, Commands, DepFramework, EnvValue, MigrationTool, MigrationsError};
use crate::i18n::t;

pub struct Doctrine;

/// The read-only status command: every migration with `migrated` or `not migrated`.
const STATUS: &str = "php bin/console doctrine:migrations:list --no-ansi --no-interaction";
/// The one migrate command there is: no version argument (so never `first`, `prev` or a
/// version behind), run only from the user's explicit confirm. Without `--no-interaction` it
/// would wait for an answer nobody can give.
const MIGRATE: &str = "php bin/console doctrine:migrations:migrate --no-interaction --no-ansi";

static VARIANTS: [Commands; 1] = [Commands { status: STATUS, migrate: MIGRATE, needs: &["php"] }];

const BUNDLE: &str = "doctrine/doctrine-migrations-bundle";
const FRAMEWORK: &str = "symfony/framework-bundle";

impl MigrationTool for Doctrine {
    fn id(&self) -> &'static str {
        "doctrine"
    }

    fn detect(&self, dir: &Path) -> bool {
        let bundle = locked(dir, BUNDLE).and_then(|version| version.split('.').next()?.parse::<u64>().ok());
        bundle.is_some_and(|major| major >= 3)
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_list(output)
    }

    fn sources(&self, _dir: &Path) -> Vec<PathBuf> {
        vec![PathBuf::from("migrations"), PathBuf::from("src/Migrations")]
    }

    fn ecosystem(&self) -> &'static str {
        "composer"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["bin/console", ".env", ".env.local", ".env.local.php"]
    }

    fn refuse(&self, dir: &Path) -> Option<String> {
        // `composer dump-env prod` wrote it, and Symfony then reads no `.env` file. It is PHP
        // with every value in it: never read.
        if dir.join(".env.local.php").is_file() {
            return Some(t!("core.error.depsProduction"));
        }
        // Without an `APP_ENV` Symfony runs as `dev`.
        let app_env = [".env.local", ".env"].iter().find_map(|name| env_value(&fs::read_to_string(dir.join(name)).ok()?, EnvValue::AppEnv))?;
        let production = ["prod", "production"].iter().any(|name| app_env.trim().eq_ignore_ascii_case(name));
        production.then(|| t!("core.error.depsProduction"))
    }

    fn framework(&self, dir: &Path) -> Option<DepFramework> {
        locked(dir, FRAMEWORK).map(|version| DepFramework { name: FRAMEWORK.into(), version })
    }
}

/// The version composer.lock locks `package` at, without its `v`, in a project with
/// `bin/console` and a composer.json; `None`: not a Symfony project, or it has no such package.
fn locked(dir: &Path, package: &str) -> Option<String> {
    if !dir.join("bin").join("console").is_file() || !dir.join("composer.json").is_file() {
        return None;
    }
    let lock = read_json(&dir.join("composer.lock"))?;
    locked_packages(&lock, true).into_iter().find(|p| p.name == package).map(|p| p.version.trim_start_matches('v').to_string())
}

/// A finished `doctrine:migrations:list`: its pending migrations, or why there is no list.
fn read_list(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    if !output.success {
        // The driver's own words: Symfony wraps the rest of a long message at 80 columns.
        const UNREACHABLE: [&str; 11] = [
            "sqlstate[hy000] [2002]",
            "sqlstate[hy000] [2006]",
            "sqlstate[hy000] [1044]",
            "sqlstate[hy000] [1045]",
            "sqlstate[hy000] [1049]",
            "sqlstate[hy000] [14]",
            "sqlstate[08001]",
            "sqlstate[08006]",
            "connection refused",
            "access denied",
            "unable to open database file",
        ];
        let text = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
        let unreachable = UNREACHABLE.iter().any(|needle| text.contains(needle));
        return Err(if unreachable { MigrationsError::Unreachable } else { MigrationsError::Failed });
    }
    Ok(pending_migrations(&output.stdout))
}

/// The `not migrated` rows of the list's table, in order, by their class without its namespace:
/// `| DoctrineMigrations\Version20240303000000 | not migrated |  |  | adds tags |`.
fn pending_migrations(output: &str) -> Vec<String> {
    let mut pending: Vec<String> = Vec::new();
    for line in output.lines().map(str::trim).filter(|line| line.starts_with('|')) {
        // The first two cells only: a description may have a `|` of its own.
        let mut cells = line.split('|').skip(1).map(str::trim);
        let (Some(class), Some("not migrated")) = (cells.next(), cells.next()) else {
            continue;
        };
        let name = class.rsplit('\\').next().unwrap_or(class);
        if !name.is_empty() && !pending.iter().any(|known| known == name) {
            pending.push(name.to_string());
        }
    }
    pending
}
