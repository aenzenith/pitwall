//! Java and Kotlin: Maven (`pom.xml`) and Gradle (`build.gradle`, `settings.gradle`, `.kts` or
//! not). Both resolve their dependencies when they build, into a cache outside the project
//! (`~/.m2`, `~/.gradle`): there is no install to run and nothing in the project to hold a lock
//! file against, so the check is `Unknown` and there is no install command.
//!
//! Neither has a scan: Maven's dependency updates come from a plugin it downloads on first use
//! (`versions:display-dependency-updates`) and prints as free text, Gradle has no outdated report
//! of its own, and neither audits.
//!
//! The runtime is the JDK, by its major: the build file's release, else what a version manager's
//! file pins; a newer JDK satisfies it.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::{CheckState, DepPackage, Ecosystem, Flavor, Runtime};

pub struct Java;

const GRADLE_BUILDS: [&str; 2] = ["build.gradle", "build.gradle.kts"];
const GRADLE_SETTINGS: [&str; 2] = ["settings.gradle", "settings.gradle.kts"];

/// `java -version` prints to stderr.
static RUNTIMES: [Runtime; 1] =
    [Runtime { name: "java", probe: "java -version 2>&1", version: java_version, required: java_required, local: None, flavor: Flavor::Custom(java_holds) }];

impl Ecosystem for Java {
    fn id(&self) -> &'static str {
        "java"
    }

    fn manifest(&self) -> &'static str {
        "pom.xml"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        let maven = dir.join("pom.xml").is_file();
        let gradle = GRADLE_BUILDS.iter().chain(&GRADLE_SETTINGS).any(|name| dir.join(name).is_file());
        match (maven, gradle) {
            // With both builds, the one whose wrapper is there.
            (true, true) if wrapper(dir, "gradlew") && !wrapper(dir, "mvnw") => Some("gradle"),
            (true, _) => Some("maven"),
            (false, true) => Some("gradle"),
            (false, false) => None,
        }
    }

    fn watched(&self) -> &'static [&'static str] {
        &[
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
            "mvnw",
            "gradlew",
            ".java-version",
            ".sdkmanrc",
            ".tool-versions",
        ]
    }

    fn check(&self, _dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        (CheckState::Unknown, Vec::new())
    }

    fn needs(&self, dir: &Path, manager: &str) -> &'static [&'static str] {
        match manager {
            "gradle" if wrapper(dir, "gradlew") => &[],
            "gradle" => &["gradle"],
            _ if wrapper(dir, "mvnw") => &[],
            _ => &["mvn"],
        }
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }
}

/* ---------- the build files ---------- */

/// The project has the build's wrapper script (`mvnw`, `gradlew`): it runs as `./mvnw`, with no
/// Maven or Gradle on PATH.
pub(super) fn wrapper(dir: &Path, name: &str) -> bool {
    dir.join(name).is_file()
}

/// pom.xml without its comments.
pub(super) fn pom(dir: &Path) -> Option<String> {
    static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
    let xml = fs::read_to_string(dir.join("pom.xml")).ok()?;
    Some(COMMENT.replace_all(&xml, "").into_owned())
}

/// The project's own Gradle build file (`build.gradle`, else `build.gradle.kts`) without its
/// comments.
pub(super) fn gradle_build(dir: &Path) -> Option<String> {
    // `//` after a space or at a line's start: not the one in `https://`.
    static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?ms)/\*.*?\*/|(?:^|\s)//[^\n]*").unwrap());
    let script = GRADLE_BUILDS.iter().find_map(|name| fs::read_to_string(dir.join(name)).ok())?;
    Some(COMMENT.replace_all(&script, "").into_owned())
}

/* ---------- the runtime ---------- */

/// A Java version's major: `21`, `21.0.4` and `21.0.4+7` are 21; `1.8` and `1.8.0_292` are 8.
fn major(version: &str) -> Option<u64> {
    let mut numbers = version.trim().split(|c: char| !c.is_ascii_digit()).map(|number| number.parse::<u64>().ok());
    match numbers.next()?? {
        1 => numbers.next().flatten().or(Some(1)),
        major => Some(major),
    }
}

