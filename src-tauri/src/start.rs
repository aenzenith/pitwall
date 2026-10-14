//! How a Claude Code session is started from Pitwall: with which model, and in which mode. Both
//! end up on a command line (`core/terminal.rs`: `claude_command`), so neither is ever text: each
//! is one of a fixed list, told from what the window, a settings file or the board's file says by
//! `Model::parse` and `Mode::parse`, and anything else is the default (no flag at all). What goes
//! onto the command line is made here from those values alone (`Start::args`), never from what
//! was read.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A model, by the alias Claude Code's `--model` takes for the newest one of its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    Opus,
    Sonnet,
    Haiku,
}

impl Model {
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Opus, Self::Sonnet, Self::Haiku];

    /// What `--model` is given.
    pub fn alias(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }

    /// One of the three, or nothing: no other text names a model.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "opus" => Some(Self::Opus),
            "sonnet" => Some(Self::Sonnet),
            "haiku" => Some(Self::Haiku),
            _ => None,
        }
    }
}

/// The mode a session starts in: Claude Code's own, plan first, or edits made without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Normal,
    Plan,
    AcceptEdits,
}

impl Mode {
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Normal, Self::Plan, Self::AcceptEdits];

    /// What `--permission-mode` is given; nothing for Claude Code's own.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Plan => Some("plan"),
            Self::AcceptEdits => Some("acceptEdits"),
        }
    }

    /// One of the three, or nothing.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "normal" => Some(Self::Normal),
            "plan" => Some(Self::Plan),
            "acceptEdits" => Some(Self::AcceptEdits),
            _ => None,
        }
    }

    pub fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
}

/// A model as a file has it: one of the list, and nothing for anything else (another type too),
/// so a value this version doesn't know costs neither the file nor the entry that holds it.
pub fn model_or_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Model>, D::Error> {
    Ok(Value::deserialize(deserializer)?.as_str().and_then(Model::parse))
}

/// A mode as a file has it: one of the list, and Claude Code's own for anything else.
pub fn mode_or_normal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Mode, D::Error> {
    Ok(Value::deserialize(deserializer)?.as_str().and_then(Mode::parse).unwrap_or_default())
}

/// How a session starts. The default is Claude Code's own on both counts: no flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Start {
    /// None: the model Claude Code picks itself.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "model_or_none")]
    pub model: Option<Model>,
    #[serde(skip_serializing_if = "Mode::is_normal", deserialize_with = "mode_or_normal")]
    pub mode: Mode,
}

impl Start {
    /// From what was said in text (the window's choice): each one of its list, or its default.
    pub fn of(model: Option<&str>, mode: Option<&str>) -> Self {
        Self { model: model.and_then(Model::parse), mode: mode.and_then(Mode::parse).unwrap_or_default() }
    }

    /// Claude's arguments for it, each a constant of this file: `--model sonnet`,
    /// `--permission-mode plan`. The argument list form (Windows hands it over as it is).
    pub fn args(self) -> Vec<&'static str> {
        let mut args = Vec::new();
        if let Some(model) = self.model {
            args.extend(["--model", model.alias()]);
        }
        if let Some(mode) = self.mode.flag() {
            args.extend(["--permission-mode", mode]);
        }
        args
    }

    /// The same as part of a shell line, each argument after a space (empty for the default):
    /// ` --model sonnet --permission-mode plan`. Letters and dashes only, so nothing to quote.
    pub fn line(self) -> String {
        self.args().iter().map(|arg| format!(" {arg}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model and the mode are written into a command line. Only the values of the two lists
    /// get there: whatever else the window, a settings file or the board's file names (shell
    /// syntax, another of Claude's options, a full model name, the right word in other letters)
    /// is the default, which adds nothing to the line.
    #[test]
    fn only_listed_models_and_modes_reach_the_command_line() {
        let hostile = [
            "",
            " ",
            "opus ",
            " sonnet",
            "Opus",
            "SONNET",
            "opus; touch /tmp/pwned",
            "sonnet --dangerously-skip-permissions",
            "$(touch /tmp/pwned)",
            "`touch /tmp/pwned`",
            "haiku\n--dangerously-skip-permissions",
            "--dangerously-skip-permissions",
            "bypassPermissions",
            "dontAsk",
            "auto",
            "acceptedits",
            "plan --model x",
            "claude-opus-5-5",
            "opus[1m]",
            "' || true #",
            "\"; rm -rf ~; \"",
        ];
        for text in hostile {
            assert_eq!(Model::parse(text), None, "{text:?}");
            assert_eq!(Mode::parse(text), None, "{text:?}");
            // As the window says it, and as a file does: the default, with nothing on the line.
            assert_eq!(Start::of(Some(text), Some(text)), Start::default(), "{text:?}");
            let read: Start = serde_json::from_value(serde_json::json!({ "model": text, "mode": text })).unwrap();
            assert_eq!(read, Start::default(), "{text:?}");
            assert!(read.args().is_empty() && read.line().is_empty(), "{text:?}");
        }
        // Not text at all: the default too, and the file still reads.
        for odd in [serde_json::json!(7), serde_json::json!(null), serde_json::json!(["opus"]), serde_json::json!({ "opus": true })] {
            let read: Start = serde_json::from_value(serde_json::json!({ "model": odd, "mode": odd })).unwrap();
            assert_eq!(read, Start::default(), "{odd}");
        }

        // Every start there is, in both forms: the words of the two lists and nothing else.
        let allowed = ["--model", "opus", "sonnet", "haiku", "--permission-mode", "plan", "acceptEdits"];
        let mut seen = std::collections::HashSet::new();
        for model in std::iter::once(None).chain(Model::ALL.into_iter().map(Some)) {
            for mode in Mode::ALL {
                let start = Start { model, mode };
                let args = start.args();
                assert!(args.iter().all(|arg| allowed.contains(arg)), "{args:?}");
                assert_eq!(args.len(), 2 * (usize::from(model.is_some()) + usize::from(mode != Mode::Normal)), "{args:?}");
                let line = start.line();
                assert_eq!(line.split(' ').filter(|word| !word.is_empty()).collect::<Vec<_>>(), args, "{line:?}");
                assert!(line.bytes().all(|byte| byte.is_ascii_alphabetic() || byte == b'-' || byte == b' '), "{line:?}");
                // What is written to a file reads back as it was.
                let written = serde_json::to_value(start).unwrap();
                assert_eq!(serde_json::from_value::<Start>(written).unwrap(), start);
                // And what the window names by the words of the lists is that start.
                assert_eq!(Start::of(model.map(Model::alias), Some(mode.flag().unwrap_or("normal"))), start);
                assert!(seen.insert(line));
            }
        }
        assert_eq!(seen.len(), 12);
        assert_eq!(Start { model: Some(Model::Sonnet), mode: Mode::Plan }.line(), " --model sonnet --permission-mode plan");
        assert_eq!(Start { model: Some(Model::Haiku), mode: Mode::AcceptEdits }.args(), ["--model", "haiku", "--permission-mode", "acceptEdits"]);
    }
}
