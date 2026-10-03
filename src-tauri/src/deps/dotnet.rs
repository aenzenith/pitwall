//! .NET: what each project asks of NuGet (its packages.lock.json, else the `PackageReference`s of
//! the project file, with Directory.Packages.props for central versions) against
//! `obj/project.assets.json`, the record `dotnet restore` writes. The projects are those of the
//! folder's solution, else the project files in the folder and one level down. The runtime is the
//! .NET SDK: global.json's `sdk.version` with its `rollForward`, else the highest target framework.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::{count, json_object, owned, read_json, severity, text};
use super::{Advisory, Audit, CheckState, DepPackage, Ecosystem, Flavor, Outdated, ReadAudit, ReadOutdated, Runtime, ScanPlan};

pub struct Dotnet;

const RESTORE: &str = "dotnet restore";
/// Both are part of the SDK (7.0.200 and later for `--format json`) and only read.
const OUTDATED: &str = "dotnet list package --outdated --format json";
const VULNERABLE: &str = "dotnet list package --vulnerable --format json";

/// The project files NuGet restores for.
const PROJECTS: [&str; 3] = ["csproj", "fsproj", "vbproj"];
const SOLUTIONS: [&str; 2] = ["sln", "slnx"];
/// Folders one level down that hold no project of their own.
const SKIPPED: [&str; 8] = ["node_modules", "vendor", "bin", "obj", "target", "dist", "packages", "artifacts"];
/// At most this many folders are looked into, entries of a folder looked at, and projects read.
const MAX_FOLDERS: usize = 64;
const MAX_ENTRIES: usize = 2048;
const MAX_PROJECTS: usize = 64;
/// Properties that move `obj` away from the project's folder: its record isn't where it is
/// looked for.
const RELOCATED: [&str; 4] = ["BaseIntermediateOutputPath", "MSBuildProjectExtensionsPath", "ArtifactsPath", "UseArtifactsOutput"];

static RUNTIMES: [Runtime; 1] =
    [Runtime { name: "dotnet", probe: "dotnet --version", version: sdk_version, required: sdk_required, local: None, flavor: Flavor::Custom(sdk_holds) }];

impl Ecosystem for Dotnet {
    fn id(&self) -> &'static str {
        "dotnet"
    }

    fn manifest(&self) -> &'static str {
        "*.csproj"
    }

    fn detect(&self, dir: &Path) -> Option<&'static str> {
        (dir.join("global.json").is_file() || uses(dir, &Listing::of(dir))).then_some("nuget")
    }

    /// Project and solution files have names of their own, and a project one level down its
    /// own `obj`: those can't be listed here.
    fn watched(&self) -> &'static [&'static str] {
        &["global.json", "packages.lock.json", "obj", "obj/project.assets.json", "Directory.Packages.props", "Directory.Build.props"]
    }

    fn check(&self, dir: &Path, _manager: &str) -> (CheckState, Vec<DepPackage>) {
        let mut packages = Vec::new();
        let (mut compared, mut never) = (false, false);
        for project in projects(dir) {
            match restored(dir, &project) {
                Restored::Unknown => {}
                Restored::Never(missing) => {
                    never = true;
                    packages.extend(missing);
                }
                Restored::Compared(differing) => {
                    compared = true;
                    packages.extend(differing);
                }
            }
        }
        let state = match (never, compared) {
            (true, _) => CheckState::Install,
            (false, true) => CheckState::Ok,
            (false, false) => CheckState::Unknown,
        };
        (state, packages)
    }

    fn needs(&self, _dir: &Path, _manager: &str) -> &'static [&'static str] {
        &["dotnet"]
    }

    fn runtimes(&self) -> &'static [Runtime] {
        &RUNTIMES
    }

    fn install_command(&self, dir: &Path, _manager: &str) -> Option<&'static str> {
        runs_here(dir).then_some(RESTORE)
    }

    fn scan_plan(&self, dir: &Path, _manager: &str) -> ScanPlan {
        if !runs_here(dir) {
            return ScanPlan::default();
        }
        ScanPlan { outdated: Some((OUTDATED, nuget_outdated as ReadOutdated)), audit: Some((VULNERABLE, nuget_audit as ReadAudit)) }
    }
}

