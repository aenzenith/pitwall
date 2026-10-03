//! mix: `mix.lock` against what is fetched into `deps/`. The runtime is Elixir: `.tool-versions`,
//! then mix.exs's `elixir:`. There is no scan: neither `mix hex.outdated` nor `mix hex.audit`
//! prints anything made for a program to read.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::{satisfies, version_in};
use super::{CheckState, DepPackage, Ecosystem, Flavor, Runtime};

pub struct Elixir;

const INSTALL: &str = "mix deps.get";

static RUNTIMES: [Runtime; 1] = [Runtime {
    name: "elixir",
    probe: "elixir --version",
    version: elixir_version,
    required: elixir_required,
    local: None,
    flavor: Flavor::Custom(elixir_satisfies),
}];

impl Ecosystem for Elixir {
    fn id(&self) -> &'static str {
        "elixir"
    }

    fn manifest(&self) -> &'static str {
        "mix.exs"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        (dir.join("mix.lock").is_file() || dir.join("mix.exs").is_file()).then_some("mix")
    }

    fn watched(&self) -> &'static [&'static str] {
        &["mix.exs", "mix.lock", "deps", ".tool-versions"]
    }

    fn check(&self, dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        check_mix(dir)
    }

    fn needs(&self, _dir: &Path, _manager: &str) -> &'static [&'static str] {
        &["mix"]
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, _dir: &Path, _manager: &str) -> Option<&'static str> {
        Some(INSTALL)
    }
}

/* ---------- the lock file ---------- */

enum Source {
    /// A Hex package at this version.
    Hex(String),
    /// A Git repository at this revision.
    Git(String),
}

struct Locked {
    source: Source,
    /// The dependencies it can't do without (its optional ones come only when something else
    /// asks for them).
    needs: Vec<String>,
}

/// mix.lock's entries by name, one per line:
///
/// ```text
/// "plug": {:hex, :plug, "1.16.1", "40c7…", [:mix], [{:mime, "~> 1.0", [hex: :mime, repo: "hexpm", optional: false]}], "hexpm", "a13f…"},
/// "phoenix": {:git, "https://github.com/phoenixframework/phoenix.git", "5d4f…", [branch: "main"]},
/// ```
fn parse_lock(text: &str) -> BTreeMap<String, Locked> {
    static ENTRY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*"?([A-Za-z0-9_]+)"?:\s*\{:(hex|git),\s*(.+)$"#).unwrap());
    static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^:"?[A-Za-z0-9_.\-]+"?,\s*"([^"]+)""#).unwrap());
    static GIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^"[^"]*",\s*"([0-9a-fA-F]{7,64})""#).unwrap());
    static NEEDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\{:"?([A-Za-z0-9_]+)"?,\s*(?:"[^"]*"|nil),\s*\[([^\]]*)\]"#).unwrap());

    let mut lock = BTreeMap::new();
    for line in text.lines() {
        let Some(entry) = ENTRY.captures(line) else {
            continue;
        };
        let rest = &entry[3];
        let locked = match &entry[2] {
            "hex" => HEX.captures(rest).map(|hex| Locked {
                source: Source::Hex(hex[1].to_string()),
                needs: NEEDS.captures_iter(rest).filter(|need| !need[2].contains("optional: true")).map(|need| need[1].to_string()).collect(),
            }),
            _ => GIT.captures(rest).map(|git| Locked { source: Source::Git(git[1].to_string()), needs: Vec::new() }),
        };
        if let Some(locked) = locked {
            lock.insert(entry[1].to_string(), locked);
        }
    }
    lock
}

/* ---------- the project ---------- */

