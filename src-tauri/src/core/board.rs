//! The board (Pano): per project, cards the user writes (a title, a note) in four columns. A
//! card given to Claude opens a Claude Code tab in one of Pitwall's terminals with its text as
//! the first prompt, then follows that session: it stays in `claude` while Claude works or waits
//! on a permission or a question, and goes to `review` once the turn is over or the session has
//! ended. The user moves it to `done`.
//!
//! A card follows the process its session runs in: a session that takes a new id there (`/clear`)
//! is still the card's. Claude Code's own `busy` / `waiting` outranks a hook gone quiet through a
//! long tool call.
//!
//! A move changes where a card is, never what it holds: it keeps its session in every column.
//! Moved back into `claude`, its session goes on: followed where it still runs, opened again
//! (`claude --resume`) once it has ended, never a second time while it may still run.
//!
//! Cards live on this Mac only, in `board.json` beside `settings.json`. They hold the text the
//! user typed, ids and times; nothing read from Claude's logs. Following a session reads only
//! what the app already has: the last scan's phases (`Inner::scanned`) and Claude Code's running
//! sessions (`sessions.rs`, its allowed fields only).
//!
//! A given card asks Claude to end with a short summary, and the card's details show what
//! Claude last said in its session once the work is over, as a session's details do
//! (`Core::last_message`): read from that session's log when asked, never kept.

use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::Value;

use super::reveal::{is_session_id, Liveness};
use super::sessions::{row_phase, status_phase, RowPhase, Running};
use super::terminal::Launch;
use super::*;
use crate::claude::TurnKind;
use crate::registry::{base36, write_atomic};
use crate::settings::set_aside;

/// Done cards are dropped this long after they got there.
const DONE_KEPT_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// A tab opened for a card shows this much of its title.
const TAB_NAME_CHARS: usize = 24;
const FILE_VERSION: u32 = 1;
/// A session opened again is given this long to show among the running ones; until then its old
/// end doesn't send its card on.
const RESUME_GRACE_MS: u64 = 60_000;
/// A card given in a terminal takes a session that starts there within this long of the give;
/// a `claude` started in that tab later is somebody else's work.
const ADOPT_MS: u64 = 2 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
    Queued,
    Claude,
    Review,
    Done,
}

impl Column {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "queued" => Some(Self::Queued),
            "claude" => Some(Self::Claude),
            "review" => Some(Self::Review),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub path: String,
    pub title: String,
    #[serde(default)]
    pub note: String,
    pub column: Column,
    pub created_at: u64,
    /// The last column change.
    pub moved_at: u64,
    /// The Claude session it follows.
    #[serde(default)]
    pub session: Option<String>,
    /// The Pitwall terminal it was started in, while that terminal is open (this run only).
    #[serde(default)]
    pub terminal: Option<u64>,
    #[serde(default)]
    pub given_at: Option<u64>,
    /// Its session has been seen at work (working, or waiting on a permission or a question)
    /// since the card entered `claude`; only then does an idle session mean the turn is over.
    #[serde(default)]
    pub worked: bool,
    /// Its session has been seen among the running ones: gone from that list since, it has
    /// ended. Kept through a restart, so a card whose session ended meanwhile still moves on.
    #[serde(default)]
    pub ran: bool,
}

pub(super) struct Board {
    file: PathBuf,
    cards: Vec<Card>,
    /// False when an unusable file could not be set aside: nothing is written over it.
    writable: bool,
    /// Sessions opened again and not seen running yet, and until when they are waited for.
    resumed: HashMap<String, u64>,
    /// The process each followed session was last seen running in: another session id in that
    /// process is the same conversation gone on (`/clear`).
    pids: HashMap<String, u32>,
    /// Cards linked to a session that was at work on something else: that turn isn't theirs.
    linked_busy: HashSet<String>,
}

impl Board {
    /// The saved board; empty when there is none. An unusable file is set aside first
    /// (`board.json.broken-<ms>`), never written over; done cards older than 30 days are dropped.
    pub(super) fn load(file: PathBuf, now: u64) -> Self {
        let (cards, writable, changed) = read_cards(&file, now);
        let board = Self { file, cards, writable, resumed: HashMap::new(), pids: HashMap::new(), linked_busy: HashSet::new() };
        if changed {
            board.save();
        }
        board
    }