/* ---------- the projects ---------- */

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()).unwrap_or_default()
}

fn is_project(name: &str) -> bool {
    PROJECTS.contains(&extension(name).as_str())
}

fn is_solution(name: &str) -> bool {
    SOLUTIONS.contains(&extension(name).as_str())
}

/// A folder's files and folders by name, sorted.
struct Listing {
    files: Vec<String>,
    folders: Vec<String>,
}

impl Listing {
    fn of(dir: &Path) -> Self {
        let mut listing = Listing { files: Vec::new(), folders: Vec::new() };
        for entry in fs::read_dir(dir).into_iter().flatten().take(MAX_ENTRIES).flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => listing.folders.push(name),
                Ok(kind) if kind.is_file() => listing.files.push(name),
                _ => {}
            }
        }
        listing.files.sort();
        listing.folders.sort();
        listing
    }
}

/// The folder is a .NET one: a solution or a project file in it, or a project one level down.
fn uses(dir: &Path, here: &Listing) -> bool {
    here.files.iter().any(|name| is_solution(name) || is_project(name)) || !projects_in(dir, here).is_empty()
}

/// The project files, relative to the folder: those its one solution lists, else those in it and
/// one level down.
pub(super) fn projects(dir: &Path) -> Vec<PathBuf> {
    projects_in(dir, &Listing::of(dir))
}

fn projects_in(dir: &Path, here: &Listing) -> Vec<PathBuf> {
    let solutions: Vec<&String> = here.files.iter().filter(|name| is_solution(name)).collect();
    if let [solution] = solutions[..] {
        let listed = solution_projects(dir, solution);
        if !listed.is_empty() {
            return listed;
        }
    }

    let mut found: Vec<PathBuf> = here.files.iter().filter(|name| is_project(name)).map(PathBuf::from).collect();
    let folders = here.folders.iter().filter(|name| !name.starts_with('.') && !SKIPPED.contains(&name.as_str()));
    for folder in folders.take(MAX_FOLDERS) {
        let inner = Listing::of(&dir.join(folder));
        found.extend(inner.files.iter().filter(|name| is_project(name)).map(|name| Path::new(folder).join(name)));
    }
    found.truncate(MAX_PROJECTS);
    found
}

/// The projects a solution lists that lie inside the folder:
/// `Project("{…}") = "App", "src\App\App.csproj", "{…}"` in a `.sln`,
/// `<Project Path="src/App/App.csproj" />` in a `.slnx`.
fn solution_projects(dir: &Path, solution: &str) -> Vec<PathBuf> {
    static SLN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\s*Project\("[^"]*"\)\s*=\s*"[^"]*"\s*,\s*"([^"]+)""#).unwrap());
    static SLNX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<Project\s[^>]*?Path\s*=\s*"([^"]+)""#).unwrap());

    let Ok(text) = fs::read_to_string(dir.join(solution)) else {
        return Vec::new();
    };
    let pattern = if extension(solution) == "slnx" { &SLNX } else { &SLN };
    let paths = pattern.captures_iter(&text).map(|found| found[1].replace('\\', "/"));
    paths.filter(|path| is_project(path) && inside(path)).map(PathBuf::from).filter(|path| dir.join(path).is_file()).take(MAX_PROJECTS).collect()
}

/// A relative path that stays inside the folder it is joined to.
fn inside(path: &str) -> bool {
    !path.starts_with('/') && !path.contains(':') && path.split('/').all(|part| !part.is_empty() && part != "..")
}

/// `dotnet restore` and `dotnet list package`, without a file named, work in a folder with one
/// solution or one project file (MSBuild counts every `*.*proj`); with one of each, when they
/// share their name. Anywhere else they ask which, and Pitwall names none.
fn runs_here(dir: &Path) -> bool {
    let here = Listing::of(dir);
    let stem = |name: &String| name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).to_lowercase();
    let solutions: Vec<&String> = here.files.iter().filter(|name| is_solution(name)).collect();
    let any_project: Vec<&String> = here.files.iter().filter(|name| extension(name).ends_with("proj")).collect();
    match (&solutions[..], &any_project[..]) {
        ([_], []) | ([], [_]) => true,
        ([solution], [project]) => stem(solution) == stem(project),
        _ => false,
    }
}

