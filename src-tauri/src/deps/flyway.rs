//! Flyway's migrations, only where the build itself has Flyway's plugin: `flyway-maven-plugin`
//! in pom.xml, the `org.flywaydb.flyway` plugin in the Gradle build. The plugin has the
//! connection; Pitwall reads none of it. A project with the Flyway library alone (Spring Boot)
//! migrates when the app starts: there is nothing to run for it, and it isn't detected.
//!
//! The pending ones come from the plugin's `info`, which only reads (an empty database gets no
//! history table from it); migrate is the plugin's `migrate`. A build that sets
//! `cleanOnValidationError` isn't detected either: there `migrate` may wipe the schema.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use super::java::{gradle_build, pom, wrapper};
use super::{Captured, Commands, MigrationTool, MigrationsError};

pub struct Flyway;

/// `-B`: Maven's batch mode, plain lines and never a question.
macro_rules! maven {
    ($maven:literal, $needs:expr) => {
        Commands { status: concat!($maven, " -B flyway:info"), migrate: concat!($maven, " flyway:migrate"), needs: $needs }
    };
}

macro_rules! gradle {
    ($gradle:literal, $needs:expr) => {
        Commands { status: concat!($gradle, " flywayInfo"), migrate: concat!($gradle, " flywayMigrate"), needs: $needs }
    };
}

/// The build's wrapper where the project has one, else the one on PATH.
static VARIANTS: [Commands; 4] = [maven!("./mvnw", &[]), maven!("mvn", &["mvn"]), gradle!("./gradlew", &[]), gradle!("gradle", &["gradle"])];

/// Flyway's own configuration files, looked at for one setting's name only.
const CONFIGS: [&str; 2] = ["flyway.conf", "flyway.toml"];
/// Lets `migrate` clean (drop everything) when a validation fails.
const CLEANS: &str = "cleanonvalidationerror";

#[derive(Clone, Copy)]
enum Build {
    Maven,
    Gradle,
}

impl MigrationTool for Flyway {
    fn id(&self) -> &'static str {
        "flyway"
    }

    fn detect(&self, dir: &Path) -> bool {
        build(dir).is_some()
    }

    fn variants(&self) -> &'static [Commands] {
        &VARIANTS
    }

    fn variant(&self, dir: &Path) -> usize {
        match build(dir) {
            Some(Build::Maven) => usize::from(!wrapper(dir, "mvnw")),
            Some(Build::Gradle) | None => 2 + usize::from(!wrapper(dir, "gradlew")),
        }
    }

    fn parse_pending(&self, output: &Captured) -> Result<Vec<String>, MigrationsError> {
        read_info(output)
    }

    /// Flyway's default location; a build may name others.
    fn sources(&self, _dir: &Path) -> Vec<PathBuf> {
        vec![PathBuf::from("src/main/resources/db/migration")]
    }

    fn ecosystem(&self) -> &'static str {
        "java"
    }

    fn watched(&self) -> &'static [&'static str] {
        &["pom.xml", "build.gradle", "build.gradle.kts", "mvnw", "gradlew", "flyway.conf", "flyway.toml"]
    }

    /// Maven and Gradle start a JVM and resolve the plugin before Flyway asks the database.
    fn status_timeout(&self) -> Duration {
        Duration::from_secs(90)
    }
}

