//! What a lap is, as `laps.json` keeps it and as the window gets it. These types are the
//! contract between the core and the Turlar page (`src/lib/laps.ts` mirrors them): every field
//! is camelCase on the wire, and a field a file doesn't have takes its default, so a file of an
//! older Pitwall still reads.

use serde::{Deserialize, Serialize};

use crate::agent::Agent;
use crate::start::Start;

/// What a job does when its moment comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LapKind {
    /// Claude or Codex is given the prompt.
    #[default]
    Agent,
    /// The script runs first; its output goes into the prompt, and it may gate the agent.
    ScriptAgent,
    /// The script alone; no agent.
    Script,
}

impl LapKind {
    pub fn has_agent(self) -> bool {
        self != Self::Script
    }

    pub fn has_script(self) -> bool {
        self != Self::Agent
    }
}

/// Where a job runs: in a listed project's folder (its working folder for the script), or in a
/// folder the user picked. `path` is absolute, as the core resolved it when the job was saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Place {
    Project { path: String },
    Folder { path: String },
}

impl Place {
    pub fn path(&self) -> &str {
        match self {
            Self::Project { path } | Self::Folder { path } => path,
        }
    }
}

/// The script a job runs first: one of its project's saved commands, by id (never one marked
/// "ask first"), or a command line the user wrote in the dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Script {
    Command { id: String },
    Text { text: String },
}

/// When a job runs. Times of day are `HH:MM` on this Mac's clock; days are 1 (Monday) to 7
/// (Sunday), an empty list being every day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum When {
    /// Once a day at `at`, on `days` ("Her gün"; "Hafta içi" is days 1–5).
    Daily {
        at: String,
        #[serde(default)]
        days: Vec<u8>,
    },
    /// Every `minutes`, from `from` to `to` (both `HH:MM`, none: the whole day), on `days`.
    Every {
        minutes: u32,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        days: Vec<u8>,
    },
    /// Once, at `at` (ms).
    Once { at: u64 },
    /// When the job's agent's quota refills (Claude's 5-hour session, else its week; Codex's
    /// 5-hour window, else its week), as its usage reading has it.
    Refill,
    /// A five-field cron expression (minute hour day-of-month month day-of-week).
    Cron { expr: String },
}

impl Default for When {
    fn default() -> Self {
        Self::Daily { at: "09:00".into(), days: Vec::new() }
    }
}

