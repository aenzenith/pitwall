//! The loop's second: which jobs are due, missed moments (asleep, Pitwall closed), overlap, the
//! once-only rule (a run is written down before anything starts) and `laps.lock`, which only one
//! Pitwall holds.

use super::*;

/// At most this many runs begin in one second; the others the next.
const MAX_STARTS: usize = 2;
/// While another Pitwall holds the lock, it is asked for again this often.
const LOCK_RETRY_MS: u64 = 60_000;
/// A moment late by more than this, for a job that doesn't catch up, is missed.
const NO_CATCH_UP_MS: u64 = 90_000;

/// How late a moment may start: the job's grace when it catches up, else a minute and a half.
fn grace_ms(missed: &super::types::Missed) -> u64 {
    if missed.catch_up { u64::from(missed.grace_min) * 60_000 } else { NO_CATCH_UP_MS }
}

/// What a second did.
#[derive(Default)]
struct Second {
    planned: Vec<Planned>,
    /// Jobs whose runs file changed.
    records: Vec<String>,
    /// `laps.json` changed.
    jobs: bool,
}

impl Laps {
    /// Holds `laps.lock` beside the settings, taking it now if it was never tried or was tried a
    /// minute ago. Some: held, `true` when just taken (the files read again, what the last run
    /// left going cut, and each job given its next moment); none: another Pitwall holds it.
    pub(super) fn ensure_lock(&mut self, now: u64) -> Option<bool> {
        if self.lock.is_some() {
            return Some(false);
        }
        if self.quit || self.lock_tried.is_some_and(|at| now.saturating_sub(at) < LOCK_RETRY_MS && at <= now) {
            return None;
        }
        self.lock_tried = Some(now);
        let _ = fs::create_dir_all(&self.store.dir);
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let held = options.open(self.store.dir.join("laps.lock")).ok().filter(|file| file.try_lock().is_ok());
        let Some(file) = held else {
            self.elsewhere = true;
            return None;
        };
        self.lock = Some(file);
        self.elsewhere = false;
        self.take_over(now);
        Some(true)
    }

    /// The lock just taken: the files as they are now (another Pitwall may have run the laps; a
    /// file with jobs that don't fit set aside, and those that do written back), a run without an
    /// end cut by a quit or a crash (never started again), and each job that is on given its next
    /// moment if it has none.
    fn take_over(&mut self, now: u64) {
        let mut store = Store::at(&self.store.dir);
        let (laps, write_back) = store.load_laps(true);
        self.laps = laps;
        self.runs = store.load_runs(&self.laps);
        self.store = store;
        self.going.clear();
        self.held.clear();
        self.woke_at = Some(now);

        let cut: Vec<String> = self
            .runs
            .iter_mut()
            .filter_map(|(lap, runs)| {
                let mut any = false;
                for run in runs.iter_mut().filter(|run| run.ended_at.is_none()) {
                    run.ended_at = Some(now);
                    run.result = Some(RunResult::Cut);
                    run.reason = Some(Reason::Quit);
                    run.tab = None;
                    any = true;
                }
                any.then(|| lap.clone())
            })
            .collect();
        for lap in cut {
            self.save_runs(&lap);
        }

        let mut changed = false;
        for index in 0..self.laps.len() {
            let lap = &self.laps[index];
            let next = match lap.next_at {
                Some(at) if lap.on && lap.paused.is_none() => Some(at),
                _ => self.next_moment(lap, now),
            };
            if next != self.laps[index].next_at {
                self.laps[index].next_at = next;
                changed = true;
            }
        }
        if changed || write_back {
            self.save();
        }
    }