/// A mix.exs without its comment lines (a new project's has `# {:dep_from_hexpm, "~> 0.3.0"}`).
fn mix_file(file: &Path) -> Option<String> {
    let text = fs::read_to_string(file).ok()?;
    Some(text.lines().filter(|line| !line.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n"))
}

/// The dependencies the project names itself, `{:phoenix, "~> 1.7"}`: mix.exs's and, in an
/// umbrella, those of the applications under `apps/`.
fn declared(dir: &Path, root: Option<&str>) -> HashSet<String> {
    static DEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\s*:([a-z][a-z0-9_]*)\s*,").unwrap());
    let apps = fs::read_dir(dir.join("apps")).into_iter().flatten().filter_map(|entry| mix_file(&entry.ok()?.path().join("mix.exs")));
    let files: Vec<String> = root.map(str::to_string).into_iter().chain(apps).collect();
    files.iter().flat_map(|text| DEP.captures_iter(text)).map(|dep| dep[1].to_string()).collect()
}

/// The version of the Hex package fetched into `at`: Hex unpacks its metadata there
/// (`hex_metadata.config`: `{<<"version">>,<<"1.16.1">>}.`); else the package's own mix.exs.
fn fetched_version(at: &Path) -> Option<String> {
    static METADATA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\{<<"version">>,\s*<<"([^"]+)">>\}"#).unwrap());
    static PROJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bversion:\s*"([^"]+)""#).unwrap());
    static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"@version\s+"([^"]+)""#).unwrap());

    let first = |pattern: &Regex, text: &str| pattern.captures(text).map(|found| found[1].to_string());
    if let Some(version) = fs::read_to_string(at.join("hex_metadata.config")).ok().and_then(|text| first(&METADATA, &text)) {
        return Some(version);
    }
    let text = fs::read_to_string(at.join("mix.exs")).ok()?;
    first(&PROJECT, &text).or_else(|| first(&ATTRIBUTE, &text))
}

/// The revision a fetched Git dependency is at: mix checks it out detached, so `.git/HEAD` is
/// the revision itself; a branch is looked up among the loose refs.
fn fetched_revision(at: &Path) -> Option<String> {
    let git = at.join(".git");
    let head = fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let revision = match head.strip_prefix("ref:").map(str::trim) {
        Some(name) if name.starts_with("refs/") && !name.split('/').any(|part| part == "..") => fs::read_to_string(git.join(name)).ok()?.trim().to_string(),
        Some(_) => return None,
        None => head.to_string(),
    };
    (revision.len() >= 7 && revision.chars().all(|c| c.is_ascii_hexdigit())).then_some(revision)
}

fn short(revision: &str) -> String {
    revision.chars().take(7).collect()
}

/// mix.lock against `deps/`: a locked dependency whose folder is missing, or holds another
/// version or revision. The lock keeps entries of dependencies the project has dropped
/// (`mix deps.unlock --unused` removes them) and mix never fetches those: only what the project
/// names, and what those can't do without, is expected. Dependencies by `path:` aren't in the
/// lock.
fn check_mix(dir: &Path) -> (CheckState, Vec<DepPackage>) {
    let unknown = (CheckState::Unknown, Vec::new());
    let root = mix_file(&dir.join("mix.exs"));
    let declared = declared(dir, root.as_deref());

    let Ok(text) = fs::read_to_string(dir.join("mix.lock")) else {
        // Never fetched: nothing to compare, unless there is nothing to fetch.
        return if root.is_some() && declared.is_empty() { (CheckState::Ok, Vec::new()) } else { unknown };
    };
    // The folder is elsewhere (an umbrella's application: `deps_path: "../../deps"`).
    if root.as_deref().is_some_and(|root| root.contains("deps_path:")) {
        return unknown;
    }
    let lock = parse_lock(&text);
    if lock.is_empty() {
        return if text.contains(':') { unknown } else { (CheckState::Ok, Vec::new()) };
    }

    let mut expected: Vec<&str> = declared.iter().map(String::as_str).filter(|name| lock.contains_key(*name)).collect();
    if expected.is_empty() {
        expected = lock.keys().map(String::as_str).collect();
    }
    let mut seen: HashSet<&str> = expected.iter().copied().collect();
    let mut packages = Vec::new();

    while let Some(name) = expected.pop() {
        let Some(locked) = lock.get(name) else {
            continue;
        };
        for need in locked.needs.iter().map(String::as_str) {
            if lock.contains_key(need) && seen.insert(need) {
                expected.push(need);
            }
        }

        // A folder whose version or revision can't be read counts as the locked one.
        let at = dir.join("deps").join(name);
        let fetched = fs::read_dir(&at).is_ok_and(|mut entries| entries.next().is_some());
        let (wanted, have, same) = match &locked.source {
            Source::Hex(version) => {
                let have = fetched_version(&at);
                let same = have.as_ref().is_none_or(|have| have == version);
                (version.clone(), have, same)
            }
            Source::Git(revision) => {
                let have = fetched_revision(&at);
                let same = have.as_ref().is_none_or(|have| have == revision);
                (short(revision), have.as_deref().map(short), same)
            }
        };
        if !fetched {
            packages.push(DepPackage { name: name.to_string(), locked: Some(wanted), installed: None });
        } else if !same {
            packages.push(DepPackage { name: name.to_string(), locked: Some(wanted), installed: have });
        }
    }

    (CheckState::Ok, packages)
}

/* ---------- the runtime ---------- */

/// The last line of `elixir --version`: `Elixir 1.18.1 (compiled with Erlang/OTP 27)`; the
/// lines before it are Erlang's.
fn elixir_version(output: &str) -> Option<String> {
    output.lines().map(str::trim).find_map(|line| line.strip_prefix("Elixir ")).and_then(version_in)
}

/// `.tool-versions`' `elixir 1.18.1-otp-27` line (asdf, mise), without the OTP it was built
/// for: `None` without one, `Some(None)` when it names no version (`system`, `ref:…`).
fn tool_versions(dir: &Path) -> Option<Option<String>> {
    let text = fs::read_to_string(dir.join(".tool-versions")).ok()?;
    let mut words = text.lines().map(str::split_whitespace).find(|words| words.clone().next() == Some("elixir"))?.skip(1);
    let version = words.next().filter(|version| version.starts_with(|c: char| c.is_ascii_digit()));
    Some(version.map(|version| version.split("-otp").next().unwrap_or(version).to_string()))
}

/// A project uses Elixir with a mix.exs (or its lock), or an `elixir` line in `.tool-versions`.
/// What it asks for: `.tool-versions`, then mix.exs's `elixir: "~> 1.17"`.
fn elixir_required(dir: &Path) -> Option<Option<String>> {
    static ELIXIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\belixir:\s*"([^"]+)""#).unwrap());
    let tool = tool_versions(dir);
    let project = mix_file(&dir.join("mix.exs"));
    if tool.is_none() && project.is_none() && !dir.join("mix.lock").is_file() {
        return None;
    }
    Some(tool.flatten().or_else(|| Some(ELIXIR.captures(&project?)?[1].to_string())))
}

/// Elixir's requirements are the shared ones with words between them: `>= 1.14.0 and < 2.0.0`,
/// `~> 1.15 or ~> 2.0`.
fn elixir_satisfies(constraint: &str, version: &str) -> Option<bool> {
    let words: Vec<&str> = constraint
        .split_whitespace()
        .filter_map(|word| match word {
            "and" => None,
            "or" => Some("||"),
            word => Some(word),
        })
        .collect();
    satisfies(&words.join(" "), version, Flavor::Npm)
}
