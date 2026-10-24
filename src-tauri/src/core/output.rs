//! Output of dev servers and custom commands: the last lines kept per project and command, and
//! new lines sent on to the UI in batches. Between the lines stand markers the core writes
//! itself: where one run ended and the next began, what Claude did there.

use super::*;

/// Output reaches the UI in batches, one per project and command at most this often.
const OUTPUT_BATCH: Duration = Duration::from_millis(50);

/// The first character of a line the core writes into an output itself (a run ended or began,
/// Claude read it): U+001E, then the kind, the time (Unix ms) and a detail, tab-separated. The
/// pane draws it as a rule; everything else that reads output lines skips it.
pub(super) const MARKER: char = '\u{1e}';

/// A marker line. The detail may be empty; it stays on the one line whatever it holds.
fn marker(kind: &str, at: u64, detail: &str) -> String {
    format!("{MARKER}{kind}\t{at}\t{}", detail.replace(['\n', '\r'], " "))
}

/// The kind of a marker line; `None` for a line of output.
fn marker_kind(line: &str) -> Option<&str> {
    line.strip_prefix(MARKER)?.split('\t').next()
}

/// How a run ended, for the `ended` marker the next one writes: `stopped` when a stop did it,
/// else `crashed:<code>` for a dev server and `exited:<code>` for a command, the code empty
/// when a signal ended it.
pub(super) fn end_detail(unit: &Unit, planned: bool, status: Option<std::process::ExitStatus>) -> String {
    if planned {
        return "stopped".into();
    }
    let code = status.and_then(|status| status.code()).map(|code| code.to_string()).unwrap_or_default();
    match unit {
        Unit::Server(_) => format!("crashed:{code}"),
        Unit::Command(..) => format!("exited:{code}"),
    }
}

/// Pitwall's own line for a run that ended: a dev server's exit code and signal, a command's
/// code and whether a stop ended it.
pub(super) fn end_line(unit: &Unit, planned: bool, status: Option<std::process::ExitStatus>) -> String {
    let said = match unit {
        Unit::Server(_) => {
            let (code, signal) = servers::describe_exit(status);
            t!("core.log.processEnded", code = code, signal = signal)
        }
        Unit::Command(..) => {
            let code = status.and_then(|status| status.code()).map(|code| code.to_string()).unwrap_or_else(|| "-".into());
            if planned { t!("core.log.stopped", code = code) } else { t!("core.log.exited", code = code) }
        }
    };
    format!("[pitwall] {said}")
}

/// Output lines on their way to the UI, gathered per project and command.
#[derive(Default)]
pub(super) struct Outbox {
    batches: Vec<(String, Option<String>, VecDeque<String>)>,
    /// A flush is on its way.
    due: bool,
}

/// Sends the gathered output, in the order it came. Under the outbox's lock (the caller holds
/// it), so a batch never overtakes the one before it.
fn flush_outbox(outbox: &mut Outbox, sink: &Sink) {
    for (path, job, lines) in outbox.batches.drain(..) {
        sink(CoreEvent::Output { path, job, lines: lines.into() });
    }
}

impl Core {
    fn outbox(&self) -> MutexGuard<'_, Outbox> {
        self.outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn output(&self, path: &str) -> Vec<String> {
        self.read_output(path)
    }

    /// The lines kept under `key` (a project's path, or a command's `job_key`). What is queued
    /// for the UI goes out first, under the lock a line is kept and queued under: a line is
    /// never both in this answer and in a batch that comes after it, which the pane that asked
    /// would show twice.
    pub(super) fn read_output(&self, key: &str) -> Vec<String> {
        let mut outbox = self.outbox();
        flush_outbox(&mut outbox, &self.sink);
        self.lock().output.get(key).map(|lines| lines.iter().cloned().collect()).unwrap_or_default()
    }

    /// A line of the project's server output: what the server printed, what a followed file
    /// held, or a word of the core's own.
    pub(super) fn push_line(&self, path: &str, line: String) {
        self.push_output(path, None, line);
    }

    /// A line of the server's output (`job` none) or of a command's. Never a marker: only
    /// the core writes those, so a line from outside that begins like one loses the character.
    pub(super) fn push_output(&self, path: &str, job: Option<&str>, line: String) {
        let line = if line.starts_with(MARKER) { line.trim_start_matches(MARKER).to_string() } else { line };
        self.keep_line(path, job, line);
    }

    /// Writes a marker into the project's server output.
    pub(super) fn push_marker(&self, path: &str, kind: &str, detail: Option<&str>) {
        self.keep_line(path, None, marker(kind, now_ms(), detail.unwrap_or_default()));
    }

    /// Keeps a line and sends it on. A marker that says what the last line already says
    /// (Claude read the output once more) takes that one's place, so repeats never push the
    /// output itself out.
    fn keep_line(&self, path: &str, job: Option<&str>, line: String) {
        self.keep_line_with(path, job, |_| Some(((), line)));
    }

