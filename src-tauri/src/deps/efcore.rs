//! EF Core's migrations: a .NET project with a `Migrations` folder that holds a model snapshot
//! (`*ModelSnapshot.cs`) and an EF Core package among its references. The pending ones come from
//! `dotnet ef migrations list`; migrate is exactly `dotnet ef database update`, which only ever
//! goes forward to the latest migration.
//!
//! Both run in the project folder without `--project`, `--startup-project` or `--context`, which
//! differ per project and so can't be constants: they work where that folder holds the one
//! project file with the one `DbContext`. Anywhere else `dotnet ef` itself refuses (no project
//! found, more than one context) and the read says `failed`; nothing is guessed.
//!
//! The status command has `--no-build`: a status read must not compile the project under a
//! running dev server. It lists what the last build knew, so the built assembly counts among the
//! sources: once the project is built again, the list is read again. A project never built gives
//! `failed`. It only reads the database: EF Core asks for the applied migrations with a `SELECT`,
//! and only where `__EFMigrationsHistory` exists; the table is made by `database update` alone.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::dotnet::{frameworks, projects, property, references};
use super::{read_json, text, Captured, Commands, MigrationTool, MigrationsError};

pub struct EfCore;

/// Read-only: the migrations the built assembly has and whether the database ran each. With
/// `--prefix-output` the JSON's lines start with `data:`, apart from what the app logs.
const STATUS: &str = "dotnet ef migrations list --no-build --no-color --json --prefix-output";
/// Forward-only: without a migration named it applies the pending ones up to the latest.
const MIGRATE: &str = "dotnet ef database update";

/// `dotnet ef` is a tool of its own: on PATH as `dotnet-ef` when installed for the user, or
/// listed in the project's tool manifest (then `dotnet` finds it).
static VARIANTS: [Commands; 2] =
    [Commands { status: STATUS, migrate: MIGRATE, needs: &["dotnet", "dotnet-ef"] }, Commands { status: STATUS, migrate: MIGRATE, needs: &["dotnet"] }];

const MANIFESTS: [&str; 2] = [".config/dotnet-tools.json", "dotnet-tools.json"];

impl MigrationTool for EfCore {
    fn id(&self) -> &'static str {
        "efcore"
    }

    fn detect(&self, dir: &Path) -> bool {
        let projects = projects(dir);
        let uses_ef = |project: &PathBuf| {
            let xml = fs::read_to_string(dir.join(project)).unwrap_or_default();
            references(&xml, "PackageReference").iter().any(|reference| reference.name.to_lowercase().contains("entityframeworkcore"))
        };
        !migration_folders(dir, &projects).is_empty() && projects.iter().any(uses_ef)
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn variant(&self, dir: &Path) -> usize {
        let lists_ef = |manifest: Value| manifest.get("tools").and_then(|tools| tools.get("dotnet-ef")).is_some();
        usize::from(MANIFESTS.iter().filter_map(|name| read_json(&dir.join(name))).any(lists_ef))
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_list(output)
    }

    fn sources(&self, dir: &Path) -> Vec<PathBuf> {
        let projects = projects(dir);
        let mut sources = migration_folders(dir, &projects);
        sources.extend(projects.iter().flat_map(|project| assemblies(dir, project)));
        sources
    }

    fn ecosystem(&self) -> &'static str {
        "dotnet"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["Migrations", ".config/dotnet-tools.json", "dotnet-tools.json"]
    }
}

/// The `Migrations` folders, relative to the project, of the projects that have a model snapshot
/// in theirs.
fn migration_folders(dir: &Path, projects: &[PathBuf]) -> Vec<PathBuf> {
    let has_snapshot = |folder: &PathBuf| {
        let files = fs::read_dir(dir.join(folder)).into_iter().flatten().flatten();
        files.take(4096).any(|file| file.file_name().to_string_lossy().ends_with("ModelSnapshot.cs"))
    };
    let mut folders: Vec<PathBuf> = projects.iter().filter_map(|project| Some(project.parent()?.join("Migrations"))).filter(has_snapshot).collect();
    folders.dedup();
    folders
}

/// Where a project's debug build puts its assembly, per target framework:
/// `bin/Debug/net8.0/App.dll`.
fn assemblies(dir: &Path, project: &Path) -> Vec<PathBuf> {
    let (Some(folder), Some(stem)) = (project.parent(), project.file_stem()) else {
        return Vec::new();
    };
    let xml = fs::read_to_string(dir.join(project)).unwrap_or_default();
    let name = property(&xml, "AssemblyName").unwrap_or_else(|| stem.to_string_lossy().into_owned());
    frameworks(&xml).into_iter().map(|framework| folder.join("bin").join("Debug").join(framework).join(format!("{name}.dll"))).collect()
}

/// A finished `migrations list`: its pending migrations, or why there is no list.
fn read_list(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    // `dotnet` found no `ef`: neither on PATH nor restored from the tool manifest.
    const NO_TOOL: [&str; 4] = [
        "dotnet-ef does not exist",
        "could not execute because the specified command or file was not found",
        "no executable found matching command",
        "dotnet tool restore",
    ];
    let said = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
    if NO_TOOL.iter().any(|needle| said.contains(needle)) {
        return Err(MigrationsError::ToolMissing);
    }
    if !output.success {
        return Err(MigrationsError::Failed);
    }

    let data: Vec<&str> = output.stdout.lines().filter_map(|line| line.trim_start().strip_prefix("data:")).collect();
    let list = serde_json::from_str::<Value>(data.join("\n").trim()).map_err(|_| MigrationsError::Failed)?;
    let mut pending = Vec::new();
    for migration in list.as_array().ok_or(MigrationsError::Failed)? {
        let id = text(migration, "id").ok_or(MigrationsError::Failed)?;
        match migration.get("applied") {
            Some(Value::Bool(true)) => {}
            Some(Value::Bool(false)) => pending.push(id.to_string()),
            // EF Core couldn't ask the database and lists the migrations without it.
            Some(Value::Null) => return Err(MigrationsError::Unreachable),
            // Tools older than 5.0 don't say.
            _ => return Err(MigrationsError::Failed),
        }
    }
    Ok(pending)
}