/// `java -version`: `openjdk version "21.0.4" 2024-07-16`, `java version "1.8.0_292"`.
fn java_version(output: &str) -> Option<String> {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"version "([^"]+)""#).unwrap());
    let version = VERSION.captures(output)?.get(1)?.as_str();
    major(version).map(|_| version.to_string())
}

/// `>=21` holds for an active JDK of major 21 or later.
fn java_holds(required: &str, active: &str) -> Option<bool> {
    let least: u64 = required.strip_prefix(">=")?.trim().parse().ok()?;
    Some(major(active)? >= least)
}

/// A project uses Java with a Maven or Gradle build, or a version manager's file that pins a
/// JDK. What it asks for, as `>=21`: pom.xml's compiler release, else the Gradle build's
/// toolchain or source compatibility, else `.java-version`, `.sdkmanrc`, `.tool-versions`.
fn java_required(dir: &Path) -> Option<Option<String>> {
    let pom = pom(dir);
    let gradle = gradle_build(dir);
    let read = |name: &str| fs::read_to_string(dir.join(name)).ok();
    let line = |text: String, prefix: &str| text.lines().find_map(|line| line.trim().strip_prefix(prefix).map(str::to_string));
    // `21`, `java=21.0.4-tem`, `java temurin-21.0.4+7`
    let pins = [
        read(".java-version").and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_string)),
        read(".sdkmanrc").and_then(|text| line(text, "java=")),
        read(".tool-versions").and_then(|text| line(text, "java ")),
    ];

    let builds = pom.is_some() || gradle.is_some() || GRADLE_SETTINGS.iter().any(|name| dir.join(name).is_file());
    if !builds && pins.iter().all(Option::is_none) {
        return None;
    }
    let asked = pom
        .as_deref()
        .and_then(pom_major)
        .or_else(|| gradle.as_deref().and_then(gradle_major))
        .or_else(|| pins.iter().flatten().find_map(|pin| pinned_major(pin)));
    Some(asked.map(|major| format!(">={major}")))
}

/// `<maven.compiler.release>`, `<java.version>` (Spring Boot's), `<maven.compiler.source>`: the
/// first that is a number (`21`, `1.8`), not another property (`${java.version}`).
fn pom_major(pom: &str) -> Option<u64> {
    static PROPERTY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<(maven\.compiler\.release|java\.version|maven\.compiler\.source)>\s*([^<\s]+)\s*</").unwrap());
    let set: Vec<(&str, &str)> = PROPERTY.captures_iter(pom).filter_map(|found| Some((found.get(1)?.as_str(), found.get(2)?.as_str()))).collect();
    let names = ["maven.compiler.release", "java.version", "maven.compiler.source"];
    names.iter().find_map(|name| set.iter().find(|(key, _)| key == name).and_then(|(_, value)| major(value)))
}

/// `jvmToolchain(21)`, `JavaLanguageVersion.of(21)`, `sourceCompatibility = JavaVersion.VERSION_21`
/// (`VERSION_1_8`), `sourceCompatibility = '21'`.
fn gradle_major(build: &str) -> Option<u64> {
    static ASKED: LazyLock<[Regex; 4]> = LazyLock::new(|| {
        [
            r"jvmToolchain\s*\(\s*(\d+)\s*\)",
            r#"JavaLanguageVersion\.of\s*\(\s*["']?(\d+)"#,
            r"sourceCompatibility\s*=?\s*(?:JavaVersion\.)?VERSION_(\d+(?:_\d+)?)",
            r#"sourceCompatibility\s*=?\s*["']?(\d+(?:\.\d+)?)"#,
        ]
        .map(|pattern| Regex::new(pattern).unwrap())
    });
    ASKED.iter().find_map(|pattern| major(pattern.captures(build)?.get(1)?.as_str()))
}

/// The major a version manager's entry pins: `21.0.4-tem` (SDKMAN), `temurin-21.0.4+7` and
/// `openjdk64-21.0.2` (asdf, jenv: the version follows the name), `22.3.r19-grl` (an old
/// GraalVM, for Java 19).
fn pinned_major(pin: &str) -> Option<u64> {
    static GRAAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.r(\d+)").unwrap());
    let pin = pin.split_whitespace().next()?;
    if let Some(java) = GRAAL.captures(pin).and_then(|found| found.get(1)?.as_str().parse().ok()) {
        return Some(java);
    }
    let digit = |text: &str| text.starts_with(|c: char| c.is_ascii_digit());
    if digit(pin) {
        return major(pin);
    }
    pin.match_indices('-').map(|(at, _)| &pin[at + 1..]).find(|rest| digit(rest)).and_then(major)
}
