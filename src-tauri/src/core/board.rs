//! The board (Pano): per project, cards the user writes (a title, a note) in four columns. A
//! card given to Claude opens a Claude Code tab in one of Pitwall's terminals with its text as
//! the first prompt, then follows that session: it stays in `claude` while Claude works or waits
//! on a permission or a question, and goes to `review` once the turn is over or the session has
//! ended. From `review` it goes back to `claude` when its session goes to work again. The user
//! moves it to `done`.
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
//! An image pasted into a card's note is `[Image #n]` in the note's text, as Claude Code writes
//! one in its prompt, and a file in the card's own folder under `board-images/`, beside
//! `board.json`. A given card names those files under its text, and Claude starts with that
//! folder among its directories (`--add-dir`), so it reads them without asking.
//!
//! A given card asks Claude to end with a short summary, and the card's details show what
//! Claude last said in its session once the work is over, as a session's details do
//! (`Core::last_message`): read from that session's log when asked, never kept.
//!
//! A card is given with a model and in a mode (`start.rs`: each one of a fixed list): those
//! chosen for it, else its project's. A card in `queued` can be told to wait (`Schedule`): the
//! core gives it when its moment comes (`board_due`), once. The schedule is the card's own, in
//! the board's file, and whoever takes it off the card gives the card: it is off, and the file
//! written, before anything starts. The cards that wait for the 5-hour session's refill share
//! one fresh quota: they are given one at a time, each next one once Claude is no longer at work
//! on the one before, or ten minutes after it when Claude was never seen at work on it
//! (`Board::line_held`).

use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::Value;

use super::reveal::{is_session_id, Liveness, Opened};
use super::sessions::{row_phase, status_phase, RowPhase, Running};
use super::terminal::{Launch, TAB_NAME_MAX};
use super::*;
use crate::claude::TurnKind;
use crate::registry::{base36, write_atomic};
use crate::settings::set_aside;
use crate::start::{Mode, Start};

/// Done cards are dropped this long after they got there.
const DONE_KEPT_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const FILE_VERSION: u32 = 1;
/// A session opened again is given this long to show among the running ones; until then its old
/// end doesn't send its card on.
const RESUME_GRACE_MS: u64 = 60_000;
/// A card given in a terminal takes the session of a Claude started there within this long of
/// the give; a `claude` started in that tab later is somebody else's work.
const ADOPT_MS: u64 = 2 * 60 * 1000;
/// Each card's images are in a folder of its own under this one, beside the board's file.
const IMAGES_DIR: &str = "board-images";
/// A card holds this many images at most, each this large at most.
pub(super) const IMAGES_MAX: usize = 12;
const IMAGE_BYTES_MAX: usize = 10 * 1024 * 1024;
/// A card whose moment passed longer ago than this (Pitwall wasn't running, or the Mac slept) is
/// not given any more: its schedule goes and it stays where it is.
const LATE_MS: u64 = 30 * 60 * 1000;
/// A card that waits for the session's refill follows a reading that names another moment, up to
/// this much later: one further off is the next session's refill, not the one it waits for.
const REFILL_REACH_MS: u64 = 4 * 60 * 60 * 1000;
/// A card given from the line whose Claude isn't seen at work this long after the give (it sits
/// idle, or nothing tells) holds the line no more: the next one goes, and the card stays.
const UNSEEN_MS: u64 = 10 * 60 * 1000;

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

/// What an image file is: the kinds Claude reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageKind {
    /// What `bytes` are, by how they start; None for anything else.
    fn of(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some(Self::Png),
            [0xFF, 0xD8, 0xFF, ..] => Some(Self::Jpeg),
            [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some(Self::Gif),
            [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some(Self::Webp),
            _ => None,
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
}

/// An image pasted into a card's note: `[Image #n]` in the note's text, and the file
/// `<n>.<kind>` in the card's image folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardImage {
    pub n: u32,
    pub kind: ImageKind,
}

impl CardImage {
    /// Its file's name: a number and one of four endings, whatever the board's file says.
    fn file_name(self) -> String {
        format!("{}.{}", self.n, self.kind.ext())
    }
}

/// An image the window hands over with a card's text: its number in the note, and its bytes as a
/// `data:` URL in base64.
#[derive(Debug, Clone, Deserialize)]
pub struct NewImage {
    pub n: u32,
    pub data: String,
}

/// `note` names image `n`: `[Image #n]` is in its text.
fn names(note: &str, n: u32) -> bool {
    note.contains(&format!("[Image #{n}]"))
}

/// The bytes of a `data:` URL and what image they are. Only how the bytes start counts, never
/// the type the URL gives.
fn decode_image(data: &str) -> Result<(ImageKind, Vec<u8>), String> {
    let too_large = || format!("an image is at most {} MB", IMAGE_BYTES_MAX / (1024 * 1024));
    let encoded = data.strip_prefix("data:").and_then(|rest| rest.split_once(";base64,")).map(|(_, encoded)| encoded).ok_or("not an image")?;
    if encoded.len() / 4 * 3 > IMAGE_BYTES_MAX + 3 {
        return Err(too_large());
    }
    let bytes = BASE64.decode(encoded).map_err(|_| "not an image")?;
    if bytes.len() > IMAGE_BYTES_MAX {
        return Err(too_large());
    }
    let kind = ImageKind::of(&bytes).ok_or("not an image")?;
    Ok((kind, bytes))
}

/// What of `images` a card with `note` takes: those the note names, each number once, decoded.
pub(super) fn taken(note: &str, images: &[NewImage]) -> Result<Vec<(CardImage, Vec<u8>)>, String> {
    let mut taken: Vec<(CardImage, Vec<u8>)> = Vec::new();
    for image in images.iter().filter(|image| image.n > 0 && names(note, image.n)) {
        if taken.iter().any(|(held, _)| held.n == image.n) {
            continue;
        }
        let (kind, bytes) = decode_image(&image.data)?;
        taken.push((CardImage { n: image.n, kind }, bytes));
    }
    Ok(taken)
}

fn too_many() -> String {
    format!("a card holds at most {IMAGES_MAX} images")
}

/// A card told to wait: when it is given, and with what.
///
/// The cards that wait for the refill are given one at a time, in the line they stand in once
/// it comes (`board_due_at`): the first at the refill, each next one once Claude is no longer at
/// work on the one given before it, or never went to work on it (`UNSEEN_MS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// The moment, in ms. In the line (`line`): when the line last moved for it (it got in, a
    /// card was given from it, Pitwall quit), which a restart reads as a missed moment's
    /// (`LATE_MS`).
    pub at: u64,
    /// The moment is the 5-hour session's refill: it follows the reading (`board_refill`).
    #[serde(default)]
    pub refill: bool,
    /// The model and the mode chosen for it.
    #[serde(default)]
    pub start: Start,
    /// Its refill came while a card before it was given or still with Claude: it waits its turn.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub line: bool,
}

/// A part of a card that is there or not (`start`, `schedule`): what the file has when it fits,
/// nothing when it doesn't. A part this version can't read never costs the card itself.
fn part_or_none<'de, T: serde::de::DeserializeOwned, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).ok())
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
    /// In `review`: its session has been seen at rest (its turn over, or ended) since the card
    /// got there. Only then does that session at work mean it went on.
    #[serde(default)]
    pub rested: bool,
    /// The images pasted into its note, by number.
    #[serde(default)]
    pub images: Vec<CardImage>,
    /// The model and the mode its last new session was started with; none before the first.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "part_or_none")]
    pub start: Option<Start>,
    /// In `queued`: told to wait, the core gives it when its moment comes (`board_due`). It goes
    /// when the card is given, edited, moved to another column or to another project.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "part_or_none")]
    pub schedule: Option<Schedule>,
}

pub(super) struct Board {
    file: PathBuf,
    /// The folder of the cards' image folders.
    images: PathBuf,
    pub(super) cards: Vec<Card>,
    /// False when an unusable file could not be set aside: nothing is written over it.
    writable: bool,
    /// Sessions opened again and not seen running yet, and until when they are waited for.
    resumed: HashMap<String, u64>,
    /// The process each followed session was last seen running in: another session id in that
    /// process is the same conversation gone on (`/clear`).
    pids: HashMap<String, u32>,
    /// Cards linked to a session that was at work on something else: that turn isn't theirs.
    linked_busy: HashSet<String>,
    /// The cards were followed once since launch (`follow_board`).
    followed: bool,
    /// Cards on their way to Claude right now (a tab is opening for them): none is given a
    /// second time meanwhile, by hand or at its moment.
    giving: HashSet<String>,
    /// The cards the board's file says wait for a moment, as it was last read or written. None
    /// of them is given until the file has taken note that it waits no longer (`board_noted`,
    /// `board_due_at`): the next run would give it at its moment, a second time.
    filed_waiting: HashSet<String>,
    /// The cards given from the line of those that waited for the refill (at their turn, or by
    /// hand), while Claude may still be at work on them: the next in line waits for them all
    /// (`line_held`). This run's own: the Claude a card was given to ends with the run.
    line: HashMap<String, Lined>,
    /// Pitwall quits (`board_quit`): no card is given at its moment from then on, while the
    /// loop that looks at them still runs.
    quit: bool,
}

/// A card given from the line, as `Board::line` keeps it.
#[derive(Debug, Clone, Copy)]
struct Lined {
    /// When it was given.
    at: u64,
    /// Its Claude has been seen at work since (`follow_board`): working, or waiting on a
    /// permission or a question.
    worked: bool,
}

