//! Go modules. Go fetches them into a cache outside the project when it builds, so there is
//! nothing to install and nothing to compare, unless the project vendors: then go.mod's
//! requirements are compared with `vendor/modules.txt`, Go's own record of what `vendor` holds,
//! and `go mod vendor` brings the two in line. The runtime is Go: go.mod's `toolchain`, else its
//! `go` line (both mean "at least"), else what `.tool-versions` pins.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{flag, owned, text, version_in};
use super::{CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadOutdated, Runtime, ScanPlan};

pub struct Go;

const MODULES: &str = "vendor/modules.txt";
/// Rewrites `vendor` from go.mod; offered only to a project that vendors already.
const VENDOR: &str = "go mod vendor";
/// `-mod=readonly`: go.mod and go.sum are never written, and a `vendor` folder doesn't stop
/// `all` from being listed.
const OUTDATED: &str = "go list -m -u -mod=readonly -json all";

/// Only `>=` and plain versions are handed back, which every flavor reads alike.
static RUNTIMES: [Runtime; 1] = [Runtime { name: "go", probe: "go version", version: version_in, required: go_required, local: None, flavor: Flavor::Npm }];

impl Ecosystem for Go {
    fn id(&self) -> &'static str {
        "go"
    }

    fn manifest(&self) -> &'static str {
        "go.mod"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        dir.join("go.mod").is_file().then_some("go")
    }

    fn watched(&self) -> &'static [&'static str] {
        &["go.mod", MODULES, ".tool-versions"]
    }

    fn check(&self, dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        let (Ok(module), Ok(vendored)) = (fs::read_to_string(dir.join("go.mod")), fs::read_to_string(dir.join(MODULES))) else {
            return (CheckState::Unknown, Vec::new());
        };
        check_vendor(&module, &vendored)
    }

    fn needs(&self, _dir: &Path, _manager: &str) -> &'static [&'static str] {
        &["go"]
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, dir: &Path, _manager: &str) -> Option<&'static str> {
        dir.join(MODULES).is_file().then_some(VENDOR)
    }

    fn scan_plan(&self, _dir: &Path, _manager: &str) -> ScanPlan {
        // `govulncheck` is a tool of its own, not part of Go: no audit.
        ScanPlan { outdated: Some((OUTDATED, go_outdated as ReadOutdated)), audit: None }
    }
}

/// A go.mod line without its comment.
fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or_default().trim()
}

/// The `require` lines of a go.mod, single or in a block: module path and version.
fn required(module: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    // Inside a block: `Some(true)` in `require (`, `Some(false)` in another directive's.
    let mut block: Option<bool> = None;

    for line in module.lines().map(code) {
        let entry = match block {
            Some(_) if line == ")" => {
                block = None;
                continue;
            }
            Some(true) => line,
            Some(false) => continue,
            None => {
                let require = line.strip_prefix("require").filter(|rest| rest.starts_with([' ', '\t', '(']));
                if line.ends_with('(') {
                    block = Some(require.is_some());
                    continue;
                }
                match require {
                    Some(rest) => rest,
                    None => continue,
                }
            }
        };
        let mut words = entry.split_whitespace().map(|word| word.trim_matches('"'));
        if let (Some(path), Some(version)) = (words.next(), words.next()) {
            found.push((path.to_string(), version.to_string()));
        }
    }
    found
}

/// go.mod's requirements against `vendor/modules.txt`, as Go itself checks them before a build
/// with `vendor`: every required module is listed there (`# path version`, also when replaced:
/// `# path version => …`) at the required version.
fn check_vendor(module: &str, vendored: &str) -> (CheckState, Vec<DepPackage>) {
    let mut listed: HashSet<(&str, &str)> = HashSet::new();
    let mut versions: HashMap<&str, &str> = HashMap::new();
    for line in vendored.lines() {
        let Some(rest) = line.strip_prefix("# ") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        // `# path => …` is a replacement of every version, not a vendored module.
        if let (Some(path), Some(version)) = (words.next(), words.next().filter(|word| *word != "=>")) {
            listed.insert((path, version));
            versions.entry(path).or_insert(version);
        }
    }

    let packages = required(module)
        .into_iter()
        .filter(|(path, version)| !listed.contains(&(path.as_str(), version.as_str())))
        .map(|(path, version)| DepPackage { installed: versions.get(path.as_str()).map(|v| v.to_string()), name: path, locked: Some(version) })
        .collect();
    (CheckState::Ok, packages)
}

/* ---------- the runtime ---------- */

/// The version `.tool-versions` (asdf, mise) names for one of `tools`: the first on its line.
fn tool_version(dir: &Path, tools: &[&str]) -> Option<String> {
    let text = fs::read_to_string(dir.join(".tool-versions")).ok()?;
    text.lines().find_map(|line| {
        let mut words = line.split('#').next().unwrap_or_default().split_whitespace();
        let tool = words.next()?;
        tools.contains(&tool).then(|| words.next().map(str::to_string)).flatten()
    })
}

/// What follows `directive` on its line of a go.mod: `1.23` of `go 1.23`.
fn directive(module: &str, directive: &str) -> Option<String> {
    module.lines().map(code).find_map(|line| {
        let rest = line.strip_prefix(directive)?;
        rest.starts_with([' ', '\t']).then(|| rest.trim().to_string())
    })
}

/// A project uses Go with a go.mod, or with a `.tool-versions` that names it. go.mod's
/// `toolchain go1.23.4`, else its `go 1.23`, is the least Go the module builds with (an older one
/// downloads that toolchain, or refuses); without either, what `.tool-versions` pins.
fn go_required(dir: &Path) -> Option<Option<String>> {
    let module = fs::read_to_string(dir.join("go.mod")).ok();
    let pinned = tool_version(dir, &["golang", "go"]);
    if module.is_none() && pinned.is_none() {
        return None;
    }
    let least = module.as_deref().and_then(|module| {
        // `toolchain default` names no version.
        let toolchain = directive(module, "toolchain").and_then(|name| name.strip_prefix("go").map(str::to_string));
        toolchain.or_else(|| directive(module, "go")).filter(|version| version.starts_with(|c: char| c.is_ascii_digit()))
    });
    Some(least.map(|version| format!(">={version}")).or(pinned))
}

/* ---------- the network scan ---------- */

/// `go list -m -u -json all`: one JSON object per module, one after the other. Listed are the
/// modules the project requires itself (not the main one, not `Indirect`) that have an `Update`.
fn go_outdated(output: &str) -> Option<Vec<Outdated>> {
    let objects = output.get(output.find('{')?..=output.rfind('}')?)?;
    let mut outdated = Vec::new();
    for module in serde_json::Deserializer::from_str(objects).into_iter::<Value>() {
        let module = module.ok()?;
        let Some(update) = module.get("Update").filter(|update| update.is_object()) else {
            continue;
        };
        if !flag(&module, "Indirect") && !flag(&module, "Main") {
            outdated.push(Outdated { name: text(&module, "Path").unwrap_or_default().to_string(), installed: owned(&module, "Version"), wanted: None, latest: owned(update, "Version") });
        }
    }
    Some(outdated)
}
