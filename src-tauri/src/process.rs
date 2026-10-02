//! Processes at the operating-system level: a login shell whose whole tree can be stopped (its
//! own process group on macOS and Linux, a job object on Windows), its output read as lines,
//! signals to a whole tree, and whether a process or a tree still exists. Also what else differs
//! per platform: the process table, when a process started and the system booted, programs found
//! on `PATH`, and child processes that never open a console window on Windows.
//! Supervision on top of this (one start at a time, crash restarts) lives in `supervise.rs`.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// An output line longer than this is cut there; the rest comes as the next line.
pub const LINE_MAX: usize = 16 * 1024;

/// A command for `program`. Every process the app starts goes through here (or `spawn_shell`):
/// on Windows it never opens a console window.
#[cfg(not(windows))]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

#[cfg(windows)]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    use std::os::windows::process::CommandExt;

    let mut command = Command::new(program);
    command.creation_flags(win::CREATE_NO_WINDOW);
    command
}

/// The user's login shell: `$SHELL`, else the system's own (zsh on macOS, sh elsewhere).
#[cfg(unix)]
fn login_shell() -> String {
    let fallback = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" };
    std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| fallback.into())
}

/// Runs `command_line` in `cwd` through the login shell, so PATH (nvm, Herd, Homebrew) matches the
/// terminal. `-l` and `-c` go as two arguments, which fish takes too. The shell leads a process
/// group of its own, so stopping it takes the `vite` under `npm` down too.
#[cfg(unix)]
pub fn spawn_shell(command_line: &str, cwd: &str) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;

    command(login_shell())
        .args(["-l", "-c", command_line])
        .current_dir(cwd)
        .env("FORCE_COLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
}

