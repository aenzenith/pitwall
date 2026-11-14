//! A chat: the user's `claude` run by the core with no terminal, over stream-json both ways
//! (docs/SOHBET.md; the window's side of every shape is `src/lib/chat/wire.ts`). What is here
//! knows nothing of the core: the process and its environment (`process.rs`), the NDJSON reader
//! (`reader.rs`), what goes over the wire each way (`wire.rs`, `control.rs`), a session's earlier
//! turns read from its log (`history.rs`) and Pitwall's own files about chats (`meta.rs`). The
//! tab, its buffer, its state and its commands are `core/chat.rs`'s.
//!
//! What the process writes (the conversation, thinking, tools' inputs and outputs) is held in
//! the core's memory for its tab alone: never written down, logged, in a snapshot, a record or a
//! backup.

pub mod control;
pub mod history;
pub mod meta;
pub mod process;
pub mod reader;
pub mod wire;

use serde::Serialize;

/// Where a chat's process stands: the CLI's own `session_state_changed`, and the core's ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatState {
    /// Started, its `initialize` not answered yet.
    Starting,
    Idle,
    Running,
    /// A card waits on the user (a permission, a question, a plan).
    RequiresAction,
    /// The process is gone; the tab stays until it is closed or started again.
    Exited,
}

impl ChatState {
    /// The CLI's word for it, or nothing.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "idle" => Some(Self::Idle),
            "running" => Some(Self::Running),
            "requires_action" => Some(Self::RequiresAction),
            _ => None,
        }
    }
}

/// Why a chat's process ended (`chat-exit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatExitReason {
    /// It was closed (the tab, a quit, chat ↔ terminal), or ended well by itself.
    Closed,
    /// It ended with a failure, or wrote what can't be read.
    Error,
    /// The session it was to go on with has no conversation.
    NotFound,
    /// It ended before it answered its `initialize`.
    StartFailed,
    /// Its session runs elsewhere.
    Elsewhere,
}

/// A chat tab's own part of its `TerminalView` (`wire.ts` `ChatView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatView {
    /// Its session's id; known before the process starts (Pitwall makes it).
    pub session: Option<String>,
    pub state: ChatState,
    /// Its session's name, as the CLI or the log names it.
    pub title: Option<String>,
}