/* ---------- project files, read by hand ---------- */

#[derive(Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Open,
    Close,
    /// `<a />`
    Alone,
}

/// One tag of an XML text (no XML parser): comments, declarations and CDATA are passed over.
struct Tag<'a> {
    name: &'a str,
    /// What stands between the name and the closing `>`.
    attributes: &'a str,
    kind: TagKind,
    /// The text up to the next tag.
    text: &'a str,
}

fn tags(xml: &str) -> Vec<Tag<'_>> {
    const PASSED: [(&str, &str); 4] = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>"), ("<!", ">")];
    let mut tags = Vec::new();
    let mut at = 0;

    while let Some(found) = xml[at..].find('<') {
        let start = at + found;
        let rest = &xml[start..];
        if let Some((open, close)) = PASSED.iter().find(|(open, _)| rest.starts_with(open)) {
            at = rest[open.len()..].find(close).map_or(xml.len(), |end| start + open.len() + end + close.len());
            continue;
        }

        // A `>` inside a quoted attribute (`Condition="'$(A)' > '1'"`) ends nothing.
        let mut quote: Option<char> = None;
        let mut end = None;
        for (index, c) in rest.char_indices().skip(1) {
            match quote {
                Some(open) if c == open => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '>' => {
                    end = Some(index);
                    break;
                }
                None => {}
            }
        }
        let Some(end) = end else {
            break;
        };

        let inner = &rest[1..end];
        let (inner, kind) = match (inner.strip_prefix('/'), inner.strip_suffix('/')) {
            (Some(name), _) => (name, TagKind::Close),
            (None, Some(body)) => (body, TagKind::Alone),
            (None, None) => (inner, TagKind::Open),
        };
        let inner = inner.trim();
        let split = inner.find(char::is_whitespace).unwrap_or(inner.len());
        at = start + end + 1;
        let until = xml[at..].find('<').map_or(xml.len(), |next| at + next);
        tags.push(Tag { name: &inner[..split], attributes: inner[split..].trim(), kind, text: xml[at..until].trim() });
    }
    tags
}

/// An attribute's value in a tag's attributes: `Include="X" Version='1.2.3'`.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attributes;
    loop {
        let (key, value) = rest.split_once('=')?;
        let value = value.trim_start();
        let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let end = value[1..].find(quote)?;
        if key.trim() == name {
            return Some(&value[1..1 + end]);
        }
        rest = &value[end + 2..];
    }
}

/// A package a project file asks for, and the version as written there, if any.
pub(super) struct Reference {
    pub(super) name: String,
    pub(super) version: Option<String>,
}

/// The `<PackageReference Include="X" Version="1.2.3" />` items of a project file, or the
/// `<PackageVersion … />` of a Directory.Packages.props (`element`): the version as an attribute,
/// in any order, or as a child element. Items under a `Condition` are left out: they may rightly
/// not be restored.
pub(super) fn references(xml: &str, element: &str) -> Vec<Reference> {
    let tags = tags(xml);
    let mut open: Vec<bool> = Vec::new();
    let mut found = Vec::new();

    for (index, tag) in tags.iter().enumerate() {
        let conditional = attribute(tag.attributes, "Condition").is_some();
        match tag.kind {
            TagKind::Close => {
                open.pop();
                continue;
            }
            TagKind::Open => open.push(conditional),
            TagKind::Alone => {}
        }
        if tag.name != element || conditional || open.contains(&true) {
            continue;
        }
        let Some(include) = attribute(tag.attributes, "Include") else {
            continue;
        };

        let mut version = ["VersionOverride", "Version"].iter().find_map(|name| attribute(tag.attributes, name)).map(str::to_string);
        if version.is_none() && tag.kind == TagKind::Open {
            let children = tags[index + 1..].iter().take_while(|next| !(next.kind == TagKind::Close && next.name == element));
            version = children
                .filter(|child| child.kind == TagKind::Open && matches!(child.name, "VersionOverride" | "Version"))
                .map(|child| child.text.to_string())
                .next();
        }
        for name in include.split(';').map(str::trim).filter(|name| !name.is_empty()) {
            found.push(Reference { name: name.to_string(), version: version.clone() });
        }
    }
    found
}

