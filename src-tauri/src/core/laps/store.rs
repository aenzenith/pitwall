//! The laps' files beside `settings.json`: `laps.json` (the jobs), `lap-runs/<lap>.json` (each
//! job's kept runs, no text) and `lap-runs/<lap>/<run>.log` (a script's last lines). All are
//! written atomically and readable by the user alone, since prompts, scripts and their output are
//! the user's text; a `laps.json` that doesn't parse is moved aside (`laps.json.broken-<ms>`) and
//! never written over, as `board.json` is.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::types::{Lap, LapRun};
use crate::core::output::MARKER as MARK;
use crate::core::restore::write_private;
use crate::resolve::strip_ansi;
use crate::settings::set_aside;

/// A script's output kept per run: its last lines, at most this many bytes.
pub const OUTPUT_LINES: usize = 200;
pub const OUTPUT_BYTES: usize = 64 * 1024;
const FILE_VERSION: u32 = 1;
const LAPS_FILE: &str = "laps.json";
const RUNS_DIR: &str = "lap-runs";

/// An id the core gives (`l-…`, `r-…`): letters, digits and dashes, so it can name a file. One a
/// file was written with by hand names none, and its job isn't read.
pub(super) fn own_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[derive(Default)]
pub struct Store {
    pub dir: PathBuf,
    /// False when a `laps.json` that couldn't be used couldn't be set aside either: nothing is
    /// written over it in this run.
    writable: bool,
}

impl Store {
    /// Beside `settings_file`.
    pub fn beside(settings_file: &Path) -> Self {
        Self { dir: settings_file.parent().map(Path::to_path_buf).unwrap_or_default(), writable: true }
    }

