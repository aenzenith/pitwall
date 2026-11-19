//! The laps over the radio: the caller's project's jobs, read, added, changed, paused and
//! deleted, each on the caller's project alone.
//!
//! The jobs there are for the radio are those bound to the caller's project (`laps::of_project`):
//! a folder's job, or another project's, is answered as an id there is none of. A session of the
//! laps' own, or one Pitwall can't tell, acts on none: a lap never breeds laps. A job an agent
//! added carries that agent (`Lap::by`), and only such a job is changed or deleted here; a script
//! line an agent wrote holds its job, paused, until the user has read it and resumed it in the
//! window (`Lap::review`), which nothing here can do. What is read of a job (its prompt, its
//! script, its last result) goes to the caller alone, read when asked and kept nowhere.

use chrono::{Datelike, Local, NaiveDateTime, TimeZone};
use serde_json::Value;

use super::super::laps::{radio_made, Barred, Lap, LapKind, LapRun, LapView, PausedBy, Place, RadioSave, Reason, RunResult, Script, ScriptGate, When, Where};
use super::tools::{fitted, refused, text};
use super::*;
use crate::start::{CodexMode, CodexModel, Mode, Model, Start};

type Outcome = (Reply, CallResult);

/// A job's name: one line of this many characters at most, as the dialog's field takes it
/// (`LapEditDialog.vue`: its `maxlength`) and `lap_save` keeps it.
const NAME_MAX: usize = 200;
/// What an agent writes into a job at once: its prompt, and its script's command line.
const PROMPT_MAX: usize = 8_000;
const SCRIPT_MAX: usize = 4_000;
/// How many times `lap_edit` reads a job the window keeps changing again before it gives up.
const EDIT_TRIES: usize = 3;
/// A job's prompt and script as `lap_list` answers them, cut there.
const SHOWN_MAX: usize = 4_000;
/// The jobs `lap_list` answers at once, and the runs of one.
const LIST_MAX: usize = 50;
const RUNS_SHOWN: usize = 10;
/// The end of the last result answered, as `server_output` answers lines: its last lines, cut
/// to size (`fitted`).
const RESULT_LINES: usize = 40;
/// Claude's models, as `start.rs` lists them (`Model::parse` takes each).
const CLAUDE_MODELS: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];
const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const NO_SUCH_JOB: &str = "This project has no job of that id in Pitwall's Laps. lap_list lists its jobs and their ids.";
const NO_ID: &str = "`lap` is the job's id, as lap_list gives it.";
const FROM_A_LAP: &str = "This call comes from one of Pitwall's laps, or from a session Pitwall can't tell: from there no job is added, changed, paused or \
                          deleted. Ask the user to do it in Pitwall's Laps page.";
const USERS_CHANGE: &str = "Only the user changes a job of theirs, one they made or changed in Pitwall. Ask them, or pause it with lap_pause.";
const USERS_DELETE: &str = "Only the user deletes a job of theirs, one they made or changed in Pitwall. Pause it with lap_pause instead.";
const HELD: &str = "It waits, paused, until the user reads its script and turns it on in Pitwall's Laps page. Tell the user.";
const WHEN_HELP: &str = "`when` is one of: {\"kind\":\"daily\",\"at\":\"09:00\",\"days\":[1,2,3,4,5]}, {\"kind\":\"every\",\"minutes\":30,\"from\":\"09:00\",\"to\":\"18:00\"}, \
                         {\"kind\":\"once\",\"at\":\"YYYY-MM-DD HH:MM\"}, {\"kind\":\"refill\"} or {\"kind\":\"cron\",\"expr\":\"0 9 * * 1\"}.";
const DAYS: &str = "`days` are numbers from 1 (Monday) to 7 (Sunday); none is every day.";
const SCRIPT_HELP: &str = "`script` is {\"command\": \"<a saved command's name>\"} or {\"text\": \"<a command line>\"}.";

