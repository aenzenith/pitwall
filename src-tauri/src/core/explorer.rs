//! The Files window (`src/window/files/`): a listed project's folders by their entries' names,
//! the Git state of its files, a file's text and its changes since the last commit. Each is read
//! when the user opens it, and kept nowhere here: the window holds what it shows in its own
//! memory. Nothing outside the project's folder is read (`inside`: every link and `..`
//! followed first), `.git` is never listed or read into, and nothing is ever written.

use std::cmp::Ordering;

use super::*;

/// Left out of a listing as VS Code leaves them out (`files.exclude`): a repository's own
/// folders and the system's files.
const HIDDEN: [&str; 6] = [".git", ".svn", ".hg", "CVS", ".DS_Store", "Thumbs.db"];
/// Folders the file index for the search doesn't enter: what is installed, built or kept there.
const UNSEARCHED: [&str; 10] = ["node_modules", "vendor", "target", "dist", "build", "venv", "storage", "var", "coverage", "tmp"];
/// A listing names at most this many entries of a folder; the rest are counted only.
const LIST_NAMES: usize = 100_000;
/// A file is shown up to this size; a larger one only by its size.
pub const TEXT_BYTES: u64 = 5 * 1024 * 1024;
/// A NUL among the first bytes makes a file binary, as Git judges it.
const SNIFF_BYTES: usize = 8000;
/// The pictures a Markdown file's preview shows, by their extension, with their media type.
const IMAGES: [(&str, &str); 9] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
];
/// The file index for the search: at most this many files, gathered for at most this long.
const INDEX_FILES: usize = 20_000;
const INDEX_TIME: Duration = Duration::from_millis(1500);
/// Git's answers: a status or a diff that takes longer is given up.
const GIT_TIME: Duration = Duration::from_secs(8);
/// A diff larger than this isn't shown.
const DIFF_BYTES: usize = 2 * 1024 * 1024;

/// One folder's entries, folders first, each group in name order as VS Code sorts it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Entries left out by the limit asked for.
    pub more: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    /// A link, to a folder or a file, or one that leads nowhere.
    pub link: bool,
    pub size: u64,
}

/// What the window shows of a file.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileView {
    /// Its text; `lossy` when it isn't valid UTF-8 and was shown with replacement characters.
    #[serde(rename_all = "camelCase")]
    Text { text: String, size: u64, lossy: bool },
    Binary { size: u64 },
    Large { size: u64 },
}

/// A path's state in Git, its path from the project's folder with `/` between its parts: `M`
/// changed, `A` added, `R` renamed, `D` deleted, `U` untracked, `!` in conflict, `I` ignored
/// (a folder's ends with `/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitMark {
    pub path: String,
    pub mark: char,
}

/// Every file of the project the search looks through, from its folder with `/`; `partial`
/// when the index stopped at its bound.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIndex {
    pub files: Vec<String>,
    pub partial: bool,
}

/// The history the Source control tab draws, and where a commit of it is seen on the web.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub commits: Vec<Commit>,
    /// The commit checked out; none before the first commit.
    pub head: Option<String>,
    /// Older commits are left to ask for.
    pub more: bool,
    pub remote: Option<Forge>,
}

/// One commit: its id and its parents', the branches and tags at it (`HEAD -> main`,
/// `origin/main`, `tag: v1.0`), who wrote it, when (seconds), the first line of its message.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,
    pub author: String,
    pub time: i64,
    pub subject: String,
}

/// The code host the remote is on, and a commit's page there (`{id}` stands for the commit).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Forge {
    pub host: String,
    pub commit_url: String,
}

/// A file a commit changed: `M` `A` `D` `R` (`from`: where it was), as `git show --name-status`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitFile {
    pub path: String,
    pub from: Option<String>,
    pub mark: char,
}

/// The most commits one page of the history holds.
const LOG_PAGE_MAX: usize = 500;

impl Core {
    /// The project's folder as the system spells it, when `project` is one the app lists.
    fn explorer_root(&self, project: &str) -> Option<PathBuf> {
        if !self.listed_paths().iter().any(|listed| listed == project) {
            return None;
        }
        fs::canonicalize(project).ok()
    }