    /// Writes the board; false when it isn't on disk as it is now.
    fn save(&self) -> bool {
        if !self.writable {
            return false;
        }
        if let Some(parent) = self.file.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let json = serde_json::json!({ "version": FILE_VERSION, "cards": self.cards });
        let written = serde_json::to_string_pretty(&json).map_err(|error| error.to_string()).and_then(|text| write_atomic(&self.file, &text).map_err(|error| error.to_string()));
        if let Err(error) = &written {
            eprintln!("[pitwall] {} could not be written: {error}", self.file.display());
        }
        written.is_ok()
    }

    /// An unusable file could not be set aside: nothing is saved in this run.
    pub(super) fn read_only(&self) -> bool {
        !self.writable
    }

    fn find(&self, id: &str) -> Option<&Card> {
        self.cards.iter().find(|card| card.id == id)
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut Card> {
        self.cards.iter_mut().find(|card| card.id == id)
    }

    /// Takes the card out and puts it at `index` among its project's cards in `column` (past the
    /// last one: at the end), as the board shows them. A new column sets `moved_at`.
    fn place(&mut self, id: &str, column: Column, index: usize, now: u64) -> Option<&mut Card> {
        let at = self.cards.iter().position(|card| card.id == id)?;
        let mut card = self.cards.remove(at);
        if card.column != column {
            card.column = column;
            card.moved_at = now;
        }

        let slots: Vec<usize> =
            self.cards.iter().enumerate().filter(|(_, c)| c.column == column && c.path == card.path).map(|(i, _)| i).collect();
        let target = match slots.get(index) {
            Some(&slot) => slot,
            None => slots.last().map_or(self.cards.len(), |&slot| slot + 1),
        };
        self.cards.insert(target, card);
        self.cards.get_mut(target)
    }
}

/// The cards in `file`, whether it may be written, and whether what was read changed (to be
/// written back).
fn read_cards(file: &Path, now: u64) -> (Vec<Card>, bool, bool) {
    let raw = match fs::read(file) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return (Vec::new(), true, false),
        Err(error) => return (Vec::new(), set_aside(file, &error.to_string()), false),
    };
    let value = match serde_json::from_slice::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => return (Vec::new(), set_aside(file, &error.to_string()), false),
    };
    let Some(items) = value.get("cards").and_then(Value::as_array) else {
        return (Vec::new(), set_aside(file, "no card list"), false);
    };

    // Each card on its own: one that doesn't fit costs only itself, and the file is kept aside.
    let mut cards: Vec<Card> = Vec::new();
    let mut unusable = 0;
    for item in items {
        match serde_json::from_value::<Card>(item.clone()) {
            Ok(card) if !card.id.is_empty() && !cards.iter().any(|c| c.id == card.id) => cards.push(card),
            _ => unusable += 1,
        }
    }
    let mut writable = true;
    let mut changed = false;
    if unusable > 0 {
        writable = set_aside(file, &format!("{unusable} unusable cards"));
        changed = true;
    }

    let before = cards.len();
    cards.retain(|card| card.column != Column::Done || now.saturating_sub(card.moved_at) < DONE_KEPT_MS);
    changed |= cards.len() != before;

    for card in &mut cards {
        if card.session.as_deref().is_some_and(|session| !is_session_id(session)) {
            card.session = None;
            changed = true;
        }
        // A terminal belongs to the run that opened it, and the Claude in it ended with that run.
        if card.terminal.take().is_some() {
            changed = true;
            if card.column == Column::Claude {
                card.column = Column::Review;
                card.moved_at = now;
            }
        }
    }

    (cards, writable, changed)
}

/// Where a followed session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stand {
    /// Working, or waiting on a permission or a question.
    Busy,
    /// Its turn is over: waiting with a finished turn (when it finished), or idle.
    Over(Option<u64>),
    /// Its process is gone, or the hook heard it end.
    Ended,
    /// Nothing tells.
    Unknown,
}

