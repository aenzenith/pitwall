//! The board over the radio, beyond adding a card and noting on the session's own: what is on
//! it, and changing, moving, deleting and taking cards, each on the caller's project alone.
//!
//! A card the radio added carries its agent (`Card::by`): the radio may change it whole and delete
//! it, until the user changes it in the window. A card the user wrote, or changed, stays theirs:
//! the radio adds a signed note at its end and moves it
//! between "Up next" and "Done", nothing more. No tool changes a card with an agent but for the
//! session that holds it, moves one with an agent or in review, or touches one that waits for its
//! moment (an edit or a move would call that wait off) or is on its way to an agent. A card of
//! another project is answered as one that isn't there.

use serde_json::Value;

use super::super::board::{stand, Card, Column, Stand};
use super::tools::{refused, signed, text, Outcome, CARD_NOTE_MAX, NOTE_MAX, TITLE_MAX};
use super::*;

const NO_CARD_ID: &str = "`card` is the card's id, as board_list gives it.";
const NO_SUCH_CARD: &str = "There is no such card on this project's board. board_list gives its cards with their ids.";
const NO_SESSION: &str = "Pitwall can't tell which session this is, so it can't give it a card.";
const LAP_SESSION: &str = "This session is one of Pitwall's laps: it takes no card from the board.";
/// Of "Done", a list names the newest this many, and how many more there are.
const DONE_LISTED: usize = 20;
/// A title in a list is cut to this many characters, a note read whole to this many, and a list to
/// this many bytes: the board is the user's to fill, and an answer has an end (`mcp::LINE_MAX`).
const LIST_TITLE: usize = 200;
const NOTE_READ: usize = 20_000;
const LIST_BYTES: usize = 40_000;

/// `column`'s name as the board shows it (in English, for the agent); a card's agent names its
/// column.
fn column_name(column: Column, agent: Agent) -> &'static str {
    match (column, agent) {
        (Column::Queued, _) => "Up next",
        (Column::Claude, Agent::Claude) => "With Claude",
        (Column::Claude, Agent::Codex) => "With Codex",
        (Column::Review, _) => "To review",
        (Column::Done, _) => "Done",
    }
}

/// A time as the card's details give it: the local day and the clock.
fn when(ms: u64) -> String {
    format!("{} {}", crate::clock::day_name(ms), crate::clock::clock(ms as i64))
}

/// `text` cut to `max` characters, saying how many more there were.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{} [cut here: {} more characters]", &text[..end], text[end..].chars().count()),
        None => text.to_string(),
    }
}

/// A title of one line, its spaces as one.
fn one_line(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Card `id` of the project at `path`: none for an id another project's card has.
fn own<'a>(cards: &'a [Card], path: &str, id: &str) -> Option<&'a Card> {
    cards.iter().find(|card| card.id == id && card.path == path)
}

/// Who added a card.
fn added(card: &Card) -> String {
    card.by.map_or_else(|| "the user".to_string(), |by| format!("{} over the radio", by.name()))
}

/// What a card is besides, in short: whom it was given to, whose session it is, what it waits
/// for.
fn flags(card: &Card, session: Option<&str>) -> Vec<String> {
    let mut flags = Vec::new();
    if card.column == Column::Claude {
        flags.push(format!("given to {}", card.agent().name()));
    }
    if session.is_some_and(|session| card.session.as_deref() == Some(session)) {
        flags.push("yours".to_string());
    }
    match card.schedule {
        Some(schedule) if schedule.line => flags.push("waits its turn after a refill".to_string()),
        Some(schedule) if schedule.refill => flags.push(format!("waits for {}'s refill, {}", schedule.start.agent.name(), when(schedule.at))),
        Some(schedule) => flags.push(format!("waits for its moment, {}", when(schedule.at))),
        None => {}
    }
    flags
}

/// Why a card stays as it is whoever asks: it waits for its moment (an edit or a move takes that
/// off it, so the user's wait would be gone), or it is on its way to an agent.
fn kept(card: &Card, giving: bool) -> Option<String> {
    let title = &card.title;
    if card.schedule.is_some_and(|schedule| schedule.line) {
        return Some(format!("\"{title}\" waits its turn to be given after a refill: leave it as it is, or ask the user."));
    }
    if card.schedule.is_some() {
        return Some(format!("\"{title}\" waits for its moment to be given: leave it as it is, or ask the user."));
    }
    if giving {
        return Some(format!("\"{title}\" is being given to an agent right now: leave it as it is."));
    }
    None
}