impl Lined {
    fn given(at: u64) -> Self {
        Self { at, worked: false }
    }
}

impl Board {
    /// The saved board; empty when there is none. An unusable file is set aside first
    /// (`board.json.broken-<ms>`), never written over; done cards older than 30 days are dropped.
    pub(super) fn load(file: PathBuf, now: u64) -> Self {
        let (cards, writable, changed, dropped) = read_cards(&file, now);
        let images = file.with_file_name(IMAGES_DIR);
        let filed_waiting = waiting_ids(&cards);
        let mut board = Self { file, images, cards, writable, resumed: HashMap::new(), pids: HashMap::new(), linked_busy: HashSet::new(), followed: false, giving: HashSet::new(), filed_waiting, line: HashMap::new(), quit: false };
        if changed && board.save() {
            // Gone from the board's file for good: their images go with them.
            for card in &dropped {
                board.remove_images(&card.id, &card.images);
            }
        }
        board
    }

    /// Card `id`'s image folder. None for an id the board never gives (anything but letters,
    /// digits and dashes): a board file written by hand names no folder elsewhere.
    pub(super) fn image_dir(&self, id: &str) -> Option<PathBuf> {
        let own = !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        own.then(|| self.images.join(id))
    }

    /// Writes a card's new images into its folder.
    pub(super) fn write_images(&self, id: &str, images: &[(CardImage, Vec<u8>)]) -> Result<(), String> {
        if images.is_empty() {
            return Ok(());
        }
        let dir = self.image_dir(id).ok_or_else(|| no_card(id))?;
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        for (image, bytes) in images {
            fs::write(dir.join(image.file_name()), bytes).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Deletes the files of `images`, a card's, and its folder once nothing is left in it. Only
    /// files the card names go, never a folder with what is in it.
    pub(super) fn remove_images(&self, id: &str, images: &[CardImage]) {
        let Some(dir) = self.image_dir(id).filter(|_| !images.is_empty()) else {
            return;
        };
        for image in images {
            let _ = fs::remove_file(dir.join(image.file_name()));
        }
        let _ = fs::remove_dir(&dir);
    }

    /// Writes the board; false when it isn't on disk as it is now.
    fn save(&mut self) -> bool {
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
        if written.is_ok() {
            self.filed_waiting = waiting_ids(&self.cards);
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
    /// last one: at the end), as the board shows them. A new column sets `moved_at`, and the
    /// card is no longer `rested`: that is told anew where it got to. Nor does it wait for a
    /// moment any more (`schedule`): only a card left in `queued` does.
    pub(super) fn place(&mut self, id: &str, column: Column, index: usize, now: u64) -> Option<&mut Card> {
        let at = self.cards.iter().position(|card| card.id == id)?;
        let mut card = self.cards.remove(at);
        if card.column != column {
            card.column = column;
            card.moved_at = now;
            card.rested = false;
            card.schedule = None;
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

    /// Claude may still be at work on a card given from the line: the next in line waits. A card
    /// on its way (a tab opening for it) holds the line, and so does one in `claude`: it leaves
    /// that column once its session's turn is over (not while Claude Code still calls it busy) or
    /// the session ended, its tab closed (`follow_board`, `board_terminal_gone`), or the user
    /// moved or deleted it. A permission wait or a question keeps it there. Without a session
    /// past `ADOPT_MS` its own Claude never started in its tab (the board shows it closed): it
    /// holds nothing. Nor does one whose Claude wasn't seen at work within `UNSEEN_MS` of the
    /// give (`Lined::worked`): it stays in `claude`, and the next goes. The cards that no longer
    /// hold the line are let go.
    fn line_held(&mut self, now: u64) -> bool {
        let Self { cards, giving, line, .. } = self;
        line.retain(|id, lined| {
            giving.contains(id)
                || cards.iter().any(|card| {
                    card.id == *id
                        && card.column == Column::Claude
                        && (card.session.is_some() || now.saturating_sub(card.given_at.unwrap_or(card.moved_at)) <= ADOPT_MS)
                        && (lined.worked || now.saturating_sub(lined.at) <= UNSEEN_MS)
                })
        });
        !line.is_empty()
    }
}

/// The cards of `cards` that wait for a moment, by id.
fn waiting_ids(cards: &[Card]) -> HashSet<String> {
    cards.iter().filter(|card| card.schedule.is_some()).map(|card| card.id.clone()).collect()
}

/// The cards in `file`, whether it may be written, whether what was read changed (to be
/// written back), and the done cards dropped for their age.
fn read_cards(file: &Path, now: u64) -> (Vec<Card>, bool, bool, Vec<Card>) {
    let raw = match fs::read(file) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return (Vec::new(), true, false, Vec::new()),
        Err(error) => return (Vec::new(), set_aside(file, &error.to_string()), false, Vec::new()),
    };
    let value = match serde_json::from_slice::<Value>(&raw) {
        Ok(value) => value,
        Err(error) => return (Vec::new(), set_aside(file, &error.to_string()), false, Vec::new()),
    };
    let Some(items) = value.get("cards").and_then(Value::as_array) else {
        return (Vec::new(), set_aside(file, "no card list"), false, Vec::new());
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

    let (mut cards, dropped): (Vec<Card>, Vec<Card>) = cards.into_iter().partition(|card| card.column != Column::Done || now.saturating_sub(card.moved_at) < DONE_KEPT_MS);
    changed |= !dropped.is_empty();

    for card in &mut cards {
        if card.session.as_deref().is_some_and(|session| !is_session_id(session)) {
            card.session = None;
            changed = true;
        }
        // Only a card up next waits for a moment, whatever the file says.
        if card.column != Column::Queued && card.schedule.take().is_some() {
            changed = true;
        }
        // The line stood still while Pitwall wasn't running: one left longer than a missed
        // moment would be is let go as that moment is (`LATE_MS`); the rest goes on from its
        // first card, one at a time (`board_due_at`).
        if card.schedule.is_some_and(|schedule| schedule.line && now.saturating_sub(schedule.at) > LATE_MS) {
            card.schedule = None;
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

    (cards, writable, changed, dropped)
}

/// Where a followed session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stand {
    /// Working, or waiting on a permission or a question.
    Busy,
    /// Its turn is over: waiting with a finished turn (when it finished), or idle.
    Over(Option<u64>),
    /// Its turn under way was interrupted (Esc): over, with nothing to tell.
    Interrupted,
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
        Some((RowPhase::Idle, _)) if scanned.is_some_and(|s| s.interrupted) => Stand::Interrupted,
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
        Liveness::Stopped | Liveness::Unknown if ended => Carry::Resume,
        Liveness::Stopped | Liveness::Unknown => Carry::Follow,
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

/// A tab's title from a card's, or from its session's name: one line, whole up to
/// `TAB_NAME_MAX`. The tab cuts what doesn't fit and shows it all on hover.
pub(super) fn tab_name(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(TAB_NAME_MAX).collect()
}

impl Core {
    pub(super) fn board(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Saves the board and sends it to the window, the lock let go first. A board that couldn't
    /// be written says so in the snapshot (`board_unsaved`).
    pub(super) fn board_changed(&self, board: MutexGuard<'_, Board>) {
        self.board_written(board);
    }

    /// `board_changed`, saying whether the board is on disk as it is now.
    fn board_written(&self, mut board: MutexGuard<'_, Board>) -> bool {
        let saved = board.save();
        self.board_told(board, saved);
        saved
    }

    /// The board written for a give by hand of a card the file says waits (`filed_waiting`): its
    /// schedule is off the file before anything starts, and the card is on its way (`giving`).
    /// When the file can't be written the give is off, as at the card's moment (`board_due_at`):
    /// the card waits on as it did (`waited`, the schedule taken off it, is put back), and the
    /// answer is the board's own words for that.
    fn board_noted(&self, mut board: MutexGuard<'_, Board>, id: &str, waited: Option<Schedule>) -> Result<(), String> {
        let saved = board.save();
        if saved {
            board.giving.insert(id.to_string());
        } else if let Some(card) = board.find_mut(id) {
            card.schedule = waited;
        }
        self.board_told(board, saved);
        if saved {
            Ok(())
        } else {
            Err(t!("board.unsaved.title"))
        }
    }

    /// Sends the board to the window, the lock let go first, and notes whether it is on disk as
    /// it is now (`saved`).
    fn board_told(&self, board: MutexGuard<'_, Board>, saved: bool) {
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

    /// A new card, at the end of `queued`. `images`: those pasted into its note; it takes the
    /// ones the note still names.
    pub fn board_add(&self, path: &str, title: &str, note: &str, images: &[NewImage]) -> Result<Card, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a card needs a title".into());
        }
        if path.trim().is_empty() {
            return Err("a card needs a project".into());
        }
        let note = note.trim();
        let taken = taken(note, images)?;
        if taken.len() > IMAGES_MAX {
            return Err(too_many());
        }
        let mut held: Vec<CardImage> = taken.iter().map(|(image, _)| *image).collect();
        held.sort_by_key(|image| image.n);

        let now = now_ms();
        let mut board = self.board();
        let id = new_id(&board.cards, now);
        board.write_images(&id, &taken)?;
        let card = Card {
            id,
            path: path.to_string(),
            title: title.to_string(),
            note: note.to_string(),
            column: Column::Queued,
            created_at: now,
            moved_at: now,
            session: None,
            terminal: None,
            given_at: None,
            worked: false,
            ran: false,
            rested: false,
            images: held,
            start: None,
            schedule: None,
        };
        board.cards.push(card.clone());
        board.place(&card.id, Column::Queued, usize::MAX, now);
        self.board_changed(board);
        Ok(card)
    }

    /// A card's new text. `images`: those pasted into its note since it was saved. An image
    /// stays while the note names it (`[Image #n]`), and its file goes once the note no longer
    /// does; a new one under a held number takes that one's place. A card that waited for its
    /// moment no longer does: what was to be given then was the text as it stood.
    pub fn board_edit(&self, id: &str, title: &str, note: &str, images: &[NewImage]) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a card needs a title".into());
        }
        let note = note.trim();
        let added = taken(note, images)?;

        let mut board = self.board();
        let before = board.find(id).ok_or_else(|| no_card(id))?.images.clone();
        let mut held: Vec<CardImage> = before.iter().copied().filter(|image| names(note, image.n) && !added.iter().any(|(new, _)| new.n == image.n)).collect();
        held.extend(added.iter().map(|(image, _)| *image));
        held.sort_by_key(|image| image.n);
        if held.len() > IMAGES_MAX {
            return Err(too_many());
        }
        board.write_images(id, &added)?;
        let gone: Vec<CardImage> = before.into_iter().filter(|image| !held.contains(image)).collect();

        let card = board.find_mut(id).ok_or_else(|| no_card(id))?;
        card.title = title.to_string();
        card.note = note.to_string();
        card.images = held;
        card.schedule = None;
        board.remove_images(id, &gone);
        self.board_changed(board);
        Ok(())
    }

    /// One of a card's images as a `data:` URL, for the window to draw.
    pub fn board_image(&self, id: &str, n: u32) -> Result<String, String> {
        let (file, kind) = {
            let board = self.board();
            let image = board.find(id).and_then(|card| card.images.iter().find(|image| image.n == n).copied()).ok_or_else(|| no_card(id))?;
            (board.image_dir(id).ok_or_else(|| no_card(id))?.join(image.file_name()), image.kind)
        };
        let bytes = fs::read(&file).map_err(|error| error.to_string())?;
        Ok(format!("data:{};base64,{}", kind.mime(), BASE64.encode(bytes)))
    }

    /// The image files of `card` that are there, by number, and the folder they are in.
    pub(super) fn card_images(&self, card: &Card) -> Option<(PathBuf, Vec<(u32, PathBuf)>)> {
        let dir = self.board().image_dir(&card.id)?;
        let files: Vec<(u32, PathBuf)> = card.images.iter().map(|image| (image.n, dir.join(image.file_name()))).filter(|(_, file)| file.is_file()).collect();
        (!files.is_empty()).then_some((dir, files))
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

    /// Deletes a card, and its images with it.
    pub fn board_delete(&self, id: &str) {
        let mut board = self.board();
        let Some(at) = board.cards.iter().position(|card| card.id == id) else {
            return;
        };
        let card = board.cards.remove(at);
        board.remove_images(&card.id, &card.images);
        self.board_changed(board);
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
            // Another project's now, with settings of its own: it waits for nothing.
            card.schedule = None;
        }
        board.cards = kept;
        board.cards.append(&mut moved);
        if any {
            self.board_changed(board);
        }
        Ok(())
    }

    /// Gives a card to Claude: a Claude Code tab in the card's project, with the card's text as
    /// the first prompt (`Give::Plan`: in plan mode) and, under it, the files of the images its
    /// note names and a line asking for a short summary at the end (the card's details show it).
    /// The card goes to `claude` and follows the session that starts in that tab
    /// (`follow_board`).
    ///
    /// `Give::Continue` keeps the session the card holds instead (`carry`): one that runs is
    /// followed where it is, with no tab opened (`None`); one that has ended opens again in a
    /// tab, as it was.
    ///
    /// A new session starts with the model and in the mode of the card's project
    /// (`board_give_with`: with those chosen for it).
    pub fn board_give(self: &Arc<Self>, id: &str, give: Give, size: Option<(u16, u16)>) -> Result<Option<TerminalView>, String> {
        self.board_give_with(id, give, None, size)
    }

    /// `board_give`, with the model and the mode chosen for the card (`chosen`). Without a
    /// choice, a card that waited for its moment starts with what was chosen when it was told to
    /// wait (it is given by hand now, and waits no longer); any other as its project's settings
    /// say, or, going on without a session to go on with (`Give::Continue`), as it started last.
    /// `Give::Plan` is plan mode whatever else holds.
    ///
    /// A card on its way to Claude already (a tab is opening for it) is left to that: `None`.
    /// A card the board's file says waits is not given while that file can't be written, by
    /// hand as at its moment (`board_noted`): refused, in the words the board has for that.
    pub fn board_give_with(self: &Arc<Self>, id: &str, give: Give, chosen: Option<Start>, size: Option<(u16, u16)>) -> Result<Option<TerminalView>, String> {
        let (card, waited) = {
            let mut board = self.board();
            if board.giving.contains(id) {
                return Ok(None);
            }
            let card = board.find_mut(id).ok_or_else(|| no_card(id))?;
            let waited = card.schedule.take();
            let card = card.clone();
            // Taken from the line by hand: the next in line waits for it too.
            if waited.is_some_and(|schedule| schedule.line) {
                board.line.insert(id.to_string(), Lined::given(now_ms()));
            }
            // Its schedule is off the file before anything starts (`board_due`).
            if waited.is_some() || board.filed_waiting.contains(id) {
                self.board_noted(board, id, waited)?;
            } else {
                board.giving.insert(id.to_string());
            }
            (card, waited)
        };

        let kept = card.start.filter(|_| give == Give::Continue);
        let mut start = chosen.or(waited.map(|schedule| schedule.start)).or(kept).unwrap_or_else(|| self.project_start(&card.path));
        if give == Give::Plan {
            start.mode = Mode::Plan;
        }

        let given = self.launch_card(&card, give, start, size, false);
        self.board().giving.remove(id);
        given
    }

    /// How card `id` starts when nothing is chosen for it: as its project's settings say.
    pub fn board_start(&self, id: &str) -> Option<Start> {
        let path = self.board().find(id)?.path.clone();
        Some(self.project_start(&path))
    }

    /// "Give now" on a card that waits for its moment: given at once, with what was chosen for
    /// it. One that waits no longer (the core gave it a moment ago, it was given or edited
    /// elsewhere) is left as it is, `None`: a card is given by whoever takes its schedule off
    /// it, so never twice for one. Nor is it given while the board's file can't be written
    /// (`board_noted`): refused, in the words the board has for that, it waits on.
    pub fn board_give_now(self: &Arc<Self>, id: &str, size: Option<(u16, u16)>) -> Result<Option<TerminalView>, String> {
        let (card, schedule) = {
            let mut board = self.board();
            if board.giving.contains(id) {
                return Ok(None);
            }
            let card = board.find_mut(id).ok_or_else(|| no_card(id))?;
            let Some(schedule) = card.schedule.take() else {
                return Ok(None);
            };
            let card = card.clone();
            // Taken from the line by hand: the next in line waits for it too.
            if schedule.line {
                board.line.insert(id.to_string(), Lined::given(now_ms()));
            }
            self.board_noted(board, id, Some(schedule))?;
            (card, schedule)
        };

        let given = self.launch_card(&card, Give::New, schedule.start, size, false);
        self.board().giving.remove(id);
        given
    }

    /// Tells a card in `queued` to wait until `at` (ms): the core gives it then (`board_due`),
    /// with `start`. `refill`: the moment is the 5-hour session's refill, and follows the
    /// reading (`board_refill`). Only a listed project's card waits: no other is ever given.
    pub fn board_schedule(&self, id: &str, at: u64, refill: bool, start: Start) -> Result<(), String> {
        let listed = self.listed_paths();
        let mut board = self.board();
        if board.giving.contains(id) {
            return Err(format!("card {id} is being given"));
        }
        let card = board.find_mut(id).ok_or_else(|| no_card(id))?;
        if card.column != Column::Queued {
            return Err(format!("card {id} is not up next"));
        }
        if !listed.contains(&card.path) {
            return Err(t!("core.error.depsNotListed", path = card.path));
        }
        if at <= now_ms() {
            return Err(t!("core.start.noRefill"));
        }
        card.schedule = Some(Schedule { at, refill, start, line: false });
        self.board_changed(board);
        Ok(())
    }

    /// The card no longer waits; it stays where it is.
    pub fn board_unschedule(&self, id: &str) {
        let mut board = self.board();
        if board.find_mut(id).and_then(|card| card.schedule.take()).is_some() {
            self.board_changed(board);
        }
    }

    /// A reading of Claude's limits says the 5-hour session refills at `at`: the cards that wait
    /// for the refill follow it. Only while their own moment is still ahead (one that has passed
    /// is `board_due`'s, whatever is read later), and only to a moment of the session they wait
    /// on: the next session's refill, hours on, is not theirs. Nor is a moment already past: a
    /// card that followed it would be given at once, or dropped as long past (`LATE_MS`). A card
    /// in the line has had its refill: it waits its turn, whatever is read.
    pub fn board_refill(&self, at: u64) {
        let now = now_ms();
        if at <= now {
            return;
        }
        let mut board = self.board();
        let mut moved = false;
        for schedule in board.cards.iter_mut().filter_map(|card| card.schedule.as_mut()) {
            if schedule.refill && !schedule.line && schedule.at > now && schedule.at != at && at < schedule.at + REFILL_REACH_MS {
                schedule.at = at;
                moved = true;
            }
        }
        if moved {
            self.board_changed(board);
        }
    }

    /// Pitwall quits, and with it the Claude of a card given from the line: the line stands still
    /// from now, and no card is given at its moment any more (`board_due_at`). Its cards say so,
    /// for the next start (`read_cards`).
    pub(super) fn board_quit(&self) {
        let now = now_ms();
        let mut board = self.board();
        board.quit = true;
        let mut lined = false;
        for schedule in board.cards.iter_mut().filter_map(|card| card.schedule.as_mut()).filter(|schedule| schedule.line) {
            schedule.at = now;
            lined = true;
        }
        if lined {
            board.save();
        }
    }

    /// Gives the cards whose moment has come, as their button would, with nothing brought to
    /// the front. Runs every second (`tick`), and costs a look at the cards while none is due
    /// and while the line waits on a card given from it.
    pub(super) fn board_due(self: &Arc<Self>) {
        self.board_due_at(now_ms(), &mut |card, start| self.launch_card(card, Give::New, start, None, true).map(|_| ()));
    }

    /// `board_due` at `now`, giving through `give`.
    ///
    /// Each due card's schedule is taken off it and the board written before anything starts,
    /// so it is given once: not again by a second look, nor after a restart, nor by a hand that
    /// gives it at the same moment. With that, the card is given only if it is still up next, its
    /// project is still listed, the board could be written (else the file would still say it
    /// waits), and its moment is not long past (`LATE_MS`: Pitwall wasn't running, or the Mac
    /// slept through it). Otherwise, and when giving it fails, it stays where it is without its
    /// schedule: nothing is tried again.
    ///
    /// The cards whose refill came are given one at a time, in the order the board shows them
    /// (the projects as listed, each one's cards up next from the top): the first while no card
    /// given from the line holds it (`Board::line_held`), the others wait their turn in the line
    /// (`Schedule::line`), where no moment makes them late. A card of the line that is not given
    /// (its project no longer listed, its give failed) holds nothing: the next goes in its place.
    /// A card waiting for a moment of its own (not the refill) is given at it, line or not.
    fn board_due_at(&self, now: u64, give: &mut dyn FnMut(&Card, Start) -> Result<(), String>) {
        {
            let mut board = self.board();
            let new = board.cards.iter().any(|card| card.schedule.is_some_and(|schedule| schedule.at <= now && !schedule.line));
            let lined = board.cards.iter().any(|card| card.schedule.is_some_and(|schedule| schedule.line));
            if !new && !(lined && !board.line_held(now)) {
                return;
            }
        }
        // Before the board is locked: it takes the core's own lock.
        let listed = self.listed_paths();
        let place = |path: &str| listed.iter().position(|listed| listed == path).unwrap_or(usize::MAX);

        let mut board = self.board();
        // Quitting: what waits goes on waiting, for the next start.
        if board.quit {
            return;
        }
        let mut free = !board.line_held(now);
        let mut changed = false;
        let mut due: Vec<(Card, Start)> = Vec::new();
        let Board { cards, giving, line, .. } = &mut *board;

        // Each at its own moment.
        for card in cards.iter_mut() {
            let Some(schedule) = card.schedule.filter(|schedule| schedule.at <= now && !schedule.refill) else {
                continue;
            };
            card.schedule = None;
            changed = true;
            let on_time = now - schedule.at <= LATE_MS;
            if on_time && card.column == Column::Queued && listed.contains(&card.path) && giving.insert(card.id.clone()) {
                due.push((card.clone(), schedule.start));
            }
        }

        // The refill's, in the line.
        let mut order: Vec<usize> = (0..cards.len()).filter(|&i| cards[i].schedule.is_some_and(|schedule| schedule.refill && schedule.at <= now)).collect();
        order.sort_by_key(|&i| (place(&cards[i].path), i));
        let mut moved = false;
        for i in order {
            let card = &mut cards[i];
            let Some(mut schedule) = card.schedule else {
                continue;
            };
            let late = !schedule.line && now - schedule.at > LATE_MS;
            if late || card.column != Column::Queued || !listed.contains(&card.path) {
                card.schedule = None;
                changed = true;
            } else if free {
                card.schedule = None;
                changed = true;
                free = false;
                moved = true;
                line.insert(card.id.clone(), Lined::given(now));
                // On its way already, by hand: that give is its own.
                if giving.insert(card.id.clone()) {
                    due.push((card.clone(), schedule.start));
                }
            } else if !schedule.line {
                schedule.line = true;
                schedule.at = now;
                card.schedule = Some(schedule);
                changed = true;
            }
        }
        // What is left in the line saw it move now: after a restart it goes on from here.
        if moved {
            for schedule in cards.iter_mut().filter_map(|card| card.schedule.as_mut()).filter(|schedule| schedule.line) {
                schedule.at = now;
            }
        }

        if !changed {
            return;
        }
        let written = self.board_written(board);

        for (card, start) in due {
            if written {
                if let Err(error) = give(&card, start) {
                    eprintln!("[pitwall] a card could not be given at its moment: {error}");
                }
            }
            self.board().giving.remove(&card.id);
        }
    }

    /// Starts `card`'s work as `give` says (`board_give`), a new session with `start`. `card`:
    /// as the board had it when the give was decided. `unasked`: given at its moment by the core
    /// (`board_due`), not by the user: its tab says so (`TerminalView::unasked`).
    fn launch_card(self: &Arc<Self>, card: &Card, give: Give, start: Start, size: Option<(u16, u16)>, unasked: bool) -> Result<Option<TerminalView>, String> {
        let id = card.id.as_str();

        let carry = match give {
            Give::New | Give::Plan => Carry::Fresh,
            Give::Continue => self.carry_of(card),
        };
        let launch = match (carry, &card.session) {
            (Carry::Follow, _) => {
                self.follow_card(id);
                return Ok(None);
            }
            (Carry::Resume, Some(session)) => Launch::Resume { session: session.clone() },
            _ => {
                let mut text = if card.note.is_empty() { card.title.clone() } else { format!("{}\n\n{}", card.title, card.note) };
                // What `[Image #n]` in the note stands for: Claude reads the file.
                let images = self.card_images(card);
                if let Some((_, files)) = &images {
                    text.push_str(&format!("\n\n{}", t!("board.imagesPrompt")));
                    for (n, file) in files {
                        text.push_str(&format!("\n[Image #{n}] {}", file.display()));
                    }
                }
                // A one-word card alone would be read as one of Claude's subcommands (`update`,
                // `doctor`, `mcp`…) even after `--`; with the line under it, it never is one word.
                let prompt = format!("{text}\n\n{}", t!("board.summaryPrompt"));
                Launch::Claude { prompt: Some(prompt), start, images: images.map(|(dir, _)| dir) }
            }
        };
        let resumed = match &launch {
            Launch::Resume { session } => Some(session.clone()),
            _ => None,
        };

        let new_tab = || self.open_terminal_as(&card.path, &launch, Some(tab_name(&card.title)), false, size, unasked);
        // `held`: its session goes on in a tab opened for it a moment ago (a click in the
        // terminals' history, before Claude Code lists its process): the card follows it there.
        let (view, held) = match &resumed {
            Some(session) => match self.open_once(session, new_tab) {
                Opened::New(view) => (view, false),
                Opened::Held(tab) => {
                    // Looked up first: the lock is let go before `follow_card` takes it again.
                    let found = self.lock().terminal_list.iter().find(|view| view.id == tab).cloned();
                    match found {
                        Some(view) => (view, true),
                        None => {
                            self.follow_card(id);
                            return Ok(None);
                        }
                    }
                }
                Opened::Opening => {
                    self.follow_card(id);
                    return Ok(None);
                }
                Opened::Failed(error) => return Err(error),
            },
            None => (new_tab()?, false),
        };

        let now = now_ms();
        let mut board = self.board();
        // Its shell may have ended already, before the card knew of it (`board_terminal_gone`).
        let open = self.terminal_sessions().contains_key(&view.id);
        let Some(card) = board.place(id, Column::Claude, usize::MAX, now) else {
            // Deleted while its tab was opening: no card, so no Claude left working on it.
            drop(board);
            if !held {
                self.close_terminal(view.id);
            }
            return Err(no_card(id));
        };
        card.terminal = open.then_some(view.id);
        card.given_at = Some(now);
        card.worked = false;
        card.schedule = None;
        if resumed.is_none() {
            card.session = None;
            card.ran = false;
            card.start = Some(start);
        }
        board.linked_busy.remove(id);
        if let (true, Some(session)) = (open, resumed) {
            board.resumed.insert(session, now + RESUME_GRACE_MS);
        }
        if !open {
            board.place(id, Column::Review, usize::MAX, now);
        }
        self.board_changed(board);
        if held {
            self.emit(CoreEvent::RevealTerminal { path: view.path.clone(), id: view.id });
        }
        Ok(Some(view))
    }

    /// Card `id` goes into Claude's column and follows its session where that runs, or is about
    /// to (`Carry::Follow`).
    fn follow_card(&self, id: &str) {
        let mut board = self.board();
        if let Some(card) = board.place(id, Column::Claude, usize::MAX, now_ms()) {
            card.worked = false;
            card.schedule = None;
        }
        self.board_changed(board);
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
        card.schedule = None;
        if busy {
            board.linked_busy.insert(id.to_string());
        } else {
            board.linked_busy.remove(id);
        }
        self.board_changed(board);
        Ok(())
    }

    /// A session opened again in a new tab, after a restart (`restore.rs`) or brought up once it
    /// had stopped (`reveal.rs`): its card follows it there, and isn't sent to review while the
    /// session comes back up.
    pub(super) fn board_resumed(&self, session: &str, terminal: u64) {
        let mut board = self.board();
        board.resumed.insert(session.to_string(), now_ms() + RESUME_GRACE_MS);

        let mut linked = false;
        for card in board.cards.iter_mut().filter(|card| card.session.as_deref() == Some(session)) {
            card.terminal = Some(terminal);
            linked = true;
        }
        if linked {
            self.board_changed(board);
        }
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
    /// work) or the session ended; one whose turn the user interrupted goes there without a
    /// word. A card in `review` goes back to `claude` when its session,
    /// seen at rest since the card got there, is at work again; a session several cards hold
    /// works for the one given last, and only that one follows it back. Runs after every refresh
    /// of Claude's state (`refresh_claude`: each heartbeat and each change to Claude's files),
    /// with or without the window, and costs nothing while no card in `claude` or `review` has a
    /// session to follow.
    pub(super) fn follow_board(&self) {
        // One pass at a time, so an older reading never lands after a newer one.
        let _pass = self.board_follow.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        // (terminal, given at) of the cards still waiting for their session, the sessions held,
        // and the process each was last seen running in. The first pass after launch only takes
        // note, as the first scan of Claude's turns does: a session that ended while the app was
        // closed sends its card to `review` without a word.
        let (unlinked, linked, processes, first) = {
            let mut board = self.board();
            let first = !std::mem::replace(&mut board.followed, true);
            let unlinked: Vec<(u64, u64)> = board
                .cards
                .iter()
                .filter(|card| card.column == Column::Claude && card.session.is_none())
                .filter_map(|card| Some((card.terminal?, card.given_at.unwrap_or(card.moved_at))))
                .collect();
            let linked: Vec<String> = board.cards.iter().filter(|card| matches!(card.column, Column::Claude | Column::Review)).filter_map(|card| card.session.clone()).collect();
            (unlinked, linked, board.pids.clone(), first)
        };
        if unlinked.is_empty() && linked.is_empty() {
            return;
        }

        // Read again when Claude's sessions folder changed, else at most every 30 s.
        let running = self.running_cached();

        // Each waiting terminal's session: that of the first Claude started in it, soon after
        // the give. Its process's start tells, not the session's own `startedAt`: Claude Code
        // lists a session only once its folder is trusted, however long that question stood.
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
                let born = process::started_at(run.pid).or(run.started_at);
                if born.is_some_and(|at| at > given + ADOPT_MS) {
                    continue;
                }
                let started = born.unwrap_or(u64::MAX);
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
        let Board { cards, resumed, pids, linked_busy, line, .. } = &mut *board;

        if let Some(running) = &running {
            pids.extend(ids.iter().filter_map(|id| Some((id.clone(), running.get(id)?.pid))));
            // Opened again and running: followed like any other from here.
            resumed.retain(|id, _| !running.contains_key(id));
        }
        resumed.retain(|_, until| *until > now);

        // A session several cards hold works for the one given last, wherever that card is.
        let mut latest: HashMap<String, (u64, String)> = HashMap::new();
        for card in cards.iter() {
            let Some(session) = card.session.as_ref().map(|session| renamed.get(session).unwrap_or(session)) else {
                continue;
            };
            let given = card.given_at.unwrap_or(0);
            if latest.get(session).is_none_or(|(at, _)| given >= *at) {
                latest.insert(session.clone(), (given, card.id.clone()));
            }
        }

        let mut over: Vec<String> = Vec::new();
        // Its turn interrupted by the user (Esc): to `review` as well, with nothing to tell.
        let mut hushed: Vec<String> = Vec::new();
        let mut again: Vec<String> = Vec::new();
        for card in cards.iter_mut().filter(|card| matches!(card.column, Column::Claude | Column::Review)) {
            if let Some(next) = card.session.as_ref().and_then(|session| renamed.get(session)) {
                card.session = Some(next.clone());
                changed = true;
            }
            if card.column == Column::Claude && card.session.is_none() {
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
            if card.column == Column::Review {
                match state {
                    // At work again after the rest that left the card here: its session went on.
                    Stand::Busy if card.rested && latest.get(session).is_some_and(|(_, id)| *id == card.id) => again.push(card.id.clone()),
                    Stand::Over(_) | Stand::Interrupted | Stand::Ended if !card.rested => {
                        card.rested = true;
                        changed = true;
                    }
                    _ => {}
                }
                continue;
            }
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

            // Given from the line and seen at work: it holds the line for as long as its turn runs.
            if let (Stand::Busy, Some(lined)) = (state, line.get_mut(&card.id)) {
                lined.worked = true;
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
                Stand::Interrupted if card.worked => hushed.push(card.id.clone()),
                Stand::Interrupted => {}
                // Opened again a moment ago: its old end says nothing until it is seen running.
                Stand::Ended if resumed.contains_key(session) => {}
                Stand::Ended => over.push(card.id.clone()),
            }
        }

        // Kept while a card holds the session or still follows it.
        pids.retain(|id, _| cards.iter().any(|card| card.session.as_ref() == Some(id)));
        linked_busy.retain(|id| cards.iter().any(|card| card.id == *id && card.column == Column::Claude));

        // Its turn was over, or its session had ended: at rest, as far as `review` goes.
        let mut reviewed: Vec<String> = Vec::new();
        for (id, told) in over.iter().map(|id| (id, true)).chain(hushed.iter().map(|id| (id, false))) {
            if let Some(card) = board.place(id, Column::Review, usize::MAX, now) {
                card.rested = true;
                if told {
                    reviewed.push(card.path.clone());
                }
            }
            changed = true;
        }
        // Seen at work already: the end of this turn sends it to `review` again.
        for id in &again {
            if let Some(card) = board.place(id, Column::Claude, usize::MAX, now) {
                card.worked = true;
            }
            changed = true;
        }

        if changed {
            self.board_changed(board);
        }
        // Told of once the board's lock is let go: a card is there to be looked at (`signals.rs`).
        for path in reviewed.into_iter().filter(|_| !first) {
            let name = self.project_name(&path);
            self.signal(super::signals::Signal { kind: SignalKind::CardReview, path, name, detail: None });
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
            rested: false,
            images: Vec::new(),
            start: None,
            schedule: None,
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

    /// A card's images are files of its own folder, and nothing else is: only bytes that are an
    /// image are written there, an image goes when its note no longer names it or its card is
    /// deleted, and a card whose id was written by hand reaches no folder outside the board's.
    #[test]
    fn a_cards_images_stay_in_its_own_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude").join("projects"),
                settings_file: config.join("settings.json"),
                claude_settings: tmp.path().join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        let url = |bytes: &[u8]| format!("data:image/png;base64,{}", BASE64.encode(bytes));
        let image = |n: u32, fill: u8| NewImage { n, data: url(&[&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[fill; 16][..]].concat()) };
        let script = |n: u32| NewImage { n, data: url(b"#!/bin/sh\nrm -rf ~\n") };
        let files = |dir: PathBuf| -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(dir).map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            names.sort();
            names
        };
        let folder = |id: &str| config.join("board-images").join(id);
        let project = "/Users/me/projects/paddock";

        // Only what the note names is taken, and only bytes that are an image.
        let first = core.board_add(project, "Card", "see [Image #1] and [Image #2]", &[image(1, 1), image(2, 2), image(3, 3)]).unwrap();
        assert_eq!(first.images, [CardImage { n: 1, kind: ImageKind::Png }, CardImage { n: 2, kind: ImageKind::Png }]);
        assert_eq!(files(folder(&first.id)), ["1.png", "2.png"]);
        assert_eq!(core.board_image(&first.id, 2).unwrap(), image(2, 2).data);
        assert!(core.board_add(project, "Card", "[Image #1]", &[script(1)]).is_err());
        assert!(core.board_edit(&first.id, "Card", "[Image #1] [Image #2] [Image #4]", &[script(4)]).is_err());
        assert_eq!(core.board_state(), std::slice::from_ref(&first));
        assert_eq!(files(config.join("board-images")), std::slice::from_ref(&first.id));

        // An edit keeps what the note still names; the file of what it no longer names goes.
        core.board_edit(&first.id, "Card", "only [Image #2] and [Image #3]", &[image(3, 3)]).unwrap();
        assert_eq!(files(folder(&first.id)), ["2.png", "3.png"]);
        assert_eq!(Board::load(config.join("board.json"), now_ms()).cards, core.board_state());

        // An id written by hand names no folder: nothing is read, written or deleted through it.
        let outside = config.join("kept");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("1.png"), b"mine").unwrap();
        let mut forged = card("../kept", Column::Queued);
        forged.note = "[Image #1]".into();
        forged.images = vec![CardImage { n: 1, kind: ImageKind::Png }];
        core.board().cards.push(forged);
        assert!(core.board_image("../kept", 1).is_err());
        assert!(core.board_edit("../kept", "Card", "[Image #1] [Image #2]", &[image(2, 2)]).is_err());
        core.board_edit("../kept", "Card", "", &[]).unwrap();
        core.board_delete("../kept");
        assert_eq!(files(outside.clone()), ["1.png"]);
        assert_eq!(fs::read(outside.join("1.png")).unwrap(), b"mine");

        // A deleted card takes its own images along, and only those.
        let second = core.board_add(project, "Other", "[Image #1]", &[image(1, 9)]).unwrap();
        core.board_delete(&first.id);
        assert!(!folder(&first.id).exists());
        assert_eq!(files(folder(&second.id)), ["1.png"]);
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

    /// A core whose files are under `root`: made again over the same folder, it is the app
    /// started again.
    fn core_at(root: &Path) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: root.join("registry"),
                claude_dir: root.join("claude").join("projects"),
                settings_file: root.join("config").join("settings.json"),
                claude_settings: root.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        )
    }

    /// A folder under `root` with a dev script, listed in `core`.
    fn listed_project(core: &Core, root: &Path) -> String {
        let project = root.join("paddock");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("package.json"), r#"{"name":"paddock","scripts":{"dev":"vite"}}"#).unwrap();
        let path = project.to_string_lossy().into_owned();
        core.add_project(&path).unwrap();
        path
    }

    const CHOSEN: Start = Start { model: Some(crate::start::Model::Sonnet), mode: Mode::Plan };

    /// The board's file says card `id` waits for a moment.
    fn waits_on_disk(file: &Path, id: &str) -> bool {
        let raw: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        raw["cards"].as_array().unwrap().iter().any(|card| card["id"] == id && !card["schedule"].is_null())
    }

    /// One look at the cards at `now` (`board_due_at`): the cards given, each with what it was
    /// given with. Giving starts nothing here, and a card must be off the file by the time it
    /// is given.
    fn look(core: &Core, now: u64) -> Vec<(String, Start)> {
        let file = core.cfg.settings_file.with_file_name("board.json");
        let mut given = Vec::new();
        core.board_due_at(now, &mut |card, start| {
            assert!(!waits_on_disk(&file, &card.id), "its schedule is off the file before it is given");
            given.push((card.id.clone(), start));
            Ok(())
        });
        given
    }

    /// A card in `queued` that waits until `at`.
    fn waiting(core: &Core, path: &str, title: &str, at: u64) -> String {
        let id = core.board_add(path, title, "", &[]).unwrap().id;
        core.board_schedule(&id, at, false, CHOSEN).unwrap();
        id
    }

    fn schedule_of(core: &Core, id: &str) -> Option<Schedule> {
        core.board_state().into_iter().find(|card| card.id == id).unwrap().schedule
    }

    /// A card told to wait is given by the core, which starts Claude on the user's account: it
    /// must happen once. Not before its moment, not again at a second look or after a restart,
    /// not at all once the moment is long past (the app wasn't running, the Mac slept), and
    /// never while the board's file can't take note of it: the file would still say it waits.
    #[test]
    fn a_card_told_to_wait_is_given_once() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_project(&core, tmp.path());
        let file = tmp.path().join("config").join("board.json");
        let at = now_ms() + 3_600_000;

        // Not before its moment; at it, once, with what was chosen; a second look gives nothing.
        let first = waiting(&core, &project, "First", at);
        assert!(waits_on_disk(&file, &first));
        assert_eq!(look(&core, at - 1), []);
        assert_eq!(schedule_of(&core, &first), Some(Schedule { at, refill: false, start: CHOSEN, line: false }));
        assert_eq!(look(&core, at), [(first.clone(), CHOSEN)]);
        assert_eq!(look(&core, at), []);
        assert_eq!(look(&core, at + 1_000), []);
        assert_eq!(schedule_of(&core, &first), None);

        // Nor does a restart give it again.
        let again = core_at(tmp.path());
        assert_eq!(again.board_state().len(), 1);
        assert_eq!(look(&again, at + 2_000), []);

        // The app starts after the moment: a little late it is still given, once, and a second
        // start gives nothing; long past, it is not given, and waits no longer.
        let late = waiting(&again, &project, "A little late", at);
        let missed = waiting(&again, &project, "Long past", at - LATE_MS - 1);
        let restarted = core_at(tmp.path());
        assert_eq!(look(&restarted, at + LATE_MS), [(late.clone(), CHOSEN)]);
        assert_eq!(schedule_of(&restarted, &missed), None);
        assert!(!waits_on_disk(&file, &missed));
        assert_eq!(restarted.board_state().iter().map(|card| card.column).collect::<Vec<_>>(), [Column::Queued; 3]);
        assert_eq!(look(&core_at(tmp.path()), at + LATE_MS), []);

        // A give that fails is not tried again.
        let failing = waiting(&restarted, &project, "Fails", at);
        let mut tries = 0;
        restarted.board_due_at(at, &mut |_, _| {
            tries += 1;
            Err("no shell".into())
        });
        assert_eq!((tries, schedule_of(&restarted, &failing)), (1, None));
        assert_eq!(look(&restarted, at + 1_000), []);
        assert!(restarted.board().giving.is_empty());

        // The board can't be written: nothing is given, in this run or the next.
        let unwritten = waiting(&restarted, &project, "Unwritten", at);
        fs::remove_file(&file).unwrap();
        fs::create_dir_all(file.join("in the way")).unwrap();
        assert_eq!(look(&restarted, at), []);
        assert_eq!(schedule_of(&restarted, &unwritten), None);
    }

    /// Pitwall quits while the loop that gives the cards still runs: a moment that comes then
    /// is not given. Its Claude would start as everything is being stopped, and the card would
    /// be off the file and in `claude`, its wait gone with nothing done.
    #[test]
    fn nothing_is_given_once_pitwall_quits() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_project(&core, tmp.path());
        let at = now_ms() + 3_600_000;
        let id = waiting(&core, &project, "At its hour", at);

        core.dispose();
        assert_eq!(look(&core, at), []);
        assert!(waits_on_disk(&tmp.path().join("config").join("board.json"), &id), "it waits for the next start");
    }

    /// What takes a card's schedule away before its moment: it is edited, moved to another
    /// column, moved to another project, called off, deleted, or its project is no longer
    /// listed. None of those is given when the moment comes; a card only moved up or down
    /// among the cards up next still is.
    #[test]
    fn a_card_that_no_longer_waits_is_not_given() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_project(&core, tmp.path());
        let at = now_ms() + 3_600_000;

        let edited = waiting(&core, &project, "Edited", at);
        let moved = waiting(&core, &project, "Moved", at);
        let called_off = waiting(&core, &project, "Called off", at);
        let deleted = waiting(&core, &project, "Deleted", at);
        let reordered = waiting(&core, &project, "Reordered", at);

        core.board_edit(&edited, "Edited since", "", &[]).unwrap();
        core.board_move(&moved, "done", 0).unwrap();
        core.board_move(&moved, "queued", 0).unwrap();
        core.board_unschedule(&called_off);
        core.board_delete(&deleted);
        core.board_move(&reordered, "queued", 0).unwrap();
        for id in [&edited, &moved, &called_off] {
            assert_eq!(schedule_of(&core, id), None);
        }
        assert_eq!(look(&core, at), [(reordered.clone(), CHOSEN)]);

        // Only a card up next of a listed project is told to wait, and only for a moment ahead.
        let given = core.board_add(&project, "With Claude", "", &[]).unwrap().id;
        core.board_move(&given, "claude", 0).unwrap();
        assert!(core.board_schedule(&given, at, false, CHOSEN).is_err());
        let stray = core.board_add("/Users/me/projects/elsewhere", "Stray", "", &[]).unwrap().id;
        assert!(core.board_schedule(&stray, at, false, CHOSEN).is_err());
        let past = core.board_add(&project, "Past", "", &[]).unwrap().id;
        assert!(core.board_schedule(&past, now_ms() - 1, false, CHOSEN).is_err());

        // Its cards went to another project, or its project left the list: not given.
        let rehomed = core.board_add("/Users/me/projects/elsewhere", "Rehomed", "", &[]).unwrap().id;
        core.board().find_mut(&rehomed).unwrap().schedule = Some(Schedule { at, refill: false, start: CHOSEN, line: false });
        core.board_rehome("/Users/me/projects/elsewhere", &project).unwrap();
        assert_eq!(schedule_of(&core, &rehomed), None);
        let unlisted = waiting(&core, &project, "Unlisted", at);
        core.remove_project(&project);
        assert_eq!(look(&core, at), []);
        assert_eq!(schedule_of(&core, &unlisted), None);
    }

    /// A listed project whose folder isn't there. A card of it can be told to wait, and a give
    /// by hand stops where its tab would open (`open_terminal_as` refuses a folder that isn't
    /// one): nothing is ever started by a test that gives by hand.
    fn listed_without_a_folder(core: &Core, root: &Path) -> String {
        let path = root.join("not-there").to_string_lossy().into_owned();
        core.set_favourite(&path, true);
        path
    }

    /// How far a give by hand got, for a card of `listed_without_a_folder`. `terminal`: it went
    /// through to where its tab would open (the error names the folder); `nothing`: the card was
    /// left as it is; `refused`: it was held back before any tab was asked for.
    fn got(given: Result<Option<TerminalView>, String>, project: &str) -> &'static str {
        match given {
            Err(error) if error.contains(project) => "terminal",
            Err(_) => "refused",
            Ok(None) => "nothing",
            Ok(Some(_)) => "started",
        }
    }

    /// A waiting card given by hand ("Give now", or a plain give) starts Claude as the core does
    /// at the card's moment, so the same holds: whoever takes the schedule off the card gives
    /// it, once. Not a second press, not the core at the moment that follows, and nobody while
    /// the card is on its way already (a tab is opening for it).
    #[test]
    fn a_waiting_card_given_by_hand_is_given_once() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_without_a_folder(&core, tmp.path());
        let file = tmp.path().join("config").join("board.json");
        let at = now_ms() + 3_600_000;
        let now = waiting(&core, &project, "Give now", at);
        let plain = waiting(&core, &project, "A plain give", at);
        let on_its_way = waiting(&core, &project, "On its way", at);

        // "Give now": the schedule is off the card and off the file, and the card is given.
        assert_eq!(got(core.board_give_now(&now, None), &project), "terminal");
        assert_eq!(schedule_of(&core, &now), None);
        assert!(!waits_on_disk(&file, &now));
        // It waits no longer: a second press gives nothing.
        assert_eq!(got(core.board_give_now(&now, None), &project), "nothing");

        // A plain give of a waiting card takes its schedule the same way, and leaves "Give now"
        // nothing to give.
        assert_eq!(got(core.board_give_with(&plain, Give::New, None, None), &project), "terminal");
        assert_eq!(schedule_of(&core, &plain), None);
        assert!(!waits_on_disk(&file, &plain));
        assert_eq!(got(core.board_give_now(&plain, None), &project), "nothing");
        assert!(core.board().giving.is_empty());

        // On its way to Claude already: no hand gives it again, and nor does the core when its
        // moment comes meanwhile.
        core.board().giving.insert(on_its_way.clone());
        assert_eq!(got(core.board_give_now(&on_its_way, None), &project), "nothing");
        assert_eq!(got(core.board_give_with(&on_its_way, Give::New, None, None), &project), "nothing");
        assert_eq!(look(&core, at), []);
    }

    /// The board's file can't be written: a card the file says waits is given by nobody, by hand
    /// as at its moment (`board_due_at`). Once given here, the file would still say it waits, and
    /// the next run would give it a second time. Refused, it waits on; and what takes its
    /// schedule off in this run (a wait called off, the moment coming) takes it off in memory
    /// only, so it still isn't given. A card that doesn't wait is given as ever: the file names
    /// no moment for it.
    #[test]
    fn a_waiting_card_is_not_given_by_hand_while_the_board_cant_be_written() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_without_a_folder(&core, tmp.path());
        let file = tmp.path().join("config").join("board.json");
        let at = now_ms() + 3_600_000;
        let now = waiting(&core, &project, "Give now", at);
        let plain = waiting(&core, &project, "A plain give", at);
        let called_off = waiting(&core, &project, "Called off", at);
        let came = waiting(&core, &project, "Its moment came", at);
        let ordinary = core.board_add(&project, "Doesn't wait", "", &[]).unwrap().id;
        let written = fs::read(&file).unwrap();
        // The disk refuses the board's file, and takes it again as it was last written.
        let refuse = || {
            fs::remove_file(&file).unwrap();
            fs::create_dir_all(file.join("in the way")).unwrap();
        };
        let take_again = || {
            fs::remove_dir_all(&file).unwrap();
            fs::write(&file, &written).unwrap();
        };
        refuse();

        // Refused, and nothing is changed by it: asked again, refused the same.
        for _ in 0..2 {
            assert_eq!(got(core.board_give_with(&plain, Give::New, None, None), &project), "refused");
            assert_eq!(got(core.board_give_now(&now, None), &project), "refused");
        }
        assert_eq!((schedule_of(&core, &now).map(|schedule| schedule.at), schedule_of(&core, &plain).map(|schedule| schedule.at)), (Some(at), Some(at)));
        assert!(core.board().giving.is_empty());

        // A wait called off, a moment that came (the core's look gave nothing): off the card,
        // not off the file.
        core.board_unschedule(&called_off);
        assert_eq!(got(core.board_give_with(&called_off, Give::New, None, None), &project), "refused");
        assert_eq!(look(&core, at), []);
        assert_eq!(schedule_of(&core, &came), None);
        assert_eq!(got(core.board_give_with(&came, Give::New, None, None), &project), "refused");
        assert_eq!(got(core.board_give_with(&ordinary, Give::New, None, None), &project), "terminal");

        // The file as it was last written still says the four wait. Started again on it, Pitwall
        // knows so before it has written anything: the same holds from the start.
        take_again();
        let next = core_at(tmp.path());
        refuse();
        next.board_unschedule(&called_off);
        assert_eq!(got(next.board_give_with(&called_off, Give::New, None, None), &project), "refused");

        // The disk is back. At their moment Pitwall gives the three that still wait there, and
        // that is the first time; and by hand it gives as ever, in either run.
        take_again();
        let mut given: Vec<String> = look(&next, at).into_iter().map(|(id, _)| id).collect();
        given.sort();
        let mut still = vec![now, plain, came];
        still.sort();
        assert_eq!(given, still);
        assert_eq!(got(next.board_give_with(&called_off, Give::New, None, None), &project), "terminal");
        assert_eq!(got(core.board_give_with(&called_off, Give::New, None, None), &project), "terminal");
    }