/// A refusal of the laps' own, in words for the agent: `deleting` names what only the user does.
fn barred(barred: Barred, deleting: bool) -> String {
    match barred {
        Barred::NotFound => NO_SUCH_JOB,
        Barred::Elsewhere => "Another Pitwall on this Mac runs the laps, so nothing about them changes from here. Ask the user to change it there.",
        Barred::Unsaved => "Pitwall couldn't write its laps file, so nothing changed.",
        Barred::Check(key) => return checked(key),
        Barred::Users if deleting => USERS_DELETE,
        Barred::Users => USERS_CHANGE,
        Barred::Going => "A run of this job goes now: its script changes once that run has ended.",
        Barred::Review => "The user turns this job on after reading its script, in Pitwall's Laps page. Tell the user.",
        Barred::Failures => "This job paused itself after failing again and again: the user looks at its failures first. Tell the user.",
        Barred::Off => "The user switched this job off: only they switch it on, in Pitwall's Laps page.",
        Barred::Changed => "The job kept changing in Pitwall while this edit was made, so nothing changed. Read it again with lap_list, then try again.",
    }
    .to_string()
}

/// Why `lap_save` refused a job (its catalog key), in words for the agent; one these don't name in
/// Pitwall's own language.
fn checked(key: &str) -> String {
    let said = match key {
        "laps.when.error.time" => "Times are HH:MM on this Mac's clock, as 09:30.",
        "laps.when.error.interval" => "With an agent, a job runs every 15 minutes at the most.",
        "laps.when.error.interval.script" => "The shortest interval is 1 minute.",
        "laps.when.error.long" => "The longest interval is 24 hours (1440 minutes).",
        "laps.when.error.window" => "`from` can't be after `to`.",
        "laps.when.error.days" => DAYS,
        "laps.when.error.past" => "That moment has passed. Give one still to come, on this Mac's clock.",
        "laps.when.error.cron" => "That isn't a cron expression Pitwall reads: five fields, minute hour day-of-month month day-of-week.",
        "laps.when.error.often" => "With an agent, a job's runs are at least 15 minutes apart.",
        "laps.when.error.never" => "This schedule never runs.",
        "laps.when.error.refill" => "`refill` is for a job with an agent: a script alone has no refill to wait for.",
        "core.laps.error.name" => "A job needs a `name` of one line, 200 characters at most.",
        "core.laps.error.place" => "This project is no longer listed in Pitwall.",
        "core.laps.error.placeRefused" => "A job can't run in Claude's, Codex's or Pitwall's own folders.",
        "core.laps.error.prompt" => "A job with an agent needs a `prompt`: what the agent should do.",
        "core.laps.error.script" => SCRIPT_HELP,
        "core.laps.error.command" => "That saved command is gone, or it is marked \"ask first\" in Pitwall: a job can't run it, only the user does.",
        "core.laps.error.timeout" => "`timeout_min` is between 1 and 1440 minutes, or 0 for no limit.",
        _ => return t!(key),
    };
    said.to_string()
}

/// `text` cut to `max` characters, saying where.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{} [cut here: {} more characters]", &text[..end], text[end..].chars().count()),
        None => text.to_string(),
    }
}

/// `text` as lines under a label, each indented.
fn block(label: &str, text: &str) -> Vec<String> {
    std::iter::once(format!("  {label}:")).chain(cut(text, SHOWN_MAX).lines().map(|line| format!("    {line}"))).collect()
}

/// A moment on this Mac's clock, as `Thu 20 Nov 09:00` (with its year when it isn't this one's).
fn moment(at: u64) -> String {
    let Some(time) = i64::try_from(at).ok().and_then(|at| Local.timestamp_millis_opt(at).single()) else {
        return "an unknown moment".into();
    };
    if time.year() == Local::now().year() {
        time.format("%a %-d %b %H:%M").to_string()
    } else {
        time.format("%a %-d %b %Y %H:%M").to_string()
    }
}

/// `YYYY-MM-DD HH:MM` (or with a `T`) on this Mac's clock; none for what isn't one or a time the
/// clocks skip.
fn local_moment(text: &str) -> Option<u64> {
    let naive = NaiveDateTime::parse_from_str(&text.trim().replacen('T', " ", 1), "%Y-%m-%d %H:%M").ok()?;
    u64::try_from(Local.from_local_datetime(&naive).earliest()?.timestamp_millis()).ok()
}