/// Runs `command_line` in `cwd` as `cmd.exe /D /S /C "<command_line>"` (`npm` is `npm.cmd`),
/// without a console window, in a job object of its own: stopping ends the job and so the whole
/// tree, and the job ends with the app however the app ends. The shell starts suspended and runs
/// only once it is in the job, so nothing it starts slips out. Without a job (one couldn't be
/// made) the tree is stopped with `taskkill /T`.
#[cfg(windows)]
pub fn spawn_shell(command_line: &str, cwd: &str) -> std::io::Result<Child> {
    use std::os::windows::process::CommandExt;

    let mut child = command(win::comspec())
        .args(["/D", "/S", "/C"])
        .raw_arg(cmd_argument(command_line))
        .current_dir(cwd)
        .env("FORCE_COLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(win::CREATE_NO_WINDOW | win::CREATE_SUSPENDED)
        .spawn()?;

    if let Err(error) = win::start_in_job(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(child)
}

/// The command line as `cmd.exe /S /C` takes it: in quotes, which it strips again, so quotes
/// inside the command (`node -e "…"`) reach it untouched.
#[cfg(any(windows, test))]
fn cmd_argument(command_line: &str) -> String {
    format!("\"{command_line}\"")
}

/// Puts a process that is already running (a terminal's shell) in a job of its own on Windows,
/// so ending it ends whatever it starts later too. Elsewhere it already leads its own process
/// group.
#[cfg(windows)]
pub fn contain(pid: u32) {
    win::contain(pid);
}

#[cfg(not(windows))]
pub fn contain(_pid: u32) {}

/// A pid that can name a process group we started: never 0 (our own group) or 1 (launchd's),
/// and never one that turns negative as a `pid_t` (which would signal a single process).
#[cfg(unix)]
fn is_group_id(pid: u32) -> bool {
    pid > 1 && pid <= i32::MAX as u32
}

/// Signals the whole process tree started for a server.
#[cfg(unix)]
pub fn kill_tree(pid: u32, force: bool) -> bool {
    if !is_group_id(pid) {
        return false;
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: killpg only sends a signal; the group id is the leader pid we spawned.
    unsafe { libc::killpg(pid as libc::pid_t, signal) == 0 }
}

/// Ends the whole tree started for a server: its job, at once (console programs on Windows
/// don't take a polite stop). A tree without a job of ours goes through `taskkill /T /F`, and
/// only while its root still is a `cmd.exe`: Windows hands out pids again quickly.
#[cfg(windows)]
pub fn kill_tree(pid: u32, _force: bool) -> bool {
    if let Some(ended) = win::terminate(pid) {
        return ended;
    }
    is_cmd(pid)
        && command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

/// Some process of the group can still be signalled. A group left with only its unreaped
/// leader doesn't count: macOS answers `EPERM` for it, and there is nothing left to stop.
#[cfg(unix)]
pub fn group_alive(pgid: u32) -> bool {
    // SAFETY: signal 0 only checks that the group exists.
    is_group_id(pgid) && unsafe { libc::killpg(pgid as libc::pid_t, 0) == 0 }
}

/// Some process of the tree's job still runs; a tree without a job of ours counts as alive while
/// its root does.
#[cfg(windows)]
pub fn group_alive(pid: u32) -> bool {
    win::job_alive(pid).unwrap_or_else(|| process_alive(pid))
}

/// The process exists (another user's, or one not reaped yet, counts too).
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The process runs (another user's, which can't be opened, counts too).
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    win::process_alive(pid)
}

/// Whether a group a dead participant left behind may be reaped. On Windows only while its pid
/// still belongs to `cmd.exe`, the shell servers run in, as pids are handed out again quickly
/// there (see PROTOCOL.md); elsewhere a group's id isn't reused while a member lives.
pub fn may_reap(pid: u32) -> bool {
    #[cfg(windows)]
    return is_cmd(pid);

    #[cfg(not(windows))]
    {
        let _ = pid;
        true
    }
}

/// The process runs `cmd.exe`.
#[cfg(windows)]
fn is_cmd(pid: u32) -> bool {
    win::image(pid).is_some_and(|image| is_cmd_image(&image))
}

/// An executable's path names `cmd.exe`, in any letter case.
#[cfg(any(windows, test))]
fn is_cmd_image(image: &str) -> bool {
    image.rsplit(['\\', '/']).next().is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
}

/// When the process started, in ms since the epoch; `None` when it is gone or that can't be
/// told.
#[cfg(target_os = "macos")]
pub fn started_at(pid: u32) -> Option<u64> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    // SAFETY: a plain C struct; all zeroes is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
    let written = unsafe {
        libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
    };
    (written == size).then(|| info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000)
}

/// From `/proc/<pid>/stat` (clock ticks after boot) and the boot time; to the second, as the
/// boot time is.
#[cfg(target_os = "linux")]
pub fn started_at(pid: u32) -> Option<u64> {
    let ticks = stat_start_ticks(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
    // SAFETY: sysconf only reads a configuration value.
    let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let per_second = u64::try_from(per_second).ok().filter(|n| *n > 0)?;
    Some(booted_at()? + ticks * 1000 / per_second)
}

#[cfg(windows)]
pub fn started_at(pid: u32) -> Option<u64> {
    win::started_at(pid)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn started_at(_pid: u32) -> Option<u64> {
    None
}

/// When the system last booted, in ms since the epoch.
#[cfg(target_os = "macos")]
pub fn booted_at() -> Option<u64> {
    // SAFETY: a plain C struct; all zeroes is a valid value.
    let mut boot: libc::timeval = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::timeval>();
    let mut name = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    // SAFETY: sysctl writes at most `size` bytes into `boot`.
    let result = unsafe {
        libc::sysctl(name.as_mut_ptr(), 2, (&mut boot as *mut libc::timeval).cast(), &mut size, std::ptr::null_mut(), 0)
    };
    (result == 0 && boot.tv_sec > 0).then(|| boot.tv_sec as u64 * 1000 + boot.tv_usec as u64 / 1000)
}

/// The `btime` line of `/proc/stat`.
#[cfg(target_os = "linux")]
pub fn booted_at() -> Option<u64> {
    boot_seconds(&std::fs::read_to_string("/proc/stat").ok()?).map(|seconds| seconds * 1000)
}

#[cfg(windows)]
pub fn booted_at() -> Option<u64> {
    win::booted_at()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn booted_at() -> Option<u64> {
    None
}

/// The `starttime` field of `/proc/<pid>/stat`, in clock ticks after boot. The command name
/// before it is in parentheses and may hold spaces and parentheses itself, so fields are counted
/// from the last `)`: `starttime` is the 20th after it.
#[cfg(any(target_os = "linux", test))]
fn stat_start_ticks(stat: &str) -> Option<u64> {
    let (_, fields) = stat.rsplit_once(')')?;
    fields.split_whitespace().nth(19)?.parse().ok()
}

/// The boot time in `/proc/stat`, in seconds since the epoch.
#[cfg(any(target_os = "linux", test))]
fn boot_seconds(stat: &str) -> Option<u64> {
    stat.lines().find_map(|line| line.strip_prefix("btime "))?.trim().parse().ok()
}

/// One process of `process_table`. Its command line stays in memory: callers only test it, and
/// never show or keep it (a `claude "…"` carries the prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub parent: u32,
    /// The command line; the executable's name when it can't be read.
    pub line: String,
    /// The executable's name (`iTerm2`, `tmux`, `WindowsTerminal.exe`).
    pub name: String,
    /// The program as it was started (the first argument), empty when it can't be read.
    pub program: String,
}

/// Every process's parent, command line and name, by pid; `None` when the table can't be read.
pub fn process_table() -> Option<HashMap<u32, ProcessInfo>> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).without_tasks());

    let table: HashMap<u32, ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let parent = process.parent().map_or(0, |parent| parent.as_u32());
            let args: Vec<String> = process.cmd().iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
            let name = process.name().to_string_lossy().into_owned();
            let line = if args.is_empty() { name.clone() } else { args.join(" ") };
            let program = args.into_iter().next().unwrap_or_default();
            (pid.as_u32(), ProcessInfo { parent, line, name, program })
        })
        .collect();

    (!table.is_empty()).then_some(table)
}