    /// Keeps the line `step` gives, in the one hold of the core's lock `step` runs under: what
    /// it changes there and its line come together for everyone else (a run taken out, and the
    /// line that says it ended). No line when `step` gives none.
    pub(super) fn keep_line_with<T>(&self, path: &str, job: Option<&str>, step: impl FnOnce(&mut Inner) -> Option<(T, String)>) -> Option<T> {
        let key = job.map_or_else(|| path.to_string(), |id| job_key(path, id));
        // Kept and queued as one step for a read (`read_output`), which takes this lock first too.
        let mut outbox = self.outbox();
        let (given, line) = {
            let mut inner = self.lock();
            let (given, line) = step(&mut inner)?;
            let lines = inner.output.entry(key.clone()).or_default();
            if marker_kind(&line).is_some_and(|kind| lines.back().and_then(|last| marker_kind(last)) == Some(kind)) {
                lines.pop_back();
            }
            lines.push_back(line.clone());
            while lines.len() > OUTPUT_LINES {
                lines.pop_front();
            }
            // The command line's `logs -f` and `run` follow along (`cli`).
            inner.cli.heard(&key, &line);
            (given, line)
        };

        self.send_output(&mut outbox, path, job, line);
        Some(given)
    }

    /// A run begins in an output that holds an earlier one of ours: how that one ended (as its
    /// exit was seen, `supervise.rs`) and that this one started go in as markers, and the
    /// earlier lines stay above them. Without a run that ended there are none: what Claude did
    /// before a first start (its markers), or the lines of a server a VS Code window ran, are
    /// no run before this one.
    pub(super) fn mark_run(&self, path: &str, job: Option<&str>) {
        let key = job.map_or_else(|| path.to_string(), |id| job_key(path, id));
        let (ended, by_claude) = {
            let mut inner = self.lock();
            let ended = inner.ended.remove(&key);
            // A server's run Claude asked for over the radio is told by Claude's own marker.
            let by_claude = if job.is_none() { inner.asked_by_claude.remove(path) } else { None };
            let empty = inner.output.get(&key).is_none_or(|lines| lines.is_empty());
            (ended.filter(|_| !empty), by_claude)
        };

        if let Some((at, detail)) = ended {
            self.keep_line(path, job, marker("ended", at, &detail));
            self.keep_line(path, job, marker("started", now_ms(), ""));
        }
        if let Some(kind) = by_claude {
            self.keep_line(path, job, marker(kind, now_ms(), ""));
        }
    }

    /// Empties an output (by its key: the project's path, or a command's `job_key`), with what
    /// was remembered of its last run.
    pub(super) fn forget_output(&self, key: &str) {
        let mut inner = self.lock();
        inner.output.remove(key);
        inner.ended.remove(key);
    }

    /// Queues a line for the UI. The first line of a batch sends the batch `OUTPUT_BATCH` later,
    /// with whatever else came for any project or command meanwhile. A batch holds as many lines
    /// as the pane keeps (`OUTPUT_LINES`, as here): of a flood, older ones would only be pushed
    /// out there.
    fn send_output(&self, outbox: &mut Outbox, path: &str, job: Option<&str>, line: String) {
        match outbox.batches.iter_mut().find(|(p, j, _)| p == path && j.as_deref() == job) {
            Some((_, _, lines)) => {
                lines.push_back(line);
                if lines.len() > OUTPUT_LINES {
                    lines.pop_front();
                }
            }
            None => outbox.batches.push((path.to_string(), job.map(str::to_string), VecDeque::from([line]))),
        }

        if !outbox.due {
            outbox.due = true;
            let (pending, sink) = (Arc::clone(&self.outbox), Arc::clone(&self.sink));

            thread::spawn(move || {
                thread::sleep(OUTPUT_BATCH);
                let mut pending = pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                pending.due = false;
                flush_outbox(&mut pending, &sink);
            });
        }
    }

    /// Sends the queued output at once: a process ended, its last lines shouldn't wait.
    pub(super) fn flush_output(&self) {
        flush_outbox(&mut self.outbox(), &self.sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A marker is the core's own voice in an output ("Claude restarted it"): a dev server, or
    /// a file a VS Code window mirrors, that prints one must not be drawn as if the core said it.
    #[test]
    fn a_line_from_outside_is_never_a_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = CoreConfig {
            registry_dir: tmp.path().join("registry"),
            claude_dir: tmp.path().join("claude"),
            settings_file: tmp.path().join("settings.json"),
            claude_settings: tmp.path().join("claude-settings.json"),
            legacy_registries: Vec::new(),
            title: "Pitwall".into(),
        };
        let core = Core::new(cfg, Arc::new(|_| {}));

        core.push_line("/p", format!("{MARKER}claude-restart\t1\t"));
        core.push_output("/p", Some("queue"), format!("{MARKER}{MARKER}ended\t1\tcrashed:1"));
        core.push_marker("/p", "claude-read", None);

        let server = core.output("/p");
        assert_eq!(server[0], "claude-restart\t1\t");
        assert_eq!(marker_kind(&server[1]), Some("claude-read"), "the core's own is one");
        assert_eq!(core.command_output("/p", "queue"), ["ended\t1\tcrashed:1"]);
    }
}
