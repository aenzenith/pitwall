//! Output of dev servers and custom commands: the last lines kept per project and command, and
//! new lines sent on to the UI in batches.

use super::*;

/// Output reaches the UI in batches, one per project and command at most this often.
const OUTPUT_BATCH: Duration = Duration::from_millis(50);

/// Output lines on their way to the UI, gathered per project and command.
#[derive(Default)]
pub(super) struct Outbox {
    batches: Vec<(String, Option<String>, Vec<String>)>,
    /// A flush is on its way.
    due: bool,
}

/// Sends the gathered output, in the order it came.
fn flush_outbox(outbox: &Mutex<Outbox>, sink: &Sink) {
    let mut outbox = outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Sent under the lock, so a batch never overtakes the one before it.
    for (path, job, lines) in outbox.batches.drain(..) {
        sink(CoreEvent::Output { path, job, lines });
    }
}

impl Core {
    pub fn output(&self, path: &str) -> Vec<String> {
        self.lock().output.get(path).map(|lines| lines.iter().cloned().collect()).unwrap_or_default()
    }

    pub(super) fn push_line(&self, path: &str, line: String) {
        {
            let mut inner = self.lock();
            let lines = inner.output.entry(path.to_string()).or_default();
            lines.push_back(line.clone());
            while lines.len() > OUTPUT_LINES {
                lines.pop_front();
            }
        }

        self.send_output(path, None, line);
    }

    /// Queues a line for the UI. The first line of a batch sends the batch `OUTPUT_BATCH` later,
    /// with whatever else came for any project or command meanwhile.
    pub(super) fn send_output(&self, path: &str, job: Option<&str>, line: String) {
        let mut outbox = self.outbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        match outbox.batches.iter_mut().find(|(p, j, _)| p == path && j.as_deref() == job) {
            Some((_, _, lines)) => lines.push(line),
            None => outbox.batches.push((path.to_string(), job.map(str::to_string), vec![line])),
        }

        if !outbox.due {
            outbox.due = true;
            let (pending, sink) = (Arc::clone(&self.outbox), Arc::clone(&self.sink));

            thread::spawn(move || {
                thread::sleep(OUTPUT_BATCH);
                pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).due = false;
                flush_outbox(&pending, &sink);
            });
        }
    }

    /// Sends the queued output at once: a process ended, its last lines shouldn't wait.
    pub(super) fn flush_output(&self) {
        flush_outbox(&self.outbox, &self.sink);
    }
}