    /// A run of `self.laps[index]` begins: its record (no end yet) and its place among those that
    /// go. One the schedule began counts towards `ends.after`, and moves the job on to its next
    /// moment; the job turns off once it has no more (`Once`, `ends`). Nothing is written here.
    pub(super) fn begin(&mut self, index: usize, due: u64, by_hand: bool, now: u64) -> Planned {
        let run = self.new_id("r", now);
        let lap = self.laps[index].clone();
        let record = LapRun { id: run.clone(), lap: lap.id.clone(), due, by_hand, started_at: now, agent: lap.kind.has_agent().then_some(lap.start.agent), tries: 1, ..Default::default() };
        self.runs.entry(lap.id.clone()).or_default().push(record);
        let phase = if lap.kind.has_script() { "script" } else { "agent" };
        let stop: StopFlag = Arc::new(Mutex::new(None));
        let undo = (!by_hand).then_some(Undo { on: lap.on, next_at: lap.next_at, runs_done: lap.runs_done, held: None });
        self.going.insert(lap.id.clone(), Going { run: run.clone(), phase, started_at: now, pgid: None, tab: None, session: None, agent: None, awake: None, stop, undo });
        if !by_hand {
            self.laps[index].runs_done += 1;
            let next = self.next_moment(&self.laps[index], now);
            let lap = &mut self.laps[index];
            lap.next_at = next;
            if next.is_none() && (matches!(lap.when, When::Once { .. }) || lap.ends.after.is_some_and(|after| lap.runs_done >= after) || lap.ends.at.is_some_and(|end| end <= now)) {
                lap.on = false;
            }
        }
        Planned { lap: lap.id, run }
    }

    /// A moment that didn't run: a `skipped` record with its reason, ended as it is written.
    pub(super) fn skipped(&mut self, lap: &str, due: u64, reason: Reason, now: u64) {
        let run = self.new_id("r", now);
        let record = LapRun { id: run, lap: lap.to_string(), due, started_at: now, ended_at: Some(now), result: Some(RunResult::Skipped), reason: Some(reason), ..Default::default() };
        self.runs.entry(lap.to_string()).or_default().push(record);
    }

    /// The moments still to come of `lap` on `date` (none while it is off or paused, or on a
    /// day off of it).
    pub(super) fn planned_on(&self, lap: &Lap, date: &str, now: u64) -> Vec<u64> {
        if !lap.on || lap.paused.is_some() || lap.days_off.iter().any(|off| off == date) {
            return Vec::new();
        }
        let moments = match lap.when {
            When::Refill => self.fuel_of(lap).map(|fuel| fuel.refills.iter().copied().filter(|&at| when::date_of(at).0 == date).collect()).unwrap_or_default(),
            _ => when::on_day(&lap.when, date),
        };
        moments.into_iter().filter(|&at| at > now && !lap.ends.at.is_some_and(|end| at > end)).collect()
    }