/// The target frameworks a project file names: `net8.0`, `net8.0-windows`, `netstandard2.0`.
pub(super) fn frameworks(xml: &str) -> Vec<String> {
    let named = tags(xml).into_iter().filter(|tag| tag.kind == TagKind::Open && matches!(tag.name, "TargetFramework" | "TargetFrameworks"));
    named.flat_map(|tag| tag.text.split(';')).map(str::trim).filter(|moniker| !moniker.is_empty() && !moniker.contains('$')).map(str::to_string).collect()
}

/// A property's text where the project file sets it plainly: `<AssemblyName>App</AssemblyName>`.
pub(super) fn property(xml: &str, name: &str) -> Option<String> {
    let set = tags(xml).into_iter().find(|tag| tag.kind == TagKind::Open && tag.name == name)?;
    (!set.text.is_empty() && !set.text.contains('$')).then(|| set.text.to_string())
}

/* ---------- the check ---------- */

/// NuGet's normal form of a version, for comparing: three numbers (a fourth unless it is 0),
/// no build metadata, lower case.
fn normal(version: &str) -> String {
    let version = version.trim().split('+').next().unwrap_or_default();
    let (numbers, label) = version.split_once('-').map_or((version, None), |(numbers, label)| (numbers, Some(label)));
    let Some(mut parts) = numbers.split('.').map(|part| part.parse().ok()).collect::<Option<Vec<u64>>>() else {
        return version.to_lowercase();
    };
    parts.resize(parts.len().max(3), 0);
    if parts.len() == 4 && parts[3] == 0 {
        parts.pop();
    }
    let numbers = parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".");
    match label {
        Some(label) => format!("{numbers}-{}", label.to_lowercase()),
        None => numbers,
    }
}

/// What a version text asks for, when that is one version. A range (`[1.0,2.0)`), a floating
/// version (`1.*`) or a property (`$(Version)`) is `None`: nothing to hold a restore against.
#[derive(PartialEq, Eq)]
enum Request {
    /// `1.2.3` (or `[1.2.3, )`, as the record has it): that version, or the next one the source
    /// has.
    AtLeast(String),
    /// `[1.2.3]`
    Exactly(String),
}

impl Request {
    fn of(version: &str) -> Option<Self> {
        let version = version.trim();
        if version.contains(['*', '$']) {
            return None;
        }
        let Some(range) = version.strip_prefix('[') else {
            return version.starts_with(|c: char| c.is_ascii_digit()).then(|| Self::AtLeast(normal(version)));
        };
        if let Some((low, high)) = range.strip_suffix(')').and_then(|range| range.split_once(',')) {
            return high.trim().is_empty().then(|| Self::AtLeast(normal(low)));
        }
        match range.strip_suffix(']')?.split_once(',') {
            None => Some(Self::Exactly(normal(range.strip_suffix(']')?))),
            Some((low, high)) if normal(low) == normal(high) => Some(Self::Exactly(normal(low))),
            Some(_) => None,
        }
    }

    fn version(&self) -> &str {
        match self {
            Self::AtLeast(version) | Self::Exactly(version) => version,
        }
    }
}

/// What `dotnet restore` recorded for a project in its `obj/project.assets.json`.
struct Assets {
    /// The restored packages' versions by lower-case name: the `Name/1.2.3` keys of `libraries`.
    libraries: HashMap<String, Vec<String>>,
    /// What each reference asked for at that restore (`[1.2.3, )`), by lower-case name:
    /// `project.frameworks.*.dependencies`.
    asked: HashMap<String, Vec<String>>,
}

impl Assets {
    fn read(file: &Path) -> Option<Self> {
        let value = read_json(file)?;
        let mut libraries: HashMap<String, Vec<String>> = HashMap::new();
        for (key, library) in value.get("libraries")?.as_object()? {
            if text(library, "type") == Some("project") {
                continue;
            }
            if let Some((name, version)) = key.split_once('/') {
                libraries.entry(name.to_lowercase()).or_default().push(version.to_string());
            }
        }

        let mut asked: HashMap<String, Vec<String>> = HashMap::new();
        let frameworks = value.get("project").and_then(|project| project.get("frameworks")).and_then(Value::as_object);
        for framework in frameworks.into_iter().flat_map(|frameworks| frameworks.values()) {
            for (name, dependency) in framework.get("dependencies").and_then(Value::as_object).into_iter().flatten() {
                if let Some(version) = text(dependency, "version") {
                    asked.entry(name.to_lowercase()).or_default().push(version.to_string());
                }
            }
        }
        Some(Assets { libraries, asked })
    }
}

