//! From a line of output to the editor: the file a line names (`Checkout.vue:42:11`), found
//! inside the project's folder and opened there at its line. What a dev server prints is not
//! to be trusted with more than that: nothing outside the project's folder is ever opened.

use std::ffi::OsStr;
use std::path::Component;

use super::*;

/// Folders a file is never looked for in by its name: what is installed, built or kept there,
/// not written. Hidden folders (`.git`, `.venv`) are left out too.
const SKIPPED: [&str; 10] = ["node_modules", "vendor", "target", "dist", "build", "venv", "storage", "var", "coverage", "tmp"];
/// A search by name gives up after this long, or this many folder entries.
const SEARCH_TIME: Duration = Duration::from_millis(300);
const SEARCH_LIMIT: usize = 200_000;

impl Core {
    /// Opens the file an output line of the project at `path` names, in the user's editor at
    /// `line` (and `column`). `file` is the name as the line printed it; where it points is
    /// decided here (`resolve`). False when the project has no such file: nothing opens then.
    /// `own_folder`: the name is written from the project's own folder, as the Files window
    /// writes it, never from its working folder. It reads the project's folder, so not on the
    /// main thread.
    pub fn open_reference(&self, path: &str, file: &str, line: u32, column: Option<u32>, own_folder: bool) -> bool {
        // The folder is the gate: it is a project the app shows, never one the caller names.
        if !self.listed_paths().iter().any(|listed| listed == path) {
            return false;
        }
        // A path written from the folder it runs in, as its dev server and commands print them.
        let work = if own_folder { None } else { self.work_dir(path) };
        let Some(found) = resolve(Path::new(path), work.as_deref(), file) else {
            return false;
        };
        let scheme = self.lock().settings.editor_scheme().to_string();

        self.emit(CoreEvent::Open(editor_url(&scheme, &found, line, column)));
        // The editor's window of the project comes up, as with "Open in editor".
        self.mark_seen(path);
        true
    }
}

/// The file `printed` names in the project at `project`: the path itself when it leads to a
/// file, taken from the folder the project runs in (`work`, inside it) and then from the
/// project's folder unless it is absolute (one written from either with a leading slash, as
/// Vite does, is tried there too); with nothing at those places, the one file of that name in
/// the folder (`find_named`).
///
/// Always an existing regular file inside the project's folder once every symlink and `..` is
/// followed. Whatever else the path leads to is refused and not looked for by name: that would
/// open another file than the one meant. The file comes back under the project's own spelling,
/// by which the editor knows its folder.
///
/// An absolute path is only looked at when, as it is written, it begins with the project's
/// folder. A line can print a share on the network or a volume that hangs, and asking the
/// system about such a path already connects there, or waits. One written to somewhere else
/// is never asked about: it may still be the project's file as another machine names it (a
/// container's `/var/www/html/src/a.ts`), which is found by name together with its folder,
/// unless it is written into the user's own home, where it is this machine's file and not the
/// project's.
fn resolve(project: &Path, work: Option<&Path>, printed: &str) -> Option<PathBuf> {
    let root = fs::canonicalize(project).ok()?;
    let printed = printed.trim().trim_matches(['"', '\'', '`']);
    let printed = printed.strip_prefix("file://").unwrap_or(printed);
    if printed.is_empty() || printed.contains('\0') {
        return None;
    }

    let wanted = Path::new(printed);
    // Joined to the folder as it is spelled: Windows reads no `/` in a canonical path.
    let under_root = project.join(printed.trim_start_matches(['/', '\\']));
    let under_work = work.filter(|work| *work != project).map(|work| (work, work.join(printed.trim_start_matches(['/', '\\']))));
    let own = wanted.has_root() && (wanted.starts_with(project) || wanted.starts_with(&root));
    let elsewhere = wanted.has_root() && !own;
    // On Windows an absolute path joined to the folder is that path again: no place of the project's.
    let places = [
        own.then(|| wanted.to_path_buf()),
        under_work.and_then(|(work, place)| place.starts_with(work).then_some(place)),
        under_root.starts_with(project).then_some(under_root),
    ];

    let real = match places.into_iter().flatten().find_map(|place| fs::canonicalize(place).ok()) {
        Some(real) => real,
        None if elsewhere && dirs::home_dir().is_some_and(|home| wanted.starts_with(home)) => return None,
        None => fs::canonicalize(find_named(&root, wanted, elsewhere)?).ok()?,
    };

    let inside = real.strip_prefix(&root).ok()?;
    real.is_file().then(|| project.join(inside))
}

