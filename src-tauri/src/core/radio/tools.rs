//! What Claude can ask over the radio, tool by tool. Each acts on the caller's own project (the
//! gates are passed in `radio_call`), answers in plain words for Claude, and says what came of
//! it for the session's list of calls.

use serde_json::Value;

use super::super::board::Column;
use super::super::output::MARKER;
use super::super::routing::{start_done, to_stop};
use super::super::sessions::{lineage, Running};
use super::super::signals::Signal;
use super::*;
use crate::claude::inside;
use crate::resolve::is_forbidden_command;

/// Lines of output answered when Claude names no count, and at most.
const OUTPUT_LINES_DEFAULT: usize = 40;
const OUTPUT_LINES_MAX: usize = 200;
/// A line of output is answered cut to this many characters, and the lines together to this many
/// bytes: a minified bundle on one line is nothing Claude reads, and the line to it has an end
/// (`mcp::LINE_MAX`).
const LINE_CHARS: usize = 2_000;
const ANSWER_BYTES: usize = 60_000;
/// A start, stop or restart is waited for as its spinner is (`routing.rs`), and never longer
/// than this.
const ACT_LIMIT: Duration = Duration::from_secs(40);
/// A command that ends by itself is given this long to, so its result comes with the answer.
const RUN_LIMIT: Duration = Duration::from_secs(10);
/// The end of such a command's output that comes with it.
const RUN_LINES: usize = 20;
/// A card's title is one line of this many characters at most; what Claude writes into a note
/// at once is bounded too, and a card's whole note is as long as its dialog's field takes
/// (`CardDialog.vue`: the note's `maxlength`), or the user could add nothing to it there.
const TITLE_MAX: usize = 120;
const NOTE_MAX: usize = 4_000;
const CARD_NOTE_MAX: usize = 8_000;

const NO_PROJECT: &str = "This folder isn't one of Pitwall's projects, so there is nothing here to ask Pitwall about.";
const SWITCHED_OFF: &str = "The user has switched this tool off in Pitwall (Settings › Claude).";
const NO_CARD: &str = "This session was given no card on Pitwall's board, so there is none to note on.";

type Outcome = (Reply, CallResult);

fn refused(text: impl Into<String>) -> Outcome {
    (Reply::refused(text), CallResult::Refused)
}

/// The server's state in a few lines, and the same for the list of calls.
fn status(project: &ProjectView) -> Outcome {
    let mut lines = vec![match project.phase {
        Some(phase) => format!("status: busy ({phase})"),
        None => format!("status: {}", project.status),
    }];
    if let Some(url) = &project.url {
        lines.push(format!("url: {url}"));
    }
    if let Some(port) = project.port {
        lines.push(format!("port: {port}"));
    }
    if let Some(issue) = &project.issue {
        lines.push(format!("issue: {}", issue.text));
    }
    if let Some(owner) = &project.owner {
        lines.push(format!("run by: {}", owner.title));
    }
    if !project.commands.is_empty() {
        let commands: Vec<String> = project.commands.iter().map(|command| format!("{} ({})", command.name, command.status)).collect();
        lines.push(format!("saved commands: {}", commands.join(", ")));
    }

    let result = CallResult::Status { status: project.status, port: project.port, issue: project.issue.as_ref().map(|issue| issue.text.clone()) };
    (Reply::ok(lines.join("\n")), result)
}

/// The project's saved command of that name, as the user wrote it or in any letter case.
pub(in crate::core) fn saved<'a>(project: &'a ProjectView, name: &str) -> Option<&'a CommandView> {
    let name = name.trim();
    project.commands.iter().find(|command| command.name == name).or_else(|| project.commands.iter().find(|command| command.name.eq_ignore_ascii_case(name)))
}

fn no_such_command(project: &ProjectView) -> Outcome {
    let names: Vec<&str> = project.commands.iter().map(|command| command.name.as_str()).collect();
    if names.is_empty() {
        refused("This project has no saved commands in Pitwall.")
    } else {
        refused(format!("This project has no saved command of that name. It has: {}.", names.join(", ")))
    }
}