/// A package a project wants restored.
struct Wanted {
    name: String,
    /// The version to show: the lock file's, or the project file's as written.
    version: Option<String>,
    /// From packages.lock.json: the very version restore must have fetched.
    locked: bool,
}

enum Restored {
    /// Nothing to tell: no NuGet references of the kind `dotnet restore` handles, or its record
    /// lies elsewhere.
    Unknown,
    /// No record: never restored. Every package it asks for.
    Never(Vec<DepPackage>),
    /// The packages whose record differs from what the project asks for now.
    Compared(Vec<DepPackage>),
}

/// One project against its restore's record.
fn restored(dir: &Path, project: &Path) -> Restored {
    let file = dir.join(project);
    let (Some(folder), Ok(xml)) = (file.parent(), fs::read_to_string(&file)) else {
        return Restored::Unknown;
    };
    // packages.config projects are restored by other means, into no assets file.
    if folder.join("packages.config").is_file() {
        return Restored::Unknown;
    }
    let lock = read_json(&folder.join("packages.lock.json"));
    let references = references(&xml, "PackageReference");
    let sdk_style = tags(&xml).iter().any(|tag| tag.name == "Sdk" || attribute(tag.attributes, "Sdk").is_some());
    if lock.is_none() && references.is_empty() && !sdk_style {
        return Restored::Unknown;
    }

    let wanted: Vec<Wanted> = match &lock {
        Some(lock) => locked(lock),
        None => {
            let central = central_versions(dir, folder);
            let version = |reference: &Reference| reference.version.clone().or_else(|| central.get(&reference.name.to_lowercase()).cloned());
            references.iter().map(|reference| Wanted { name: reference.name.clone(), version: version(reference), locked: false }).collect()
        }
    };

    let stem = project.file_stem().unwrap_or_default();
    let records = [folder.join("obj").join("project.assets.json"), dir.join("artifacts").join("obj").join(stem).join("project.assets.json")];
    let Some(record) = records.iter().find(|file| file.is_file()) else {
        let props = [folder.join("Directory.Build.props"), dir.join("Directory.Build.props")];
        let moved = |text: &str| RELOCATED.iter().any(|name| text.contains(name));
        if moved(&xml) || props.iter().any(|file| fs::read_to_string(file).is_ok_and(|text| moved(&text))) {
            return Restored::Unknown;
        }
        let shown = |version: Option<String>| version.filter(|version| !version.contains('$'));
        return Restored::Never(wanted.into_iter().map(|wanted| DepPackage { name: wanted.name, locked: shown(wanted.version), installed: None }).collect());
    };
    let Some(assets) = Assets::read(record) else {
        return Restored::Unknown;
    };

    let differing = wanted.into_iter().filter_map(|wanted| {
        let key = wanted.name.to_lowercase();
        let installed = assets.libraries.get(&key);
        let has = |version: &str| installed.is_some_and(|versions| versions.iter().any(|installed| normal(installed) == normal(version)));
        let differs = match (installed, wanted.version.as_deref()) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(_), Some(version)) if wanted.locked => !has(version),
            // The same request as at the last restore is up to date whatever it resolved to (a
            // source without that very version gives the next); without a record of the
            // request, the restored version itself is compared.
            (Some(_), Some(version)) => Request::of(version).is_some_and(|now| {
                let then: Vec<Request> = assets.asked.get(&key).into_iter().flatten().filter_map(|asked| Request::of(asked)).collect();
                if then.is_empty() {
                    !has(now.version())
                } else {
                    !then.contains(&now)
                }
            }),
        };
        differs.then(|| DepPackage { name: wanted.name, locked: wanted.version, installed: installed.and_then(|versions| versions.first().cloned()) })
    });
    Restored::Compared(differing.collect())
}

