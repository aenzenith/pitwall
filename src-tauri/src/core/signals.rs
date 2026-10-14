//! Everything worth telling the user goes through one door: a waiting Claude, a server that
//! crashed, a command that ended. `signal` decides what becomes of it on this Mac (banner, sound,
//! the list of what was missed) and hands it on to the phone's channel (`relay.rs`).
//!
//! In this order: a muted project is not heard of at all; an event Settings switched off shows no
//! banner (its sound is the Sounds page's, as ever); while it is quiet (a period the user
//! started, or the quiet hours) nothing shows and, when Settings says so, nothing sounds, and
//! what would have shown goes on the list of what was missed, a question or a permission aside
//! when those are let through. What quiet did not hold is offered to the phone, which has its own
//! list of events; what it takes is on the list too.
//!
//! Every sound the core plays starts here as well (`sound`), so quiet holds a server's and a
//! command's sounds with the rest.

use chrono::{DateTime, Days, Local, NaiveDate, NaiveTime, TimeDelta, TimeZone, Timelike};

use super::*;
use crate::deps::{CheckState, DepReport, ToolState};
use crate::settings::{Notifications, QuietPeriod, Sounds};

/// One turn this long is told of, once (`ClaudeLongTurn`).
const LONG_TURN_MS: u64 = 20 * 60 * 1000;
/// The list of what was missed holds this many, the newest.
const MISSED_MAX: usize = 20;
const HOUR_MS: u64 = 60 * 60 * 1000;
/// "Until tomorrow morning" ends then, unless the quiet hours say when the morning is.
const MORNING: u32 = 8 * 60;
/// Quiet hours that end later than this end no morning: theirs is a working day's.
const NOON: u32 = 12 * 60;
/// Before this hour the night isn't over: "tomorrow morning" is the morning that comes.
const NIGHT_ENDS: u32 = 4;
/// A banner that isn't Claude's says what it is about in its identifier:
/// `pitwall:signal:<kind>:<path>`, and after a unit separator the command's name. Claude's own
/// are the project's path, as they always were.
const BANNER: &str = "pitwall:signal:";
const DETAIL: char = '\u{1f}';
/// The first run's test banner (`test_banner`); a click on it brings Pitwall's window up (lib.rs).
pub const TEST_BANNER: &str = "pitwall:signal:test";

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignalKind {
    /// Claude asked the user something.
    ClaudeAsking,
    /// Claude waits for a permission; `detail` is the tool's name.
    ClaudePermission,
    /// Claude finished its turn.
    ClaudeFinished,
    /// Claude has been in one turn for a long time.
    ClaudeLongTurn,
    /// The project's dev server crashed.
    ServerCrashed,
    /// A command of the project ended well; `detail` is the command's name.
    CommandDone,
    /// A command of the project failed; `detail` is the command's name.
    CommandFailed,
    /// The Dependencies page found something to install, or a migration that waits.
    DepsWaiting,
    /// Claude's usage passed the alert's mark. About no project: `path` and `name` are empty.
    FuelHigh,
    /// A card went to the review column.
    CardReview,
}

/// One thing worth telling the user, as its source saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub kind: SignalKind,
    /// The project's folder; empty when it is about no project.
    pub path: String,
    /// The project's name as listed.
    pub name: String,
    /// A permission's tool, a command's name: see `SignalKind`. Nothing else.
    pub detail: Option<String>,
}

impl SignalKind {
    /// Claude waits on the user with it: one of these is listed per project, the newest, and it
    /// goes once the turn is seen.
    fn waits(self) -> bool {
        matches!(self, Self::ClaudeAsking | Self::ClaudePermission | Self::ClaudeFinished)
    }

    /// A question or a permission: what quiet lets through when Settings says so.
    fn asks(self) -> bool {
        matches!(self, Self::ClaudeAsking | Self::ClaudePermission)
    }

    /// Opening it brings a Claude session up, not Pitwall's window.
    pub fn is_claudes(self) -> bool {
        self.waits() || self == Self::ClaudeLongTurn
    }

    /// As serde writes it: `server-crashed`.
    fn id(self) -> String {
        serde_json::to_value(self).ok().and_then(|id| id.as_str().map(str::to_string)).unwrap_or_default()
    }

    fn parse(id: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(id.to_string())).ok()
    }
}

/// How long a quiet period the user starts lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietFor {
    Hour,
    /// Until tomorrow morning: the quiet hours' end when they are on and end in a morning, else 08:00.
    Morning,
    /// Until it is switched off.
    Open,
}

impl QuietFor {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "hour" => Some(Self::Hour),
            "morning" => Some(Self::Morning),
            "open" => Some(Self::Open),
            _ => None,
        }
    }
}

/// Quiet now: until when (`None`: until it is switched off), and whether the quiet hours made it so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietView {
    pub until: Option<u64>,
    pub hours: bool,
}

