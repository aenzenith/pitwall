//! The speech models a voice message is written with: which there are, where they live, and their
//! one download. A file is fetched into `<name>.part`, hashed as it comes, and renamed into place
//! only when it is whole and its SHA-256 is the one written here; anything else is deleted unused.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The two speech models the user picks between (Settings › Terminal › Sohbet › Sesli mesaj).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Model {
    /// whisper small, q5_1: the default.
    Small,
    /// whisper large-v3-turbo, q5_0: hears terms better, writes more slowly.
    Large,
}

/// A file of the catalog: its name, its size and its SHA-256, and the pinned revision it is also
/// on at Hugging Face.
#[derive(Debug)]
pub struct File {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub source: &'static str,
}

/// Pitwall's own copy, a release of the app's repository; the pinned Hugging Face revision is
/// asked when it isn't there. Either way only the hash below makes a file usable.
const MIRROR: &str = "https://github.com/aenzenith/pitwall/releases/download/voice-models-1/";
const WHISPER: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/";
const SILERO: &str = "https://huggingface.co/ggml-org/whisper-vad/resolve/9ffd54a1e1ee413ddf265af9913beaf518d1639b/";

pub const SMALL: File = File {
    name: "ggml-small-q5_1.bin",
    bytes: 190_085_487,
    sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    source: WHISPER,
};
pub const LARGE: File = File {
    name: "ggml-large-v3-turbo-q5_0.bin",
    bytes: 574_041_195,
    sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
    source: WHISPER,
};
/// The speech gate (Silero VAD): whether a take holds speech at all, asked before whisper is.
pub const GATE: File = File {
    name: "ggml-silero-v5.1.2.bin",
    bytes: 885_098,
    sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
    source: SILERO,
};

impl Model {
    pub const ALL: [Model; 2] = [Model::Small, Model::Large];

    pub fn parse(word: &str) -> Option<Model> {
        match word {
            "small" => Some(Model::Small),
            "large" => Some(Model::Large),
            _ => None,
        }
    }

    pub fn file(self) -> &'static File {
        match self {
            Model::Small => &SMALL,
            Model::Large => &LARGE,
        }
    }
}

pub fn path(dir: &Path, file: &File) -> PathBuf {
    dir.join(file.name)
}

/// Whether a file is there whole: a regular file of the catalog's size (its hash was checked as
/// it came in, and nothing but this module writes the folder).
pub fn present(dir: &Path, file: &File) -> bool {
    fs::symlink_metadata(path(dir, file)).is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
}

/// Whether a voice message can be written with the model: it and the gate are both there.
pub fn ready(dir: &Path, model: Model) -> bool {
    present(dir, &GATE) && present(dir, model.file())
}

/// What a download of the model still has to fetch, the gate first.
pub fn missing(dir: &Path, model: Model) -> Vec<&'static File> {
    [&GATE, model.file()].into_iter().filter(|file| !present(dir, file)).collect()
}

/// How a fetch ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Fetch {
    Done,
    Cancelled,
    Failed(&'static str),
}