    /// In `dir`.
    pub fn at(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf(), writable: true }
    }

    pub fn laps_file(&self) -> PathBuf {
        self.dir.join(LAPS_FILE)
    }

    fn runs_file(&self, lap: &str) -> Option<PathBuf> {
        own_id(lap).then(|| self.dir.join(RUNS_DIR).join(format!("{lap}.json")))
    }

    fn output_dir(&self, lap: &str) -> Option<PathBuf> {
        own_id(lap).then(|| self.dir.join(RUNS_DIR).join(lap))
    }

    fn output_file(&self, lap: &str, run: &str) -> Option<PathBuf> {
        Some(self.output_dir(lap)?.join(format!("{}.log", own_id(run).then_some(run)?)))
    }

    /// The jobs. A file that isn't JSON or holds no list gives none; a job in it that doesn't fit
    /// (or has an id the core never gives, or twice) costs only itself. Such a file is moved aside
    /// (`laps.json.broken-<ms>`) only with `aside`, by the Pitwall that holds the lock, which then
    /// writes back the jobs that fit (`true`); without it nothing is moved, and nothing is written
    /// over the file from this store.
    pub fn load_laps(&mut self, aside: bool) -> (Vec<Lap>, bool) {
        self.writable = true;
        let file = self.laps_file();
        let mut unusable = |reason: &str| self.writable = aside && set_aside(&file, reason);
        let raw = match fs::read(&file) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return (Vec::new(), false),
            Err(error) => {
                unusable(&error.to_string());
                return (Vec::new(), false);
            }
        };
        let items = match serde_json::from_slice::<Value>(&raw) {
            Ok(value) => match value.get("laps").and_then(Value::as_array) {
                Some(items) => items.clone(),
                None => {
                    unusable("no list of laps");
                    return (Vec::new(), false);
                }
            },
            Err(error) => {
                unusable(&error.to_string());
                return (Vec::new(), false);
            }
        };

        let mut laps: Vec<Lap> = Vec::new();
        let mut left_out = 0;
        for item in items {
            match serde_json::from_value::<Lap>(item) {
                Ok(lap) if own_id(&lap.id) && !laps.iter().any(|kept| kept.id == lap.id) => laps.push(lap),
                _ => left_out += 1,
            }
        }
        if left_out > 0 {
            unusable(&format!("{left_out} unusable laps"));
        }
        let write_back = left_out > 0 && self.writable;
        (laps, write_back)
    }

    /// Whether the jobs could be written.
    pub fn save_laps(&self, laps: &[Lap]) -> bool {
        if !self.writable {
            return false;
        }
        let json = serde_json::json!({ "v": FILE_VERSION, "laps": laps });
        let written = serde_json::to_string_pretty(&json).map_err(|error| error.to_string()).and_then(|text| write_private(&self.laps_file(), &text).map_err(|error| error.to_string()));
        if let Err(error) = &written {
            eprintln!("[pitwall] {} could not be written: {error}", self.laps_file().display());
        }
        written.is_ok()
    }

    /// Every job's kept runs, oldest first. A file that can't be read gives none; a run in it
    /// that doesn't fit is left out.
    pub fn load_runs(&self, laps: &[Lap]) -> HashMap<String, Vec<LapRun>> {
        laps.iter()
            .filter_map(|lap| {
                let raw = fs::read(self.runs_file(&lap.id)?).ok()?;
                let items: Vec<Value> = serde_json::from_slice(&raw).ok()?;
                let runs: Vec<LapRun> = items.into_iter().filter_map(|item| serde_json::from_value::<LapRun>(item).ok()).filter(|run| own_id(&run.id) && run.lap == lap.id).collect();
                Some((lap.id.clone(), runs))
            })
            .collect()
    }

    /// Writes `lap`'s runs, keeping the last `keep` (at least one) and deleting the output of
    /// those dropped: the oldest that ended, never one that still goes. Whether they are on disk
    /// as they are now.
    pub fn save_runs(&self, lap: &str, runs: &mut Vec<LapRun>, keep: u32) -> bool {
        let Some(file) = self.runs_file(lap) else {
            return false;
        };
        let mut over = runs.len().saturating_sub(keep.max(1) as usize);
        runs.retain(|run| {
            if over == 0 || run.ended_at.is_none() {
                return true;
            }
            over -= 1;
            if let Some(output) = self.output_file(lap, &run.id) {
                let _ = fs::remove_file(output);
            }
            false
        });
        let written = serde_json::to_string_pretty(runs).map_err(|error| error.to_string()).and_then(|text| write_private(&file, &text).map_err(|error| error.to_string()));
        if let Err(error) = &written {
            eprintln!("[pitwall] {} could not be written: {error}", file.display());
        }
        written.is_ok()
    }

    /// Keeps a script's output for a run: the command line it ran (`command`, each of its lines
    /// marked with U+001E, as the core's own lines of an output are, so that nothing reading the
    /// output takes it for a line the script printed), then the last `OUTPUT_LINES`, ANSI
    /// stripped, at most `OUTPUT_BYTES`, mode 0600.
    pub fn save_output(&self, lap: &str, run: &str, command: &str, lines: &[String]) -> bool {
        let Some(file) = self.output_file(lap, run) else {
            return false;
        };
        let ran = command.lines().map(|line| format!("{MARK}{}", line.chars().filter(|c| *c != MARK).collect::<String>()));
        let text = ran.chain(kept_output(lines).into_iter().map(|line| line.replace(MARK, ""))).collect::<Vec<_>>().join("\n");
        if let Some(dir) = file.parent() {
            if fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        write_private(&file, &text).is_ok()
    }

    /// A run's kept output, the lines its script printed alone.
    pub fn read_output(&self, lap: &str, run: &str) -> Option<Vec<String>> {
        self.read_kept(lap, run).map(|(_, lines)| lines)
    }

    /// A run's kept output: the command line it ran (none in an output kept before it was), and
    /// the lines its script printed.
    pub fn read_kept(&self, lap: &str, run: &str) -> Option<(Option<String>, Vec<String>)> {
        let text = fs::read_to_string(self.output_file(lap, run)?).ok()?;
        if text.is_empty() {
            return Some((None, Vec::new()));
        }
        let (ran, lines): (Vec<&str>, Vec<&str>) = text.split('\n').partition(|line| line.starts_with(MARK));
        let command = ran.iter().map(|line| line.trim_start_matches(MARK)).collect::<Vec<_>>().join("\n");
        Some(((!command.is_empty()).then_some(command), lines.into_iter().map(str::to_string).collect()))
    }

    /// Everything of a job (its runs and outputs).
    pub fn forget(&self, lap: &str) {
        if let Some(file) = self.runs_file(lap) {
            let _ = fs::remove_file(file);
        }
        if let Some(dir) = self.output_dir(lap) {
            // Only the job's own outputs are in it.
            for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                if entry.file_name().to_string_lossy().ends_with(".log") {
                    let _ = fs::remove_file(entry.path());
                }
            }
            let _ = fs::remove_dir(&dir);
        }
    }
}

/// What of a script's `lines` is kept: the last `OUTPUT_LINES`, ANSI stripped, the oldest left
/// out until they hold `OUTPUT_BYTES` at most.
pub(super) fn kept_output(lines: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = lines[lines.len().saturating_sub(OUTPUT_LINES)..].iter().map(|line| strip_ansi(line)).collect();
    let mut bytes: usize = kept.iter().map(|line| line.len() + 1).sum();
    let mut from = 0;
    while bytes > OUTPUT_BYTES && from < kept.len() {
        bytes -= kept[from].len() + 1;
        from += 1;
    }
    kept.drain(..from);
    kept
}