/// The build that has Flyway's plugin; `None` without the plugin, and where the build or
/// Flyway's configuration sets `cleanOnValidationError`.
fn build(dir: &Path) -> Option<Build> {
    static MAVEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<artifactId>\s*flyway-maven-plugin\s*</artifactId>").unwrap());
    // `id 'org.flywaydb.flyway'`, `id("org.flywaydb.flyway")`, `apply plugin: 'org.flywaydb.flyway'`
    static GRADLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?:\bid\s*\(?\s*|\bapply\s+plugin\s*:\s*|\bapply\s*\(\s*plugin\s*=\s*)["']org\.flywaydb\.flyway["']"#).unwrap());

    let maven = pom(dir).filter(|pom| MAVEN.is_match(pom)).map(|pom| (Build::Maven, pom));
    let (build, script) = maven.or_else(|| gradle_build(dir).filter(|script| GRADLE.is_match(script)).map(|script| (Build::Gradle, script)))?;

    let setting = |line: &str| line.split(['=', ':']).next().unwrap_or_default().to_lowercase().contains(CLEANS);
    let configured = CONFIGS.iter().filter_map(|name| fs::read_to_string(dir.join(name)).ok()).any(|config| config.lines().any(setting));
    (!script.to_lowercase().contains(CLEANS) && !configured).then_some(build)
}

/// A finished `info`: its pending migrations, or why there is no list.
fn read_info(output: &Captured) -> Result<Vec<String>, MigrationsError> {
    if !output.success {
        // The wrapper found no JDK.
        const NO_JAVA: [&str; 3] = ["java_home", "unable to locate a java runtime", "no 'java' command"];
        // Flyway's own words; a refused connection alone may be Maven's to its repository.
        const UNREACHABLE: [&str; 2] = ["unable to obtain connection from database", "unable to connect to the database"];
        let said = format!("{}\n{}", output.stdout, output.stderr).to_lowercase();
        return Err(if UNREACHABLE.iter().any(|needle| said.contains(needle)) {
            MigrationsError::Unreachable
        } else if NO_JAVA.iter().any(|needle| said.contains(needle)) {
            MigrationsError::ToolMissing
        } else {
            MigrationsError::Failed
        });
    }
    pending_migrations(&output.stdout).ok_or(MigrationsError::Failed)
}

/// The Pending rows of `info`'s table, in order, as version and description (a repeatable one
/// has no version); `None` when the output has no such table.
///
/// ```text
/// +-----------+---------+---------------------+------+---------------------+---------+----------+
/// | Category  | Version | Description         | Type | Installed On        | State   | Undoable |
/// +-----------+---------+---------------------+------+---------------------+---------+----------+
/// | Versioned | 1       | Create person table | SQL  | 2026-10-03 12:18:31 | Success | No       |
/// | Versioned | 2       | Add people          | SQL  |                     | Pending | No       |
/// +-----------+---------+---------------------+------+---------------------+---------+----------+
/// ```
///
/// The columns differ by Flyway version, so they are taken from the header; its bars are every
/// row's, so a `|` inside a description is no column. Maven puts `[INFO] ` before a line.
fn pending_migrations(output: &str) -> Option<Vec<String>> {
    struct Columns {
        bars: Vec<usize>,
        version: usize,
        description: Option<usize>,
        state: usize,
    }
    let cells = |row: &[char], bars: &[usize]| -> Vec<String> {
        bars.windows(2).map(|pair| row[pair[0] + 1..pair[1]].iter().collect::<String>().trim().to_string()).collect()
    };

    let mut columns: Option<Columns> = None;
    let mut pending: Vec<String> = Vec::new();
    for line in output.lines() {
        let Some(start) = line.find('|') else {
            continue;
        };
        let row: Vec<char> = line[start..].trim_end().chars().collect();
        let bars: Vec<usize> = row.iter().enumerate().filter(|(_, c)| **c == '|').map(|(at, _)| at).collect();
        let named = cells(&row, &bars);
        let column = |name: &str| named.iter().position(|cell| cell == name);
        if let (Some(version), Some(state)) = (column("Version"), column("State")) {
            columns = Some(Columns { bars, version, description: column("Description"), state });
            continue;
        }

        let Some(columns) = &columns else {
            continue;
        };
        if !columns.bars.iter().all(|at| row.get(*at) == Some(&'|')) {
            continue;
        }
        let row = cells(&row, &columns.bars);
        if row[columns.state] != "Pending" {
            continue;
        }
        let name: Vec<&str> =
            [Some(columns.version), columns.description].into_iter().flatten().map(|at| row[at].as_str()).filter(|cell| !cell.is_empty()).collect();
        pending.push(name.join(" "));
    }
    columns.map(|_| pending)
}