/// Of the cards in `column` of the project at `path`, where card `id` is (from 1) and how many.
fn place_of(cards: &[Card], path: &str, column: Column, id: &str) -> (usize, usize) {
    let ids: Vec<&str> = cards.iter().filter(|card| card.path == path && card.column == column).map(|card| card.id.as_str()).collect();
    (ids.iter().position(|known| *known == id).map_or(ids.len(), |at| at + 1), ids.len())
}

impl Core {
    /// The project's cards by column in the board's order, each with its id and its flags, or one
    /// card whole: its note as it is stored (its images as their `[Image #n]` marks only), who
    /// added it, when it was made and last moved.
    pub(super) fn radio_board_list(&self, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let cards: Vec<Card> = self.board().cards.iter().filter(|card| card.path == project.path).cloned().collect();

        if let Some(id) = text(args, "card") {
            let Some(card) = own(&cards, &project.path, id) else {
                return refused(NO_SUCH_CARD);
            };
            let mut lines = vec![
                format!("card {}: {}", card.id, cut(&card.title, LIST_TITLE)),
                format!("column: {}", column_name(card.column, card.agent())),
                format!("added by: {}", added(card)),
                format!("made: {}", when(card.created_at)),
                format!("last moved: {}", when(card.moved_at)),
            ];
            let flags = flags(card, session);
            if !flags.is_empty() {
                lines.push(format!("also: {}", flags.join(", ")));
            }
            lines.push(if card.note.is_empty() { "note: (none)".to_string() } else { format!("note:\n{}", cut(&card.note, NOTE_READ)) });
            let result = CallResult::Listed { count: 1, title: Some(card.title.clone()) };
            return (Reply::ok(lines.join("\n")), result);
        }

        let mut lines: Vec<String> = Vec::new();
        let mut listed = 0;
        let mut left = 0;
        let mut bytes = 0;
        for column in [Column::Queued, Column::Claude, Column::Review, Column::Done] {
            let mut shown: Vec<&Card> = cards.iter().filter(|card| card.column == column).collect();
            let all = shown.len();
            let head = match column {
                Column::Claude => "With an agent",
                _ => column_name(column, Agent::Claude),
            };
            let count = if all == 1 { "1 card".to_string() } else { format!("{all} cards") };
            let head = if column == Column::Done && all > DONE_LISTED {
                shown.sort_by_key(|card| std::cmp::Reverse(card.moved_at));
                shown.truncate(DONE_LISTED);
                format!("{head} · {count}, the newest {DONE_LISTED} first ({} more not listed)", all - DONE_LISTED)
            } else if all == 0 {
                format!("{head} · none")
            } else {
                format!("{head} · {count}")
            };
            bytes += head.len() + 1;
            lines.push(head);

            for card in shown {
                let mut line = format!("- {} · {}", card.id, cut(&one_line(&card.title), LIST_TITLE));
                let by = card.by.map(|by| format!("added by {} over the radio", by.name()));
                let flags: Vec<String> = by.into_iter().chain(flags(card, session)).collect();
                if !flags.is_empty() {
                    line.push_str(&format!(" · {}", flags.join(", ")));
                }
                bytes += line.len() + 1;
                if bytes > LIST_BYTES {
                    left += 1;
                    continue;
                }
                lines.push(line);
                listed += 1;
            }
        }
        if left > 0 {
            lines.push(format!("[left out: {left} more cards; an answer is {} KB at most]", LIST_BYTES / 1000));
        }

        (Reply::ok(lines.join("\n")), CallResult::Listed { count: listed, title: None })
    }