/// Days 1 (Monday) to 7 as a schedule says them; none for every day.
fn days_text(days: &[u8]) -> Option<String> {
    let mut days: Vec<u8> = days.iter().copied().filter(|day| (1..=7).contains(day)).collect();
    days.sort_unstable();
    days.dedup();
    match days.as_slice() {
        [] | [1, 2, 3, 4, 5, 6, 7] => None,
        [1, 2, 3, 4, 5] => Some("weekdays".into()),
        [6, 7] => Some("weekends".into()),
        days => Some(format!("on {}", days.iter().map(|&day| DAY_NAMES[usize::from(day - 1)]).collect::<Vec<_>>().join(", "))),
    }
}

/// When a job runs, in plain words: `every day at 09:00`, `weekdays at 18:30`.
fn when_text(when: &When, agent: Agent) -> String {
    match when {
        When::Daily { at, days } => match days_text(days) {
            Some(days) => format!("{days} at {at}"),
            None => format!("every day at {at}"),
        },
        When::Every { minutes, from, to, days } => {
            let mut text = if *minutes >= 60 && minutes % 60 == 0 { format!("every {} h", minutes / 60) } else { format!("every {minutes} min") };
            if from.is_some() || to.is_some() {
                text.push_str(&format!(" from {} to {}", from.as_deref().unwrap_or("00:00"), to.as_deref().unwrap_or("23:59")));
            }
            if let Some(days) = days_text(days) {
                text.push_str(&format!(", {days}"));
            }
            text
        }
        When::Once { at } => format!("once on {}", moment(*at)),
        When::Refill => format!("at {}'s next refill", agent.name()),
        When::Cron { expr } => format!("cron {expr}"),
    }
}

/// What a job is: its kind and, with an agent, which one, its model and mode and where it runs.
fn kind_text(lap: &Lap) -> String {
    let kind = match lap.kind {
        LapKind::Agent => "agent",
        LapKind::Script => "script",
        LapKind::ScriptAgent => "script then agent",
    };
    if !lap.kind.has_agent() {
        return kind.to_string();
    }
    let start = lap.start;
    let mut parts = vec![start.agent.name().to_string()];
    match start.agent {
        Agent::Claude => {
            parts.extend(start.model.map(|model| format!("model {}", model.alias())));
            parts.extend(start.mode.flag().map(|mode| format!("mode {mode}")));
        }
        Agent::Codex => {
            parts.extend(start.codex.model.map(|model| format!("model {}", model.id())));
            parts.extend((!start.codex.mode.is_normal()).then(|| format!("mode {}", start.codex.mode.word())));
        }
    }
    parts.push(if lap.place_run == Where::Background { "in the background, read-only".into() } else { "in a tab".into() });
    format!("{kind} ({})", parts.join(", "))
}

/// Where a job stands, and its next moment.
fn state_text(view: &LapView) -> String {
    let lap = &view.lap;
    let mut text = if lap.review {
        "paused: it waits for the user to read its script and resume it".to_string()
    } else if lap.paused == Some(PausedBy::Failures) {
        let count = lap.streak.max(lap.pause_after.unwrap_or(0));
        format!("paused after {count} failure{}", if count == 1 { "" } else { "s" })
    } else if lap.paused.is_some() {
        "paused".to_string()
    } else if !lap.on {
        "switched off".to_string()
    } else {
        "on".to_string()
    };
    if view.running.is_some() {
        text.push_str(", running now");
    }
    match lap.next_at {
        Some(at) => text.push_str(&format!(", next {}", moment(at))),
        None if lap.on && lap.paused.is_none() && lap.when == When::Refill && view.no_reading => {
            text.push_str(&format!(", no next moment until Pitwall has read {}'s usage", lap.start.agent.name()));
        }
        None if lap.on && lap.paused.is_none() => text.push_str(", no moment to come"),
        None => {}
    }
    text
}

fn result_word(result: RunResult) -> &'static str {
    match result {
        RunResult::Ok => "done",
        RunResult::Failed => "failed",
        RunResult::Skipped => "skipped",
        RunResult::Silent => "silent (the same result as the run before)",
        RunResult::Cut => "cut",
    }
}