/// What the script's end says about calling the agent (`Betik + ajan`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ScriptGate {
    /// The agent is always called after the script.
    #[default]
    None,
    /// Only when the script failed (its exit code isn't 0): "Betik başarılı biterse ajanı çağırma".
    OnFailure,
    /// Only when the script printed something: "Betik boş çıkarsa ajanı çağırma".
    OnOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Gate {
    pub script: ScriptGate,
    /// Skip when the folder's Git HEAD is the one the last run saw (a Git work tree only).
    pub git: bool,
    /// A result the same as the last run's is silent: told of nothing, grey.
    pub same_silent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FuelThen {
    #[default]
    Skip,
    Wait,
}

/// "<agent> haftalık %over üstündeyse atla / dolumu bekle".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuelGuard {
    pub over: u8,
    #[serde(default)]
    pub then: FuelThen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Missed {
    /// A moment missed (asleep, Pitwall closed) runs once on return, within `grace_min`.
    pub catch_up: bool,
    pub grace_min: u32,
}

impl Default for Missed {
    fn default() -> Self {
        Self { catch_up: true, grace_min: 120 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Overlap {
    /// The previous run still goes: this moment is skipped.
    #[default]
    Skip,
    /// It starts once the previous one ended, within the grace.
    Wait,
    /// The previous run is stopped, then this one starts.
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Where {
    /// In a Pitwall tab that doesn't take the view or the keyboard: it may ask for permissions.
    #[default]
    Tab,
    /// No tab: read-only tools alone, nothing can be asked.
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Retry {
    pub times: u8,
    pub every_min: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notify {
    pub done: bool,
    pub failed: bool,
    pub sound: bool,
}

impl Default for Notify {
    fn default() -> Self {
        Self { done: false, failed: true, sound: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Ends {
    pub at: Option<u64>,
    pub after: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PausedBy {
    User,
    /// `pause_after` failures in a row.
    Failures,
}

/// A job, as `laps.json` keeps it. The fields after `ends` are the core's own: what the window
/// sends of them is ignored by `lap_save`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Lap {
    /// `l-<base36>`; empty in a new job the window sends.
    pub id: String,
    pub name: String,
    pub kind: LapKind,
    pub place: Place,
    /// The agent, model and mode (`start.rs`' lists); unused by a `script` job.
    pub start: Start,
    /// The account of the job's agent it runs with, by id; none: the one in use. This Mac's own.
    pub account: Option<String>,
    pub script: Option<Script>,
    pub prompt: String,
    pub when: When,
    /// Dates (`YYYY-MM-DD`) a moment is skipped on ("Yalnız bu gün"); dropped once past.
    pub days_off: Vec<String>,
    /// Weekdays taken out of `when` by "Bundan sonra her <gün>" in the calendar, so "geri aç" can
    /// put them back; one the user put back by editing `when` leaves this list.
    pub weekdays_off: Vec<u8>,
    /// The switch.
    pub on: bool,
    pub paused: Option<PausedBy>,
    pub gate: Gate,
    pub timeout_min: Option<u32>,
    /// Keep the computer awake while a run goes.
    pub awake: bool,
    pub fuel: Option<FuelGuard>,
    #[serde(rename = "where")]
    pub place_run: Where,
    pub missed: Missed,
    pub overlap: Overlap,
    /// "Süreklilik": the previous run's result goes into the prompt.
    pub carry: bool,
    pub retry: Option<Retry>,
    /// Pause after this many failures in a row; none: never.
    pub pause_after: Option<u32>,
    pub notify: Notify,
    /// Runs kept (records and script output).
    pub keep: u32,
    /// Where each run's result is written: a path template under the home folder with
    /// `{date}`, `{time}`, `{name}` (or `{tarih}`, `{saat}`, `{ad}`); none: nowhere.
    pub file: Option<String>,
    pub ends: Ends,

    /* ---------- the core's own ---------- */
    /// The next moment (ms), none while off, paused or ended.
    pub next_at: Option<u64>,
    /// Failures in a row.
    pub streak: u32,
    /// Runs started on schedule so far (for `ends.after`).
    pub runs_done: u32,
    pub created_at: u64,
}

impl Default for Lap {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: LapKind::Agent,
            place: Place::Folder { path: String::new() },
            start: Start::default(),
            account: None,
            script: None,
            prompt: String::new(),
            when: When::default(),
            days_off: Vec::new(),
            weekdays_off: Vec::new(),
            on: true,
            paused: None,
            gate: Gate::default(),
            timeout_min: None,
            awake: false,
            fuel: None,
            place_run: Where::Tab,
            missed: Missed::default(),
            overlap: Overlap::Skip,
            carry: false,
            retry: None,
            pause_after: Some(3),
            notify: Notify::default(),
            keep: 30,
            file: None,
            ends: Ends::default(),
            next_at: None,
            streak: 0,
            runs_done: 0,
            created_at: 0,
        }
    }
}

/// How a run ended. `None` on a run while it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunResult {
    Ok,
    Failed,
    /// Not run: a gate, a guard, a day off, the user's skip, a missed moment.
    Skipped,
    /// Ran, and its result was the same as the last one's.
    Silent,
    /// Cut by a quit or a crash; not counted as a failure.
    Cut,
}

/// Why a run ended as it did, in Pitwall's own words (`laps.reason.<code>` in the catalogs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// The script gate: the agent wasn't called.
    Gate,
    /// No commit since the last run.
    Git,
    /// The fuel guard.
    Fuel,
    /// The fuel guard had no reading of the account the job runs with.
    NoReading,
    /// The job's agent is off in Settings › Agents.
    AgentOff,
    /// A moment missed past its grace (asleep, Pitwall closed).
    Missed,
    /// The previous run still went.
    Overlap,
    /// A day off.
    DayOff,
    /// The user skipped this moment (Bu seferi atla).
    UserSkip,
    /// The saved command is marked "ask first".
    AskFirst,
    /// It ran past its time and was stopped.
    Timeout,
    /// The script or the agent's process ended with an error (`code`).
    Exit,
    /// The folder is gone, or no longer inside its project.
    NotFolder,
    /// The project is no longer listed.
    NotListed,
    /// The user stopped it.
    Stopped,
    /// Pitwall quit or crashed while it went.
    Quit,
    /// The script or the agent couldn't be started (`laps.json` unwritable, no shell, no CLI).
    NotStarted,
}

/// One run, as `lap-runs/<lap>.json` keeps it: no text of anyone's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LapRun {
    /// `r-<base36>`.
    pub id: String,
    pub lap: String,
    /// The moment it was planned for (ms); a run started by hand: when it was asked for.
    pub due: u64,
    /// Started by the user (Şimdi çalıştır, Şimdi dene, Yeniden çalıştır), not by the schedule.
    pub by_hand: bool,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub result: Option<RunResult>,
    pub reason: Option<Reason>,
    /// The script's exit code, or the background agent's.
    pub code: Option<i32>,
    /// The agent's session (a UUID), once known.
    pub session: Option<String>,
    pub agent: Option<Agent>,
    /// The account it ran with, by id; none: the main one.
    pub account: Option<String>,
    /// The terminal tab it runs in, while it does.
    pub tab: Option<u64>,
    /// The file its result was written to.
    pub file: Option<String>,
    /// SHA-256 (hex) of its result's text, for "same as before"; never the text.
    pub hash: Option<String>,
    /// The folder's Git HEAD when it ran, for the git gate.
    pub head: Option<String>,
    /// Tries so far (a retry is the same run).
    pub tries: u32,
    /// The script ran (its output is kept beside the record).
    pub script: bool,
    /// It ended done or failed, and its result wasn't opened since (`lap_read`): the page's orange
    /// dot. A run of a file written before there was this reads as read.
    pub unread: bool,
}

/* ---------- what the window gets ---------- */

/// A run's place in "Son 10" and the calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMark {
    pub id: String,
    pub due: u64,
    pub result: Option<RunResult>,
    pub unread: bool,
}

/// What a job's run in progress is at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapRunning {
    pub run: String,
    pub started_at: u64,
    /// `script` | `agent` | `waiting` (for the refill or the previous run) | `retry`.
    pub phase: &'static str,
    pub tab: Option<u64>,
    /// A permission it waits on: the tool's name only.
    pub permission: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapView {
    #[serde(flatten)]
    pub lap: Lap,
    /// The listed project's name, for a project's job.
    pub project: Option<String>,
    /// The folder as shown: under the home folder as `~/…`.
    pub folder: String,
    /// The last ten runs, oldest first.
    pub last: Vec<RunMark>,
    pub running: Option<LapRunning>,
    /// The folder is a Git work tree (the git gate can be set).
    pub git: bool,
    /// No usage reading of the account its agent runs with yet: a refill job has no moment, and
    /// the fuel guard skips its runs.
    pub no_reading: bool,
    /// Its kept runs not read yet (any of them, not only the last ten): the tower's dot.
    pub unread: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LapsState {
    pub laps: Vec<LapView>,
    /// Jobs whose last run failed, or that paused themselves: the sidebar's count.
    pub failing: usize,
    /// The kept runs not read yet, of every job: the sidebar's orange dot.
    pub unread: usize,
    /// Another Pitwall holds the laps (`laps.lock`): nothing runs from this one.
    pub elsewhere: bool,
}

/// A run's result, read when the dialog opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapResult {
    pub run: LapRun,
    /// The command line the script ran, as it was then (a saved command may have changed since).
    pub command: Option<String>,
    /// The script's kept output, its last lines.
    pub script: Option<Vec<String>>,
    /// What the agent last said, read from its session's log now.
    pub message: Option<String>,
    /// The run had a session whose log is no longer there.
    pub gone: bool,
}

/// What a run's dialog shows in its terminal body (`Core::lap_terminal`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LapTerminal {
    /// The terminal to show: a tab of the laps' own (`TerminalView::apart`), the run's or one its
    /// session was opened again in.
    Tab { id: u64 },
    /// Its agent runs in the background, read only: there is no terminal.
    Background,
    /// No agent yet: the run is in its script, or waits (the refill, the previous run, a retry).
    NotYet,
    /// Its session runs outside the laps' tabs (an editor, a terminal app, a panel tab the user
    /// opened it in): brought up there, nothing to show here.
    Elsewhere,
}

/// One day of the calendar popover's month.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDay {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The kept runs of that day, in their order.
    pub results: Vec<RunResult>,
    /// Moments still to come that day.
    pub planned: u32,
    /// A job has a day off on it.
    pub off: bool,
}

/// A job on one day of the popover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayRow {
    pub lap: String,
    pub name: String,
    /// Its moments that day still to come (ms), the first ones; `count` is all of them, `last`
    /// the last.
    pub at: Vec<u64>,
    pub count: u32,
    pub last: Option<u64>,
    /// Off that day: `day` ("Yalnız bu gün") or `weekday` ("Bundan sonra her <gün>").
    pub off: Option<&'static str>,
    /// The runs kept of that day.
    pub runs: Vec<RunMark>,
}

/// A folder picked for a job of no listed project (`lap_pick_folder`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PickedFolder {
    pub path: String,
    /// As shown: under the home folder as `~/…`.
    pub shown: String,
}