    /// A card that waits for the session's refill is given when the reading says the session
    /// refills: it follows a reading that names another moment of that session, never one that
    /// names the next session's (five hours on) or a moment already past, and never once its
    /// own moment has passed.
    #[test]
    fn a_card_that_waits_for_the_refill_follows_the_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_project(&core, tmp.path());
        let at = now_ms() + 3_600_000;
        let minutes = |n: u64| n * 60_000;

        let refill = core.board_add(&project, "At the refill", "", &[]).unwrap().id;
        core.board_schedule(&refill, at, true, CHOSEN).unwrap();
        let fixed = waiting(&core, &project, "At its hour", at);

        core.board_refill(at + minutes(7));
        assert_eq!(schedule_of(&core, &refill).map(|schedule| schedule.at), Some(at + minutes(7)));
        assert_eq!(schedule_of(&core, &fixed).map(|schedule| schedule.at), Some(at));
        assert_eq!(look(&core, at), [(fixed.clone(), CHOSEN)]);

        // Earlier again; the next session's refill is not followed.
        core.board_refill(at - minutes(2));
        core.board_refill(at - minutes(2) + 5 * minutes(60));
        assert_eq!(schedule_of(&core, &refill).map(|schedule| schedule.at), Some(at - minutes(2)));
        assert_eq!(look(&core, at - minutes(2) - 1), []);
        assert_eq!(look(&core, at - minutes(2)), [(refill.clone(), CHOSEN)]);