/// The last `count` of `lines`, the core's own markers left out, and of the current run only: a
/// launch leaves the run before in the output, over a marker that this one started
/// (`output.rs`: `mark_run`). Answered with both, a run that passes would read as the one that
/// failed before it.
pub(in crate::core) fn last_lines(lines: &[String], count: usize) -> Vec<&str> {
    let started = format!("{MARKER}started\t");
    let run = lines.iter().rposition(|line| line.starts_with(&started)).map_or(0, |at| at + 1);
    let shown: Vec<&str> = lines[run..].iter().filter(|line| !line.starts_with(MARKER)).map(String::as_str).collect();
    shown[shown.len().saturating_sub(count)..].to_vec()
}

/// `lines` as one answer, and how many of them are in it: each cut to `LINE_CHARS`, and of the
/// lot the last that fit `ANSWER_BYTES` as they are sent (a control character goes as six,
/// `\u0001`). What was cut is said where it was.
fn fitted(lines: &[&str]) -> (String, usize) {
    let mut kept: Vec<String> = Vec::new();
    let mut bytes = 0;
    for line in lines.iter().rev() {
        let line = match line.char_indices().nth(LINE_CHARS) {
            Some((end, _)) => format!("{} [cut here: {} more characters]", &line[..end], line[end..].chars().count()),
            None => line.to_string(),
        };
        // As it is sent: its quotes count for the `\n` it ends with.
        bytes += serde_json::to_string(&line).map_or(line.len() + 2, |sent| sent.len());
        if bytes > ANSWER_BYTES {
            break;
        }
        kept.push(line);
    }

    let count = kept.len();
    let before = lines.len() - count;
    if before > 0 {
        let lines = if before == 1 { "line" } else { "lines" };
        kept.push(format!("[left out: {before} earlier {lines}; an answer is {} KB at most]", ANSWER_BYTES / 1000));
    }
    kept.reverse();
    (kept.join("\n"), count)
}

/// An argument's text; an empty one counts as left out.
fn text<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

impl Core {
    /// One call, through the gates (see the top of `radio`): the radio on, a tool there is, a
    /// folder of a listed project, the tool's switch.
    pub(super) fn radio_call(self: &Arc<Self>, caller: &Caller, tool: &str, args: &Value) -> Reply {
        let (on, allowed) = {
            let inner = self.lock();
            (inner.settings.radio.on, inner.settings.radio.allows(tool))
        };
        if !on {
            return Reply::refused(mcp::RADIO_OFF);
        }
        let Some(tool) = TOOLS.iter().find(|known| known.name == tool) else {
            return Reply::refused(format!("Unknown tool: {tool}"));
        };

        let session = self.radio_session(caller.ppid);
        let (reply, result) = match self.radio_project(&caller.cwd) {
            None => refused(NO_PROJECT),
            Some(_) if !allowed => refused(SWITCHED_OFF),
            Some(project) => match tool.name {
                "server_status" => status(&project),
                "server_output" => self.radio_output(&project, args),
                "server_restart" => self.radio_act(&project, args),
                "run_command" => self.radio_run(&project, args),
                "board_add" => self.radio_card(&project, args),
                "board_note" => self.radio_card_note(&project, session.as_deref(), args),
                _ => refused(format!("Unknown tool: {}", tool.name)),
            },
        };

        self.radio_note(session.as_deref(), tool.name, result);
        reply
    }

    /// The listed project that holds `cwd`: the longest match, each folder taken as the file
    /// system names it (a project listed through a link is the folder it leads to). A folder
    /// that isn't on disk is in no project, and holds none: its text alone
    /// (`<project>/gone/../../<another>`) doesn't say where it is.
    pub(in crate::core) fn radio_project(&self, cwd: &str) -> Option<ProjectView> {
        let real = |path: &str| fs::canonicalize(path).ok().map(|real| real.to_string_lossy().into_owned());
        let cwd = real(cwd)?;

        self.snapshot()
            .projects
            .into_iter()
            .filter_map(|project| Some((real(&project.path)?, project)))
            .filter(|(folder, _)| inside(&cwd, folder))
            .max_by_key(|(folder, _)| folder.len())
            .map(|(_, project)| project)
    }

    /// The project as it is now.
    fn radio_view(&self, path: &str) -> Option<ProjectView> {
        self.snapshot().projects.into_iter().find(|project| project.path == path)
    }