    /// The second's work on the state alone: nothing starts here and nothing is told.
    fn second(&mut self, now: u64) -> Second {
        let mut second = Second::default();
        // Asleep, suspended, closed or the laps held elsewhere: with catch-up off, a moment that
        // came meanwhile is missed, however little late.
        if now.saturating_sub(self.last_tick) > NO_CATCH_UP_MS {
            self.woke_at = Some(now);
        }
        let woke_at = self.woke_at;
        let since = std::mem::replace(&mut self.last_tick, now);

        // Days off that are past leave the jobs, once a day.
        let today = when::date_of(now).0;
        if today != self.last_date {
            self.last_date = today.clone();
            for lap in &mut self.laps {
                let before = lap.days_off.len();
                lap.days_off.retain(|off| *off >= today);
                second.jobs |= lap.days_off.len() != before;
            }
        }
        // Ended by its date: off.
        for lap in &mut self.laps {
            if lap.on && lap.ends.at.is_some_and(|end| end <= now) && lap.next_at.is_none_or(|at| at > now) {
                lap.on = false;
                lap.next_at = None;
                second.jobs = true;
            }
        }

        // Today off: the job's next moment is past it already; the first of today's moments
        // that comes leaves the day's one record.
        for index in 0..self.laps.len() {
            let lap = &self.laps[index];
            if !lap.on || lap.paused.is_some() || !lap.days_off.contains(&today) {
                continue;
            }
            let Some(moment) = when::on_day(&lap.when, &today).into_iter().find(|&at| at > since && at <= now) else {
                continue;
            };
            let id = lap.id.clone();
            if self.day_off(index, moment, now) {
                second.records.push(id);
                second.jobs = true;
            }
        }

        // A moment held for the run before it: it starts once that one has ended, within the
        // grace; past it, it is skipped as overlapping. A job paused or switched off meanwhile
        // lets it go.
        let held: Vec<(String, Held)> = self.held.iter().map(|(lap, held)| (lap.clone(), *held)).collect();
        for (id, held) in held {
            let Some(index) = self.laps.iter().position(|lap| lap.id == id && lap.on && lap.paused.is_none()) else {
                self.held.remove(&id);
                continue;
            };
            let late = now.saturating_sub(held.at) > grace_ms(&self.laps[index].missed);
            if (self.going.contains_key(&id) || second.planned.len() >= MAX_STARTS) && !late {
                continue;
            }
            self.held.remove(&id);
            if late {
                self.skipped(&id, held.at, Reason::Overlap, now);
            } else {
                let planned = self.begin(index, held.at, false, now);
                if let Some(undo) = self.going.get_mut(&id).and_then(|going| going.undo.as_mut()) {
                    undo.held = Some(held);
                }
                second.planned.push(planned);
            }
            second.records.push(id);
            second.jobs = true;
        }

        for index in 0..self.laps.len() {
            if second.planned.len() >= MAX_STARTS {
                break;
            }
            let lap = &self.laps[index];
            let Some(at) = lap.next_at.filter(|&at| at <= now && lap.on && lap.paused.is_none()) else {
                continue;
            };
            let id = lap.id.clone();
            let grace = if !lap.missed.catch_up && woke_at.is_some_and(|woke| at <= woke) { 0 } else { grace_ms(&lap.missed) };
            // However many moments passed (asleep, Pitwall closed), the latest within the grace
            // runs once (never one past the job's end or on a day off); none within it: one
            // missed record.
            let moment = if now - at <= grace {
                Some(at)
            } else {
                match lap.when {
                    When::Refill | When::Once { .. } => None,
                    _ => when::next_after(&lap.when, now.saturating_sub(grace).saturating_sub(1), &lap.days_off).filter(|&moment| moment <= now && moment > at && !lap.ends.at.is_some_and(|end| moment > end)),
                }
            };
            second.records.push(id.clone());
            second.jobs = true;
            let date = moment.map(|moment| when::date_of(moment).0);
            match moment {
                None => {
                    self.skipped(&id, at, Reason::Missed, now);
                    self.move_on(index, now);
                }
                Some(moment) if date.as_ref().is_some_and(|date| self.laps[index].days_off.contains(date)) => {
                    self.day_off(index, moment, now);
                    self.move_on(index, now);
                }
                Some(moment) if self.going.contains_key(&id) => {
                    match self.laps[index].overlap {
                        Overlap::Skip => self.skipped(&id, moment, Reason::Overlap, now),
                        Overlap::Wait => {
                            self.held.insert(id.clone(), Held { at: moment });
                        }
                        Overlap::Stop => {
                            self.held.insert(id.clone(), Held { at: moment });
                            if let Some(going) = self.going.get(&id) {
                                *going.stop.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Reason::Stopped);
                            }
                        }
                    }
                    self.move_on(index, now);
                }
                Some(moment) => second.planned.push(self.begin(index, moment, false, now)),
            }
        }
        second
    }

    /// `moment` of `self.laps[index]` is on a day off: a `skipped / dayOff` record, the day's
    /// only one; a one-time job's day then ends it. Whether anything changed.
    fn day_off(&mut self, index: usize, moment: u64, now: u64) -> bool {
        let id = self.laps[index].id.clone();
        let date = when::date_of(moment).0;
        let noted = self.runs.get(&id).is_some_and(|runs| runs.iter().any(|run| run.reason == Some(Reason::DayOff) && when::date_of(run.due).0 == date));
        if !noted {
            self.skipped(&id, moment, Reason::DayOff, now);
        }
        if matches!(self.laps[index].when, When::Once { .. }) {
            self.move_on(index, now);
        }
        !noted
    }

    /// The job's next moment after `now`, without a run; off once it has none left by its ends.
    fn move_on(&mut self, index: usize, now: u64) {
        let next = self.next_moment(&self.laps[index], now);
        let lap = &mut self.laps[index];
        lap.next_at = next;
        if next.is_none() && (matches!(lap.when, When::Once { .. }) || lap.ends.at.is_some_and(|end| end <= now)) {
            lap.on = false;
        }
    }

    /// What `second` changed is written: `laps.json` first, then the records. A run whose
    /// `laps.json` or record couldn't be written never starts: its record says `notStarted`,
    /// should it be written at all. The runs that may start.
    fn write_second(&mut self, mut second: Second, now: u64) -> Vec<Planned> {
        let jobs_saved = !second.jobs || self.save();
        let mut starting = Vec::new();
        for planned in std::mem::take(&mut second.planned) {
            if jobs_saved {
                starting.push(planned);
            } else {
                self.not_started(&planned, now);
            }
        }
        second.records.sort();
        second.records.dedup();
        let mut unwritten: Vec<String> = Vec::new();
        for lap in &second.records {
            if !self.save_runs(lap) {
                unwritten.push(lap.clone());
            }
        }
        let mut undone = false;
        starting.retain(|planned| {
            if unwritten.contains(&planned.lap) {
                self.not_started(planned, now);
                undone = true;
                return false;
            }
            true
        });
        if undone {
            self.save();
        }
        starting
    }

    /// A begun run that may not start: ended `skipped / notStarted` and no longer going. The
    /// job is put back as it was before (its switch, its moment, a moment held for it, its count
    /// towards its end), so that moment is tried again at the next second, within its grace; it
    /// keeps one such record however many seconds it can't start.
    fn not_started(&mut self, planned: &Planned, now: u64) {
        let undo = self.going.remove(&planned.lap).filter(|going| going.run == planned.run).and_then(|going| going.undo);
        let Some(record) = self.record_mut(&planned.lap, &planned.run) else {
            return;
        };
        record.ended_at = Some(now);
        record.result = Some(RunResult::Skipped);
        record.reason = Some(Reason::NotStarted);
        let due = record.due;
        if let Some(runs) = self.runs.get_mut(&planned.lap) {
            runs.retain(|run| run.id == planned.run || !(run.due == due && run.reason == Some(Reason::NotStarted)));
        }
        let Some(undo) = undo else {
            return;
        };
        if let Some(lap) = self.lap_mut(&planned.lap) {
            lap.on = undo.on;
            lap.next_at = undo.next_at;
            lap.runs_done = undo.runs_done;
        }
        if let Some(held) = undo.held {
            self.held.insert(planned.lap.clone(), held);
        }
    }
}

impl Core {
    /// `laps_due` at `now`, each run that may start handed to `start` once its record and
    /// `laps.json` are written. Nothing while another Pitwall holds the laps.
    pub(super) fn laps_due_at(self: &Arc<Self>, now: u64, start: &mut dyn FnMut(&Arc<Core>, Planned)) {
        let (planned, changed) = {
            let mut laps = self.laps();
            if laps.quit {
                return;
            }
            let elsewhere = laps.elsewhere;
            let Some(taken) = laps.ensure_lock(now) else {
                // Another Pitwall took the laps: the page says so at once, not at its next load.
                let told = laps.elsewhere != elsewhere;
                drop(laps);
                if told {
                    self.laps_changed();
                }
                return;
            };
            let second = laps.second(now);
            let changed = taken || second.jobs || !second.records.is_empty();
            if !changed {
                return;
            }
            (laps.write_second(second, now), changed)
        };
        if changed {
            self.laps_changed();
        }
        for planned in planned {
            start(self, planned);
        }
    }
}