        // A moment that has passed stays where it was: it is given or dropped by it.
        let passed = core.board_add(&project, "Passed", "", &[]).unwrap().id;
        let before = now_ms() - minutes(1);
        core.board().find_mut(&passed).unwrap().schedule = Some(Schedule { at: before, refill: true, start: CHOSEN, line: false });
        core.board_refill(now_ms() + minutes(30));
        assert_eq!(schedule_of(&core, &passed).map(|schedule| schedule.at), Some(before));

        // A reading that names a moment already past is no refill to wait for: followed, the card
        // would be given at once, or dropped as long past. It keeps its own moment.
        let ahead = core.board_add(&project, "Ahead", "", &[]).unwrap().id;
        core.board_schedule(&ahead, at, true, CHOSEN).unwrap();
        core.board_refill(now_ms() - minutes(10));
        core.board_refill(now_ms() - minutes(120));
        assert_eq!(schedule_of(&core, &ahead).map(|schedule| schedule.at), Some(at));
        assert!(look(&core, now_ms()).iter().all(|(id, _)| *id != ahead));
        assert_eq!(schedule_of(&core, &ahead).map(|schedule| schedule.at), Some(at));
    }

    /// One look at the cards at `now` (`board_due_at`), and Claude started on each card given as
    /// `launch_card` leaves it: with Claude, in a tab of its own, its session found (a new one).
    fn give_due(core: &Core, now: u64) -> Vec<String> {
        let mut given = Vec::new();
        core.board_due_at(now, &mut |card, _| {
            given.push(card.id.clone());
            Ok(())
        });
        for id in &given {
            let mut board = core.board();
            let n = board.cards.iter().filter(|card| card.session.is_some()).count();
            let session = format!("9bb85e86-f618-4861-9858-{n:012}");
            let card = board.place(id, Column::Claude, usize::MAX, now_ms()).unwrap();
            (card.terminal, card.given_at, card.session, card.worked, card.ran) = (Some(1), Some(now), Some(session), false, false);
        }
        given
    }

    /// Where card `id`'s session stands, as the hook tells it; then the board follows it.
    fn session_is(core: &Core, id: &str, phase: SessionPhase, turn: Option<(TurnKind, u64)>, ended: bool) {
        let session = core.board_state().into_iter().find(|card| card.id == id).unwrap().session.unwrap();
        {
            let mut inner = core.lock();
            inner.scanned.retain(|state| state.id != session);
            let turn = turn.map(|(kind, at)| crate::claude::Turn { kind, at });
            inner.scanned.push(SessionState { id: session, path: String::new(), folder: None, title: None, phase, turn, since: Some(now_ms()), hooked: true, ended, tool: None, subagents: 0, interrupted: false });
        }
        core.follow_board();
    }

    fn column_of(core: &Core, id: &str) -> Column {
        core.board_state().into_iter().find(|card| card.id == id).unwrap().column
    }

    /// Cards that wait for one refill share its fresh quota, and Claude started on them all at
    /// once would burn it in parallel, with nobody there. They are given one at a time, in the
    /// order the board shows them: at the refill only the first; the next only once Claude is no
    /// longer at work on the one before (its turn over, or its session ended), not while it
    /// works or waits on a permission. A restart in the middle goes on the same way: the Claude
    /// it was at work on ended with the run, and only the next card goes.
    #[test]
    fn cards_that_wait_for_one_refill_are_given_one_at_a_time() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_project(&core, tmp.path());
        let at = now_ms() + 3_600_000;
        let minutes = |n: u64| n * 60_000;

        let mut cards = Vec::new();
        for title in ["First", "Second", "Third", "Fourth"] {
            let id = core.board_add(&project, title, "", &[]).unwrap().id;
            core.board_schedule(&id, at, true, CHOSEN).unwrap();
            cards.push(id);
        }
        let [first, second, third, fourth] = <[String; 4]>::try_from(cards).unwrap();
        // Up next, the fourth stands second.
        core.board_move(&fourth, "queued", 1).unwrap();

        // At the refill only the first goes; a second look gives nothing more.
        assert_eq!(give_due(&core, at - 1), Vec::<String>::new());
        assert_eq!(give_due(&core, at), [first.as_str()]);
        assert_eq!(give_due(&core, at + 1_000), Vec::<String>::new());
        assert!([&second, &third, &fourth].iter().all(|id| column_of(&core, id) == Column::Queued));

        // Claude at work on it, then waiting on a permission: the line waits.
        session_is(&core, &first, SessionPhase::Working, None, false);
        assert_eq!(give_due(&core, at + minutes(10)), Vec::<String>::new());
        session_is(&core, &first, SessionPhase::Waiting, Some((TurnKind::Permission, at + minutes(20))), false);
        assert_eq!(give_due(&core, at + minutes(60)), Vec::<String>::new());

        // Its turn is over: the next on the board, alone.
        session_is(&core, &first, SessionPhase::Waiting, Some((TurnKind::Finished, at + minutes(61))), false);
        assert_eq!(column_of(&core, &first), Column::Review);
        assert_eq!(give_due(&core, at + minutes(62)), [fourth.as_str()]);
        assert_eq!(give_due(&core, at + minutes(63)), Vec::<String>::new());

        // Pitwall starts again while Claude works on it: that Claude ended with the run, and of
        // the two left only the next goes.
        session_is(&core, &fourth, SessionPhase::Working, None, false);
        let restarted = core_at(tmp.path());
        assert_eq!(give_due(&restarted, at + minutes(64)), [second.as_str()]);
        assert_eq!(give_due(&restarted, at + minutes(65)), Vec::<String>::new());

        // Its session ends before its turn does: the last one goes.
        session_is(&restarted, &second, SessionPhase::Working, None, false);
        assert_eq!(give_due(&restarted, at + minutes(66)), Vec::<String>::new());
        session_is(&restarted, &second, SessionPhase::Working, None, true);
        assert_eq!(column_of(&restarted, &second), Column::Review);
        assert_eq!(give_due(&restarted, at + minutes(67)), [third.as_str()]);
    }

    /// A card given from the line whose Claude never goes to work (it started and sits idle, or
    /// nothing tells) would hold the line, and the refill's quota with it, until the user moved
    /// the card: ten minutes after the give the next goes, and the card stays where it is. One
    /// seen at work, however late in those ten minutes, holds the line as long as its turn runs.
    #[test]
    fn a_card_never_seen_at_work_holds_the_line_ten_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_at(tmp.path());
        let project = listed_project(&core, tmp.path());
        let at = now_ms() + 3_600_000;
        let minutes = |n: u64| n * 60_000;

        let mut cards = Vec::new();
        for title in ["First", "Second", "Third"] {
            let id = core.board_add(&project, title, "", &[]).unwrap().id;
            core.board_schedule(&id, at, true, CHOSEN).unwrap();
            cards.push(id);
        }
        let [first, second, third] = <[String; 3]>::try_from(cards).unwrap();

        // Its Claude started and sits idle: the line waits ten minutes, then the next goes.
        assert_eq!(give_due(&core, at), [first.as_str()]);
        session_is(&core, &first, SessionPhase::Idle, None, false);
        assert_eq!(give_due(&core, at + minutes(10)), Vec::<String>::new());
        assert_eq!(give_due(&core, at + minutes(11)), [second.as_str()]);
        assert_eq!(column_of(&core, &first), Column::Claude);

        // Seen at work nine minutes in, then waiting on a permission: the line waits past ten.
        let given = at + minutes(11);
        assert_eq!(give_due(&core, given + minutes(9)), Vec::<String>::new());
        session_is(&core, &second, SessionPhase::Working, None, false);
        session_is(&core, &second, SessionPhase::Waiting, Some((TurnKind::Permission, given + minutes(10))), false);
        assert_eq!(give_due(&core, given + minutes(60)), Vec::<String>::new());

        // Its turn is over: the next.
        session_is(&core, &second, SessionPhase::Waiting, Some((TurnKind::Finished, given + minutes(61))), false);
        assert_eq!(give_due(&core, given + minutes(62)), [third.as_str()]);
    }

    /// The board's file is the only copy of the cards. One written before cards could wait
    /// loads as it is, untouched; one this version writes (a card that waits, a card's model and
    /// mode) loads back the same; and a card whose new parts don't fit (another version's, a
    /// hand's) keeps everything else, with nothing set aside.
    #[test]
    fn the_boards_file_loads_across_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config").join("board.json");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let now = now_ms();

        // As 1.10.0 wrote it.
        let before = format!(
            r#"{{
  "version": 1,
  "cards": [
    {{ "id": "mf1x-0", "path": "/Users/me/projects/paddock", "title": "Invoice header", "note": "two lines", "column": "queued", "createdAt": 1790000000000, "movedAt": 1790000000000, "session": null, "terminal": null, "givenAt": null, "worked": false, "ran": false, "rested": false, "images": [{{ "n": 1, "kind": "png" }}] }},
    {{ "id": "mf1x-1", "path": "/Users/me/projects/paddock", "title": "Coupon rounding", "note": "", "column": "review", "createdAt": 1790000000000, "movedAt": {now}, "session": "9bb85e86-f618-4861-9858-03ec8fc36c28", "terminal": null, "givenAt": 1790000100000, "worked": true, "ran": true, "rested": true, "images": [] }}
  ]
}}"#
        );
        fs::write(&file, &before).unwrap();
        let old = Board::load(file.clone(), now);
        assert!(old.writable);
        assert_eq!(old.cards.iter().map(|card| (card.id.as_str(), card.title.as_str(), card.column)).collect::<Vec<_>>(), [("mf1x-0", "Invoice header", Column::Queued), ("mf1x-1", "Coupon rounding", Column::Review)]);
        assert_eq!(old.cards[0].images, [CardImage { n: 1, kind: ImageKind::Png }]);
        assert_eq!((old.cards[1].session.as_deref(), old.cards[1].given_at, old.cards[1].worked), (Some("9bb85e86-f618-4861-9858-03ec8fc36c28"), Some(1_790_000_100_000), true));
        assert!(old.cards.iter().all(|card| card.start.is_none() && card.schedule.is_none()));
        assert_eq!(fs::read_to_string(&file).unwrap(), before, "loading it changes nothing");

        // As this version writes it: a card that waits, and one given with a model and a mode.
        let mut new = Board::load(file.clone(), now);
        new.cards[0].schedule = Some(Schedule { at: now + 3_600_000, refill: true, start: CHOSEN, line: false });
        new.cards[1].start = Some(Start { model: Some(crate::start::Model::Opus), mode: Mode::AcceptEdits });
        assert!(new.save());
        let again = Board::load(file.clone(), now);
        assert_eq!(again.cards, new.cards);
        assert_eq!(again.cards[0].schedule, Some(Schedule { at: now + 3_600_000, refill: true, start: CHOSEN, line: false }));

        // Parts that don't fit cost only themselves: the cards and the file stay.
        let mut odd: Value = serde_json::from_str(&before).unwrap();
        odd["cards"][0]["schedule"] = serde_json::json!({ "at": "at four", "start": { "model": "sonnet" } });
        odd["cards"][0]["start"] = serde_json::json!("opus");
        odd["cards"][1]["schedule"] = serde_json::json!({ "at": now + 3_600_000, "refill": 1 });
        odd["cards"][1]["start"] = serde_json::json!({ "model": "opus; touch /tmp/pwned", "mode": "bypassPermissions", "later": true });
        fs::write(&file, odd.to_string()).unwrap();
        let read = Board::load(file.clone(), now);
        assert!(read.writable);
        assert_eq!(read.cards.iter().map(|card| card.title.as_str()).collect::<Vec<_>>(), ["Invoice header", "Coupon rounding"]);
        assert_eq!((read.cards[0].schedule, read.cards[0].start), (None, None));
        assert_eq!((read.cards[1].schedule, read.cards[1].start), (None, Some(Start::default())));
        let aside = fs::read_dir(file.parent().unwrap()).unwrap().flatten().filter(|entry| entry.file_name().to_string_lossy().contains(".broken-")).count();
        assert_eq!(aside, 0);

        // A card that isn't up next waits for nothing, whatever the file says.
        let mut stray: Value = serde_json::from_str(&before).unwrap();
        stray["cards"][1]["schedule"] = serde_json::json!({ "at": now + 3_600_000 });
        fs::write(&file, stray.to_string()).unwrap();
        assert_eq!(Board::load(file.clone(), now).cards[1].schedule, None);
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