/// Something the user wasn't told while it was quiet, or was told on the phone: a line of the
/// popover's list. Kept while the app runs, never written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missed {
    pub kind: SignalKind,
    pub path: String,
    pub name: String,
    /// What its banner names: a permission's tool, a command's name, the fuel alert's line.
    pub detail: Option<String>,
    pub at: u64,
}

/// A banner: what a click on it opens (`open_banner`), and its two lines.
struct Banner {
    id: String,
    title: String,
    body: String,
}

/// What becomes of a signal.
#[derive(Debug, Default, PartialEq, Eq)]
struct Verdict {
    banner: bool,
    /// The sound it plays, by id.
    sound: Option<String>,
    /// Held: it goes on the list of what was missed.
    missed: bool,
    /// The phone's channel is asked.
    relay: bool,
}

/// Each trouble told lately, and when (`trouble`).
pub(super) type Told = HashMap<(SignalKind, String, Option<String>), Instant>;

/// What each source of a project's dependencies said last (`deps_waiting`): an ecosystem's check
/// (`install`) or a migration tool's read (`migrate`), and whether something waited there.
pub(super) type DepsKnown = HashMap<String, HashMap<(&'static str, &'static str), bool>>;

fn is_muted(own: &Notifications, path: &str) -> bool {
    !path.is_empty() && own.muted.iter().any(|muted| same_path(muted, path))
}

pub(super) fn muted(inner: &Inner, path: &str) -> bool {
    is_muted(&inner.settings.notifications, path)
}

/// The decision itself. `quiet`: it is quiet now. `again`: this trouble was told a moment ago
/// (`trouble`), so it is only heard.
fn decide(settings: &Settings, signal: &Signal, quiet: bool, again: bool) -> Verdict {
    use SignalKind::*;

    let own = &settings.notifications;
    if is_muted(own, &signal.path) {
        return Verdict::default();
    }

    // Whether Settings wants a banner for it, and the sound the Sounds page gives it. Claude's
    // sounds come with its notifications.
    let sounds = &settings.sounds;
    let (on, sound) = match signal.kind {
        ClaudeAsking | ClaudePermission => (settings.notify, settings.notify.then_some(&sounds.claude_asking)),
        ClaudeFinished => (settings.notify, settings.notify.then_some(&sounds.claude_finished)),
        // One of Claude's: off with the rest of them.
        ClaudeLongTurn => (settings.notify && own.long_turn, None),
        ServerCrashed => (own.server_crashed, Some(&sounds.server_crashed)),
        CommandDone | CommandFailed => (own.command, Some(&sounds.command_done)),
        DepsWaiting => (own.deps, None),
        FuelHigh => (settings.fuel_alert, None),
        // Never a banner on the Mac: it is there for the phone's list.
        CardReview => (false, None),
    };
    let sound = sound.filter(|id| !id.is_empty());

    if again {
        return Verdict { sound: sound.filter(|_| !(quiet && own.sounds_too)).cloned(), ..Verdict::default() };
    }

    let held = quiet && !(signal.kind.asks() && own.questions_through);
    Verdict {
        banner: on && !held,
        sound: sound.filter(|_| !(held && own.sounds_too)).cloned(),
        // What quiet kept from showing is listed: an event Settings switched off, or one that
        // has no banner, was never going to show.
        missed: on && held,
        relay: !held,
    }
}

/// A trouble as `Told` keeps it. A server or a worker that keeps crashing comes back three times
/// in a few seconds: it is told once, and heard each time.
fn trouble(signal: &Signal) -> Option<(SignalKind, String, Option<String>)> {
    let kept = matches!(signal.kind, SignalKind::ServerCrashed | SignalKind::CommandFailed);
    kept.then(|| (signal.kind, signal.path.clone(), signal.detail.clone()))
}

fn banner_id(signal: &Signal) -> String {
    if signal.kind.is_claudes() {
        return signal.path.clone();
    }
    let mut id = format!("{BANNER}{}:{}", signal.kind.id(), signal.path);
    if let Some(detail) = &signal.detail {
        id.push(DETAIL);
        id.push_str(detail);
    }
    id
}

/// A signal's banner; none for the fuel alert, which brings its own (`fuel_high`), and for a
/// card in review, which has none.
fn banner(signal: &Signal) -> Option<Banner> {
    use SignalKind::*;

    let detail = signal.detail.as_deref().unwrap_or_default();
    let body = match signal.kind {
        ClaudeAsking => t!("core.notify.asking"),
        ClaudePermission if signal.detail.is_some() => t!("core.signal.permission", tool = detail),
        ClaudePermission => t!("core.notify.permission"),
        ClaudeFinished => t!("core.notify.finished"),
        ClaudeLongTurn => t!("core.signal.longTurn"),
        ServerCrashed => t!("core.signal.serverCrashed"),
        CommandDone => t!("core.signal.commandDone", name = detail),
        CommandFailed => t!("core.signal.commandFailed", name = detail),
        DepsWaiting => t!("core.signal.deps"),
        FuelHigh | CardReview => return None,
    };
    Some(Banner { id: banner_id(signal), title: signal.name.clone(), body })
}