    /// A card's title or note, under the board's lock from the look to the write. A card the radio
    /// added: its title replaced, its note replaced or added to. A card the user wrote: its note
    /// added to, signed as `board_note` signs, nothing else. Never a card with an agent that isn't
    /// the caller's session, nor one that waits or is on its way; the card stays where it is.
    pub(super) fn radio_board_edit(&self, agent: Agent, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let Some(id) = text(args, "card") else {
            return refused(NO_CARD_ID);
        };
        let title = text(args, "title").map(one_line);
        let note = text(args, "note");
        let replace = match text(args, "mode") {
            None | Some("append") => false,
            Some("replace") => true,
            Some(_) => return refused("`mode` is append (when left out) or replace."),
        };
        if title.is_none() && note.is_none() {
            return refused("Give the card a new `title`, a `note` to write on it, or both.");
        }
        if title.as_ref().is_some_and(|title| title.chars().count() > TITLE_MAX) {
            return refused(format!("A card's `title` is one short line ({TITLE_MAX} characters at most)."));
        }
        if note.is_some_and(|note| note.chars().count() > NOTE_MAX) {
            return refused(format!("A `note` is {NOTE_MAX} characters at most at once."));
        }

        let now = now_ms();
        let mut board = self.board();
        let Some(card) = own(&board.cards, &project.path, id).cloned() else {
            return refused(NO_SUCH_CARD);
        };
        if card.column == Column::Claude && card.session.as_deref().is_none_or(|held| Some(held) != session) {
            return refused(format!("\"{}\" is with {} in another session: only that session writes on it, with board_note.", card.title, card.agent().name()));
        }
        if let Some(why) = kept(&card, board.is_giving(id)) {
            return refused(why);
        }
        if card.by.is_none() && title.is_some() {
            return refused(format!("\"{}\" is the user's card, one they wrote or changed in Pitwall: only they change its title. You can add a note to it.", card.title));
        }
        if card.by.is_none() && replace {
            return refused(format!("\"{}\" is the user's card, one they wrote or changed in Pitwall: its note is only added to, never replaced. Leave `mode` out to add yours under it.", card.title));
        }

        let noted = match note {
            Some(note) if replace => note.to_string(),
            Some(note) => signed(agent, &card.note, note, now),
            None => card.note.clone(),
        };
        // Counted as the dialog's field counts.
        let over = noted.encode_utf16().count().saturating_sub(CARD_NOTE_MAX);
        if over > 0 {
            return refused(format!("That card's note holds {CARD_NOTE_MAX} characters at most, and this would be {over} too many. Write a shorter note."));
        }
        let renamed = title.is_some();
        let title = title.unwrap_or(card.title);
        board.retext(id, &title, &noted);
        self.board_changed(board);

        let mut done = Vec::new();
        if renamed {
            done.push("its title replaced");
        }
        match note {
            Some(_) if replace => done.push("its note replaced"),
            Some(_) => done.push("your note added under its own"),
            None => {}
        }
        (Reply::ok(format!("Edited the card \"{title}\": {}.", done.join(", "))), CallResult::Edited { title })
    }

    /// A card in "Up next" or "Done" to a place in one of those two: the user's cards too, for
    /// that is no change to what they wrote. Never into or out of an agent's column or review:
    /// giving a card and closing a review are the user's.
    pub(super) fn radio_board_move(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = text(args, "card") else {
            return refused(NO_CARD_ID);
        };
        let column = match text(args, "column") {
            Some("queued") => Column::Queued,
            Some("done") => Column::Done,
            _ => return refused("`column` is queued (\"Up next\") or done (\"Done\"): a card goes to an agent or to review only by the user's hand."),
        };
        let index = match args.get("index") {
            None | Some(Value::Null) => usize::MAX,
            Some(index) => match index.as_u64() {
                Some(index) => usize::try_from(index).unwrap_or(usize::MAX),
                None => return refused("`index` is a place from 0, the top; left out, the bottom."),
            },
        };

        let now = now_ms();
        let mut board = self.board();
        let Some(card) = own(&board.cards, &project.path, id).cloned() else {
            return refused(NO_SUCH_CARD);
        };
        if !matches!(card.column, Column::Queued | Column::Done) {
            let column = column_name(card.column, card.agent());
            return refused(format!("\"{}\" is in \"{column}\": giving a card and closing a review are the user's, so it stays there.", card.title));
        }
        if let Some(why) = kept(&card, board.is_giving(id)) {
            return refused(why);
        }

        board.place(id, column, index, now);
        let (place, of) = place_of(&board.cards, &project.path, column, id);
        self.board_changed(board);

        let reply = match column {
            Column::Queued => format!("Moved \"{}\" to \"Up next\", place {place} of {of}.", card.title),
            _ => format!("Moved \"{}\" to \"Done\".", card.title),
        };
        (Reply::ok(reply), CallResult::Moved { title: card.title, column })
    }