/// packages.lock.json's packages with the version each resolved to, over every framework;
/// project references have none.
fn locked(lock: &Value) -> Vec<Wanted> {
    let frameworks = lock.get("dependencies").and_then(Value::as_object);
    let entries = frameworks.into_iter().flat_map(|frameworks| frameworks.values()).filter_map(Value::as_object).flatten();
    entries
        .filter(|(_, entry)| text(entry, "type") != Some("Project"))
        .filter_map(|(name, entry)| Some(Wanted { name: name.clone(), version: Some(text(entry, "resolved")?.to_string()), locked: true }))
        .collect()
}

/// The versions of the nearest Directory.Packages.props, from the project's folder up to the
/// project root, by lower-case name.
fn central_versions(dir: &Path, folder: &Path) -> HashMap<String, String> {
    let mut at = Some(folder);
    while let Some(here) = at.filter(|here| here.starts_with(dir)) {
        if let Ok(xml) = fs::read_to_string(here.join("Directory.Packages.props")) {
            return references(&xml, "PackageVersion").into_iter().filter_map(|central| Some((central.name.to_lowercase(), central.version?))).collect();
        }
        at = here.parent();
    }
    HashMap::new()
}

/* ---------- the runtime ---------- */

/// A pre-release label's part: numbers sort before words, and by value.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Word(String),
}

/// An SDK version, ordered: its three numbers, then a release after its own previews
/// (`9.0.100-rc.2.24474.11` is before `9.0.100`), then the label.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Sdk {
    numbers: [u64; 3],
    release: bool,
    label: Vec<Part>,
}

impl Sdk {
    /// `8.0.404`, `9.0.100-rc.2.24474.11`, and `8` (8.0.0) in a requirement.
    fn parse(version: &str) -> Option<Self> {
        let (numbers, label) = version.trim().split_once('-').map_or((version.trim(), None), |(numbers, label)| (numbers, Some(label)));
        let parts = numbers.split('.').map(|part| part.parse().ok()).collect::<Option<Vec<u64>>>()?;
        if parts.len() > 3 || label.is_some_and(str::is_empty) {
            return None;
        }
        let mut sdk = Sdk { numbers: [0; 3], release: label.is_none(), label: Vec::new() };
        sdk.numbers[..parts.len()].copy_from_slice(&parts);
        let label = label.into_iter().flat_map(|label| label.split('.'));
        sdk.label = label.map(|part| part.parse().map_or_else(|_| Part::Word(part.to_string()), Part::Number)).collect();
        Some(sdk)
    }
}

/// `dotnet --version`: the SDK's version on a line of its own, `8.0.404` or
/// `9.0.100-rc.2.24474.11` (a first run prints a welcome before it).
fn sdk_version(output: &str) -> Option<String> {
    output.lines().map(str::trim).find(|line| line.split('.').count() >= 3 && Sdk::parse(line).is_some()).map(str::to_string)
}

/// A project uses .NET with a global.json, a solution or a project file. What it asks for:
/// global.json's SDK, as its `rollForward` lets it roll; else an SDK that knows the highest
/// framework its projects target (`net8.0`: `>=8`).
fn sdk_required(dir: &Path) -> Option<Option<String>> {
    let global = dir.join("global.json");
    let here = Listing::of(dir);
    if !global.is_file() && !uses(dir, &here) {
        return None;
    }
    if let Some(range) = read_json(&global).as_ref().and_then(sdk_range) {
        return Some(Some(range));
    }
    let files = projects_in(dir, &here).into_iter().map(|project| dir.join(project)).chain([dir.join("Directory.Build.props")]);
    let majors = files.filter_map(|file| fs::read_to_string(file).ok()).flat_map(|xml| frameworks(&xml)).filter_map(|moniker| framework_major(&moniker));
    Some(majors.max().map(|major| format!(">={major}")))
}

/// global.json's `sdk.version` as the range its `rollForward` allows (`latestPatch` unless
/// said): the same feature band (`>=8.0.404 <8.0.500`), minor, major, anything later, or that
/// very version (`=8.0.404`).
fn sdk_range(global: &Value) -> Option<String> {
    let sdk = global.get("sdk")?;
    let version = text(sdk, "version")?.trim();
    let [major, minor, patch] = Sdk::parse(version)?.numbers;
    let below = match text(sdk, "rollForward").unwrap_or("latestPatch").to_ascii_lowercase().as_str() {
        "patch" | "latestpatch" => format!(" <{major}.{minor}.{}", (patch / 100 + 1) * 100),
        "feature" | "latestfeature" => format!(" <{major}.{}.0", minor + 1),
        "minor" | "latestminor" => format!(" <{}.0.0", major + 1),
        "major" | "latestmajor" => String::new(),
        "disable" => return Some(format!("={version}")),
        _ => return None,
    };
    Some(format!(">={version}{below}"))
}

