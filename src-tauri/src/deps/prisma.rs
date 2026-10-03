//! Prisma Migrate: a project with `prisma` in package.json, a Prisma schema and a `migrations`
//! folder beside it (without one the project doesn't migrate: `db push`, MongoDB). The pending
//! ones come from `prisma migrate status`, which only reads; migrate is `prisma migrate deploy`,
//! which applies what is pending and nothing else (never `migrate dev`, which can reset).
//! Prisma up to 7 only: the `prisma` package of version 8 is another command line, without
//! `migrate`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::node_exec::{depends, failure, plain, runner, through_runners};
use super::{read_json, text, Captured, Commands, DepDatabase, MigrationTool, MigrationsError};

pub struct Prisma;

static VARIANTS: [Commands; 4] = through_runners!("prisma migrate status", "prisma migrate deploy");

/// The last major version whose command line has `migrate status` and `migrate deploy`.
const LAST_MAJOR: u64 = 7;

impl MigrationTool for Prisma {
    fn id(&self) -> &'static str {
        "prisma"
    }

    fn detect(&self, dir: &Path) -> bool {
        layout(dir).is_some()
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn variant(&self, dir: &Path) -> usize {
        runner(dir)
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_status(output)
    }

    fn sources(&self, dir: &Path) -> Vec<PathBuf> {
        layout(dir).map(|layout| layout.migrations).into_iter().collect()
    }

    fn ecosystem(&self) -> &'static str {
        "npm"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["prisma/schema.prisma", "schema.prisma", "node_modules/prisma/package.json"]
    }

    /// The schema's `provider` (`postgresql`, `sqlite`); where the database is stays Prisma's
    /// to read.
    fn database(&self, dir: &Path) -> Option<DepDatabase> {
        let schema = fs::read_to_string(dir.join(layout(dir)?.schema)).ok()?;
        Some(DepDatabase { connection: Some(provider(&schema)?), name: None })
    }
}

/// Where a project keeps its schema and its migrations, relative to it.
struct Layout {
    /// The schema file, or the folder of a schema in several files.
    schema: PathBuf,
    migrations: PathBuf,
}

/// The project's Prisma schema and the `migrations` folder beside it; `None` when it has no
/// `prisma` up to version 7, no schema or no migrations. The schema is where package.json's
/// `prisma.schema` says, else `prisma/schema.prisma`, `prisma/schema` or `schema.prisma`.
fn layout(dir: &Path) -> Option<Layout> {
    let (manifest, range) = depends(dir, "prisma")?;
    if major(dir, &range).is_some_and(|major| major > LAST_MAJOR) {
        return None;
    }

    let configured = manifest.get("prisma").and_then(|prisma| text(prisma, "schema")).map(PathBuf::from).filter(|path| inside(path));
    let schema = match configured {
        Some(schema) => schema,
        None => ["prisma/schema.prisma", "prisma/schema", "schema.prisma"].iter().map(PathBuf::from).find(|path| dir.join(path).exists())?,
    };

    // Beside a schema file; in a schema folder, or beside it.
    let at = dir.join(&schema);
    let beside = schema.parent().unwrap_or(Path::new("")).join("migrations");
    let candidates = if at.is_dir() {
        vec![schema.join("migrations"), beside]
    } else if at.is_file() {
        vec![beside]
    } else {
        return None;
    };
    let migrations = candidates.into_iter().find(|path| dir.join(path).is_dir())?;
    Some(Layout { schema, migrations })
}

/// A path that stays inside the project when joined to it.
fn inside(path: &Path) -> bool {
    path.components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

/// The major version of the installed `prisma`, else of the range package.json asks for
/// (`^6.19.0`); `None` when neither names a number (`latest`).
fn major(dir: &Path, range: &str) -> Option<u64> {
    let installed = read_json(&dir.join("node_modules").join("prisma").join("package.json"));
    let version = installed.as_ref().and_then(|package| text(package, "version")).unwrap_or(range);
    let digits: String = version.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The `provider = "…"` of a schema's `datasource` block, when it is a plain word.
fn provider(schema: &str) -> Option<String> {
    let mut lines = schema.lines().map(str::trim).filter(|line| !line.starts_with("//"));
    lines.find(|line| line.starts_with("datasource ") && line.ends_with('{'))?;
    let value = lines.take_while(|line| !line.starts_with('}')).find_map(|line| line.strip_prefix("provider")?.trim_start().strip_prefix('='))?;
    let value = value.trim().strip_prefix('"')?.split('"').next()?;
    (!value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric())).then(|| value.to_string())
}

/// A finished `migrate status`: its pending migrations, or why there is no list. It exits with
/// 1 when migrations are pending, so the list counts before the exit code.
fn read_status(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    if let Some(pending) = pending_migrations(&plain(&output.stdout)) {
        return Ok(pending);
    }
    if output.success {
        return Ok(Vec::new());
    }

    // P1000 and P1010: refused; P1001, P1002, P1011 and P1017: not reached; P1003: no such
    // database.
    const UNREACHABLE: [&str; 7] = ["P1000:", "P1001:", "P1002:", "P1003:", "P1010:", "P1011:", "P1017:"];
    let text = format!("{}\n{}", output.stdout, output.stderr);
    if UNREACHABLE.iter().any(|code| text.contains(code)) {
        return Err(MigrationsError::Unreachable);
    }
    Err(failure(&text, "prisma"))
}

/// The names under `Following migration(s) have not yet been applied:`, up to the next empty
/// line; `None` when the output has no such heading (`Database schema is up to date!`).
fn pending_migrations(output: &str) -> Option<Vec<String>> {
    let mut lines = output.lines().map(str::trim);
    lines.find(|line| line.ends_with("have not yet been applied:"))?;
    Some(lines.take_while(|line| !line.is_empty()).map(str::to_string).collect())
}