fn reason_text(reason: Reason) -> &'static str {
    match reason {
        Reason::Gate => "the script's gate called no agent",
        Reason::Git => "no commit since the last run",
        Reason::Fuel => "the fuel guard",
        Reason::NoReading => "no usage reading of the agent's account yet",
        Reason::AgentOff => "its agent is off in Pitwall",
        Reason::Missed => "missed: Pitwall was closed or the Mac asleep",
        Reason::Overlap => "the run before still went",
        Reason::DayOff => "a day off",
        Reason::UserSkip => "the user skipped it",
        Reason::AskFirst => "its saved command asks first",
        Reason::Timeout => "it ran past its time limit",
        Reason::Exit => "it ended with an error",
        Reason::NotFolder => "its folder is gone",
        Reason::NotListed => "its project is no longer listed",
        Reason::Stopped => "the user stopped it",
        Reason::Quit => "Pitwall quit while it ran",
        Reason::NotStarted => "it couldn't be started",
    }
}

/// One run, in a line.
fn run_line(run: &LapRun) -> String {
    let mut parts = vec![moment(run.due), run.result.map_or("running", result_word).to_string()];
    if run.by_hand {
        parts.push("run by hand".into());
    }
    parts.extend(run.code.map(|code| format!("exit code {code}")));
    parts.extend(run.reason.map(|reason| reason_text(reason).to_string()));
    format!("    {}", parts.join(" · "))
}

/// A job in a few lines, as `lap_list` lists it.
fn job_lines(view: &LapView) -> Vec<String> {
    let lap = &view.lap;
    let agent = lap.start.agent;
    let mut lines = vec![
        format!("{} · \"{}\"", lap.id, lap.name),
        format!("  kind: {}", kind_text(lap)),
        format!("  when: {}", when_text(&lap.when, agent)),
        format!("  state: {}", state_text(view)),
    ];
    lines.push(match view.last.last() {
        Some(mark) => format!("  last run: {}, {}", mark.result.map_or("running", result_word), moment(mark.due)),
        None => "  last run: none yet".into(),
    });
    if let Some(by) = lap.by {
        lines.push(format!("  added over the radio by {}", by.name()));
    }
    lines
}

/// What `lap_add` and `lap_edit` were given, each read and checked; none for one left out.
#[derive(Default)]
struct Given {
    name: Option<String>,
    kind: Option<LapKind>,
    prompt: Option<String>,
    script: Option<Script>,
    when: Option<When>,
    agent: Option<Agent>,
    model: Option<String>,
    mode: Option<String>,
    place_run: Option<Where>,
    carry: Option<bool>,
    /// `Some(None)`: given as null, taken off.
    ends_after: Option<Option<u32>>,
    timeout_min: Option<Option<u32>>,
}

impl Given {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.kind.is_none()
            && self.prompt.is_none()
            && self.script.is_none()
            && self.when.is_none()
            && self.agent.is_none()
            && self.model.is_none()
            && self.mode.is_none()
            && self.place_run.is_none()
            && self.carry.is_none()
            && self.ends_after.is_none()
            && self.timeout_min.is_none()
    }

    /// The fields of `args`; a field of the wrong form is refused, with what it takes.
    fn read(args: &Value, project: &ProjectView) -> Result<Self, String> {
        let word = |key: &str| args.get(key).filter(|value| !value.is_null());
        let mut given = Self::default();

        if let Some(name) = text(args, "name") {
            let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
            if name.chars().count() > NAME_MAX {
                return Err(format!("A job's `name` is one line of {NAME_MAX} characters at most."));
            }
            given.name = Some(name);
        }
        if let Some(kind) = word("kind") {
            given.kind = Some(match kind.as_str() {
                Some("agent") => LapKind::Agent,
                Some("script") => LapKind::Script,
                Some("script_agent" | "scriptAgent") => LapKind::ScriptAgent,
                _ => return Err("`kind` is one of: agent, script, script_agent.".into()),
            });
        }
        if let Some(prompt) = text(args, "prompt") {
            if prompt.chars().count() > PROMPT_MAX {
                return Err(format!("A job's `prompt` is {PROMPT_MAX} characters at most."));
            }
            given.prompt = Some(prompt.to_string());
        }
        if let Some(script) = word("script") {
            given.script = Some(script_of(script, project)?);
        }
        if let Some(when) = word("when") {
            given.when = Some(when_of(when)?);
        }
        if let Some(agent) = word("agent") {
            given.agent = Some(agent.as_str().and_then(Agent::parse).ok_or("`agent` is claude or codex.")?);
        }
        given.model = text(args, "model").map(str::to_string);
        given.mode = text(args, "mode").map(str::to_string);
        if let Some(place) = word("where") {
            given.place_run = Some(match place.as_str() {
                Some("tab") => Where::Tab,
                Some("background") => Where::Background,
                _ => return Err("`where` is tab or background.".into()),
            });
        }
        if let Some(carry) = word("carry") {
            given.carry = Some(carry.as_bool().ok_or("`carry` is true or false.")?);
        }
        // 0 takes the end or the limit off (null too, as an older caller may send it).
        let count = |key: &str, what: &str| -> Result<Option<Option<u32>>, String> {
            match args.get(key) {
                None => Ok(None),
                Some(Value::Null) => Ok(Some(None)),
                Some(value) => match value.as_u64().and_then(|count| u32::try_from(count).ok()) {
                    Some(0) => Ok(Some(None)),
                    Some(count) => Ok(Some(Some(count))),
                    None => Err(what.to_string()),
                },
            }
        };
        given.ends_after = count("ends_after", "`ends_after` is how many runs the job makes before it switches itself off, or 0 for no end.")?;
        given.timeout_min = count("timeout_min", "`timeout_min` is between 1 and 1440 minutes, or 0 for no limit.")?;
        Ok(given)
    }
}