/// The one file under `root` that `wanted` can mean when nothing is where it says: named as
/// `wanted` is and, among several, the one whose folders match `wanted`'s furthest back
/// (`with_folder`: its own folder at least, the name alone is not enough). `None` when there is
/// none, when two fit as well, or when the folder takes too long to look through (`SEARCH_TIME`,
/// `SEARCH_LIMIT`). Folders of installed or built files, hidden ones and links to folders are
/// not entered.
///
/// A search that gave up gives nothing, also when it had a file of that name in hand: it cannot
/// know that one to be the only one, and another file than the one meant, opened without a
/// word, is worse than none.
fn find_named(root: &Path, wanted: &Path, with_folder: bool) -> Option<PathBuf> {
    let name = wanted.file_name()?;
    // The name, then its folders from the nearest.
    let tail: Vec<&OsStr> = wanted.components().rev().filter_map(|part| if let Component::Normal(part) = part { Some(part) } else { None }).collect();

    let mut best: Option<(usize, PathBuf)> = None;
    let mut tied = false;
    let mut seen = 0;
    let started = Instant::now();
    let mut folders = VecDeque::from([root.to_path_buf()]);

    while let Some(folder) = folders.pop_front() {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            seen += 1;
            if seen > SEARCH_LIMIT || (seen % 256 == 0 && started.elapsed() > SEARCH_TIME) {
                return None;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let entry_name = entry.file_name();

            if kind.is_dir() {
                if !entry_name.to_str().is_some_and(|name| name.starts_with('.') || SKIPPED.contains(&name)) {
                    folders.push_back(entry.path());
                }
            } else if entry_name == name {
                let path = entry.path();
                let fit = path.components().rev().zip(&tail).take_while(|(have, want)| have.as_os_str() == **want).count();
                match &best {
                    Some((top, _)) if fit < *top => {}
                    Some((top, _)) if fit == *top => tied = true,
                    _ => {
                        best = Some((fit, path));
                        tied = false;
                    }
                }
            }
        }
    }

    best.filter(|(fit, _)| !tied && (*fit > 1 || !with_folder)).map(|(_, path)| path)
}