/// A banner Claude's mark doesn't belong on: a server's, a command's, the Garage's.
pub fn plain_banner(id: &str) -> bool {
    id.starts_with(BANNER)
}

/// Puts `entry` first on the list: one per project and kind, of Claude's waiting turns one per
/// project, the newest `MISSED_MAX` in all.
fn miss(list: &mut Vec<Missed>, entry: Missed) {
    list.retain(|known| !(same_path(&known.path, &entry.path) && (known.kind == entry.kind || (known.kind.waits() && entry.kind.waits()))));
    list.insert(0, entry);
    list.truncate(MISSED_MAX);
}

fn missed(signal: &Signal, banner: Option<&Banner>, at: u64) -> Missed {
    // The fuel alert has no detail of its own: its line is the banner's.
    let line = banner.filter(|_| signal.kind == SignalKind::FuelHigh).map(|banner| banner.title.clone());
    Missed { kind: signal.kind, path: signal.path.clone(), name: signal.name.clone(), detail: signal.detail.clone().or(line), at }
}

/// The list as the snapshot gives it: a line goes with a project that left the list, and bears
/// the name its project has now. What is no project's (the fuel alert) stays as it is.
pub(super) fn listed(missed: &[Missed], projects: &[ProjectView]) -> Vec<Missed> {
    let named = |entry: &Missed| {
        if entry.path.is_empty() {
            return Some(entry.clone());
        }
        let project = projects.iter().find(|project| same_path(&project.path, &entry.path))?;
        Some(Missed { name: project.name.clone(), ..entry.clone() })
    };
    missed.iter().filter_map(named).collect()
}

/* ---------- quiet: a period the user started, and the quiet hours ---------- */

fn local(ms: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(i64::try_from(ms).ok()?).single()
}

