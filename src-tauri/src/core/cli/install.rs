//! Where the `pitwall` command is: a link named `pitwall`, to this app's executable, in a folder
//! of the shell's PATH. Installing it asks for no password, makes no folder outside the home
//! folder and takes nobody's place: a file or a link that is there under the name and isn't this
//! app's stays as it is, and so it does when the command is taken out again. This app's is also
//! the link it left when it was moved (`left_behind`): installing puts that one right.

use std::path::Component;

use super::*;

/// The command's name, and the link's.
const COMMAND: &str = "pitwall";

/// Why the command wasn't installed.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refused {
    /// The shell's PATH has a `pitwall` already, here, and it isn't this app's.
    Taken(PathBuf),
    /// No folder of the shell's PATH can be written to without administrator rights.
    NoFolder,
}

/// What installing goes by: the folders of the shell's PATH in its order, the home folder, and
/// the app the link leads to.
pub(super) struct Places {
    pub(super) path: Vec<PathBuf>,
    pub(super) home: PathBuf,
    pub(super) app: PathBuf,
}

/// Whether `link` is this app's: a link, and the app is where it leads.
fn is_own(link: &Path, app: &Path) -> bool {
    let linked = fs::symlink_metadata(link).is_ok_and(|meta| meta.file_type().is_symlink());
    linked && matches!((fs::canonicalize(link), fs::canonicalize(app)), (Ok(leads), Ok(app)) if leads == app)
}

/// Something is there under the name: a file, a folder, or a link, wherever it leads.
fn taken(link: &Path) -> bool {
    fs::symlink_metadata(link).is_ok()
}

/// The last four parts of an executable in an app's bundle, `<Name>.app/Contents/MacOS/<name>`;
/// none for a file that isn't one by its path.
fn in_bundle(file: &Path) -> Option<Vec<&std::ffi::OsStr>> {
    let mut parts: Vec<_> = file.components().rev().take(4).map(|part| part.as_os_str()).collect();
    parts.reverse();
    let bundled = parts.len() == 4 && Path::new(parts[0]).extension().is_some_and(|kind| kind == "app") && parts[1] == "Contents" && parts[2] == "MacOS";
    bundled.then_some(parts)
}

/// Whether `link` is one this app left behind when it was moved (run from its disk image or
/// the Downloads folder, then dragged to Applications): a link that leads nowhere any more, to
/// what was this very executable in this very bundle, by its last four parts. Both, or it is
/// somebody else's: a link that leads nowhere else, or to a copy of the app that is still there.
fn left_behind(link: &Path, app: &Path) -> bool {
    let nowhere = fs::metadata(link).is_err();
    let led = fs::read_link(link).ok();
    nowhere && led.as_deref().and_then(in_bundle).is_some_and(|led| Some(led) == in_bundle(app))
}

#[cfg(unix)]
fn link_to(app: &Path, link: &Path) -> std::io::Result<()> {
    // Never in the place of what is there: a name that is taken is an error.
    std::os::unix::fs::symlink(app, link)
}