/// Where session `id` stands, as the Sessions page tells it: the hook's events where they
/// decide, else Claude Code's own status, else its log. `ran`: it was in the running list
/// before, so missing from it now means it ended.
fn stand(id: &str, scanned: Option<&SessionState>, running: Option<&HashMap<String, Running>>, ran: bool) -> Stand {
    let run = running.and_then(|all| all.get(id));
    if run.is_none() && (scanned.is_some_and(|s| s.ended) || (running.is_some() && ran)) {
        return Stand::Ended;
    }
    // Claude Code's own word for a session at work or with a prompt open outranks the hook: its
    // events stop through a long tool call, and a session that quiet reads as idle.
    if matches!(run.and_then(|r| r.status.as_deref()), Some("busy" | "waiting")) {
        return Stand::Busy;
    }

    let by_scan = scanned.map(|s| (row_phase(s.phase), s.turn.filter(|_| s.phase == SessionPhase::Waiting)));
    let decided = scanned.is_some_and(|s| s.hooked);
    let phase = match (decided, status_phase(run.and_then(|r| r.status.as_deref()), by_scan)) {
        (false, Some(by_status)) => Some(by_status),
        _ => by_scan,
    };

    match phase {
        Some((RowPhase::Working, _)) => Stand::Busy,
        Some((RowPhase::Waiting, Some(turn))) if turn.kind == TurnKind::Finished => Stand::Over(Some(turn.at)),
        Some((RowPhase::Waiting, _)) => Stand::Busy,
        Some((RowPhase::Idle, _)) => Stand::Over(None),
        Some((RowPhase::Ended, _)) => Stand::Ended,
        None => Stand::Unknown,
    }
}

/// How a card is given to Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Give {
    /// A new session, the card's text its first prompt.
    New,
    /// The same, in plan mode.
    Plan,
    /// The session it holds goes on (a card moved back into `claude`); a card without one
    /// starts a new one.
    Continue,
}

impl Give {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "new" => Some(Self::New),
            "plan" => Some(Self::Plan),
            "continue" => Some(Self::Continue),
            _ => None,
        }
    }
}

/// What going on means for the session a card holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carry {
    /// It runs, or may: the card follows it where it is.
    Follow,
    /// It has ended: it opens again.
    Resume,
    /// There is none to go on with: a new one.
    Fresh,
}

/// How a card moved back into `claude` goes on. `session`: it holds one; `starting`: it holds
/// none yet, but the terminal it was given in is still open; `logged`: its session's log is
/// there to open again; `ended`: the session is known to have ended though its log is fresh
/// (seen running and gone since, or heard ending). A session that may still run is never opened
/// again: two processes would write one log.
fn carry(session: bool, starting: bool, liveness: &Liveness, logged: bool, ended: bool) -> Carry {
    if !session {
        return if starting { Carry::Follow } else { Carry::Fresh };
    }

    match liveness {
        Liveness::Running { .. } => Carry::Follow,
        _ if !logged => Carry::Fresh,
        Liveness::Ended => Carry::Resume,
        Liveness::Unknown if ended => Carry::Resume,
        Liveness::Unknown => Carry::Follow,
    }
}

fn no_card(id: &str) -> String {
    format!("no such card: {id}")
}

fn new_id(cards: &[Card], now: u64) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    loop {
        let id = format!("{}-{}", base36(now), base36(COUNTER.fetch_add(1, Ordering::Relaxed)));
        if !cards.iter().any(|card| card.id == id) {
            return id;
        }
    }
}