    /// The Claude Code session that runs as process `pid` or, started through something in
    /// between, as the nearest of its parents.
    fn radio_session(&self, pid: u32) -> Option<String> {
        let among = |running: &HashMap<String, Running>, pid: u32| running.iter().find(|(_, run)| run.pid == pid).map(|(id, _)| id.clone());
        if let Some(id) = self.running_cached().and_then(|running| among(&running, pid)) {
            return Some(id);
        }

        // Not among the sessions as last read: one that began since, so they are read again.
        let running = self.read_running().filter(|running| !running.is_empty())?;
        among(&running, pid).or_else(|| lineage(&process::process_table()?, pid).into_iter().skip(1).find_map(|parent| among(&running, parent)))
    }

    /// Waits, a quarter of a second at a time and never under the lock, until `done` or `limit`.
    fn radio_wait(&self, limit: Duration, done: impl Fn(&Inner) -> bool) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if done(&self.lock()) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn radio_output(&self, project: &ProjectView, args: &Value) -> Outcome {
        let count = args.get("lines").and_then(Value::as_u64).map_or(OUTPUT_LINES_DEFAULT, |count| (count as usize).clamp(1, OUTPUT_LINES_MAX));

        let lines = match text(args, "command") {
            Some(name) => match saved(project, name) {
                Some(command) => self.command_output(&project.path, &command.id),
                None => return no_such_command(project),
            },
            None => {
                let lines = self.output(&project.path);
                // Marked in the output Claude read, each time: of several in a row the output
                // keeps the last (`output.rs`: `keep_line`).
                if !lines.is_empty() {
                    self.push_marker(&project.path, "claude-read", None);
                }
                lines
            }
        };

        let (text, count) = fitted(&last_lines(&lines, count));
        let reply = if count == 0 { Reply::ok("(no output)") } else { Reply::ok(text) };
        (reply, CallResult::Lines { count })
    }

    /// Start, stop or restart, routed as a click on the project's button is, and answered once
    /// its spinner would stop.
    fn radio_act(self: &Arc<Self>, project: &ProjectView, args: &Value) -> Outcome {
        let Some(action) = text(args, "action").and_then(Action::parse) else {
            return refused("`action` is one of: start, stop, restart.");
        };
        let path = project.path.as_str();

        // Nothing to do leaves no mark in the output.
        let idle = match action {
            Action::Start => project.status == "running",
            Action::Stop => !to_stop(&self.lock(), path),
            Action::Restart => false,
        };
        if idle {
            let (reply, result) = status(project);
            let said = if action == Action::Start { "It is running already." } else { "There is nothing to stop." };
            return (Reply::ok(format!("{said}\n{}", reply.text)), result);
        }

        let kind = match action {
            Action::Start => "claude-start",
            Action::Stop => "claude-stop",
            Action::Restart => "claude-restart",
        };
        // As a click is, but for the browser: nobody asked to see this start's address. Marked
        // in the output where it happens: a stop once it is under way, a start where its run
        // begins (`output.rs`: `mark_run`). A start the core refuses leaves no mark.
        self.mark_pending(path, action);
        if action != Action::Stop {
            self.lock().asked_by_claude.insert(path.to_string(), kind);
        }
        if self.route(path, action, false, Origin::Claude) && action == Action::Stop {
            self.push_marker(path, kind, None);
        }
        self.radio_wait(ACT_LIMIT, |inner| start_done(inner, path));
        // A run that didn't begin here (refused, or a window's to start) takes no mark later.
        self.lock().asked_by_claude.remove(path);
        // Its spinner goes with the next look at the state.
        self.notify();

        match self.radio_view(path) {
            Some(project) => status(&project),
            None => (Reply::ok("status: stopped"), CallResult::Status { status: "stopped", port: None, issue: None }),
        }
    }