    /// The folder `dir` of the project (`""` its root): at most `limit` entries.
    pub fn explorer_list(&self, project: &str, dir: &str, limit: usize) -> Option<Listing> {
        let root = self.explorer_root(project)?;
        let folder = inside(&root, dir)?;
        if !folder.is_dir() {
            return None;
        }

        let mut names: Vec<String> = Vec::new();
        let mut uncounted = 0;
        for entry in fs::read_dir(&folder).ok()?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                continue;
            }
            if names.len() < LIST_NAMES {
                names.push(name);
            } else {
                uncounted += 1;
            }
        }

        let mut entries: Vec<Entry> = names
            .into_iter()
            .map(|name| {
                let path = folder.join(&name);
                let link = fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
                // A link is shown as what it leads to: entering one that leaves the folder is
                // refused all the same (`inside`).
                let meta = fs::metadata(&path).ok();
                let dir = meta.as_ref().is_some_and(|meta| meta.is_dir());
                let size = meta.as_ref().filter(|meta| meta.is_file()).map_or(0, |meta| meta.len());
                Entry { name, dir, link, size }
            })
            .collect();
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| natural(&a.name, &b.name)));

        let more = entries.len().saturating_sub(limit) + uncounted;
        entries.truncate(limit);
        Some(Listing { entries, more })
    }

    /// The file `file` of the project, as the window shows it. `None` when it isn't a regular
    /// file inside the project's folder (a folder, a pipe, a device, gone, outside) or can't be
    /// read.
    pub fn explorer_read(&self, project: &str, file: &str) -> Option<FileView> {
        let root = self.explorer_root(project)?;
        let path = inside(&root, file)?;
        let meta = fs::metadata(&path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let size = meta.len();
        if size > TEXT_BYTES {
            return Some(FileView::Large { size });
        }

        // A regular file only, opened without waiting and read to the bound (`files`).
        let bytes = crate::files::read_regular(&path, TEXT_BYTES)?;
        if bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) {
            return Some(FileView::Binary { size });
        }
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").map(<[u8]>::to_vec).unwrap_or(bytes);
        Some(match String::from_utf8(bytes) {
            Ok(text) => FileView::Text { text, size, lossy: false },
            Err(error) => FileView::Text { text: String::from_utf8_lossy(error.as_bytes()).into_owned(), size, lossy: true },
        })
    }

    /// A picture a Markdown file names, for its preview: a `data:` URL of a picture inside the
    /// project's folder (by its extension, a regular file, read as a file is, up to `TEXT_BYTES`);
    /// `None` for anything else.
    pub fn explorer_image(&self, project: &str, file: &str) -> Option<String> {
        let root = self.explorer_root(project)?;
        let path = inside(&root, file)?;
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let (_, media) = IMAGES.iter().find(|(name, _)| *name == extension)?;
        let meta = fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > TEXT_BYTES {
            return None;
        }
        let bytes = crate::files::read_regular(&path, TEXT_BYTES)?;
        use base64::Engine as _;
        Some(format!("data:{media};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }

    /// The Git state of the project's paths; `None` when its folder isn't a Git work tree or
    /// Git didn't answer in time.
    pub fn explorer_status(&self, project: &str) -> Option<Vec<GitMark>> {
        let root = self.explorer_root(project)?;
        if !root.join(".git").exists() {
            return None;
        }
        let mut command = crate::git::read_command(project);
        command.args(["status", "--porcelain=v1", "-z", "--ignored=matching", "--untracked-files=all"]);
        let output = process::output_within(&mut command, GIT_TIME).ok()?;
        output.status.success().then(|| parse_marks(&String::from_utf8_lossy(&output.stdout)))
    }

    /// What changed in `file` since the last commit, as `git diff` writes it (no external diff
    /// tool or text conversion of the user's configuration runs for it). Empty when nothing
    /// did; `None` when it can't be asked (no commit yet, not a work tree, too large).
    pub fn explorer_diff(&self, project: &str, file: &str) -> Option<String> {
        let root = self.explorer_root(project)?;
        // A file still there is followed to where it is; one deleted is named as Git names it.
        let relative = match inside(&root, file) {
            Some(path) => slashed(path.strip_prefix(&root).ok()?),
            None if !root.join(file).exists() => plain_path(file)?,
            None => return None,
        };
        let mut command = crate::git::read_command(project);
        // The path as it is, never a pattern: `b*.txt` or `a[1].txt` would take other files' changes too.
        command.env("GIT_LITERAL_PATHSPECS", "1");
        command.args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "-U3", "HEAD", "--", &relative]);
        let output = process::output_within(&mut command, GIT_TIME).ok()?;
        (output.status.success() && output.stdout.len() <= DIFF_BYTES).then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The project's files for the search (`FileIndex`): hidden folders and those of installed or
    /// built files left out, links to folders not followed.
    pub fn explorer_files(&self, project: &str) -> Option<FileIndex> {
        let root = self.explorer_root(project)?;
        let started = Instant::now();
        let mut files = Vec::new();
        let mut partial = false;
        let mut folders = VecDeque::from([PathBuf::new()]);

        'walk: while let Some(folder) = folders.pop_front() {
            let Ok(read) = fs::read_dir(root.join(&folder)) else {
                continue;
            };
            for entry in read.flatten() {
                if files.len() >= INDEX_FILES || started.elapsed() > INDEX_TIME {
                    partial = true;
                    break 'walk;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if HIDDEN.contains(&name.as_str()) {
                    continue;
                }
                let path = folder.join(&name);
                if kind.is_dir() {
                    if !name.starts_with('.') && !UNSEARCHED.contains(&name.as_str()) {
                        folders.push_back(path);
                    }
                } else if kind.is_file() || kind.is_symlink() {
                    files.push(slashed(&path));
                }
            }
        }
        Some(FileIndex { files, partial })
    }

    /// The history of the project's branch and of its upstream, newest first (`git log
    /// --topo-order`), `limit` commits after the first `skip`.
    pub fn explorer_log(&self, project: &str, skip: usize, limit: usize) -> Option<History> {
        let root = self.explorer_root(project)?;
        if !root.join(".git").exists() {
            return None;
        }
        let limit = limit.clamp(1, LOG_PAGE_MAX);
        let ask = |args: &[&str]| -> Option<String> {
            let mut command = crate::git::read_command(project);
            command.args(args);
            let output = process::output_within(&mut command, crate::git::LOOKUP_LIMIT).ok()?;
            output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        };
        let head = ask(&["rev-parse", "--verify", "--quiet", "HEAD"]);
        let upstream = ask(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).filter(|name| !name.is_empty());
        let remote = upstream.as_deref().and_then(|name| name.split_once('/')).map_or("origin", |(remote, _)| remote).to_string();
        let remote = ask(&["remote", "get-url", "--", &remote]).and_then(|url| forge(&url));
        let Some(head) = head else {
            return Some(History { commits: Vec::new(), head: None, more: false, remote });
        };

        let mut command = crate::git::read_command(project);
        let (count, skip) = ((limit + 1).to_string(), skip.to_string());
        command.args(["log", "--topo-order", "--no-color", "--format=%H%x1f%P%x1f%D%x1f%an%x1f%at%x1f%s%x1e", "-n", &count, "--skip", &skip, "HEAD"]);
        if upstream.is_some() {
            command.arg("@{upstream}");
        }
        command.arg("--");
        let output = process::output_within(&mut command, crate::git::LOG_LIMIT).ok()?;
        if !output.status.success() {
            return None;
        }
        let mut commits = parse_log(&String::from_utf8_lossy(&output.stdout));
        let more = commits.len() > limit;
        commits.truncate(limit);
        Some(History { commits, head: Some(head), more, remote })
    }

    /// The files commit `id` changed against its first parent.
    pub fn explorer_commit(&self, project: &str, id: &str) -> Option<Vec<CommitFile>> {
        let root = self.explorer_root(project)?;
        if !root.join(".git").exists() || !commit_id(id) {
            return None;
        }
        let mut command = crate::git::read_command(project);
        command.args(["show", "--no-color", "--no-ext-diff", "--no-textconv", "--diff-merges=first-parent", "-M", "--format=", "--name-status", id, "--"]);
        let output = process::output_within(&mut command, crate::git::LOG_LIMIT).ok()?;
        output.status.success().then(|| parse_name_status(&String::from_utf8_lossy(&output.stdout)))
    }

    /// What commit `id` changed in `file` (and, for a rename, `from`), as `git show` writes it.
    pub fn explorer_commit_diff(&self, project: &str, id: &str, file: &str, from: Option<&str>) -> Option<String> {
        let root = self.explorer_root(project)?;
        if !root.join(".git").exists() || !commit_id(id) {
            return None;
        }
        let file = plain_path(file)?;
        let from = match from {
            Some(from) => Some(plain_path(from)?),
            None => None,
        };
        let mut command = crate::git::read_command(project);
        command.env("GIT_LITERAL_PATHSPECS", "1");
        command.args(["show", "--no-color", "--no-ext-diff", "--no-textconv", "--diff-merges=first-parent", "-M", "-U3", "--format=", id, "--"]);
        command.args(from.iter().chain([&file]));
        let output = process::output_within(&mut command, crate::git::LOG_LIMIT).ok()?;
        (output.status.success() && output.stdout.len() <= DIFF_BYTES).then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The path of `file` in the project, checked as a read is: for the window to show it in
    /// Finder.
    pub fn explorer_path(&self, project: &str, file: &str) -> Option<PathBuf> {
        let root = self.explorer_root(project)?;
        inside(&root, file).filter(|path| path.exists())
    }
}

/// `relative` under `root` (its canonical folder): the path itself once every link and `..` in
/// it is followed, and only when that is still under `root` and not in `.git`. `""` is the
/// root.
fn inside(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.contains('\0') {
        return None;
    }
    let relative = Path::new(relative.trim_start_matches(['/', '\\']));
    // An absolute path joined to the folder is that path again (Windows: a drive or a share).
    if relative.has_root() || relative.components().any(|part| matches!(part, std::path::Component::Prefix(_))) {
        return None;
    }
    let real = fs::canonicalize(root.join(relative)).ok()?;
    let within = real.strip_prefix(root).ok()?;
    if within.components().any(|part| part.as_os_str() == ".git") {
        return None;
    }
    Some(real)
}

/// A path inside the project as words only (no root, no `..`, nothing in `.git`), for Git to
/// look up: a file deleted since, or one of a past commit.
fn plain_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|part| !part.is_empty() && *part != ".").collect();
    let plain = !parts.is_empty() && !path.contains('\0') && !path.starts_with(['/', '\\']) && parts.iter().all(|part| *part != ".." && *part != ".git" && !part.contains(':'));
    plain.then(|| parts.join("/"))
}