/// A script as the radio gives it: one of the project's saved commands by its name, or a command
/// line. A command marked "ask first" is refused by `lap_save` itself.
fn script_of(value: &Value, project: &ProjectView) -> Result<Script, String> {
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
    match (field("command"), field("text")) {
        (Some(name), None) => match saved(project, name) {
            Some(command) => Ok(Script::Command { id: command.id.clone() }),
            None => {
                let names: Vec<&str> = project.commands.iter().map(|command| command.name.as_str()).collect();
                Err(if names.is_empty() {
                    "This project has no saved commands in Pitwall: give the script as {\"text\": \"<a command line>\"}.".to_string()
                } else {
                    format!("This project has no saved command of that name. It has: {}.", names.join(", "))
                })
            }
        },
        (None, Some(line)) if line.chars().count() > SCRIPT_MAX => Err(format!("A script's command line is {SCRIPT_MAX} characters at most.")),
        (None, Some(line)) => Ok(Script::Text { text: line.to_string() }),
        _ => Err(SCRIPT_HELP.into()),
    }
}

/// A schedule as the radio gives it. What its times and numbers say is `lap_save`'s to check.
fn when_of(value: &Value) -> Result<When, String> {
    let words = |key: &str| value.get(key).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string);
    let days = || -> Result<Vec<u8>, String> {
        match value.get("days") {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(days)) => days.iter().map(|day| day.as_u64().filter(|day| (1..=7).contains(day)).and_then(|day| u8::try_from(day).ok()).ok_or_else(|| DAYS.to_string())).collect(),
            Some(_) => Err(DAYS.into()),
        }
    };
    match value.get("kind").and_then(Value::as_str) {
        Some("daily") => Ok(When::Daily { at: words("at").ok_or("A daily job needs `at`, its time of day as HH:MM.")?, days: days()? }),
        Some("every") => {
            let minutes = value.get("minutes").and_then(Value::as_u64).and_then(|minutes| u32::try_from(minutes).ok()).ok_or("An `every` job needs `minutes`: how many minutes apart its runs are.")?;
            Ok(When::Every { minutes, from: words("from"), to: words("to"), days: days()? })
        }
        Some("once") => {
            let at = words("at").and_then(|at| local_moment(&at)).ok_or("A one-time job needs `at` as YYYY-MM-DD HH:MM, a time that is on this Mac's clock.")?;
            Ok(When::Once { at })
        }
        Some("refill") => Ok(When::Refill),
        Some("cron") => Ok(When::Cron { expr: words("expr").ok_or("A cron job needs `expr`: minute hour day-of-month month day-of-week.")? }),
        _ => Err(WHEN_HELP.into()),
    }
}

