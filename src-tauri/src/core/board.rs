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

use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
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
/// A card given in a terminal takes the session of a Claude started there within this long of
/// the give; a `claude` started in that tab later is somebody else's work.
const ADOPT_MS: u64 = 2 * 60 * 1000;
/// Each card's images are in a folder of its own under this one, beside the board's file.
const IMAGES_DIR: &str = "board-images";
/// A card holds this many images at most, each this large at most.
const IMAGES_MAX: usize = 12;
const IMAGE_BYTES_MAX: usize = 10 * 1024 * 1024;

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
fn taken(note: &str, images: &[NewImage]) -> Result<Vec<(CardImage, Vec<u8>)>, String> {
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
}

pub(super) struct Board {
    file: PathBuf,
    /// The folder of the cards' image folders.
    images: PathBuf,
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
        let (cards, writable, changed, dropped) = read_cards(&file, now);
        let images = file.with_file_name(IMAGES_DIR);
        let board = Self { file, images, cards, writable, resumed: HashMap::new(), pids: HashMap::new(), linked_busy: HashSet::new() };
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
    fn image_dir(&self, id: &str) -> Option<PathBuf> {
        let own = !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
        own.then(|| self.images.join(id))
    }

    /// Writes a card's new images into its folder.
    fn write_images(&self, id: &str, images: &[(CardImage, Vec<u8>)]) -> Result<(), String> {
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
    fn remove_images(&self, id: &str, images: &[CardImage]) {
        let Some(dir) = self.image_dir(id).filter(|_| !images.is_empty()) else {
            return;
        };
        for image in images {
            let _ = fs::remove_file(dir.join(image.file_name()));
        }
        let _ = fs::remove_dir(&dir);
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
    /// last one: at the end), as the board shows them. A new column sets `moved_at`, and the
    /// card is no longer `rested`: that is told anew where it got to.
    fn place(&mut self, id: &str, column: Column, index: usize, now: u64) -> Option<&mut Card> {
        let at = self.cards.iter().position(|card| card.id == id)?;
        let mut card = self.cards.remove(at);
        if card.column != column {
            card.column = column;
            card.moved_at = now;
            card.rested = false;
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
        };
        board.cards.push(card.clone());
        board.place(&card.id, Column::Queued, usize::MAX, now);
        self.board_changed(board);
        Ok(card)
    }

    /// A card's new text. `images`: those pasted into its note since it was saved. An image
    /// stays while the note names it (`[Image #n]`), and its file goes once the note no longer
    /// does; a new one under a held number takes that one's place.
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
    fn card_images(&self, card: &Card) -> Option<(PathBuf, Vec<(u32, PathBuf)>)> {
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
                let mut text = if card.note.is_empty() { card.title.clone() } else { format!("{}\n\n{}", card.title, card.note) };
                // What `[Image #n]` in the note stands for: Claude reads the file.
                let images = self.card_images(&card);
                if let Some((_, files)) = &images {
                    text.push_str(&format!("\n\n{}", t!("board.imagesPrompt")));
                    for (n, file) in files {
                        text.push_str(&format!("\n[Image #{n}] {}", file.display()));
                    }
                }
                // A one-word card alone would be read as one of Claude's subcommands (`update`,
                // `doctor`, `mcp`…) even after `--`; with the line under it, it never is one word.
                let prompt = format!("{text}\n\n{}", t!("board.summaryPrompt"));
                Launch::Claude { prompt: Some(prompt), plan: give == Give::Plan, images: images.map(|(dir, _)| dir) }
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

    /// A session opened again in a new tab after a restart (`restore.rs`): its card follows it
    /// there, and isn't sent to review while the session comes back up.
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
    /// work) or the session ended. A card in `review` goes back to `claude` when its session,
    /// seen at rest since the card got there, is at work again; a session several cards hold
    /// works for the one given last, and only that one follows it back. Runs after every refresh
    /// of Claude's state (`refresh_claude`: each heartbeat and each change to Claude's files),
    /// with or without the window, and costs nothing while no card in `claude` or `review` has a
    /// session to follow.
    pub(super) fn follow_board(&self) {
        // One pass at a time, so an older reading never lands after a newer one.
        let _pass = self.board_follow.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        // (terminal, given at) of the cards still waiting for their session, the sessions held,
        // and the process each was last seen running in.
        let (unlinked, linked, processes) = {
            let board = self.board();
            let unlinked: Vec<(u64, u64)> = board
                .cards
                .iter()
                .filter(|card| card.column == Column::Claude && card.session.is_none())
                .filter_map(|card| Some((card.terminal?, card.given_at.unwrap_or(card.moved_at))))
                .collect();
            let linked: Vec<String> = board.cards.iter().filter(|card| matches!(card.column, Column::Claude | Column::Review)).filter_map(|card| card.session.clone()).collect();
            (unlinked, linked, board.pids.clone())
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
        let Board { cards, resumed, pids, linked_busy, .. } = &mut *board;

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
                    Stand::Over(_) | Stand::Ended if !card.rested => {
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

        // Its turn was over, or its session had ended: at rest, as far as `review` goes.
        for id in &over {
            if let Some(card) = board.place(id, Column::Review, usize::MAX, now) {
                card.rested = true;
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