/// A commit's id as the window may name one: 4 to 64 hexadecimal digits, so it is never taken
/// for an option (`--output=` would have git write a file) or a range.
fn commit_id(id: &str) -> bool {
    (4..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// `git log --format=%H%x1f%P%x1f%D%x1f%an%x1f%at%x1f%s%x1e`.
fn parse_log(output: &str) -> Vec<Commit> {
    output
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.trim_start_matches('\n').split('\x1f');
            let id = fields.next().filter(|id| commit_id(id))?.to_string();
            let parents = fields.next()?.split_whitespace().map(str::to_string).collect();
            let refs = fields.next()?.split(", ").filter(|name| !name.is_empty()).map(str::to_string).collect();
            let author = fields.next()?.to_string();
            let time = fields.next()?.parse().unwrap_or(0);
            let subject: String = fields.next()?.chars().take(300).collect();
            Some(Commit { id, parents, refs, author, time, subject })
        })
        .collect()
}

/// `git show --name-status`: `M\tpath`, `R087\told\tnew`.
fn parse_name_status(output: &str) -> Vec<CommitFile> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let code = fields.next()?.chars().next()?;
            let first = fields.next()?.to_string();
            let (path, from) = match fields.next() {
                Some(second) => (second.to_string(), Some(first)),
                None => (first, None),
            };
            let mark = match code {
                'A' | 'C' => 'A',
                'D' => 'D',
                'R' => 'R',
                _ => 'M',
            };
            Some(CommitFile { path, from: if mark == 'R' { from } else { None }, mark })
        })
        .collect()
}

