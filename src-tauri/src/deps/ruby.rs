//! Bundler: `Gemfile.lock` against the gems in the project's own bundle path (`vendor/bundle`,
//! or `.bundle/config`'s `BUNDLE_PATH` when it stays inside the project). Gems installed
//! anywhere else (the system's, a version manager's) can't be told from the project's files:
//! the check is `Unknown` then. The runtime is Ruby: `.ruby-version`, the Gemfile's `ruby`, the
//! lock's `RUBY VERSION`, then `.tool-versions`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::version_in;
use super::{CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadOutdated, Runtime, ScanPlan};

pub struct Ruby;

const GEMFILES: [&str; 2] = ["Gemfile", "gems.rb"];
const LOCKS: [&str; 2] = ["Gemfile.lock", "gems.locked"];

const INSTALL: &str = "bundle install";
/// Read-only; the gems of the Gemfile only, one per line. It exits with 1 when any is outdated.
const OUTDATED: &str = "bundle outdated --only-explicit --parseable";

static RUNTIMES: [Runtime; 1] =
    [Runtime { name: "ruby", probe: "ruby -v", version: ruby_version, required: ruby_required, local: None, flavor: Flavor::Custom(ruby_satisfies) }];

/// The gems that ship inside Ruby 3.1 or later ("default gems"). When the lock asks for the
/// very version Ruby has, Bundler uses Ruby's and installs nothing into the bundle path, and
/// which version Ruby has can't be read from the project: these are never listed as differing.
const DEFAULT_GEMS: [&str; 74] = [
    "abbrev",
    "base64",
    "benchmark",
    "bigdecimal",
    "bundler",
    "cgi",
    "csv",
    "date",
    "delegate",
    "did_you_mean",
    "digest",
    "drb",
    "english",
    "erb",
    "error_highlight",
    "etc",
    "fcntl",
    "fiddle",
    "fileutils",
    "find",
    "forwardable",
    "getoptlong",
    "io-console",
    "io-nonblock",
    "io-wait",
    "ipaddr",
    "irb",
    "json",
    "logger",
    "mutex_m",
    "net-http",
    "net-protocol",
    "nkf",
    "observer",
    "open-uri",
    "open3",
    "openssl",
    "optparse",
    "ostruct",
    "pathname",
    "pp",
    "prettyprint",
    "prism",
    "pstore",
    "psych",
    "racc",
    "rdoc",
    "readline",
    "readline-ext",
    "reline",
    "resolv",
    "resolv-replace",
    "rinda",
    "ruby2_keywords",
    "securerandom",
    "set",
    "shellwords",
    "singleton",
    "stringio",
    "strscan",
    "syntax_suggest",
    "syslog",
    "tempfile",
    "time",
    "timeout",
    "tmpdir",
    "tsort",
    "un",
    "uri",
    "weakref",
    "win32-registry",
    "win32ole",
    "yaml",
    "zlib",
];

impl Ecosystem for Ruby {
    fn id(&self) -> &'static str {
        "ruby"
    }

    fn manifest(&self) -> &'static str {
        "Gemfile"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        LOCKS.iter().chain(&GEMFILES).any(|name| dir.join(name).is_file()).then_some("bundler")
    }

    fn watched(&self) -> &'static [&'static str] {
        &[
            "Gemfile",
            "Gemfile.lock",
            "gems.rb",
            "gems.locked",
            ".bundle/config",
            "vendor/bundle",
            // RubyGems' record of what it installed, in a folder named after Ruby's ABI version
            // (a watched name takes no pattern).
            "vendor/bundle/ruby/3.1.0/specifications",
            "vendor/bundle/ruby/3.2.0/specifications",
            "vendor/bundle/ruby/3.3.0/specifications",
            "vendor/bundle/ruby/3.4.0/specifications",
            "vendor/bundle/ruby/3.5.0/specifications",
            "vendor/bundle/ruby/4.0.0/specifications",
            "vendor/bundle/ruby/4.1.0/specifications",
            ".ruby-version",
            ".tool-versions",
        ]
    }

    fn check(&self, dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        check_bundle(dir)
    }

    fn needs(&self, _dir: &Path, _manager: &str) -> &'static [&'static str] {
        &["bundle"]
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, _dir: &Path, _manager: &str) -> Option<&'static str> {
        Some(INSTALL)
    }

    /// No audit: `bundler-audit` is a gem of its own, not part of Bundler.
    fn scan_plan(&self, _dir: &Path, _manager: &str) -> ScanPlan {
        ScanPlan { outdated: Some((OUTDATED, bundle_outdated as ReadOutdated)), audit: None }
    }
}