/// Copies `from` into `dest` through `dest` + `.part`, hashing as it goes: renamed into place only
/// when exactly `file.bytes` came and their SHA-256 is `file.sha256`; otherwise, cancelled or
/// broken off, the part is deleted and nothing is left at `dest`.
pub fn write_verified(from: &mut impl Read, file: &File, dest: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Fetch {
    let part = part_of(dest);
    let ended = copy_hashed(from, file, &part, cancel, progress);
    match ended {
        Fetch::Done => match fs::rename(&part, dest) {
            Ok(()) => Fetch::Done,
            Err(_) => {
                let _ = fs::remove_file(&part);
                Fetch::Failed("write")
            }
        },
        other => {
            let _ = fs::remove_file(&part);
            other
        }
    }
}

fn part_of(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

fn copy_hashed(from: &mut impl Read, file: &File, part: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Fetch {
    let mut out = match open_private(part) {
        Ok(out) => out,
        Err(_) => return Fetch::Failed("write"),
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Fetch::Cancelled;
        }
        let read = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Fetch::Failed("network"),
        };
        done += read as u64;
        if done > file.bytes {
            return Fetch::Failed("size");
        }
        if out.write_all(&buffer[..read]).is_err() {
            return Fetch::Failed("write");
        }
        hasher.update(&buffer[..read]);
        progress(done);
    }
    if done != file.bytes {
        return Fetch::Failed("size");
    }
    if out.flush().is_err() || out.sync_all().is_err() {
        return Fetch::Failed("write");
    }
    if hex(&hasher.finalize()) != file.sha256 {
        return Fetch::Failed("hash");
    }
    Fetch::Done
}

fn open_private(part: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(part)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Fetches a file of the catalog into `dir`: Pitwall's own copy first, else the pinned Hugging
/// Face revision. Nothing of the user's is sent: a plain GET of a fixed address.
pub fn download(dir: &Path, file: &File, cancel: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Fetch {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

    if fs::create_dir_all(dir).is_err() {
        return Fetch::Failed("write");
    }
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(60 * 60)))
        .http_status_as_error(false)
        .user_agent(concat!("Pitwall/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let dest = path(dir, file);
    let mut last = Fetch::Failed("network");
    for base in [MIRROR, file.source] {
        let url = format!("{base}{}", file.name);
        let response = match agent.get(&url).call() {
            Ok(response) => response,
            Err(_) => {
                last = Fetch::Failed("network");
                continue;
            }
        };
        if response.status().as_u16() != 200 {
            last = Fetch::Failed("http");
            continue;
        }
        let mut reader = response.into_body().into_reader();
        match write_verified(&mut reader, file, &dest, cancel, progress) {
            Fetch::Done => return Fetch::Done,
            Fetch::Cancelled => return Fetch::Cancelled,
            failed => last = failed,
        }
    }
    last
}

/// Deletes a model's file, and the gate once no model is left.
pub fn delete(dir: &Path, model: Model) {
    let _ = fs::remove_file(path(dir, model.file()));
    let _ = fs::remove_file(part_of(&path(dir, model.file())));
    if Model::ALL.iter().all(|each| !present(dir, each.file())) {
        let _ = fs::remove_file(path(dir, &GATE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn file_of(bytes: &[u8]) -> File {
        let sha = hex(&Sha256::digest(bytes));
        File { name: "test.bin", bytes: bytes.len() as u64, sha256: Box::leak(sha.into_boxed_str()), source: "" }
    }

    // A model is code-like input to whisper.cpp: a file that isn't the one whose hash is pinned must
    // never be left where `ready` would take it, whole or in part.
    #[test]
    fn only_a_file_of_the_pinned_hash_is_put_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.bin");
        let good = b"the right model".to_vec();
        let file = file_of(&good);
        let never = AtomicBool::new(false);

        let mut wrong = good.clone();
        wrong[0] ^= 1;
        let ended = write_verified(&mut Cursor::new(wrong), &file, &dest, &never, &mut |_| {});
        assert_eq!(ended, Fetch::Failed("hash"));
        assert!(!dest.exists());
        assert!(!part_of(&dest).exists());

        let ended = write_verified(&mut Cursor::new(good[..5].to_vec()), &file, &dest, &never, &mut |_| {});
        assert_eq!(ended, Fetch::Failed("size"));
        assert!(!dest.exists() && !part_of(&dest).exists());

        let mut longer = good.clone();
        longer.push(b'!');
        let ended = write_verified(&mut Cursor::new(longer), &file, &dest, &never, &mut |_| {});
        assert_eq!(ended, Fetch::Failed("size"));
        assert!(!dest.exists() && !part_of(&dest).exists());

        let cancelled = AtomicBool::new(true);
        let ended = write_verified(&mut Cursor::new(good.clone()), &file, &dest, &cancelled, &mut |_| {});
        assert_eq!(ended, Fetch::Cancelled);
        assert!(!dest.exists() && !part_of(&dest).exists());

        let ended = write_verified(&mut Cursor::new(good.clone()), &file, &dest, &never, &mut |_| {});
        assert_eq!(ended, Fetch::Done);
        assert_eq!(fs::read(&dest).unwrap(), good);
        assert!(!part_of(&dest).exists());
    }
}