/// `start` with the model and the mode the radio named, each of its agent's list (`start.rs`);
/// `default` for the model is the one the agent picks itself.
fn started(mut start: Start, model: Option<&str>, mode: Option<&str>) -> Result<Start, String> {
    match start.agent {
        Agent::Claude => {
            if let Some(model) = model {
                start.model = match model {
                    "default" => None,
                    model => Some(Model::parse(model).ok_or_else(|| format!("`model` for Claude is one of: {}, or default.", CLAUDE_MODELS.join(", ")))?),
                };
            }
            if let Some(mode) = mode {
                start.mode = Mode::parse(mode).ok_or("`mode` for Claude is one of: normal, plan, acceptEdits.")?;
            }
        }
        Agent::Codex => {
            if let Some(model) = model {
                start.codex.model = match model {
                    "default" => None,
                    model => {
                        let ids: Vec<&str> = CodexModel::ALL.iter().map(|model| model.id()).collect();
                        Some(CodexModel::parse(model).ok_or_else(|| format!("`model` for Codex is one of: {}, or default.", ids.join(", ")))?)
                    }
                };
            }
            if let Some(mode) = mode {
                let words: Vec<&str> = CodexMode::ALL.iter().map(|mode| mode.word()).collect();
                start.codex.mode = CodexMode::parse(mode).ok_or_else(|| format!("`mode` for Codex is one of: {}.", words.join(", ")))?;
            }
        }
    }
    Ok(start)
}

/// What comes next of a job just saved.
fn next_text(lap: &Lap, held: bool) -> String {
    if held || lap.review {
        return HELD.into();
    }
    if lap.paused.is_some() {
        return "It stays paused.".into();
    }
    match lap.next_at {
        Some(at) => format!("Its next run is {}.", moment(at)),
        None if lap.when == When::Refill => format!("It runs at {0}'s next refill, once Pitwall has read {0}'s usage.", lap.start.agent.name()),
        None => "It has no moment to come.".into(),
    }
}

impl Core {
    /// The lap tools (`mcp::TOOLS`): `name` for `agent`'s session `session`, on `project` alone.
    /// Those that act are refused from a session of the laps' own.
    pub(super) fn radio_lap(&self, agent: Agent, project: &ProjectView, name: &str, session: Option<&str>, args: &Value) -> Outcome {
        if name != "lap_list" && self.radio_lap_layer(agent, session) {
            return refused(FROM_A_LAP);
        }
        match name {
            "lap_list" => self.radio_lap_list(project, args),
            "lap_add" => self.radio_lap_add(agent, project, args),
            "lap_edit" => self.radio_lap_edit(agent, project, args),
            "lap_pause" => self.radio_lap_pause(project, args),
            "lap_delete" => self.radio_lap_delete(project, args),
            _ => refused(format!("Unknown tool: {name}")),
        }
    }

    /// The project's jobs, each in a few lines; one of them (`lap`) whole: its prompt, its script,
    /// its last runs and what its last finished run left, read now.
    fn radio_lap_list(&self, project: &ProjectView, args: &Value) -> Outcome {
        let state = self.laps_of(&project.path);
        let behind = state.elsewhere.then(|| "Another Pitwall on this Mac runs these laps: what this one knows of them may be behind.".to_string());

        if let Some(id) = text(args, "lap") {
            let Some(view) = state.laps.iter().find(|view| view.lap.id == id) else {
                return refused(NO_SUCH_JOB);
            };
            let mut lines = job_lines(view);
            lines.extend(self.radio_lap_whole(view, project));
            lines.extend(behind);
            return (Reply::ok(lines.join("\n")), CallResult::LapRead { name: view.lap.name.clone() });
        }

        let count = state.laps.len();
        let mut lines = vec![match count {
            0 => "This project has no jobs in Pitwall's Laps.".to_string(),
            1 => "1 job in this project's laps:".to_string(),
            count => format!("{count} jobs in this project's laps:"),
        }];
        for view in state.laps.iter().take(LIST_MAX) {
            lines.push(String::new());
            lines.extend(job_lines(view));
        }
        if count > LIST_MAX {
            lines.push(format!("[left out: {} more jobs]", count - LIST_MAX));
        }
        lines.extend(behind);
        (Reply::ok(lines.join("\n")), CallResult::LapList { count })
    }