/* ---------- the lock file ---------- */

/// What this app reads of a `Gemfile.lock`.
#[derive(Default)]
pub(super) struct Lock {
    /// The `GEM` sections' gems by name, each with every variant locked: `1.16.0`,
    /// `1.16.0-arm64-darwin`.
    gems: BTreeMap<String, Vec<String>>,
    /// The `GIT` sections' gems.
    git: Vec<GitGem>,
    /// The `PATH` sections' gems, used where they lie: name and version.
    path: Vec<(String, String)>,
    /// `RUBY VERSION`: the Ruby the lock was written with.
    ruby: Option<String>,
}

struct GitGem {
    name: String,
    version: String,
    revision: String,
}

impl Lock {
    pub(super) fn read(dir: &Path) -> Option<Self> {
        LOCKS.iter().find_map(|name| fs::read_to_string(dir.join(name)).ok()).map(|text| Self::parse(&text))
    }

    /// A section's name is at the line's start, its gems four spaces in (`    rack (2.0.9)`);
    /// deeper lines are what a gem depends on.
    fn parse(text: &str) -> Self {
        let mut lock = Lock::default();
        let mut section = "";
        let mut revision = "";

        for line in text.lines() {
            let body = line.trim();
            if body.is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if indent == 0 {
                section = body;
                revision = "";
                continue;
            }

            match section {
                // `   ruby 3.3.0p0`
                "RUBY VERSION" => {
                    if let Some(ruby) = body.strip_prefix("ruby ") {
                        lock.ruby = version_in(ruby);
                    }
                }
                "GIT" if indent == 2 => {
                    if let Some(sha) = body.strip_prefix("revision:") {
                        revision = sha.trim();
                    }
                }
                "GEM" | "GIT" | "PATH" if indent == 4 => {
                    let Some((name, version)) = body.strip_suffix(')').and_then(|spec| spec.split_once(" (")) else {
                        continue;
                    };
                    let (name, version) = (name.to_string(), version.to_string());
                    match section {
                        "GEM" => lock.gems.entry(name).or_default().push(version),
                        "GIT" => lock.git.push(GitGem { name, version, revision: revision.to_string() }),
                        _ => lock.path.push((name, version)),
                    }
                }
                _ => {}
            }
        }

        lock
    }

    /// The version `name` is locked at, whatever its source.
    pub(super) fn version(&self, name: &str) -> Option<&str> {
        let gem = self.gems.get(name).and_then(|variants| variants.first()).map(|version| plain(version));
        let git = || self.git.iter().find(|gem| gem.name == name).map(|gem| gem.version.as_str());
        let path = || self.path.iter().find(|(known, _)| known == name).map(|(_, version)| version.as_str());
        gem.or_else(git).or_else(path)
    }
}

/// A locked or installed version without its platform: `1.16.0-arm64-darwin` is `1.16.0`.
fn plain(version: &str) -> &str {
    version.split('-').next().unwrap_or(version)
}

/* ---------- where the gems are ---------- */

/// The settings of `.bundle/config` this app reads: where the gems go and whether some groups
/// are left out. Never another key of that file (it can hold a gem server's credentials).
#[derive(Default)]
struct Config {
    path: Option<String>,
    system: bool,
    deployment: bool,
    /// `BUNDLE_WITHOUT` or `BUNDLE_ONLY`: not every locked gem is installed.
    partial: bool,
}

impl Config {
    /// Its lines are `BUNDLE_PATH: "vendor/bundle"`.
    fn read(dir: &Path) -> Self {
        let mut config = Config::default();
        let text = fs::read_to_string(dir.join(".bundle").join("config")).unwrap_or_default();

        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_matches(['"', '\'']);
            match key.trim() {
                "BUNDLE_PATH" => config.path = Some(value.to_string()).filter(|path| !path.is_empty()),
                "BUNDLE_PATH__SYSTEM" => config.system = value == "true",
                "BUNDLE_DEPLOYMENT" => config.deployment = value == "true",
                "BUNDLE_WITHOUT" | "BUNDLE_ONLY" => config.partial |= !value.is_empty(),
                _ => {}
            }
        }

        config
    }
}