/// A tab's title from a card's: one line, cut to `TAB_NAME_CHARS`.
fn tab_name(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() <= TAB_NAME_CHARS {
        return title;
    }
    let cut: String = title.chars().take(TAB_NAME_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

impl Core {
    fn board(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Saves the board and sends it to the window, the lock let go first. A board that couldn't
    /// be written says so in the snapshot (`board_unsaved`).
    fn board_changed(&self, board: MutexGuard<'_, Board>) {
        let saved = board.save();
        let cards = board.cards.clone();
        drop(board);
        self.emit(CoreEvent::Board(cards));

        let turned = {
            let mut inner = self.lock();
            let turned = inner.board_unsaved == saved;
            inner.board_unsaved = !saved;
            turned
        };
        if turned {
            self.notify();
        }
    }

    /// Every card, in order: a column's cards are in the order they come here.
    pub fn board_state(&self) -> Vec<Card> {
        self.board().cards.clone()
    }

    /// A new card, at the end of `queued`.
    pub fn board_add(&self, path: &str, title: &str, note: &str) -> Result<Card, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a card needs a title".into());
        }
        if path.trim().is_empty() {
            return Err("a card needs a project".into());
        }

        let now = now_ms();
        let mut board = self.board();
        let card = Card {
            id: new_id(&board.cards, now),
            path: path.to_string(),
            title: title.to_string(),
            note: note.trim().to_string(),
            column: Column::Queued,
            created_at: now,
            moved_at: now,
            session: None,
            terminal: None,
            given_at: None,
            worked: false,
            ran: false,
        };
        board.cards.push(card.clone());
        board.place(&card.id, Column::Queued, usize::MAX, now);
        self.board_changed(board);
        Ok(card)
    }

    pub fn board_edit(&self, id: &str, title: &str, note: &str) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a card needs a title".into());
        }

        let mut board = self.board();
        let card = board.find_mut(id).ok_or_else(|| no_card(id))?;
        card.title = title.to_string();
        card.note = note.trim().to_string();
        self.board_changed(board);
        Ok(())
    }

    /// Moves a card to `index` among `column`'s cards (the card itself left out; clamped). Only
    /// its column and place change: it keeps its session wherever it goes. Moved into `claude` it
    /// waits for its session to work again before an idle one sends it on.
    pub fn board_move(&self, id: &str, column: &str, index: usize) -> Result<(), String> {
        let column = Column::parse(column).ok_or_else(|| format!("unknown column: {column}"))?;
        let now = now_ms();
        let mut board = self.board();
        let from = board.find(id).ok_or_else(|| no_card(id))?.column;
        let card = board.place(id, column, index, now).ok_or_else(|| no_card(id))?;

        if column == Column::Claude && from != Column::Claude {
            card.worked = false;
        }
        self.board_changed(board);
        Ok(())
    }

    pub fn board_delete(&self, id: &str) {
        let mut board = self.board();
        let before = board.cards.len();
        board.cards.retain(|card| card.id != id);
        if board.cards.len() != before {
            self.board_changed(board);
        }
    }

    /// Every card of `from` goes to `to`, a listed project: for a folder taken off the list or
    /// renamed, whose cards no board shows. Columns are kept; the cards come after `to`'s own.
    pub fn board_rehome(&self, from: &str, to: &str) -> Result<(), String> {
        if !self.listed_paths().iter().any(|listed| listed == to) {
            return Err(t!("core.error.depsNotListed", path = to));
        }
        if from == to {
            return Ok(());
        }

        let mut board = self.board();
        let (mut moved, kept): (Vec<Card>, Vec<Card>) = std::mem::take(&mut board.cards).into_iter().partition(|card| card.path == from);
        let any = !moved.is_empty();
        for card in &mut moved {
            card.path = to.to_string();
        }
        board.cards = kept;
        board.cards.append(&mut moved);
        if any {
            self.board_changed(board);
        }
        Ok(())
    }

    /// Gives a card to Claude: a Claude Code tab in the card's project, with the card's text as
    /// the first prompt (`Give::Plan`: in plan mode) and, under it, a line asking for a short
    /// summary at the end (the card's details show it). The card goes to `claude` and follows the
    /// session that starts in that tab (`follow_board`).
    ///
    /// `Give::Continue` keeps the session the card holds instead (`carry`): one that runs is
    /// followed where it is, with no tab opened (`None`); one that has ended opens again in a
    /// tab, as it was.
    pub fn board_give(self: &Arc<Self>, id: &str, give: Give, size: Option<(u16, u16)>) -> Result<Option<TerminalView>, String> {
        let card = self.board().find(id).ok_or_else(|| no_card(id))?.clone();

        let carry = match give {
            Give::New | Give::Plan => Carry::Fresh,
            Give::Continue => self.carry_of(&card),
        };
        let launch = match (carry, &card.session) {
            (Carry::Follow, _) => {
                let mut board = self.board();
                if let Some(card) = board.place(id, Column::Claude, usize::MAX, now_ms()) {
                    card.worked = false;
                }
                self.board_changed(board);
                return Ok(None);
            }
            (Carry::Resume, Some(session)) => Launch::Resume { session: session.clone() },
            _ => {
                let text = if card.note.is_empty() { card.title.clone() } else { format!("{}\n\n{}", card.title, card.note) };
                // A one-word card alone would be read as one of Claude's subcommands (`update`,
                // `doctor`, `mcp`…) even after `--`; with the line under it, it never is one word.
                let prompt = format!("{text}\n\n{}", t!("board.summaryPrompt"));
                Launch::Claude { prompt: Some(prompt), plan: give == Give::Plan }
            }
        };
        let resumed = match &launch {
            Launch::Resume { session } => Some(session.clone()),
            _ => None,
        };

        let view = self.open_terminal_as(&card.path, &launch, Some(tab_name(&card.title)), size)?;

        let now = now_ms();
        let mut board = self.board();
        // Its shell may have ended already, before the card knew of it (`board_terminal_gone`).
        let open = self.terminal_sessions().contains_key(&view.id);
        let Some(card) = board.place(id, Column::Claude, usize::MAX, now) else {
            // Deleted while its tab was opening: no card, so no Claude left working on it.
            drop(board);
            self.close_terminal(view.id);
            return Err(no_card(id));
        };
        card.terminal = open.then_some(view.id);
        card.given_at = Some(now);
        card.worked = false;
        if resumed.is_none() {
            card.session = None;
            card.ran = false;
        }
        board.linked_busy.remove(id);
        if let (true, Some(session)) = (open, resumed) {
            board.resumed.insert(session, now + RESUME_GRACE_MS);
        }
        if !open {
            board.place(id, Column::Review, usize::MAX, now);
        }
        self.board_changed(board);
        Ok(Some(view))
    }

    /// How `card`, moved back into `claude`, goes on (`carry`), from what is known of its session
    /// now.
    fn carry_of(&self, card: &Card) -> Carry {
        let Some(session) = card.session.as_deref() else {
            let starting = card.terminal.is_some_and(|terminal| self.terminal_sessions().contains_key(&terminal));
            return carry(false, starting, &Liveness::Unknown, false, false);
        };

        let liveness = self.liveness(session);
        let logged = self.session_log(session).is_some();
        let gone = card.ran && self.running_cached().is_some_and(|running| !running.contains_key(session));
        let heard = self.lock().scanned.iter().any(|s| s.id == session && s.ended);
        carry(true, false, &liveness, logged, gone || heard)
    }

    /// Hands a card to a session that already runs (a Claude Code tab in VS Code…).
    pub fn board_link(&self, id: &str, session: &str) -> Result<(), String> {
        if !is_session_id(session) {
            return Err(format!("not a session id: {session}"));
        }
        // At work on something else right now: that turn's end isn't the card's (`follow_board`).
        let running = self.running_cached();
        let scanned = self.lock().scanned.iter().find(|s| s.id == session).cloned();
        let busy = stand(session, scanned.as_ref(), running.as_ref(), false) == Stand::Busy;

        let now = now_ms();
        let mut board = self.board();
        let card = board.place(id, Column::Claude, usize::MAX, now).ok_or_else(|| no_card(id))?;
        card.session = Some(session.to_string());
        card.terminal = None;
        card.given_at = Some(now);
        card.worked = false;
        card.ran = false;
        if busy {
            board.linked_busy.insert(id.to_string());
        } else {
            board.linked_busy.remove(id);
        }
        self.board_changed(board);
        Ok(())
    }

    /// A terminal closed: a card whose Claude ran in it goes to review.
    pub(super) fn board_terminal_gone(&self, terminal: u64) {
        let now = now_ms();
        let mut board = self.board();
        let ids: Vec<(String, Column)> = board.cards.iter().filter(|c| c.terminal == Some(terminal)).map(|c| (c.id.clone(), c.column)).collect();
        if ids.is_empty() {
            return;
        }

        for (id, column) in &ids {
            if *column == Column::Claude {
                board.place(id, Column::Review, usize::MAX, now);
            }
            if let Some(card) = board.find_mut(id) {
                card.terminal = None;
            }
        }
        self.board_changed(board);
    }

    /// Follows the cards given to Claude: a card started in a Pitwall terminal takes the session
    /// that starts in it, a card whose session took a new id in its process (`/clear`) takes that
    /// one, and a card goes to `review` once its session's turn is over (after it was seen at
    /// work) or the session ended. Runs after every refresh of Claude's state (`refresh_claude`:
    /// each heartbeat and each change to Claude's files), with or without the window, and costs
    /// nothing while no card is in `claude`.
    pub(super) fn follow_board(&self) {
        // One pass at a time, so an older reading never lands after a newer one.
        let _pass = self.board_follow.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        // (terminal, given at) of the cards still waiting for their session, the sessions held,
        // and the process each was last seen running in.
        let (unlinked, linked, processes) = {
            let board = self.board();
            let given: Vec<&Card> = board.cards.iter().filter(|card| card.column == Column::Claude).collect();
            let unlinked: Vec<(u64, u64)> =
                given.iter().filter(|card| card.session.is_none()).filter_map(|card| Some((card.terminal?, card.given_at.unwrap_or(card.moved_at)))).collect();
            let linked: Vec<String> = given.iter().filter_map(|card| card.session.clone()).collect();
            (unlinked, linked, board.pids.clone())
        };
        if unlinked.is_empty() && linked.is_empty() {
            return;
        }

        // Read again when Claude's sessions folder changed, else at most every 30 s.
        let running = self.running_cached();

        // Each waiting terminal's session: the first one started in it, soon after the give.
        let mut links: HashMap<u64, (String, u64)> = HashMap::new();
        if let (false, Some(running)) = (unlinked.is_empty(), &running) {
            let shells = self.terminal_shells();
            for (session, run) in running {
                let Some(terminal) = shells.iter().find(|(shell, _, _)| run.lineage.contains(shell)).map(|(_, id, _)| *id) else {
                    continue;
                };
                let Some(&(_, given)) = unlinked.iter().find(|(id, _)| *id == terminal) else {
                    continue;
                };
                if run.started_at.is_some_and(|at| at > given + ADOPT_MS) {
                    continue;
                }
                let started = run.started_at.unwrap_or(u64::MAX);
                if links.get(&terminal).is_none_or(|(_, first)| started < *first) {
                    links.insert(terminal, (session.clone(), started));
                }
            }
        }

        // A held session gone from the list while its process runs another one: the same
        // conversation under a new id (`/clear`).
        let mut renamed: HashMap<String, String> = HashMap::new();
        if let Some(running) = &running {
            for session in linked.iter().filter(|session| !running.contains_key(*session)) {
                let next = processes.get(session).and_then(|pid| running.iter().find(|(_, run)| run.pid == *pid));
                if let Some((next, _)) = next {
                    renamed.insert(session.clone(), next.clone());
                }
            }
        }

        let ids: HashSet<String> =
            linked.iter().map(|session| renamed.get(session).unwrap_or(session).clone()).chain(links.values().map(|(session, _)| session.clone())).collect();
        let scanned: HashMap<String, SessionState> = self.lock().scanned.iter().filter(|s| ids.contains(&s.id)).map(|s| (s.id.clone(), s.clone())).collect();

        let now = now_ms();
        let mut board = self.board();
        let mut changed = false;
        let Board { cards, resumed, pids, linked_busy, .. } = &mut *board;

        if let Some(running) = &running {
            pids.extend(ids.iter().filter_map(|id| Some((id.clone(), running.get(id)?.pid))));
            // Opened again and running: followed like any other from here.
            resumed.retain(|id, _| !running.contains_key(id));
        }
        resumed.retain(|_, until| *until > now);

        let mut over: Vec<String> = Vec::new();
        for card in cards.iter_mut().filter(|card| card.column == Column::Claude) {
            if let Some(next) = card.session.as_ref().and_then(|session| renamed.get(session)) {
                card.session = Some(next.clone());
                changed = true;
            }
            if card.session.is_none() {
                if let Some((session, _)) = card.terminal.and_then(|terminal| links.get(&terminal)) {
                    card.session = Some(session.clone());
                    changed = true;
                }
            }
            let Some(session) = card.session.as_deref() else {
                continue;
            };
            if !card.ran && running.as_ref().is_some_and(|running| running.contains_key(session)) {
                card.ran = true;
                changed = true;
            }

            let state = stand(session, scanned.get(session), running.as_ref(), card.ran);
            // Linked while its session was at work on something else: the card's own work
            // starts once that turn is over.
            if linked_busy.contains(&card.id) {
                if state == Stand::Busy {
                    continue;
                }
                linked_busy.remove(&card.id);
                card.given_at = Some(now);
                changed = true;
                if state != Stand::Ended {
                    continue;
                }
            }

            match state {
                Stand::Busy if !card.worked => {
                    card.worked = true;
                    changed = true;
                }
                Stand::Busy | Stand::Unknown => {}
                // A finished turn after the card came in is work done too, though it went unseen.
                Stand::Over(at) => {
                    let since = card.given_at.unwrap_or(0).max(card.moved_at);
                    if card.worked || at.is_some_and(|at| at > since) {
                        over.push(card.id.clone());
                    }
                }
                // Opened again a moment ago: its old end says nothing until it is seen running.
                Stand::Ended if resumed.contains_key(session) => {}
                Stand::Ended => over.push(card.id.clone()),
            }
        }

        // Kept while a card holds the session or still follows it.
        pids.retain(|id, _| cards.iter().any(|card| card.session.as_ref() == Some(id)));
        linked_busy.retain(|id| cards.iter().any(|card| card.id == *id && card.column == Column::Claude));

        for id in &over {
            board.place(id, Column::Review, usize::MAX, now);
            changed = true;
        }

        if changed {
            self.board_changed(board);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, column: Column) -> Card {
        Card {
            id: id.into(),
            path: "/Users/me/projects/paddock".into(),
            title: format!("Card {id} · ğüşiöç"),
            note: "line one\nline two \"quoted\"".into(),
            column,
            created_at: 1_790_000_000_000,
            moved_at: now_ms(),
            session: None,
            terminal: None,
            given_at: None,
            worked: false,
            ran: false,
        }
    }

    /// The board is the only copy of what the user wrote: it comes back whole after a restart,
    /// and a file that can't be read is kept aside, never written over by the next save.
    #[test]
    fn cards_survive_a_restart_and_a_broken_board_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config").join("board.json");

        let mut board = Board::load(file.clone(), now_ms());
        assert!(board.cards.is_empty());
        let mut given = card("b", Column::Review);
        given.session = Some("9bb85e86-f618-4861-9858-03ec8fc36c28".into());
        given.given_at = Some(1_790_000_100_000);
        given.worked = true;
        board.cards = vec![card("a", Column::Queued), given, card("c", Column::Done)];
        board.save();

        let again = Board::load(file.clone(), now_ms());
        assert_eq!(again.cards, board.cards);

        let backups = || -> Vec<PathBuf> {
            fs::read_dir(file.parent().unwrap())
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("board.json.broken-"))
                .collect()
        };

        // Not JSON: set aside, the board starts empty, and saving it leaves the copy alone.
        let broken = "{\"version\":1,\"cards\":[{\"id\":\"a\",";
        fs::write(&file, broken).unwrap();
        let mut empty = Board::load(file.clone(), now_ms());
        assert!(empty.cards.is_empty());
        assert_eq!(backups().len(), 1);
        empty.cards.push(card("d", Column::Queued));
        empty.save();
        assert_eq!(fs::read_to_string(&backups()[0]).unwrap(), broken);

        // One card that doesn't fit: the others stay, and the file as it was is kept too.
        let fits = card("e", Column::Queued);
        let mixed = serde_json::json!({ "version": 1, "cards": [fits, { "id": "f", "column": "elsewhere" }] }).to_string();
        fs::write(&file, &mixed).unwrap();
        let salvaged = Board::load(file.clone(), now_ms());
        assert_eq!(salvaged.cards, vec![fits]);
        let kept = backups();
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|path| fs::read_to_string(path).unwrap() == mixed));
    }

    /// A move changes where a card is, never what it holds: its session comes along through
    /// every column, back to `queued` too, and is still there after a restart.
    #[test]
    fn a_moved_card_keeps_its_session() {
        let tmp = tempfile::tempdir().unwrap();
        let settings_file = tmp.path().join("config").join("settings.json");
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude").join("projects"),
                settings_file: settings_file.clone(),
                claude_settings: tmp.path().join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );

        let mut given = card("a", Column::Claude);
        given.session = Some("9bb85e86-f618-4861-9858-03ec8fc36c28".into());
        given.given_at = Some(1_790_000_100_000);
        core.board().cards.push(given.clone());

        for column in ["queued", "review", "done", "queued", "claude", "queued"] {
            core.board_move("a", column, 0).unwrap();
            let moved = core.board_state().remove(0);
            assert_eq!(serde_json::to_value(moved.column).unwrap(), column);
            assert_eq!((&moved.session, moved.given_at, &moved.title, &moved.note), (&given.session, given.given_at, &given.title, &given.note), "{column}");
        }

        let again = Board::load(settings_file.with_file_name("board.json"), now_ms());
        assert_eq!(again.cards, core.board_state());
    }

    /// Cards of a folder that left the list go to a listed project whole: every card and its
    /// order kept, nothing else on the board touched, and never to a folder that isn't listed.
    #[test]
    fn rehomed_cards_all_arrive_in_their_order() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("paddock");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("package.json"), r#"{"name":"paddock","scripts":{"dev":"vite"}}"#).unwrap();
        let listed = project.to_string_lossy().into_owned();
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude").join("projects"),
                settings_file: tmp.path().join("config").join("settings.json"),
                claude_settings: tmp.path().join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        core.add_project(&listed).unwrap();

        let at = |id: &str, column: Column, path: &str| Card { path: path.into(), ..card(id, column) };
        let gone = "/Users/me/projects/renamed-away";
        let other = "/Users/me/projects/elsewhere";
        core.board().cards = vec![
            at("a", Column::Queued, gone),
            at("b", Column::Queued, &listed),
            at("c", Column::Done, gone),
            at("d", Column::Queued, other),
            at("e", Column::Queued, gone),
        ];

        assert!(core.board_rehome(gone, other).is_err(), "not a listed project");
        assert_eq!(core.board_state().iter().filter(|c| c.path == gone).count(), 3);

        core.board_rehome(gone, &listed).unwrap();
        let cards = core.board_state();
        let of = |path: &str| -> Vec<(String, Column)> { cards.iter().filter(|c| c.path == path).map(|c| (c.id.clone(), c.column)).collect() };
        assert_eq!(cards.len(), 5);
        assert_eq!(of(&listed), [("b".to_string(), Column::Queued), ("a".into(), Column::Queued), ("c".into(), Column::Done), ("e".into(), Column::Queued)]);
        assert_eq!(of(other), [("d".to_string(), Column::Queued)]);
        assert!(of(gone).is_empty());
    }

    /// A card moved back into `claude` goes on with its own session, and a session that runs or
    /// may still run is never opened a second time (two processes would write one log).
    #[test]
    fn a_session_opens_again_only_once_it_has_ended() {
        let running = Liveness::Running { entrypoint: Some("cli".into()), lineage: vec![900, 700] };
        // (holds a session, its terminal still starting, liveness, log there, known ended)
        let cases = [
            // Running: followed, whatever else is known.
            (true, false, running.clone(), true, false, Carry::Follow),
            (true, false, running, true, true, Carry::Follow),
            // Ended: opened again; with no log left there is nothing to open, so a new one.
            (true, false, Liveness::Ended, true, false, Carry::Resume),
            (true, false, Liveness::Ended, false, false, Carry::Fresh),
            // Can't tell: only followed, unless it is known to have ended.
            (true, false, Liveness::Unknown, true, false, Carry::Follow),
            (true, false, Liveness::Unknown, true, true, Carry::Resume),
            (true, false, Liveness::Unknown, false, false, Carry::Fresh),
            // No session yet: the one starting in its terminal, else a new one.
            (false, true, Liveness::Unknown, false, false, Carry::Follow),
            (false, false, Liveness::Unknown, false, false, Carry::Fresh),
        ];

        for (session, starting, liveness, logged, ended, expected) in cases {
            assert_eq!(carry(session, starting, &liveness, logged, ended), expected, "{session} {starting} {liveness:?} {logged} {ended}");
        }
    }
}