/// `HH:MM` as minutes into the day.
fn minutes(clock: &str) -> Option<u32> {
    let (hours, minutes) = clock.split_once(':')?;
    let (hours, minutes) = (hours.parse::<u32>().ok()?, minutes.parse::<u32>().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// The moment the local clock reads `minutes` into `day`; where the clocks skip it, an hour on.
fn clock_on(day: NaiveDate, minutes: u32) -> Option<u64> {
    let time = day.and_time(NaiveTime::from_hms_opt(minutes / 60, minutes % 60, 0)?);
    let moment = Local.from_local_datetime(&time).earliest().or_else(|| Local.from_local_datetime(&(time + TimeDelta::hours(1))).earliest())?;
    u64::try_from(moment.timestamp_millis()).ok()
}

/// The next moment after `now` at which the local clock reads `minutes` into the day.
fn next_at(now: u64, minutes: u32) -> Option<u64> {
    let today = local(now)?.date_naive();
    (0..=1).find_map(|days| clock_on(today.checked_add_days(Days::new(days))?, minutes).filter(|at| *at > now))
}

/// The end of the quiet hours `now` falls in; none outside them, and with them off.
fn hours_end(own: &Notifications, now: u64) -> Option<u64> {
    let hours = &own.quiet_hours;
    if !hours.on {
        return None;
    }
    let (from, to) = (minutes(&hours.from)?, minutes(&hours.to)?);
    let time = local(now)?;
    let at = time.hour() * 60 + time.minute();
    // Over midnight when they end before they begin.
    let inside = if from <= to { from <= at && at < to } else { at >= from || at < to };
    if inside { next_at(now, to) } else { None }
}

/// Whether it is quiet at `now`: a period the user started that isn't over, else the quiet hours
/// (unless they were switched off by hand for this once).
pub(super) fn quiet_at(own: &Notifications, now: u64) -> Option<QuietView> {
    if let Some(period) = own.quiet {
        if period.until.is_none_or(|until| now < until) {
            return Some(QuietView { until: period.until, hours: false });
        }
    }
    // The cheap questions first: this is asked with every snapshot.
    if !own.quiet_hours.on || own.quiet_hours.off_until.is_some_and(|off| now < off) {
        return None;
    }
    hours_end(own, now).map(|until| QuietView { until: Some(until), hours: true })
}

/// When "until tomorrow morning" ends: at the quiet hours' end when they are on and end in a
/// morning, else at 08:00. Asked in the small hours it is the morning that comes, later in the
/// day the one after the night.
fn morning(own: &Notifications, now: u64) -> u64 {
    let wake = Some(&own.quiet_hours).filter(|hours| hours.on).and_then(|hours| minutes(&hours.to)).filter(|to| *to < NOON).unwrap_or(MORNING);
    let found = local(now).and_then(|time| {
        let today = time.date_naive();
        let coming = clock_on(today, wake).filter(|at| time.hour() < NIGHT_ENDS && *at > now);
        coming.or_else(|| clock_on(today.checked_add_days(Days::new(1))?, wake))
    });
    found.unwrap_or(now + 12 * HOUR_MS)
}

/* ---------- what the sources hand in ---------- */

/// A project's name as it is listed, for a path one of its sources gave.
pub(super) fn name_of(inner: &Inner, path: &str) -> String {
    inner.favourites.iter().find(|favourite| same_path(&favourite.path, path)).map(|favourite| favourite.name.clone()).unwrap_or_else(|| folder_name(path))
}

/// The sessions of listed projects that have been in one turn for twenty minutes, each told once
/// for that turn. Only the hook tells when a turn began (a prompt sent, a question answered); a
/// session known from its log alone is never told of. The first scan after launch only takes
/// note, as it does of waiting turns.
pub(super) fn long_turns(inner: &mut Inner, sessions: &[SessionState], first: bool) -> Vec<Signal> {
    let now = now_ms();
    let working = |session: &&SessionState| session.hooked && session.phase == SessionPhase::Working && !session.path.is_empty();
    let long: Vec<(&SessionState, u64)> =
        sessions.iter().filter(working).filter_map(|session| Some((session, session.since.filter(|since| now.saturating_sub(*since) >= LONG_TURN_MS)?))).collect();

    // A turn that ended (the session's phase began anew) is forgotten, and what is listed of it
    // goes: the next one is told of again. A session that fell silent reads idle for a while, or
    // isn't scanned at all: by when it began, that is the same turn still.
    let over: Vec<&SessionState> = sessions.iter().filter(|session| inner.long_turns.get(&session.id).is_some_and(|since| session.since != Some(*since))).collect();
    for session in over {
        inner.long_turns.remove(&session.id);
        inner.missed.retain(|entry| !(entry.kind == SignalKind::ClaudeLongTurn && same_path(&entry.path, &session.path)));
    }

    let mut found = Vec::new();
    for (session, since) in long {
        if inner.long_turns.get(&session.id) == Some(&since) {
            continue;
        }
        inner.long_turns.insert(session.id.clone(), since);
        if first {
            continue;
        }
        found.push(Signal { kind: SignalKind::ClaudeLongTurn, path: session.path.clone(), name: name_of(inner, &session.path), detail: None });
    }
    found
}

/// Claude's waiting turns on the list of what was missed go once their project waits no more
/// (`waiting`: the projects that do, as just scanned): the turn was seen, here or in its window.
pub(super) fn turns_seen(inner: &mut Inner, waiting: &HashMap<String, Turn>) {
    inner.missed.retain(|entry| !entry.kind.waits() || waiting.contains_key(&entry.path));
}

/// The same for one project, looked at just now (`mark_seen`) or waiting anew (`tell`): the
/// long turn listed of it is over, or seen, with that. True when a line went.
pub(super) fn turn_seen(inner: &mut Inner, path: &str) -> bool {
    let before = inner.missed.len();
    inner.missed.retain(|entry| !(entry.kind.is_claudes() && same_path(&entry.path, path)));
    inner.missed.len() != before
}

impl Core {
    /// The one door.
    pub(super) fn signal(&self, signal: Signal) {
        self.signals(vec![signal]);
    }

    /// Several at once (the turns one scan found waiting): each on its own, one sound for the lot.
    pub(super) fn signals(&self, batch: Vec<Signal>) {
        self.tell(batch.into_iter().map(|signal| (banner(&signal), signal)).collect(), true);
    }

    /// Claude's usage passed the alert's mark (`fuel.rs`, which words the banner and names it
    /// `id`: a click on it opens the Fuel page). `again`: the door had this one before, while the
    /// alert was off, so the phone isn't asked a second time. True when the user was told of it,
    /// by its banner or on the list.
    pub fn fuel_high(&self, id: &str, title: String, body: String, again: bool) -> bool {
        let signal = Signal { kind: SignalKind::FuelHigh, path: String::new(), name: String::new(), detail: None };
        let told = self.tell(vec![(Some(Banner { id: id.to_string(), title, body }), signal)], !again);
        told.iter().any(|verdict| verdict.banner || verdict.missed)
    }

    /// A banner the user asked for (the first run's "Send a test"), so nothing holds it: not
    /// quiet, not an event's switch. A banner only: no sound, nothing listed, nothing to the phone.
    pub fn test_banner(&self) {
        self.emit(CoreEvent::Notify { path: TEST_BANNER.to_string(), title: "Pitwall".into(), body: t!("core.notify.test") });
    }

    /// `phone`: its channel is asked. What became of each comes back.
    fn tell(&self, batch: Vec<(Option<Banner>, Signal)>, phone: bool) -> Vec<Verdict> {
        if batch.is_empty() {
            return Vec::new();
        }
        let now = now_ms();

        // Decided under the lock, done once it is let go.
        let (verdicts, mut listed) = {
            let mut inner = self.lock();
            let inner = &mut *inner;
            let quiet = quiet_at(&inner.settings.notifications, now).is_some();
            let mut listed = false;
            let verdicts: Vec<Verdict> = batch
                .iter()
                .map(|(banner, signal)| {
                    let trouble = trouble(signal);
                    let again = trouble.as_ref().is_some_and(|trouble| inner.told.get(trouble).is_some_and(|at| at.elapsed() < STABLE));
                    let verdict = decide(&inner.settings, signal, quiet, again);
                    // Told is what was shown: a crash quiet held leaves the next one its banner.
                    if let (true, Some(trouble)) = (verdict.banner, trouble) {
                        inner.told.retain(|_, at| at.elapsed() < STABLE);
                        inner.told.insert(trouble, Instant::now());
                    }
                    // A newer turn of Claude's takes the place of the one that was held.
                    if signal.kind.waits() {
                        listed |= turn_seen(inner, &signal.path);
                    }
                    if verdict.missed {
                        miss(&mut inner.missed, missed(signal, banner.as_ref(), now));
                        listed = true;
                    }
                    verdict
                })
                .collect();
            (verdicts, listed)
        };

        let mut sound: Option<&String> = None;
        let mut handed: Vec<Missed> = Vec::new();
        for ((banner, signal), verdict) in batch.iter().zip(&verdicts) {
            if let (true, Some(banner)) = (verdict.banner, banner) {
                self.emit(CoreEvent::Notify { path: banner.id.clone(), title: banner.title.clone(), body: banner.body.clone() });
            }
            // One sound for the lot; a question outranks the rest.
            if let Some(id) = &verdict.sound {
                if sound.is_none() || signal.kind.asks() {
                    sound = Some(id);
                }
            }
            // The phone has its own list of events: it is asked whatever this Mac shows.
            if phone && verdict.relay && self.relay(signal) {
                handed.push(missed(signal, banner.as_ref(), now));
            }
        }
        if let Some(id) = sound {
            self.emit(CoreEvent::Sound(id.clone()));
        }

        // Told on the phone: the user is away, and finds it on the list when back.
        if !handed.is_empty() {
            let mut inner = self.lock();
            for entry in handed {
                miss(&mut inner.missed, entry);
            }
            listed = true;
        }
        if listed {
            self.notify();
        }
        verdicts
    }

    /// A sound that is no signal's own (servers that came up, in `paths`), once for them all:
    /// nothing for muted projects, nor while quiet holds the sounds too.
    pub(super) fn sound(&self, paths: &[&str], which: impl Fn(&Sounds) -> &String) {
        let id = {
            let inner = self.lock();
            let own = &inner.settings.notifications;
            let id = which(&inner.settings.sounds);
            if id.is_empty() || paths.iter().all(|path| is_muted(own, path)) || (own.sounds_too && quiet_at(own, now_ms()).is_some()) {
                return;
            }
            id.clone()
        };
        self.emit(CoreEvent::Sound(id));
    }

    /// A project's name as it is listed.
    pub(super) fn project_name(&self, path: &str) -> String {
        name_of(&self.lock(), path)
    }

    /// From the Garage's reports (`dependencies.rs`): a project that newly has something to
    /// install, or a migration that newly waits, is told of once. What a check or a migration
    /// tool says the first time it is read is only noted, so a start tells of nothing that was
    /// there before it.
    pub(super) fn deps_waiting(&self, reports: &[DepReport]) {
        let found: Vec<Signal> = {
            let mut inner = self.lock();
            let inner = &mut *inner;
            inner.deps_known.retain(|path, _| reports.iter().any(|report| &report.path == path));

            let mut found = Vec::new();
            for report in reports {
                let known = inner.deps_known.entry(report.path.clone()).or_default();
                let mut fresh = false;
                let mut note = |source: (&'static str, &'static str), waits: bool| fresh |= known.insert(source, waits) == Some(false) && waits;

                for check in report.checks.iter().filter(|check| check.state != CheckState::Unknown) {
                    note(("install", check.ecosystem), check.state == CheckState::Install && check.tool == ToolState::Ok);
                }
                // A read that failed keeps the list from before: it says nothing new.
                for read in report.migrations.iter().filter(|read| read.at > 0 && !read.running && read.error.is_none()) {
                    note(("migrate", read.tool), !read.pending.is_empty());
                }
                if fresh {
                    found.push(Signal { kind: SignalKind::DepsWaiting, path: report.path.clone(), name: name_of(inner, &report.path), detail: None });
                }
            }
            found
        };
        self.signals(found);
    }

    /* ---------- quiet, muted projects and the event switches ---------- */

    fn change_notifications(&self, change: impl FnOnce(&mut Notifications)) {
        change(&mut self.lock().settings.notifications);
        self.save_settings();
        self.notify();
    }

    /// Quiet from now on, for `span`. The quiet hours count again after it.
    pub fn quiet_start(&self, span: QuietFor) {
        let now = now_ms();
        self.change_notifications(|own| {
            let until = match span {
                QuietFor::Hour => Some(now + HOUR_MS),
                QuietFor::Morning => Some(morning(own, now)),
                QuietFor::Open => None,
            };
            own.quiet = Some(QuietPeriod { until });
            own.quiet_hours.off_until = None;
        });
    }

    /// Quiet is over: the period the user started, and the quiet hours under way for this once.
    /// What was missed stays listed.
    pub fn quiet_end(&self) {
        let now = now_ms();
        self.change_notifications(|own| {
            own.quiet = None;
            own.quiet_hours.off_until = hours_end(own, now);
        });
    }

    /// One of quiet's two options: `questions` (questions and permissions still come through) or
    /// `sounds` (the sounds are held too).
    pub fn set_quiet_option(&self, option: &str, on: bool) -> Result<(), String> {
        if !matches!(option, "questions" | "sounds") {
            return Err(format!("unknown option: {option}"));
        }
        self.change_notifications(|own| match option {
            "questions" => own.questions_through = on,
            _ => own.sounds_too = on,
        });
        Ok(())
    }

    /// The quiet hours: on or off, from when to when (`HH:MM`). Changed, they count from now on,
    /// whatever was switched off by hand.
    pub fn set_quiet_hours(&self, on: bool, from: &str, to: &str) -> Result<(), String> {
        if let Some(wrong) = [from, to].into_iter().find(|clock| minutes(clock).is_none()) {
            return Err(format!("not a time: {wrong}"));
        }
        self.change_notifications(|own| {
            own.quiet_hours.on = on;
            own.quiet_hours.from = from.to_string();
            own.quiet_hours.to = to.to_string();
            own.quiet_hours.off_until = None;
        });
        Ok(())
    }

    /// Whether an event shows a banner: `claude` (Claude waits: `Settings::notify`),
    /// `serverCrashed`, `command`, `longTurn`, `deps` or `fuel` (the fuel alert:
    /// `Settings::fuel_alert`).
    pub fn set_notification(&self, event: &str, on: bool) -> Result<(), String> {
        {
            let mut inner = self.lock();
            let settings = &mut inner.settings;
            match event {
                "claude" => settings.notify = on,
                "serverCrashed" => settings.notifications.server_crashed = on,
                "command" => settings.notifications.command = on,
                "longTurn" => settings.notifications.long_turn = on,
                "deps" => settings.notifications.deps = on,
                "fuel" => settings.fuel_alert = on,
                _ => return Err(format!("unknown event: {event}")),
            }
        }
        self.save_settings();
        self.notify();
        Ok(())
    }

    /// A muted project is told of nothing; what was listed of it goes too.
    pub fn mute_project(&self, path: &str, muted: bool) {
        {
            let mut inner = self.lock();
            let path = super::snapshot::listed_path(&inner, path);
            let own = &mut inner.settings.notifications;
            own.muted.retain(|known| !same_path(known, &path));
            if muted {
                own.muted.push(path.clone());
                inner.missed.retain(|entry| !same_path(&entry.path, &path));
            }
        }
        self.save_settings();
        self.notify();
    }

    /* ---------- what was missed, and what a click opens ---------- */

    /// A line of the list is opened: what a click on its banner would bring up, and the line goes.
    pub fn missed_open(self: &Arc<Self>, path: &str, kind: SignalKind) {
        let entry = {
            let mut inner = self.lock();
            let at = inner.missed.iter().position(|entry| entry.kind == kind && entry.path == path);
            at.map(|at| inner.missed.remove(at))
        };
        let Some(entry) = entry else {
            return;
        };
        self.notify();
        self.open_signal(entry.kind, &entry.path, entry.detail.as_deref());
    }

    pub fn missed_clear(&self) {
        self.lock().missed.clear();
        self.notify();
    }

    /// A click on a banner, by its identifier: Claude's are their project's path (its waiting
    /// session comes up where it runs: `open_claude`), the others say what they are about.
    pub fn open_banner(self: &Arc<Self>, id: &str) {
        let about = id.strip_prefix(BANNER).and_then(|rest| rest.split_once(':')).and_then(|(kind, rest)| Some((SignalKind::parse(kind)?, rest)));
        let Some((kind, rest)) = about else {
            return self.open_claude(id);
        };
        let (path, detail) = match rest.split_once(DETAIL) {
            Some((path, detail)) => (path, Some(detail)),
            None => (rest, None),
        };
        self.open_signal(kind, path, detail);
    }

    /// What a signal brings up: Claude's, the session where it runs; a server's or a command's,
    /// the project in Pitwall's window (the command's output with it); the rest, their page.
    fn open_signal(self: &Arc<Self>, kind: SignalKind, path: &str, detail: Option<&str>) {
        use SignalKind::*;

        match kind {
            ClaudeAsking | ClaudePermission | ClaudeFinished => self.open_claude(path),
            ClaudeLongTurn => {
                let working = self.lock().claude_sessions.get(path).and_then(|sessions| sessions.iter().find(|session| session.phase == SessionPhase::Working).map(|session| session.id.clone()));
                match working {
                    Some(session) => self.reveal_claude(path, &session),
                    None => self.open_claude(path),
                }
            }
            ServerCrashed => self.emit(CoreEvent::RevealProject { path: path.to_string(), job: None }),
            CommandDone | CommandFailed => {
                let job = self.lock().settings.project(path).commands.into_iter().find(|command| Some(command.name.as_str()) == detail).map(|command| command.id);
                self.emit(CoreEvent::RevealProject { path: path.to_string(), job });
            }
            DepsWaiting => self.emit(CoreEvent::RevealPage("deps")),
            FuelHigh => self.emit(CoreEvent::RevealPage("fuel")),
            CardReview => self.emit(CoreEvent::RevealPage("board")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [SignalKind; 10] = [
        SignalKind::ClaudeAsking,
        SignalKind::ClaudePermission,
        SignalKind::ClaudeFinished,
        SignalKind::ClaudeLongTurn,
        SignalKind::ServerCrashed,
        SignalKind::CommandDone,
        SignalKind::CommandFailed,
        SignalKind::DepsWaiting,
        SignalKind::FuelHigh,
        SignalKind::CardReview,
    ];

    fn about(kind: SignalKind, path: &str) -> Signal {
        Signal { kind, path: path.into(), name: "paddock".into(), detail: None }
    }

    /// The door's decision, case by case. What a muted project and a quiet period hold never
    /// shows, sounds or leaves for the phone; a question passes quiet only while Settings lets it.
    #[test]
    fn a_muted_project_and_quiet_hold_what_they_must() {
        // Every event on and with a sound of its own, `/muted` muted.
        let all_on = || {
            let mut settings = Settings::default();
            settings.sounds.command_done = "chime".into();
            settings.notifications.command = true;
            settings.notifications.deps = true;
            settings.notifications.muted = vec!["/muted".into()];
            settings
        };
        let told = |banner: bool, sound: Option<&str>, missed: bool, relay: bool| Verdict { banner, sound: sound.map(str::to_string), missed, relay };
        let nothing = Verdict::default;

        // A muted project: nothing of any kind, quiet or not.
        for kind in KINDS {
            for quiet in [false, true] {
                assert_eq!(decide(&all_on(), &about(kind, "/muted"), quiet, false), nothing(), "muted, {kind:?}, quiet {quiet}");
            }
        }

        type Change = fn(&mut Settings);
        let as_set: Change = |_| {};
        let cases: Vec<(&str, Change, SignalKind, bool, bool, Verdict)> = vec![
            ("a finished turn", as_set, SignalKind::ClaudeFinished, false, false, told(true, Some("boxbox"), false, true)),
            ("a crash", as_set, SignalKind::ServerCrashed, false, false, told(true, Some("yellowflag"), false, true)),
            ("a card in review is the phone's alone", as_set, SignalKind::CardReview, false, false, told(false, None, false, true)),
            // Quiet holds the banner, the sound and the phone, and lists it.
            ("quiet: a finished turn", as_set, SignalKind::ClaudeFinished, true, false, told(false, None, true, false)),
            ("quiet: a crash", as_set, SignalKind::ServerCrashed, true, false, told(false, None, true, false)),
            ("quiet: a command", as_set, SignalKind::CommandFailed, true, false, told(false, None, true, false)),
            ("quiet: a long turn", as_set, SignalKind::ClaudeLongTurn, true, false, told(false, None, true, false)),
            ("quiet: the fuel alert", as_set, SignalKind::FuelHigh, true, false, told(false, None, true, false)),
            // It has no banner for quiet to hold: nothing to list.
            ("quiet: a card in review", as_set, SignalKind::CardReview, true, false, nothing()),
            // A question and a permission pass, while Settings lets them.
            ("quiet: a question passes", as_set, SignalKind::ClaudeAsking, true, false, told(true, Some("limiter"), false, true)),
            ("quiet: a permission passes", as_set, SignalKind::ClaudePermission, true, false, told(true, Some("limiter"), false, true)),
            ("quiet: a question is held", |s| s.notifications.questions_through = false, SignalKind::ClaudeAsking, true, false, told(false, None, true, false)),
            ("quiet: a permission is held", |s| s.notifications.questions_through = false, SignalKind::ClaudePermission, true, false, told(false, None, true, false)),
            // Only the banner is held when the sounds are not.
            ("quiet, sounds not held", |s| s.notifications.sounds_too = false, SignalKind::ServerCrashed, true, false, told(false, Some("yellowflag"), true, false)),
            // Switched off in Settings: no banner, its sound as the Sounds page says, the phone asked.
            ("crash banners off", |s| s.notifications.server_crashed = false, SignalKind::ServerCrashed, false, false, told(false, Some("yellowflag"), false, true)),
            ("crash banners off, quiet: not listed either", |s| s.notifications.server_crashed = false, SignalKind::ServerCrashed, true, false, nothing()),
            ("Claude's off: its sounds too", |s| s.notify = false, SignalKind::ClaudeFinished, false, false, told(false, None, false, true)),
            ("Claude's off: a long turn is one of its", |s| s.notify = false, SignalKind::ClaudeLongTurn, false, false, told(false, None, false, true)),
            ("the fuel alert off", |s| s.fuel_alert = false, SignalKind::FuelHigh, false, false, told(false, None, false, true)),
            // The same crash again within the minute is only heard.
            ("a crash told a moment ago", as_set, SignalKind::ServerCrashed, false, true, told(false, Some("yellowflag"), false, false)),
            ("a crash told a moment ago, quiet", as_set, SignalKind::ServerCrashed, true, true, nothing()),
        ];

        for (name, change, kind, quiet, again, expected) in cases {
            let mut settings = all_on();
            change(&mut settings);
            assert_eq!(decide(&settings, &about(kind, "/paddock"), quiet, again), expected, "{name}");
        }
    }

    fn core_with(tmp: &Path, sink: Sink) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude").join("projects"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            sink,
        )
    }

    /// The door does as it decided: while it is quiet and for a muted project no banner and no
    /// sound leave the core, whatever the source, and what quiet held is listed until it is over
    /// and looked at. Quiet outlives a restart.
    #[test]
    fn nothing_leaves_the_door_while_quiet_or_for_a_muted_project() {
        let tmp = tempfile::tempdir().unwrap();
        let heard: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = |heard: &Arc<Mutex<Vec<String>>>| -> Sink {
            let heard = Arc::clone(heard);
            Arc::new(move |event| match event {
                CoreEvent::Notify { path, .. } => heard.lock().unwrap().push(format!("banner {path}")),
                CoreEvent::Sound(id) => heard.lock().unwrap().push(format!("sound {id}")),
                _ => {}
            })
        };
        let take = || std::mem::take(&mut *heard.lock().unwrap());
        let core = core_with(tmp.path(), sink(&heard));
        // Listed: what was missed of a project is shown while it is.
        core.set_favourite("/paddock", true);
        core.set_favourite("/garage", true);

        core.signal(about(SignalKind::ClaudeFinished, "/paddock"));
        assert_eq!(take(), ["banner /paddock", "sound boxbox"]);

        core.mute_project("/paddock", true);
        for kind in KINDS {
            core.signal(about(kind, "/paddock"));
        }
        core.sound(&["/paddock"], |sounds| &sounds.claude_finished);
        assert_eq!(take(), [] as [&str; 0], "a muted project");
        assert!(core.snapshot().missed.is_empty());
        core.mute_project("/paddock", false);

        core.quiet_start(QuietFor::Open);
        core.signals(vec![about(SignalKind::ClaudeFinished, "/paddock"), about(SignalKind::ServerCrashed, "/garage")]);
        core.fuel_high("pitwall:fuel", "92% used".into(), String::new(), false);
        core.sound(&["/garage"], |sounds| &sounds.claude_finished);
        assert_eq!(take(), [] as [&str; 0], "quiet");
        let missed: Vec<(SignalKind, String)> = core.snapshot().missed.into_iter().map(|entry| (entry.kind, entry.path)).collect();
        assert_eq!(missed, [(SignalKind::FuelHigh, String::new()), (SignalKind::ServerCrashed, "/garage".into()), (SignalKind::ClaudeFinished, "/paddock".into())]);

        // Started again: still quiet, as the settings file says.
        drop(core);
        let core = core_with(tmp.path(), sink(&heard));
        assert_eq!(core.snapshot().quiet, Some(QuietView { until: None, hours: false }));
        core.signal(about(SignalKind::ServerCrashed, "/garage"));
        assert_eq!(take(), [] as [&str; 0], "quiet after a restart");

        core.quiet_end();
        core.signal(about(SignalKind::ClaudeFinished, "/paddock"));
        assert_eq!(take(), ["banner /paddock", "sound boxbox"]);
    }
}