/// The project's own bundle path: `.bundle/config`'s, `vendor/bundle` under deployment, else
/// `vendor/bundle` or `.bundle` when gems are there. `None`: the gems are outside the project.
fn bundle_home(dir: &Path, config: &Config) -> Option<PathBuf> {
    if config.system {
        return None;
    }
    if let Some(path) = &config.path {
        let path = Path::new(path);
        let relative = if path.is_absolute() { path.strip_prefix(dir).ok()? } else { path };
        let inside = relative.components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
        return inside.then(|| dir.join(relative));
    }
    if config.deployment {
        return Some(dir.join("vendor").join("bundle"));
    }
    ["vendor/bundle", ".bundle"].iter().map(|name| dir.join(name)).find(|home| !abis(home, None).is_empty())
}

fn entries(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries.filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned())).collect()
}

/// The folders RubyGems installs into under a bundle path: `<engine>/<ABI version>`
/// (`ruby/3.3.0`). With several (an older Ruby's left behind), the one of the Ruby the project
/// asks for, when it is there.
fn abis(home: &Path, wanted: Option<&str>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for engine in entries(home) {
        for abi in entries(&home.join(&engine)) {
            let at = home.join(&engine).join(&abi);
            if ["specifications", "gems", "bundler"].iter().any(|name| at.join(name).is_dir()) {
                found.push(at);
            }
        }
    }
    let named = |abi: &PathBuf| wanted.is_some() && abi.file_name().and_then(|name| name.to_str()) == wanted;
    if found.iter().any(named) {
        found.retain(named);
    }
    found
}

/// The ABI version of the Ruby the project names exactly: `3.3.4` and `= 3.3.4` are `3.3.0`.
fn wanted_abi(dir: &Path) -> Option<String> {
    let required = ruby_required(dir)??;
    let version = segments(required.trim_start_matches('=').trim())?;
    (version.len() >= 2).then(|| format!("{}.{}.0", version[0], version[1]))
}

/// What RubyGems has installed under a bundle path.
#[derive(Default)]
struct Installed {
    /// `rack-2.0.9`, `nokogiri-1.16.0-arm64-darwin`.
    gems: HashSet<String>,
    /// The Git checkouts: `rack-test-198baafa2722`, the repository's name and the first twelve
    /// characters of the revision.
    checkouts: Vec<String>,
}

impl Installed {
    fn read(abis: &[PathBuf]) -> Self {
        let mut installed = Installed::default();
        for abi in abis {
            let specifications = entries(&abi.join("specifications"));
            installed.gems.extend(specifications.iter().filter_map(|name| name.strip_suffix(".gemspec")).map(str::to_string));
            installed.gems.extend(entries(&abi.join("gems")));
            installed.checkouts.extend(entries(&abi.join("bundler").join("gems")));
        }
        installed
    }

    /// The installed versions by locked gem, platforms left out. `http-2-1.0.0` is `http-2`'s
    /// when the lock has it, not `http`'s: the longest locked name before a version wins.
    fn versions<'a>(&'a self, lock: &Lock) -> HashMap<&'a str, Vec<&'a str>> {
        let mut versions: HashMap<&str, Vec<&str>> = HashMap::new();
        for full in &self.gems {
            let owner = full
                .rmatch_indices('-')
                .filter(|(at, _)| full[at + 1..].starts_with(|c: char| c.is_ascii_digit()))
                .map(|(at, _)| (&full[..at], plain(&full[at + 1..])))
                .find(|(name, _)| lock.gems.contains_key(*name));
            if let Some((name, version)) = owner {
                versions.entry(name).or_default().push(version);
            }
        }
        versions
    }
}

