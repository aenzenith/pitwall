//! Processes at the operating-system level: a login shell in its own process group, its output
//! read as lines, signals to a whole group, and whether a process or a group still exists.
//! Supervision on top of this (one start at a time, crash restarts) lives in `supervise.rs`.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// An output line longer than this is cut there; the rest comes as the next line.
pub const LINE_MAX: usize = 16 * 1024;

#[cfg(unix)]
pub fn spawn_shell(command: &str, cwd: &str) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;

    // A login shell so PATH (nvm, Herd, Homebrew) matches the terminal; its own process group
    // so stopping it takes the `vite` under `npm` down too.
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".into());

    Command::new(shell)
        .args(["-l", "-c", command])
        .current_dir(cwd)
        .env("FORCE_COLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
}

#[cfg(windows)]
pub fn spawn_shell(command: &str, cwd: &str) -> std::io::Result<Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    Command::new("cmd")
        .args(["/D", "/S", "/C", command])
        .current_dir(cwd)
        .env("FORCE_COLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
}

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

#[cfg(windows)]
pub fn kill_tree(pid: u32, _force: bool) -> bool {
    use std::os::windows::process::CommandExt;
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000)
        .status()
        .is_ok_and(|s| s.success())
}

/// Some process of the group can still be signalled. A group left with only its unreaped
/// leader doesn't count: macOS answers `EPERM` for it, and there is nothing left to stop.
#[cfg(unix)]
pub fn group_alive(pgid: u32) -> bool {
    // SAFETY: signal 0 only checks that the group exists.
    is_group_id(pgid) && unsafe { libc::killpg(pgid as libc::pid_t, 0) == 0 }
}

/// `taskkill /T /F` waits for the tree to go, so nothing is left after it.
#[cfg(windows)]
pub fn group_alive(_pgid: u32) -> bool {
    false
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

/// Without a cheap check, a process counts as alive: nothing of it is ever reaped.
#[cfg(windows)]
pub fn process_alive(_pid: u32) -> bool {
    true
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

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
pub fn booted_at() -> Option<u64> {
    None
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