/// The SDK's major that builds a framework: `net8.0` and `net8.0-windows` are 8, `netcoreapp3.1`
/// is 3. Any SDK builds .NET Standard and .NET Framework (`netstandard2.0`, `net48`): `None`.
fn framework_major(moniker: &str) -> Option<u64> {
    let moniker = moniker.to_ascii_lowercase();
    let (major, minor) = moniker.strip_prefix("netcoreapp").or_else(|| moniker.strip_prefix("net"))?.split_once('.')?;
    minor.starts_with(|c: char| c.is_ascii_digit()).then(|| major.parse().ok())?
}

/// Whether the SDK `dotnet --version` names is one a requirement of `sdk_required` allows.
/// SDKs install side by side and that one is only the newest: when it is older than asked, none
/// will do (`false`); when it is past the range, an older one beside it may still be there, which
/// can't be told from here (`None`).
fn sdk_holds(required: &str, active: &str) -> Option<bool> {
    let active = Sdk::parse(active)?;
    let mut holds = Some(true);
    for bound in required.split_whitespace() {
        if let Some(least) = bound.strip_prefix(">=") {
            if active < Sdk::parse(least)? {
                return Some(false);
            }
        } else if let Some(below) = bound.strip_prefix('<') {
            if active >= Sdk::parse(below)? {
                holds = None;
            }
        } else {
            let only = Sdk::parse(bound.strip_prefix('=')?)?;
            if active < only {
                return Some(false);
            }
            if active > only {
                holds = None;
            }
        }
    }
    holds
}

/* ---------- the network scan ---------- */

/// `dotnet list package --outdated --format json` and `--vulnerable`: the top-level packages
/// listed under `projects[].frameworks[].topLevelPackages[]`, each once however many projects
/// and frameworks have it. A project it couldn't read (not restored) is in `problems` as an
/// error: no list then.
fn listed(output: &str) -> Option<Vec<Value>> {
    let value = json_object(output)?;
    let failed = |problem: &Value| text(problem, "level").is_some_and(|level| level.eq_ignore_ascii_case("error"));
    if value.get("problems").and_then(Value::as_array).is_some_and(|problems| problems.iter().any(failed)) {
        return None;
    }
    let lists = |value: &Value, key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let frameworks = value.get("projects")?.as_array()?.iter().flat_map(|project| lists(project, "frameworks"));
    let mut seen = BTreeSet::new();
    let packages = frameworks.flat_map(|framework| lists(&framework, "topLevelPackages"));
    Some(packages.filter(|package| text(package, "id").is_some_and(|id| seen.insert(id.to_lowercase()))).collect())
}

/// `--outdated`: what is resolved and the latest there is.
fn nuget_outdated(output: &str) -> Option<Vec<Outdated>> {
    let packages = listed(output)?;
    let outdated = packages.iter().map(|package| Outdated {
        name: text(package, "id").unwrap_or_default().to_string(),
        installed: owned(package, "resolvedVersion"),
        wanted: None,
        latest: owned(package, "latestVersion"),
    });
    Some(outdated.collect())
}

/// `--vulnerable`: each package's `vulnerabilities`, a severity and the advisory's address.
fn nuget_audit(output: &str) -> Option<Audit> {
    let packages = listed(output)?;
    let mut advisories = Vec::new();
    for package in &packages {
        let of_package = Advisory { package: text(package, "id").unwrap_or_default().to_string(), installed: owned(package, "resolvedVersion"), ..Advisory::default() };
        let found = package.get("vulnerabilities").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        if found.is_empty() {
            advisories.push(of_package.clone());
        }
        for vulnerability in found {
            advisories.push(Advisory { severity: severity(text(vulnerability, "severity")), url: owned(vulnerability, "advisoryurl"), ..of_package.clone() });
        }
    }
    Some(Audit { packages: count(packages.len()), advisories })
}