/// The code host a remote's address is on, and its pages for a commit; none for one it doesn't know.
fn forge(remote: &str) -> Option<Forge> {
    let web = crate::links::web_url(remote)?;
    let host = web.trim_start_matches("https://").split('/').next()?.to_string();
    let (name, page) = if host.contains("github") {
        ("GitHub", "commit")
    } else if host.contains("gitlab") {
        ("GitLab", "-/commit")
    } else if host.contains("bitbucket") {
        ("Bitbucket", "commits")
    } else {
        return None;
    };
    Some(Forge { host: name.to_string(), commit_url: format!("{web}/{page}/{{id}}") })
}

/// A path with `/` between its parts, as Git and the window write them.
fn slashed(path: &Path) -> String {
    path.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// `git status --porcelain=v1 -z`: `XY path`, a rename's old path after it in a field of its
/// own.
fn parse_marks(output: &str) -> Vec<GitMark> {
    let mut marks = Vec::new();
    let mut fields = output.split('\0');
    while let Some(field) = fields.next() {
        if field.len() < 4 {
            continue;
        }
        let (code, path) = field.split_at(3);
        let mut code = code.chars();
        let (x, y) = (code.next().unwrap_or(' '), code.next().unwrap_or(' '));
        if x == 'R' || x == 'C' {
            // The old path follows.
            fields.next();
        }
        let mark = match (x, y) {
            ('?', '?') => 'U',
            ('!', '!') => 'I',
            ('D', 'D') | ('A', 'U') | ('U', 'D') | ('U', 'A') | ('D', 'U') | ('A', 'A') | ('U', 'U') => '!',
            (_, 'M') | (_, 'T') | ('M', ' ') | ('T', ' ') => 'M',
            (_, 'D') | ('D', ' ') => 'D',
            ('R', _) => 'R',
            ('A', _) | ('C', _) => 'A',
            _ => continue,
        };
        marks.push(GitMark { path: path.to_string(), mark });
    }
    marks
}

/// Names as VS Code's explorer orders them: letter case aside, and a run of digits by its
/// number (`file2` before `file10`).
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        chars.next();
                    }
                    digits
                };
                let (x, y) = (take(&mut a), take(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        tmp: tempfile::TempDir,
        core: Arc<Core>,
        project: String,
    }

    /// A project the app lists, with a file, a folder and a `.git` of its own, and beside it a
    /// folder that isn't the project's.
    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        for (file, text) in [
            ("paddock/src/a.ts", "a"),
            ("paddock/.git/config", "[core]"),
            ("paddock/.env", "KEY=value"),
            ("outside/secret.ts", "secret"),
            ("outside/secret.png", "\u{89}PNG secret"),
            ("paddock/docs/shot.png", "\u{89}PNG shot"),
        ] {
            let file = tmp.path().join(file);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }

        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude"),
                settings_file: tmp.path().join("settings.json"),
                claude_settings: tmp.path().join("claude-settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        let project = project.to_string_lossy().into_owned();
        core.add_project(&project).unwrap();
        Fixture { tmp, core, project }
    }

    /// The gate: the window names a path, and nothing outside the project's folder is listed,
    /// read, diffed or shown in Finder through it (absolute, through `..`, through a link), nor
    /// anything in its `.git`, nor a folder the app doesn't list.
    #[test]
    fn nothing_outside_the_project_is_read() {
        let fx = fixture();
        let outside = fx.tmp.path().join("outside");
        let refused: Vec<String> = vec![
            outside.join("secret.ts").to_string_lossy().into_owned(),
            "../outside/secret.ts".into(),
            "src/../../outside/secret.ts".into(),
            ".git/config".into(),
            "src/../.git/config".into(),
            "\\\\server\\share\\a.ts".into(),
        ];
        #[cfg(unix)]
        let refused = {
            let project = Path::new(&fx.project);
            std::os::unix::fs::symlink(outside.join("secret.ts"), project.join("link.ts")).unwrap();
            std::os::unix::fs::symlink(&outside, project.join("linked")).unwrap();
            std::os::unix::fs::symlink(outside.join("secret.png"), project.join("link.png")).unwrap();
            let mut refused = refused;
            refused.extend(["link.ts".into(), "linked/secret.ts".into(), "link.png".into(), "linked/secret.png".into()]);
            refused
        };
        let refused: Vec<String> = refused
            .into_iter()
            .chain([outside.join("secret.png").to_string_lossy().into_owned(), "../outside/secret.png".into(), "docs/../../outside/secret.png".into()])
            .collect();

        for file in &refused {
            assert!(fx.core.explorer_read(&fx.project, file).is_none(), "{file:?} must not be read");
            assert!(fx.core.explorer_diff(&fx.project, file).is_none(), "{file:?} must not be diffed");
            assert!(fx.core.explorer_path(&fx.project, file).is_none(), "{file:?} must not be shown");
            assert!(fx.core.explorer_image(&fx.project, file).is_none(), "{file:?} must not be pictured");
        }
        // A picture is a picture of the project's: the same gate, and nothing but a picture.
        assert!(fx.core.explorer_image(&fx.project, "docs/shot.png").is_some_and(|url| url.starts_with("data:image/png;base64,")));
        for file in [".env", "src/a.ts", "docs"] {
            assert!(fx.core.explorer_image(&fx.project, file).is_none(), "{file:?} is no picture");
        }
        for dir in ["..", "../outside", ".git", outside.to_string_lossy().as_ref()] {
            assert!(fx.core.explorer_list(&fx.project, dir, 500).is_none(), "{dir:?} must not be listed");
        }
        // A path from the root is the project's own: `/` is its folder, not the disk's.
        let names = |dir| fx.core.explorer_list(&fx.project, dir, 500).map(|listing| listing.entries.into_iter().map(|entry| entry.name).collect::<Vec<_>>());
        assert_eq!(names("/"), names(""));
        #[cfg(unix)]
        assert!(fx.core.explorer_list(&fx.project, "linked", 500).is_none(), "a link out of the folder must not be listed");

        // The folder above the project holds the project's files too, and is no project.
        let above = fx.tmp.path().to_string_lossy().into_owned();
        assert!(fx.core.explorer_read(&above, "paddock/src/a.ts").is_none());
        assert!(fx.core.explorer_list(&above, "", 500).is_none());
        assert!(fx.core.explorer_files(&above).is_none());
    }

    /// A commit is named by its id alone: nothing that git could take for an option (`--output=`
    /// would have it write a file), a range or another revision reaches it, nor a path that
    /// leaves the project or goes into `.git`.
    #[test]
    fn a_commit_is_named_by_its_id_alone() {
        let fx = fixture();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git").args(["-C", &fx.project, "-c", "user.email=t@t", "-c", "user.name=t"]).args(args).output().unwrap();
            assert!(output.status.success(), "git {args:?}");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        git(&["add", "src/a.ts"]);
        git(&["commit", "-qm", "one"]);
        let id = git(&["rev-parse", "HEAD"]);

        let files = fx.core.explorer_commit(&fx.project, &id).unwrap();
        assert_eq!(files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["src/a.ts"]);
        assert!(fx.core.explorer_commit_diff(&fx.project, &id, "src/a.ts", None).is_some_and(|diff| diff.contains("+a")));

        let written = Path::new(&fx.project).join("written.txt");
        for bad in ["--output=written.txt", "-p", "HEAD", "HEAD~1", "main", "abc..def", "abcd^", ""] {
            assert!(fx.core.explorer_commit(&fx.project, bad).is_none(), "{bad:?} is no commit id");
            assert!(fx.core.explorer_commit_diff(&fx.project, bad, "src/a.ts", None).is_none(), "{bad:?} is no commit id");
        }
        for path in ["../outside/secret.ts", "/etc/hosts", ".git/config", "src/../../outside/secret.ts", ":(top)src/a.ts"] {
            assert!(fx.core.explorer_commit_diff(&fx.project, &id, path, None).is_none(), "{path:?} is no path of the project");
            assert!(fx.core.explorer_commit_diff(&fx.project, &id, "src/a.ts", Some(path)).is_none(), "{path:?} is no path of the project");
        }
        assert!(!written.exists(), "nothing may be written");
    }

    #[test]
    fn the_project_is_listed_and_read_without_its_git_folder() {
        let fx = fixture();
        let root = fx.core.explorer_list(&fx.project, "", 500).unwrap();
        let names: Vec<&str> = root.entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["docs", "src", ".env"], "folders first, `.git` left out");

        match fx.core.explorer_read(&fx.project, "src/a.ts") {
            Some(FileView::Text { text, lossy: false, .. }) => assert_eq!(text, "a"),
            other => panic!("{other:?}"),
        }
        let index = fx.core.explorer_files(&fx.project).unwrap();
        assert!(index.files.contains(&"src/a.ts".to_string()) && !index.files.iter().any(|file| file.starts_with(".git")), "{:?}", index.files);
    }
}
