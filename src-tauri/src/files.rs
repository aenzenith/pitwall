//! A project's own files read without taking them on trust: its `.env` can be a named pipe
//! (a secrets manager hands it over that way, and `open` waits for a writer that may never
//! come), its `package.json` a link to `/dev/zero` (which never ends), and any of them as large
//! as a disk. Only a regular file is read, and only up to a bound.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

/// A `.env` larger than this isn't one a person wrote.
pub const ENV_BYTES: u64 = 1024 * 1024;
/// Nor a `package.json`.
pub const MANIFEST_BYTES: u64 = 1024 * 1024;

/// The file's bytes when it is a regular file of at most `max` bytes; None for anything else
/// (missing, unreadable, a folder, a pipe, a device, larger). Whether it is a regular file is
/// asked once more of the file as opened, so one swapped in meanwhile isn't read either; on
/// unix it is opened without waiting (`O_NONBLOCK`), so a pipe put there in between is turned
/// down at once too. It is read through `take`, so a file that grows while it is read still
/// stops at the bound.
pub fn read_regular(path: &Path, max: u64) -> Option<Vec<u8>> {
    let regular = |metadata: fs::Metadata| metadata.is_file() && metadata.len() <= max;
    if !regular(fs::metadata(path).ok()?) {
        return None;
    }

    let file = open(path).ok()?;
    if !regular(file.metadata().ok()?) {
        return None;
    }

    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

/// `read_regular`, when the file is UTF-8 text.
pub fn read_regular_string(path: &Path, max: u64) -> Option<String> {
    String::from_utf8(read_regular(path, max)?).ok()
}

/// Opened for reading without waiting on a pipe, and never as the controlling terminal. Only
/// the opening doesn't wait: what is read (a regular file only) is read as any file is, on
/// whatever file system it lives.
#[cfg(unix)]
fn open(path: &Path) -> std::io::Result<File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let file = fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY).open(path)?;
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags != -1 {
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) };
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open(path: &Path) -> std::io::Result<File> {
    File::open(path)
}