/// The Gemfile has a group that is installed only when asked for (`group :docs, optional: true`).
fn optional_groups(dir: &Path) -> bool {
    static OPTIONAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*group\b[^#\n]*\boptional\b").unwrap());
    GEMFILES.iter().filter_map(|name| fs::read_to_string(dir.join(name)).ok()).any(|text| OPTIONAL.is_match(&text))
}

/// The lock file against the gems in the project's bundle path. A gem counts as installed in
/// any of its locked variants (one per platform); a `PATH` gem is used where it lies. Where
/// groups are left out (`BUNDLE_WITHOUT`, `BUNDLE_ONLY`, an optional group), which gems those
/// are isn't in the lock: a gem that isn't there at all is then no difference, one that is there
/// in another version still is.
fn check_bundle(dir: &Path) -> (CheckState, Vec<DepPackage>) {
    let unknown = (CheckState::Unknown, Vec::new());
    let Some(lock) = Lock::read(dir) else {
        return unknown;
    };
    let config = Config::read(dir);
    let Some(home) = bundle_home(dir, &config) else {
        return unknown;
    };

    let installed = Installed::read(&abis(&home, wanted_abi(dir).as_deref()));
    let versions = installed.versions(&lock);
    let nothing = installed.gems.is_empty() && installed.checkouts.is_empty();
    let absent_differs = nothing || !(config.partial || optional_groups(dir));
    let mut packages = Vec::new();

    for (name, variants) in &lock.gems {
        if DEFAULT_GEMS.contains(&name.as_str()) {
            continue;
        }
        let have = versions.get(name.as_str()).map(Vec::as_slice).unwrap_or_default();
        if variants.iter().any(|variant| have.contains(&plain(variant))) || (have.is_empty() && !absent_differs) {
            continue;
        }
        let newest = have.iter().max_by_key(|version| segments(version));
        packages.push(DepPackage {
            name: name.clone(),
            locked: variants.first().map(|version| plain(version).to_string()),
            installed: newest.map(|version| version.to_string()),
        });
    }

    for gem in &lock.git {
        let short = gem.revision.get(..12).unwrap_or(&gem.revision);
        let checked_out = !short.is_empty() && installed.checkouts.iter().any(|name| name.strip_suffix(short).is_some_and(|rest| rest.ends_with('-')));
        if checked_out || !absent_differs {
            continue;
        }
        let revision = gem.revision.get(..7).unwrap_or(&gem.revision);
        packages.push(DepPackage { name: gem.name.clone(), locked: Some(format!("{} ({revision})", gem.version)), installed: None });
    }

    (CheckState::Ok, packages)
}

/* ---------- the runtime ---------- */

/// The Ruby version in `ruby -v`: `ruby 3.3.0 (2023-12-25 revision 5124f9ac75) [arm64-darwin23]`.
/// JRuby and TruffleRuby name the Ruby they stand for: `jruby 9.4.5.0 (3.1.4) …`,
/// `truffleruby 23.1.2, like ruby 3.2.2, …`.
fn ruby_version(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|line| ["ruby ", "jruby ", "truffleruby "].iter().any(|engine| line.starts_with(engine)))?;
    let version = match line.strip_prefix("ruby ") {
        Some(version) => version,
        None => line.split_once("like ruby ").or_else(|| line.split_once('(')).map(|(_, rest)| rest)?,
    };
    version_in(version)
}

/// The version a version file names: `3.3.0`, `ruby-3.3.0`, or its `ruby 3.3.0` line
/// (`.tool-versions`).
fn named_version(text: &str) -> Option<String> {
    static RUBY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^ruby[\s-]*(?:=\s*)?["']?([^\s#"']+)"#).unwrap());
    let version = match RUBY.captures(text) {
        Some(found) => found.get(1)?.as_str(),
        None => text.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#'))?,
    };
    Some(version.to_string())
}

/// `.tool-versions`' `ruby 3.3.0` line (asdf, mise): `None` without one, `Some(None)` when it
/// names no version (`system`, `ref:…`).
fn tool_versions(dir: &Path) -> Option<Option<String>> {
    let text = fs::read_to_string(dir.join(".tool-versions")).ok()?;
    let mut words = text.lines().map(str::split_whitespace).find(|words| words.clone().next() == Some("ruby"))?.skip(1);
    Some(words.next().filter(|version| version.starts_with(|c: char| c.is_ascii_digit())).map(str::to_string))
}

/// The Gemfile's `ruby "3.4.1"`, `ruby "~> 3.3"`, `ruby ">= 3.2", "< 3.5"` or
/// `ruby file: ".ruby-version"`. A bare version there is exactly that version (`= 3.4.1`).
/// `None` when the line is Ruby code this app can't read (`ruby RUBY_VERSION`).
fn gemfile_ruby(dir: &Path) -> Option<String> {
    static RUBY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*ruby[ \t(]+(.+)$").unwrap());
    static FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^(?:file:|:file\s*=>)\s*["']([^"']+)["']"#).unwrap());
    static LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*["']([^"']+)["']\s*,?"#).unwrap());

    let text = GEMFILES.iter().find_map(|name| fs::read_to_string(dir.join(name)).ok())?;
    let mut arguments = RUBY.captures(&text)?.get(1)?.as_str();

    if let Some(file) = FILE.captures(arguments.trim_start()) {
        let file = Path::new(file.get(1)?.as_str());
        if !file.components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir)) {
            return None;
        }
        return named_version(&fs::read_to_string(dir.join(file)).ok()?);
    }

    let mut wanted: Vec<String> = Vec::new();
    while let Some(found) = LITERAL.captures(arguments) {
        let clause = found.get(1)?.as_str().trim();
        let exact = clause.starts_with(|c: char| c.is_ascii_digit());
        wanted.push(if exact { format!("= {clause}") } else { clause.to_string() });
        arguments = &arguments[found.get(0)?.end()..];
    }
    (!wanted.is_empty()).then(|| wanted.join(", "))
}