    /// The rest of a job read whole. What its last finished run left is read as the Laps page
    /// reads it when asked (`lap_result`), and kept nowhere.
    fn radio_lap_whole(&self, view: &LapView, project: &ProjectView) -> Vec<String> {
        let lap = &view.lap;
        let mut lines = Vec::new();
        if lap.kind.has_agent() {
            lines.extend(block("prompt", &lap.prompt));
        }
        match &lap.script {
            Some(Script::Command { id }) => lines.push(match project.commands.iter().find(|command| command.id == *id) {
                Some(command) => format!("  script: the saved command \"{}\"", command.name),
                None => "  script: a saved command that is gone".to_string(),
            }),
            Some(Script::Text { text }) => lines.extend(block("script", text)),
            None => {}
        }

        let runs = self.lap_runs(&lap.id);
        if !runs.is_empty() {
            lines.push("  last runs, the newest first:".into());
            lines.extend(runs.iter().take(RUNS_SHOWN).map(run_line));
        }
        // The newest run that ended and left something: an agent's words, a script's output.
        let Some(run) = runs.iter().find(|run| run.ended_at.is_some() && (run.session.is_some() || run.script)) else {
            return lines;
        };
        let Ok(result) = self.lap_result(&run.id) else {
            return lines;
        };
        let (said, text): (String, Vec<&str>) = match (&result.message, &result.script) {
            (Some(message), _) if !message.trim().is_empty() => {
                (format!("what {} last said in it", run.agent.unwrap_or(lap.start.agent).name()), message.lines().collect())
            }
            (_, Some(script)) if !script.is_empty() => ("its script's last output lines".to_string(), script.iter().map(String::as_str).collect()),
            _ if result.gone => {
                lines.push(format!("  last result ({}): its session's log is gone", moment(run.due)));
                return lines;
            }
            _ => return lines,
        };
        let (shown, _) = fitted(&text[text.len().saturating_sub(RESULT_LINES)..]);
        lines.push(format!("  last result ({}), {said}:", moment(run.due)));
        lines.push(shown);
        lines
    }

    /// A new job of the project's, as the Laps page's dialog starts one with what the call gave.
    /// A command line the agent wrote holds it, paused, for the user.
    fn radio_lap_add(&self, agent: Agent, project: &ProjectView, args: &Value) -> Outcome {
        let given = match Given::read(args, project) {
            Ok(given) => given,
            Err(why) => return refused(why),
        };
        if given.name.is_none() {
            return refused(format!("A job needs a `name`: one line of {NAME_MAX} characters at most."));
        }
        if given.kind.is_none() {
            return refused("A job needs a `kind`: agent, script or script_agent.");
        }
        if given.when.is_none() {
            return refused(format!("A job needs `when`. {WHEN_HELP}"));
        }
        // The dialog's new job (`blankLap` in `laps.ts`, the same as `Lap::default`): the account
        // in use, no result file, the computer not kept awake.
        let blank = Lap { place: Place::Project { path: project.path.clone() }, ..Lap::default() };
        let (lap, held) = match self.radio_lap_shaped(blank, None, given, agent, project) {
            Ok(shaped) => shaped,
            Err(why) => return refused(why),
        };
        match self.lap_save_radio(lap, &RadioSave { agent, project: &project.path, held, from: None }) {
            Ok(lap) => {
                let said = format!("Added \"{}\" (id {}) to this project's laps. {}", lap.name, lap.id, next_text(&lap, held));
                (Reply::ok(said), CallResult::LapAdded { name: lap.name, held })
            }
            Err(why) => refused(barred(why, false)),
        }
    }

    /// One of the project's jobs an agent added, changed by the fields given; a new command line
    /// holds it again. The fields are put into the job as it is kept when it is saved: one the
    /// window changed meanwhile (`Barred::Changed`) is read again, the user's change kept.
    fn radio_lap_edit(&self, agent: Agent, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = text(args, "lap") else {
            return refused(NO_ID);
        };
        for _ in 0..EDIT_TRIES {
            let Some(stored) = self.lap_of(id, &project.path) else {
                return refused(NO_SUCH_JOB);
            };
            if !radio_made(&stored) {
                return refused(USERS_CHANGE);
            }
            let given = match Given::read(args, project) {
                Ok(given) if given.is_empty() => return refused("Give the fields to change: those given replace the job's own."),
                Ok(given) => given,
                Err(why) => return refused(why),
            };
            let (lap, held) = match self.radio_lap_shaped(stored.clone(), Some(&stored), given, agent, project) {
                Ok(shaped) => shaped,
                Err(why) => return refused(why),
            };
            match self.lap_save_radio(lap, &RadioSave { agent, project: &project.path, held, from: Some(&stored) }) {
                Ok(lap) => {
                    let said = format!("Changed \"{}\". {}", lap.name, next_text(&lap, held));
                    return (Reply::ok(said), CallResult::LapEdited { name: lap.name, held });
                }
                Err(Barred::Changed) => continue,
                Err(why) => return refused(barred(why, false)),
            }
        }
        refused(barred(Barred::Changed, false))
    }