/// Where `name` is on `PATH`: on Windows as `name.exe`, `.cmd` or `.bat` (the launchers editors
/// install are `.cmd` files), elsewhere under its own name. Relative entries are skipped.
#[cfg(not(target_os = "macos"))]
pub fn find_program(name: &str) -> Option<std::path::PathBuf> {
    let extensions: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    let path = std::env::var_os("PATH")?;

    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| extensions.iter().map(move |extension| dir.join(format!("{name}{extension}"))))
        .find(|file| file.is_file())
}

/// Whether `exe` is an installed app rather than a development build: inside an `.app` on
/// macOS; on Linux under `/usr` or `/opt`, or an AppImage (`appimage`, its `$APPIMAGE`); on
/// Windows anywhere but a cargo `target` folder (Program Files, `%LOCALAPPDATA%\Programs`).
pub fn installed_app(exe: &Path, appimage: bool) -> bool {
    if cfg!(target_os = "macos") {
        return exe.to_string_lossy().contains(".app/Contents/MacOS");
    }
    if cfg!(target_os = "linux") {
        return appimage || exe.starts_with("/usr") || exe.starts_with("/opt");
    }
    !cargo_build(exe)
}

/// `exe` lies in a cargo `target` folder: `target/debug`, `target/release`, or the same under a
/// target triple.
fn cargo_build(exe: &Path) -> bool {
    let names: Vec<String> = exe.components().map(|part| part.as_os_str().to_string_lossy().to_lowercase()).collect();

    names.iter().enumerate().any(|(at, name)| {
        (name == "debug" || name == "release") && ((at >= 1 && names[at - 1] == "target") || (at >= 2 && names[at - 2] == "target"))
    })
}

/// Ends whole process groups: SIGTERM, then SIGKILL for any still there after `grace`, then up
/// to `kill_wait` for those to go too. True once no group has a process left.
pub fn stop_groups(pgids: &[u32], grace: Duration, kill_wait: Duration) -> bool {
    let all_gone = || pgids.iter().all(|&pgid| !group_alive(pgid));

    for &pgid in pgids {
        kill_tree(pgid, false);
    }
    if wait_until(grace, all_gone) {
        return true;
    }

    for &pgid in pgids.iter().filter(|&&pgid| group_alive(pgid)) {
        kill_tree(pgid, true);
    }
    wait_until(kill_wait, all_gone)
}

pub fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;

    while Instant::now() < deadline {
        if done() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }

    done()
}

/// Reads `stream` to its end on a thread of its own, handing over each line. A line ends at
/// `\n` (a `\r` before it is dropped). Text after a lone `\r` replaces the line so far, as on a
/// terminal, so a progress bar that redraws one line never makes it grow; and a line that
/// reaches `LINE_MAX` is handed over there.
pub fn read_lines(stream: impl Read + Send + 'static, mut line: impl FnMut(String) + Send + 'static) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut current: Vec<u8> = Vec::new();
        let mut carriage = false;

        loop {
            let chunk = match reader.fill_buf() {
                Ok([]) => break,
                Ok(chunk) => chunk,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            let read = chunk.len();

            for &byte in chunk {
                match byte {
                    b'\n' => {
                        line(String::from_utf8_lossy(&current).into_owned());
                        current.clear();
                        carriage = false;
                    }
                    b'\r' => carriage = true,
                    _ => {
                        if carriage {
                            current.clear();
                            carriage = false;
                        }
                        current.push(byte);
                        if current.len() >= LINE_MAX {
                            line(String::from_utf8_lossy(&current).into_owned());
                            current.clear();
                        }
                    }
                }
            }

            reader.consume(read);
        }

        if !current.is_empty() {
            line(String::from_utf8_lossy(&current).into_owned());
        }
    });
}