/// `vscode://file/<path>:<line>:<column>`. Every editor Settings offers is a VS Code one, and
/// they all take a file and a place in it this way; the path is percent-encoded, so a space or
/// a `#` in it stays part of it.
fn editor_url(scheme: &str, file: &Path, line: u32, column: Option<u32>) -> String {
    let mut path = file.to_string_lossy().replace('\\', "/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }

    let mut url = format!("{scheme}://file");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => url.push(byte as char),
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url.push_str(&format!(":{}", line.max(1)));
    if let Some(column) = column {
        url.push_str(&format!(":{}", column.max(1)));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        tmp: tempfile::TempDir,
        core: Arc<Core>,
        project: String,
        /// Every address the core asked to have opened.
        opened: Arc<Mutex<Vec<String>>>,
    }

    /// A project the app lists, with `src/a.ts`, two `index.ts` and an installed package, and
    /// beside it a folder that isn't the project's, with a file of its own.
    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        for (file, text) in [
            ("paddock/src/a.ts", "a"),
            ("paddock/src/Pages/index.ts", "pages"),
            ("paddock/src/lib/index.ts", "lib"),
            ("paddock/node_modules/vite/client.ts", "installed"),
            ("outside/secret.ts", "secret"),
            ("outside/a.ts", "another a"),
        ] {
            let file = tmp.path().join(file);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }

        let opened = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&opened);
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude"),
                settings_file: tmp.path().join("settings.json"),
                claude_settings: tmp.path().join("claude-settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(move |event| {
                if let CoreEvent::Open(url) = event {
                    sink.lock().unwrap().push(url);
                }
            }),
        );

        let project = project.to_string_lossy().into_owned();
        core.add_project(&project).unwrap();
        Fixture { project, tmp, core, opened }
    }

    /// The gate: what a dev server prints can name any path. One that leaves the project's
    /// folder (absolute, through `..`, through a link) opens nothing, and isn't swapped for a
    /// file of the same name inside either. Nor is a folder the app doesn't list a project.
    #[test]
    fn a_reference_that_leaves_the_project_opens_nothing() {
        let fx = fixture();
        let outside = fx.tmp.path().join("outside");
        // Absolute, to somewhere else.
        let elsewhere: Vec<String> = vec![
            outside.join("secret.ts").to_string_lossy().into_owned(),
            // `a.ts` is in the project too: the one named is still the one outside.
            outside.join("a.ts").to_string_lossy().into_owned(),
            format!("file://{}", outside.join("secret.ts").to_string_lossy()),
            // A share on the network, as Windows and as a URL write it.
            "\\\\server\\share\\a.ts".into(),
            "//server/share/a.ts".into(),
        ];
        let mut refused = elsewhere.clone();
        refused.extend([
            "../outside/secret.ts".into(),
            "src/../../outside/secret.ts".into(),
            "../outside/a.ts".into(),
            // Not in the project by name either.
            "secret.ts".into(),
            // A folder, nothing, and the project's folder itself are no file.
            "src".into(),
            "".into(),
            ".".into(),
        ]);

        #[cfg(unix)]
        {
            let project = Path::new(&fx.project);
            std::os::unix::fs::symlink(outside.join("secret.ts"), project.join("link.ts")).unwrap();
            std::os::unix::fs::symlink(&outside, project.join("linked")).unwrap();
            refused.extend(["link.ts".into(), "linked/secret.ts".into(), "./linked/a.ts".into()]);
        }

        for file in &refused {
            assert!(!fx.core.open_reference(&fx.project, file, 1, Some(1), false), "{file:?} must be refused");
        }
        // An absolute path to somewhere else is not even looked at: with nothing there any
        // more, the answer is the same.
        fs::remove_dir_all(&outside).unwrap();
        for file in &elsewhere {
            assert!(!fx.core.open_reference(&fx.project, file, 1, Some(1), false), "{file:?} must be refused, whatever is there");
        }

        // The folder above the project holds the project's files too, and is no project.
        let above = fx.tmp.path().to_string_lossy().into_owned();
        assert!(!fx.core.open_reference(&above, "paddock/src/a.ts", 1, Some(1), false), "a folder that isn't listed is no project");
        assert!(fx.opened.lock().unwrap().is_empty(), "nothing may be opened: {:?}", fx.opened.lock().unwrap());
    }

    #[test]
    fn a_file_of_the_project_opens_at_its_line() {
        let fx = fixture();
        let project = Path::new(&fx.project);
        let a = editor_url("vscode", &project.join("src").join("a.ts"), 12, Some(5));
        let pages = editor_url("vscode", &project.join("src").join("Pages").join("index.ts"), 12, Some(5));
        assert!(a.starts_with("vscode://file/") && a.ends_with("/paddock/src/a.ts:12:5"), "{a}");

        let inside = project.join("src").join("a.ts").to_string_lossy().into_owned();
        // A file no search by name would find, under the folder as the system spells it
        // (macOS: `/private/var/…` for `/var/…`).
        let installed = Path::new("node_modules").join("vite").join("client.ts");
        let real = fs::canonicalize(project).unwrap().join(&installed).to_string_lossy().into_owned();
        let installed = editor_url("vscode", &project.join(installed), 12, Some(5));
        for (file, url) in [
            ("src/a.ts", &a),
            ("./src/a.ts", &a),
            // From the project's root, as Vite writes it.
            ("/src/a.ts", &a),
            (inside.as_str(), &a),
            (real.as_str(), &installed),
            // By its name, when it is the only one; a path from elsewhere (a container's) too.
            ("a.ts", &a),
            ("/var/www/html/src/a.ts", &a),
            // Two of a name: the one whose folder is named.
            ("Pages/index.ts", &pages),
        ] {
            assert!(fx.core.open_reference(&fx.project, file, 12, Some(5), false), "{file:?} is in the project");
            assert_eq!(fx.opened.lock().unwrap().pop().as_ref(), Some(url), "{file:?}");
        }

        // Two of a name and nothing to tell them apart; a file of an installed package.
        for file in ["index.ts", "client.ts"] {
            assert!(!fx.core.open_reference(&fx.project, file, 1, None, false), "{file:?} is nobody's");
        }
        assert!(fx.opened.lock().unwrap().is_empty());

        // The editor is the one Settings names; a place without a column is its line.
        let mut settings = fx.core.settings();
        settings.editor = "cursor".into();
        fx.core.set_settings(settings);
        assert!(fx.core.open_reference(&fx.project, "src/a.ts", 3, None, false));
        let url = fx.opened.lock().unwrap().pop().unwrap();
        assert!(url.starts_with("cursor://file/") && url.ends_with("/paddock/src/a.ts:3"), "{url}");
    }
}