/// A project uses Ruby with a Gemfile or its lock, a `.ruby-version`, or a `ruby` line in
/// `.tool-versions`. What it asks for: `.ruby-version`, the Gemfile's `ruby`, the Ruby the lock
/// was written with (any patch of it: Bundler itself holds to the Gemfile only), then
/// `.tool-versions`.
fn ruby_required(dir: &Path) -> Option<Option<String>> {
    let version_file = fs::read_to_string(dir.join(".ruby-version")).ok();
    let tool = tool_versions(dir);
    if version_file.is_none() && tool.is_none() && !LOCKS.iter().chain(&GEMFILES).any(|name| dir.join(name).is_file()) {
        return None;
    }

    let locked = || {
        let version = segments(&Lock::read(dir)?.ruby?)?;
        (version.len() >= 2).then(|| format!("~> {}.{}.0", version[0], version[1]))
    };
    Some(version_file.as_deref().and_then(named_version).or_else(|| gemfile_ruby(dir)).or_else(locked).or_else(|| tool.flatten()))
}

/// `3.3.0` as numbers; `None` when a part isn't one (`3.5.0.preview1`, `jruby-9.4`).
fn segments(version: &str) -> Option<Vec<u64>> {
    version.trim().split('.').map(|part| part.parse().ok()).collect()
}

/// Missing numbers count as 0, as in RubyGems: `3.3` is `3.3.0`.
fn compare(a: &[u64], b: &[u64]) -> Ordering {
    let at = |version: &[u64], i: usize| version.get(i).copied().unwrap_or(0);
    (0..a.len().max(b.len())).map(|i| at(a, i).cmp(&at(b, i))).find(|order| order.is_ne()).unwrap_or(Ordering::Equal)
}

/// RubyGems' requirements: `=`, `!=`, `>`, `>=`, `<`, `<=` and `~>` (`~> 3.3` is below 4,
/// `~> 3.3.0` below 3.4), joined by `,` for "and". A version without an operator is what a
/// version file names: that version, or any that starts with it (`3.3` is 3.3.x).
fn ruby_satisfies(constraint: &str, version: &str) -> Option<bool> {
    let version = segments(version)?;
    let mut holds = None;

    for clause in constraint.split(',').map(str::trim) {
        let operators = ["~>", ">=", "<=", "!=", "=", ">", "<"];
        let (operator, wanted) = operators.iter().find_map(|op| clause.strip_prefix(op).map(|rest| (*op, rest))).unwrap_or(("", clause));
        let wanted = segments(wanted)?;
        let order = compare(&version, &wanted);

        let clause_holds = match operator {
            "" => version.starts_with(&wanted),
            "=" => order.is_eq(),
            "!=" => order.is_ne(),
            ">" => order.is_gt(),
            ">=" => order.is_ge(),
            "<" => order.is_lt(),
            "<=" => order.is_le(),
            _ => {
                // The last number given may rise: the one before it is the limit.
                let mut limit = wanted[..wanted.len().saturating_sub(1).max(1)].to_vec();
                *limit.last_mut()? += 1;
                order.is_ge() && compare(&version, &limit).is_lt()
            }
        };
        holds = Some(holds.unwrap_or(true) && clause_holds);
    }

    holds
}

/* ---------- the network scan ---------- */

/// `bundle outdated --only-explicit --parseable`: an empty line, then one line per outdated gem
/// of the Gemfile (`rack (newest 3.2.7, installed 2.0.9, requested = 2.0.9)`). A failure has no
/// empty line last: Bundler 2 writes its error to stderr and nothing here, Bundler 1 writes it
/// here.
fn bundle_outdated(output: &str) -> Option<Vec<Outdated>> {
    static GEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\S+) \(newest (.+?), installed ([^,)]+).*\)$").unwrap());
    let gem = |line: &str| {
        let found = GEM.captures(line.trim())?;
        Some(Outdated { name: found[1].to_string(), installed: Some(found[3].trim().to_string()), wanted: None, latest: Some(found[2].trim().to_string()) })
    };
    let outdated: Vec<Outdated> = output.lines().filter_map(gem).collect();
    if !outdated.is_empty() {
        return Some(outdated);
    }
    let output = output.replace("\r\n", "\n");
    (output == "\n" || output.ends_with("\n\n")).then(Vec::new)
}