#[cfg(not(unix))]
fn link_to(_app: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

impl Places {
    /// This machine's: the terminal's PATH (the login shell is asked once, as for a dev
    /// server), the user's home folder, and the app that runs.
    pub(super) fn here() -> Option<Self> {
        Some(Self { path: process::terminal_path(), home: dirs::home_dir()?, app: std::env::current_exe().ok()? })
    }

    /// The folders of the PATH a command can be in, each once: the absolute ones, as written.
    fn folders(&self) -> Vec<&Path> {
        let mut folders: Vec<&Path> = Vec::new();
        for folder in self.path.iter().map(PathBuf::as_path).filter(|folder| folder.is_absolute()) {
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
        folders
    }

    /// The folders the link is put in, the first that takes it: the usual ones first
    /// (`/usr/local/bin`, `/opt/homebrew/bin`, `~/.local/bin`, `~/bin`), then the rest of the
    /// PATH in its order. Only folders of the PATH: anywhere else the command wouldn't be found.
    fn candidates(&self) -> Vec<&Path> {
        let usual = [PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/homebrew/bin"), self.home.join(".local").join("bin"), self.home.join("bin")];
        let folders = self.folders();
        let first = usual.iter().filter_map(|usual| folders.iter().copied().find(|folder| *folder == usual.as_path()));
        let mut candidates: Vec<&Path> = first.collect();
        for folder in folders {
            if !candidates.contains(&folder) {
                candidates.push(folder);
            }
        }
        candidates
    }

    /// Whether `folder` lies in the home folder: by its own text (nothing in it leads back
    /// out), and by where the part of it that is there already really is (no link on the way
    /// leads out either).
    fn in_home(&self, folder: &Path) -> bool {
        let by_text = folder.starts_with(&self.home) && folder != self.home && folder.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
        let there = folder.ancestors().find(|part| part.exists()).map(fs::canonicalize);
        by_text && matches!((there, fs::canonicalize(&self.home)), (Some(Ok(there)), Ok(home)) if there.starts_with(&home))
    }

    /// Where the command is: this app's link, in the first folder of the PATH that has it.
    pub(super) fn found(&self) -> Option<PathBuf> {
        self.folders().into_iter().map(|folder| folder.join(COMMAND)).find(|link| is_own(link, &self.app))
    }

    /// Puts the link in the first candidate folder that takes it, and says where it is. There
    /// already, it stays; one this app left behind when it was moved is taken away first. A
    /// `pitwall` of somebody else's anywhere on the PATH is left alone and nothing is installed
    /// beside it: one of the two would never be the one that runs.
    pub(super) fn install(&self) -> Result<PathBuf, Refused> {
        if let Some(link) = self.found() {
            return Ok(link);
        }
        for link in self.folders().into_iter().map(|folder| folder.join(COMMAND)).filter(|link| left_behind(link, &self.app)) {
            let _ = fs::remove_file(link);
        }
        if let Some(other) = self.folders().into_iter().map(|folder| folder.join(COMMAND)).find(|link| taken(link)) {
            return Err(Refused::Taken(other));
        }

        for folder in self.candidates() {
            // A folder of the PATH that isn't there yet is made only inside the home folder.
            if !folder.is_dir() && !(self.in_home(folder) && fs::create_dir_all(folder).is_ok()) {
                continue;
            }
            let link = folder.join(COMMAND);
            // Not allowed here (the folder is the system's): the next one.
            if link_to(&self.app, &link).is_ok() {
                return Ok(link);
            }
        }
        Err(Refused::NoFolder)
    }

    /// Takes this app's link out of every folder of the PATH that has it, and nothing else.
    pub(super) fn remove(&self) {
        for link in self.folders().into_iter().map(|folder| folder.join(COMMAND)) {
            if is_own(&link, &self.app) {
                let _ = fs::remove_file(&link);
            }
        }
    }

    /// `file` as Settings shows it: with `~` for the home folder.
    pub(super) fn shown(&self, file: &Path) -> String {
        match file.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.to_string_lossy()),
            Err(_) => file.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    struct Machine {
        _tmp: tempfile::TempDir,
        /// The temp folder, as the file system names it.
        root: PathBuf,
        places: Places,
    }

    impl Machine {
        fn dir(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
    }

    /// A machine in a temp folder: an app, a home folder, and a PATH of `folders` (each made,
    /// unless it is in the home folder or its name begins with `missing`).
    fn machine(folders: &[&str]) -> Machine {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let app = root.join("Pitwall.app").join("Contents").join("MacOS").join("pitwall");
        fs::create_dir_all(app.parent().unwrap()).unwrap();
        fs::write(&app, "the app").unwrap();
        fs::create_dir_all(root.join("home")).unwrap();

        let path = folders.iter().map(|name| root.join(name)).collect::<Vec<_>>();
        for (name, folder) in folders.iter().zip(&path) {
            if !name.starts_with("home/") && !name.starts_with("missing") {
                fs::create_dir_all(folder).unwrap();
            }
        }
        Machine { places: Places { path, home: root.join("home"), app }, root, _tmp: tmp }
    }

    /// Installing takes nobody's place and taking out removes nobody's: a file or a link named
    /// `pitwall` that isn't this app's is there afterwards exactly as it was, whichever of the
    /// two was tried, and nothing was installed beside it.
    #[test]
    fn a_pitwall_of_somebody_elses_is_never_replaced_or_removed() {
        type Plant = fn(&Machine, &Path);
        let plants: [(&str, Plant); 4] = [
            ("a file", |_, link| fs::write(link, "#!/bin/sh\necho theirs\n").unwrap()),
            ("a link to another program", |machine, link| {
                fs::write(machine.dir("other"), "another program").unwrap();
                symlink(machine.dir("other"), link).unwrap();
            }),
            ("a link that leads nowhere", |machine, link| symlink(machine.dir("gone").join("pitwall"), link).unwrap()),
            ("a folder", |_, link| fs::create_dir(link).unwrap()),
        ];

        for (what, plant) in plants {
            // Theirs is in the second folder of the PATH: the first one is free.
            let machine = machine(&["first", "second"]);
            let theirs = machine.dir("second").join(COMMAND);
            plant(&machine, &theirs);
            let before = (fs::symlink_metadata(&theirs).unwrap().file_type(), fs::read_link(&theirs).ok(), fs::read(&theirs).ok());
            let as_before = || (fs::symlink_metadata(&theirs).unwrap().file_type(), fs::read_link(&theirs).ok(), fs::read(&theirs).ok()) == before;

            assert_eq!(machine.places.install(), Err(Refused::Taken(theirs.clone())), "{what}");
            assert!(as_before(), "{what}: replaced by an install");
            assert!(!taken(&machine.dir("first").join(COMMAND)), "{what}: nothing is installed beside it");
            assert_eq!(machine.places.found(), None, "{what}: it isn't taken for this app's");

            machine.places.remove();
            assert!(as_before(), "{what}: removed");
        }

        // This app's own link is what both are about: put there, found, and taken out again.
        let machine = machine(&["first", "second"]);
        let link = machine.dir("first").join(COMMAND);
        assert_eq!(machine.places.install(), Ok(link.clone()));
        assert_eq!(fs::read_link(&link).unwrap(), machine.places.app);
        assert_eq!((machine.places.install(), machine.places.found()), (Ok(link.clone()), Some(link.clone())), "installed twice, it is the one link");
        machine.places.remove();
        assert!(!taken(&link));
    }

    /// The link goes into a folder of the PATH only, and a folder that isn't there is made only
    /// inside the home folder: never one beside it, and never through a path that leaves it.
    #[test]
    fn no_folder_is_made_outside_the_home_folder() {
        // None of the usual folders: the PATH's own order, and the first two aren't the home's.
        let made = machine(&["missing/bin", "home/../outside/bin", "home/tools/bin"]);
        let link = made.dir("home").join("tools").join("bin").join(COMMAND);

        assert_eq!(made.places.install(), Ok(link));
        assert!(!made.dir("missing").exists() && !made.dir("outside").exists());

        // With no folder that takes it there is no command, and nothing was made for it.
        let bare = machine(&["missing/bin"]);
        assert_eq!(bare.places.install(), Err(Refused::NoFolder));
        assert!(!bare.dir("missing").exists());
        assert!(!bare.dir("home").join("bin").exists() && !bare.dir("home").join(".local").exists(), "not on the PATH: not made");
    }

    /// The app was moved after the command was installed (run from the disk image, then
    /// dragged to Applications): the link it left leads nowhere now, and installing puts it
    /// right. Only that link: one that leads nowhere to another app's executable, and one to a
    /// copy of this app that is still there, are somebody else's and stay as they are.
    #[test]
    fn only_a_link_this_app_left_when_it_moved_is_put_right() {
        {
            let mut machine = machine(&["first"]);
            let link = machine.dir("first").join(COMMAND);
            assert_eq!(machine.places.install(), Ok(link.clone()));

            let moved = machine.root.join("Applications").join("Pitwall.app");
            fs::create_dir_all(moved.parent().unwrap()).unwrap();
            fs::rename(machine.root.join("Pitwall.app"), &moved).unwrap();
            machine.places.app = moved.join("Contents").join("MacOS").join("pitwall");

            assert_eq!(machine.places.found(), None, "it leads to where the app was");
            assert_eq!(machine.places.install(), Ok(link.clone()));
            assert_eq!(fs::read_link(&link).unwrap(), machine.places.app);
        }

        type Plant = fn(&Machine, &Path);
        let theirs: [(&str, Plant); 2] = [
            ("a link that leads nowhere, to another app's executable", |machine, link| {
                symlink(machine.dir("Other.app").join("Contents").join("MacOS").join("other"), link).unwrap();
            }),
            ("a link to a copy of this app that is still there", |machine, link| {
                let copy = machine.dir("Downloads").join("Pitwall.app").join("Contents").join("MacOS").join("pitwall");
                fs::create_dir_all(copy.parent().unwrap()).unwrap();
                fs::write(&copy, "the app, as it was downloaded").unwrap();
                symlink(copy, link).unwrap();
            }),
        ];
        for (what, plant) in theirs {
            let machine = machine(&["first"]);
            let link = machine.dir("first").join(COMMAND);
            plant(&machine, &link);
            let led = fs::read_link(&link).unwrap();

            assert_eq!(machine.places.install(), Err(Refused::Taken(link.clone())), "{what}");
            assert_eq!(fs::read_link(&link).unwrap(), led, "{what}: replaced");
        }
    }

    /// A PATH folder that is in the home folder by its text, under a link that leads out of
    /// it: the missing folder would be made where the link leads.
    #[test]
    fn no_folder_is_made_outside_the_home_folder_through_a_link() {
        let machine = machine(&["home/dotfiles/bin"]);
        fs::create_dir_all(machine.dir("outside")).unwrap();
        symlink(machine.dir("outside"), machine.dir("home").join("dotfiles")).unwrap();

        assert_eq!(machine.places.install(), Err(Refused::NoFolder));
        assert!(!machine.dir("outside").join("bin").exists());
    }
}