/// Windows: job objects per tree, and what is asked of single processes.
#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::process::Child;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, OpenThread, QueryFullProcessImageNameW, ResumeThread, WaitForSingleObject, PROCESS_ACCESS_RIGHTS,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        THREAD_SUSPEND_RESUME,
    };

    pub use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

    /// 1601-01-01 (where `FILETIME` counts from) to the Unix epoch, in 100 ns units.
    const EPOCH_GAP: u64 = 116_444_736_000_000_000;

    /// A job object that ends its processes when its last handle closes: when the app ends, even
    /// by a crash, nothing it started stays behind.
    struct Job(OwnedHandle);

    impl Job {
        fn new() -> io::Result<Self> {
            // SAFETY: no name and default security; a null handle means failure.
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: a fresh handle that nothing else owns.
            let job = Job(unsafe { OwnedHandle::from_raw_handle(raw) });

            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the struct and its size match the information class.
            let set = unsafe {
                SetInformationJobObject(
                    job.raw(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }

        fn raw(&self) -> HANDLE {
            self.0.as_raw_handle()
        }

        fn assign(&self, process: HANDLE) -> io::Result<()> {
            // SAFETY: both handles are open for the duration of the call.
            if unsafe { AssignProcessToJobObject(self.raw(), process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn terminate(&self) -> bool {
            // SAFETY: the job handle is open.
            unsafe { TerminateJobObject(self.raw(), 1) != 0 }
        }

        /// How many of its processes still run; `None` when that can't be read.
        fn active(&self) -> Option<u32> {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: the struct and its size match the information class.
            let read = unsafe {
                QueryInformationJobObject(
                    self.raw(),
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (read != 0).then_some(info.ActiveProcesses)
        }
    }

    /// A tree we started, by its root's pid.
    enum Tree {
        Running(Job),
        /// Everything of it has ended; its job is closed. Kept so that its pid, which Windows
        /// may hand to another program now, is never stopped as ours.
        Ended,
    }

    static TREES: LazyLock<Mutex<HashMap<u32, Tree>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

    fn trees() -> MutexGuard<'static, HashMap<u32, Tree>> {
        TREES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes the tree of `pid` on. One of ours that still holds the pid (its root is gone, the
    /// rest lingers) goes now, as its job closes.
    fn track(pid: u32, job: Job) {
        trees().insert(pid, Tree::Running(job));
    }

    /// `cmd.exe`, as `%ComSpec%` names it.
    pub fn comspec() -> OsString {
        std::env::var_os("ComSpec").filter(|value| !value.is_empty()).unwrap_or_else(|| "cmd.exe".into())
    }

    /// Puts a suspended child in a new job, then lets it run. Only a child that can't be let run
    /// is an error; one without a job runs untracked.
    pub fn start_in_job(child: &Child) -> io::Result<()> {
        let job = Job::new().and_then(|job| job.assign(child.as_raw_handle()).map(|()| job));
        resume(child.id())?;
        if let Ok(job) = job {
            track(child.id(), job);
        }
        Ok(())
    }

    pub fn contain(pid: u32) {
        let Some(process) = open(pid, PROCESS_SET_QUOTA | PROCESS_TERMINATE) else {
            return;
        };
        if let Ok(job) = Job::new().and_then(|job| job.assign(process.as_raw_handle()).map(|()| job)) {
            track(pid, job);
        }
    }

    /// Ends the tree of `pid`: `Some(ended)` for a tree of ours, `None` for a pid we hold no
    /// job for.
    pub fn terminate(pid: u32) -> Option<bool> {
        match trees().get(&pid)? {
            Tree::Running(job) => Some(job.terminate()),
            Tree::Ended => Some(false),
        }
    }

    /// Whether anything of the tree of `pid` runs; `None` for a pid we hold no job for. A job
    /// found empty is closed.
    pub fn job_alive(pid: u32) -> Option<bool> {
        let mut trees = trees();
        let alive = match trees.get(&pid)? {
            Tree::Running(job) => job.active().is_some_and(|count| count > 0),
            Tree::Ended => false,
        };
        if !alive {
            trees.insert(pid, Tree::Ended);
        }
        Some(alive)
    }

    fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
        if pid == 0 {
            return None;
        }
        // SAFETY: a null handle means failure.
        let raw = unsafe { OpenProcess(access, 0, pid) };
        // SAFETY: a fresh handle that nothing else owns.
        (!raw.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(raw) })
    }

    pub fn process_alive(pid: u32) -> bool {
        match open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE) {
            // SAFETY: waits zero ms on an open process handle.
            Some(process) => unsafe { WaitForSingleObject(process.as_raw_handle(), 0) == WAIT_TIMEOUT },
            None => pid != 0 && io::Error::last_os_error().raw_os_error() == Some(ERROR_ACCESS_DENIED as i32),
        }
    }

    /// The path of the program the process runs.
    pub fn image(pid: u32) -> Option<String> {
        let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let mut buffer = vec![0u16; 32_768];
        let mut size = buffer.len() as u32;
        // SAFETY: the buffer holds `size` UTF-16 units; `size` comes back as the length written.
        let read = unsafe { QueryFullProcessImageNameW(process.as_raw_handle(), PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size) };
        (read != 0).then(|| OsString::from_wide(&buffer[..size as usize]).to_string_lossy().into_owned())
    }

    pub fn started_at(pid: u32) -> Option<u64> {
        let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: four FILETIMEs to fill, on an open process handle.
        let read = unsafe { GetProcessTimes(process.as_raw_handle(), created, exited, kernel, user) };
        if read == 0 {
            return None;
        }
        let since_1601 = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        since_1601.checked_sub(EPOCH_GAP).map(|since_epoch| since_epoch / 10_000)
    }

    /// Now less the time since boot (which counts sleep too).
    pub fn booted_at() -> Option<u64> {
        // SAFETY: no arguments; reads a counter.
        let uptime = unsafe { GetTickCount64() };
        crate::registry::now_ms().checked_sub(uptime)
    }

    /// Lets every thread of a process started suspended run (it has one: its first).
    fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: a snapshot of every thread; INVALID_HANDLE_VALUE means failure.
        let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if raw == INVALID_HANDLE_VALUE || raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a fresh handle that nothing else owns.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };

        let mut entry = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut resumed = false;
        // SAFETY: `entry` has its size set, as the API asks.
        let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } != 0;

        while more {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: a null handle means failure.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // SAFETY: a fresh handle that nothing else owns.
                    let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
                    // SAFETY: an open thread handle with THREAD_SUSPEND_RESUME.
                    resumed |= unsafe { ResumeThread(thread.as_raw_handle()) } != u32::MAX;
                }
            }
            // SAFETY: as for Thread32First.
            more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } != 0;
        }

        if resumed {
            Ok(())
        } else {
            Err(io::Error::other("the started process could not be resumed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What runs a server on Windows: the command reaches `cmd.exe /S /C` whole, quotes and all;
    /// and only a `cmd.exe` is ever reaped or killed by pid there.
    #[test]
    fn windows_runs_the_command_line_whole_and_kills_only_its_shell() {
        assert_eq!(cmd_argument("npm run dev -- --port 5174"), r#""npm run dev -- --port 5174""#);
        assert_eq!(cmd_argument(r#"node -e "process.exit(3)""#), r#""node -e "process.exit(3)"""#);

        assert!(is_cmd_image(r"C:\Windows\System32\cmd.exe"));
        assert!(is_cmd_image(r"C:\WINDOWS\system32\CMD.EXE"));
        for other in [r"C:\Program Files\nodejs\node.exe", r"C:\tools\notcmd.exe", r"C:\cmd.exe\node.exe", ""] {
            assert!(!is_cmd_image(other), "{other}");
        }
    }

    /// The pid-reuse guard on Linux reads start and boot times from `/proc`; a command name with
    /// spaces and parentheses must not shift the fields.
    #[test]
    fn linux_start_and_boot_times_are_read_from_proc() {
        let stat = "4242 (node (dev) x) S 1 4242 4242 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 987654 0 0";
        assert_eq!(stat_start_ticks(stat), Some(987_654));
        assert_eq!(stat_start_ticks("4242 (node"), None);
        assert_eq!(boot_seconds("cpu  1 2 3\nintr 5\nbtime 1790000000\nprocesses 9\n"), Some(1_790_000_000));
        assert_eq!(boot_seconds("cpu 1\n"), None);
    }
}