    /// A card the radio added, from "Up next" or "Done", and its images with it. Never a card the
    /// user wrote, wherever it is.
    pub(super) fn radio_board_delete(&self, project: &ProjectView, args: &Value) -> Outcome {
        let Some(id) = text(args, "card") else {
            return refused(NO_CARD_ID);
        };

        let mut board = self.board();
        let Some(card) = own(&board.cards, &project.path, id).cloned() else {
            return refused(NO_SUCH_CARD);
        };
        if card.by.is_none() {
            return refused("Only the user deletes a card of theirs, one they wrote or changed in Pitwall. Move it to Done with board_move instead.");
        }
        if !matches!(card.column, Column::Queued | Column::Done) {
            let column = column_name(card.column, card.agent());
            return refused(format!("\"{}\" is in \"{column}\": only a card in \"Up next\" or \"Done\" is deleted over the radio.", card.title));
        }
        if let Some(why) = kept(&card, board.is_giving(id)) {
            return refused(why);
        }

        let Some(at) = board.cards.iter().position(|known| known.id == card.id) else {
            return refused(NO_SUCH_CARD);
        };
        let gone = board.cards.remove(at);
        board.remove_images(&gone.id, &gone.images);
        self.board_changed(board);
        (Reply::ok(format!("Deleted the card \"{}\".", card.title)), CallResult::Deleted { title: card.title })
    }

    /// The calling session takes a card of "Up next" for itself (the one named, else the top one
    /// that waits for nothing): it goes to the agent's column tied to that session, and the turn
    /// under way is its work (`Board::take`), so the card goes to review once that turn is over,
    /// the session ends or it calls `board_note`, as a card given from the board does. Never for
    /// a session that can't be told or one of the laps' layer (`radio_lap_layer`), nor while the
    /// session holds a card with it already, nor a card the board's file says waits.
    pub(super) fn radio_board_take(&self, agent: Agent, project: &ProjectView, session: Option<&str>, args: &Value) -> Outcome {
        let Some(session) = session else {
            return refused(NO_SESSION);
        };
        if self.radio_lap_layer(agent, Some(session)) {
            return refused(LAP_SESSION);
        }

        // Where the session stands, before the board is locked: it takes the core's own lock.
        let scanned = self.lock().scanned.iter().find(|state| state.id == session).cloned();
        let running = self.running_of(agent);
        let busy = stand(session, scanned.as_ref(), running.as_ref(), false) == Stand::Busy;
        let ran = running.as_ref().is_some_and(|running| running.contains_key(session));
        let start = self.lock().settings.project(&project.path).start_with(agent);

        let now = now_ms();
        let mut board = self.board();
        let held = board.cards.iter().find(|card| card.path == project.path && card.column == Column::Claude && card.session.as_deref() == Some(session));
        if let Some(held) = held {
            return refused(format!("This session holds the card \"{}\" already: finish it with board_note first.", held.title));
        }
        let card = match text(args, "card") {
            Some(id) => {
                let Some(card) = own(&board.cards, &project.path, id) else {
                    return refused(NO_SUCH_CARD);
                };
                if card.column != Column::Queued {
                    let column = column_name(card.column, card.agent());
                    return refused(format!("\"{}\" is in \"{column}\": only a card in \"Up next\" is taken.", card.title));
                }
                if let Some(why) = kept(card, board.is_giving(id)) {
                    return refused(why);
                }
                if board.waits_on_file(id) {
                    return refused(format!("\"{}\" still waits for its moment in the board's file, which Pitwall can't write now: leave it as it is, or ask the user.", card.title));
                }
                card
            }
            None => {
                let free = |card: &&Card| card.schedule.is_none() && !board.is_giving(&card.id) && !board.waits_on_file(&card.id);
                let top = board.cards.iter().filter(|card| card.path == project.path && card.column == Column::Queued).find(free);
                let Some(card) = top else {
                    return refused("No card in \"Up next\" is free to take: none is there, or each waits for its moment.");
                };
                card
            }
        };
        let (id, title) = (card.id.clone(), card.title.clone());
        let start = (card.agent() != agent).then_some(start);

        board.take(&id, session, start, busy, ran, now);
        self.board_changed(board);
        let column = column_name(Column::Claude, agent);
        let reply = format!("You took the card \"{title}\": it is in \"{column}\" now, and goes to \"To review\" when this turn ends. Say what you did with board_note.");
        (Reply::ok(reply), CallResult::Taken { title, agent })
    }
}