    /// One of the project's saved commands. Never one the user wants to be asked about first,
    /// and never what the app itself won't run (`is_forbidden_command`).
    fn radio_run(self: &Arc<Self>, project: &ProjectView, args: &Value) -> Outcome {
        let Some(command) = text(args, "name").and_then(|name| saved(project, name)) else {
            return no_such_command(project);
        };
        let name = command.name.as_str();

        if command.confirm {
            return refused(format!("\"{name}\" is marked \"ask first\" in Pitwall: only the user runs it. Ask them to."));
        }
        if is_forbidden_command(&command.command) {
            return refused(format!("\"{name}\" runs a production build, which Pitwall never does: it would break the dev server."));
        }
        if command.status == "running" || command.status == "busy" {
            return refused(format!("\"{name}\" is running already. Read its output with server_output."));
        }

        let key = job_key(&project.path, &command.id);
        let began = now_ms();
        self.run_command(&project.path, &command.id);
        let ran = CallResult::Ran { name: name.to_string() };

        // This run's result, not the one before's: that one stays until this one is written,
        // a moment after the command is gone from the running ones.
        let over = |inner: &Inner| inner.job_results.get(&key).is_some_and(|result| result.finished_at >= began);
        if command.keep_running || !self.radio_wait(RUN_LIMIT, over) {
            return (Reply::ok(format!("\"{name}\" is running. Read its output with server_output.")), ran);
        }

        let code = self.lock().job_results.get(&key).and_then(|result| result.code);
        let code = code.map_or_else(|| "none".to_string(), |code| code.to_string());
        let output = self.command_output(&project.path, &command.id);
        (Reply::ok(format!("\"{name}\" ended (exit code {code}).\n{}", fitted(&last_lines(&output, RUN_LINES)).0)), ran)
    }

    /// A card in the first column of the project's board.
    fn radio_card(&self, project: &ProjectView, args: &Value) -> Outcome {
        let title = text(args, "title").map(|title| title.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
        let note = text(args, "note").unwrap_or_default();
        if title.is_empty() || title.chars().count() > TITLE_MAX {
            return refused(format!("A card needs a `title` of one short line ({TITLE_MAX} characters at most)."));
        }
        if note.chars().count() > NOTE_MAX {
            return refused(format!("A card's `note` is {NOTE_MAX} characters at most."));
        }

        match self.board_add(&project.path, &title, note, &[]) {
            Ok(card) => (Reply::ok(format!("Added to the board's first column: {}", card.title)), CallResult::Card { title: card.title }),
            Err(error) => refused(error),
        }
    }

    /// A note under the text of the card this session was given (the one given last, of several),
    /// with Claude's name, the day and the time over it; the card goes to review, which is told
    /// as it is of a card the board moves there itself. Only a card of this project that is with
    /// Claude or in review: one the user put elsewhere stays theirs.
    fn radio_card_note(&self, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let Some(note) = text(args, "note").filter(|note| note.chars().count() <= NOTE_MAX) else {
            return refused(format!("`note` is what to write on the card ({NOTE_MAX} characters at most)."));
        };
        let Some(session) = session else {
            return refused(NO_CARD);
        };

        let now = now_ms();
        let mut board = self.board();
        let held = board
            .cards
            .iter()
            .filter(|card| card.session.as_deref() == Some(session) && card.path == project.path && matches!(card.column, Column::Claude | Column::Review))
            .max_by_key(|card| card.given_at.unwrap_or(card.moved_at))
            .map(|card| (card.id.clone(), card.column, card.note.clone()));
        let Some((id, column, before)) = held else {
            return refused(NO_CARD);
        };
        let signed = format!("Claude · {} {}\n{note}", crate::clock::day_name(now), crate::clock::clock(now as i64));
        let noted = if before.is_empty() { signed } else { format!("{before}\n\n{signed}") };
        // Counted as the dialog's field counts.
        let over = noted.encode_utf16().count().saturating_sub(CARD_NOTE_MAX);
        if over > 0 {
            return refused(format!("That card's note holds {CARD_NOTE_MAX} characters at most, and this would be {over} too many. Write a shorter note."));
        }

        // Already in review, it keeps its place there.
        let moved = column != Column::Review;
        if moved {
            board.place(&id, Column::Review, usize::MAX, now);
        }
        let Some(card) = board.cards.iter_mut().find(|card| card.id == id) else {
            return refused(NO_CARD);
        };
        card.note = noted;
        let title = card.title.clone();
        self.board_changed(board);
        // Once, where it got there; the board's own pass tells only of the cards it moves.
        if moved {
            self.signal(Signal { kind: SignalKind::CardReview, path: project.path.clone(), name: project.name.clone(), detail: None });
        }

        (Reply::ok(format!("Noted on the card \"{title}\", which is in review now.")), CallResult::Noted)
    }
}