    /// `lap` (a new job, or `stored` as kept) with the fields `given` in place, made as the
    /// dialog sends a job (`built` in `LapEditDialog.vue`: what its kind has no use for left out);
    /// and whether its script is a command line the agent writes now, which holds it.
    fn radio_lap_shaped(&self, mut lap: Lap, stored: Option<&Lap>, given: Given, caller: Agent, project: &ProjectView) -> Result<(Lap, bool), String> {
        let had_agent = stored.is_some_and(|stored| stored.kind.has_agent());
        lap.place = Place::Project { path: project.path.clone() };
        lap.name = given.name.unwrap_or(lap.name);
        lap.kind = given.kind.unwrap_or(lap.kind);
        lap.prompt = given.prompt.unwrap_or(lap.prompt);
        lap.script = given.script.or(lap.script);
        lap.when = given.when.unwrap_or(lap.when);
        lap.place_run = given.place_run.unwrap_or(lap.place_run);
        lap.carry = given.carry.unwrap_or(lap.carry);
        lap.ends.after = given.ends_after.unwrap_or(lap.ends.after);
        lap.timeout_min = given.timeout_min.unwrap_or(lap.timeout_min);

        if lap.kind.has_agent() {
            // The caller's own agent for a new one; another one starts as the project starts it.
            let agent = given.agent.unwrap_or(if had_agent { lap.start.agent } else { caller });
            if !had_agent || agent != lap.start.agent {
                lap.start = project.settings.start_with(agent);
                lap.account = None;
            }
            if !self.agent_on(agent) {
                return Err(format!("{} is off in Pitwall (Settings › Agents), so no job is given to it.", agent.name()));
            }
            lap.start = started(lap.start, given.model.as_deref(), given.mode.as_deref())?;
            if lap.prompt.trim().is_empty() {
                return Err("A job with an agent needs a `prompt`: what the agent should do.".into());
            }
        } else {
            lap.start = Start::default();
            lap.prompt = String::new();
            lap.fuel = None;
            lap.carry = false;
            lap.account = None;
        }
        if lap.kind != LapKind::ScriptAgent {
            lap.gate.script = ScriptGate::None;
        }
        if !lap.kind.has_script() {
            lap.script = None;
        } else if lap.script.is_none() {
            return Err(format!("A job with a script needs `script`. {SCRIPT_HELP}"));
        }
        let held = matches!(lap.script, Some(Script::Text { .. })) && lap.script.as_ref() != stored.and_then(|stored| stored.script.as_ref());
        Ok((lap, held))
    }

    /// Pauses any of the project's jobs; resumes one only a pause holds.
    fn radio_lap_pause(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = text(args, "lap") else {
            return refused(NO_ID);
        };
        let Some(paused) = args.get("paused").and_then(Value::as_bool) else {
            return refused("`paused` is true to pause the job, false to resume it.");
        };
        match self.lap_pause_radio(id, &project.path, paused) {
            Ok(lap) => {
                let said = match lap.paused {
                    Some(_) => format!("\"{}\" is paused: it runs on no moment until it is resumed.", lap.name),
                    None => format!("\"{}\" is on. {}", lap.name, next_text(&lap, false)),
                };
                (Reply::ok(said), CallResult::LapPaused { name: lap.name, paused })
            }
            Err(why) => refused(barred(why, false)),
        }
    }

    /// Deletes one of the project's jobs an agent added, with its runs.
    fn radio_lap_delete(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = text(args, "lap") else {
            return refused(NO_ID);
        };
        match self.lap_delete_radio(id, &project.path) {
            Ok(lap) => (Reply::ok(format!("Deleted \"{}\" and its runs.", lap.name)), CallResult::LapDeleted { name: lap.name }),
            Err(why) => refused(barred(why, true)),
        }
    }
}
