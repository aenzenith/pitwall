//! How a session is started from Pitwall: of which agent (`agent.rs`), with which model, and in
//! which mode. Both end up on a command line (`core/terminal.rs`: `claude_command`,
//! `codex_command`), so neither is ever text: each is one of a fixed list, told from what the
//! window, a settings file or the board's file says by `Model::parse` and `Mode::parse` (Codex's:
//! `CodexModel::parse`, `CodexMode::parse`), and anything else is the default (no flag at all).
//! What goes onto the command line is made here from those values alone (`Start::args`,
//! `CodexStart::args`), never from what was read.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::agent::Agent;

/// A model, by the alias Claude Code's `--model` takes for the newest one of its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    Fable,
    Opus,
    Sonnet,
    Haiku,
}

impl Model {
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Fable, Self::Opus, Self::Sonnet, Self::Haiku];

    /// What `--model` is given.
    pub fn alias(self) -> &'static str {
        match self {
            Self::Fable => "fable",
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }

    /// One of the list, or nothing: no other text names a model.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "fable" => Some(Self::Fable),
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

/// A Codex model, by the id Codex's `-m` takes. The list rots fast: a saved model no longer
/// listed starts with no flag, Codex's own choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CodexModel {
    #[serde(rename = "gpt-6.1-sol")]
    Sol,
    #[serde(rename = "gpt-6.1-terra")]
    Terra,
    #[serde(rename = "gpt-6-astra")]
    Astra,
    #[serde(rename = "gpt-6-luna")]
    Luna,
}

impl CodexModel {
    /// In the order the window offers them.
    pub const ALL: [Self; 4] = [Self::Sol, Self::Terra, Self::Astra, Self::Luna];

    /// What `-m` is given.
    pub fn id(self) -> &'static str {
        match self {
            Self::Sol => "gpt-6.1-sol",
            Self::Terra => "gpt-6.1-terra",
            Self::Astra => "gpt-6-astra",
            Self::Luna => "gpt-6-luna",
        }
    }

    /// One of the list, or nothing: no other text names a model.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.id() == text)
    }
}

/// The mode a Codex session starts in: Codex's own, or read-only (`--sandbox read-only`). Nothing
/// else: no plan mode (Codex has no start flag for it), never one that skips approvals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexMode {
    #[default]
    Normal,
    ReadOnly,
}

impl CodexMode {
    pub const ALL: [Self; 2] = [Self::Normal, Self::ReadOnly];

    /// The window's word for it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::ReadOnly => "readOnly",
        }
    }

    /// One of the two, or nothing.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.word() == text)
    }

    pub fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
}

/// A Codex model as a file has it: one of the list, and nothing for anything else.
pub fn codex_model_or_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<CodexModel>, D::Error> {
    Ok(Value::deserialize(deserializer)?.as_str().and_then(CodexModel::parse))
}

/// A Codex mode as a file has it: one of the list, and Codex's own for anything else.
pub fn codex_mode_or_normal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<CodexMode, D::Error> {
    Ok(Value::deserialize(deserializer)?.as_str().and_then(CodexMode::parse).unwrap_or_default())
}

/// How a Codex session starts. The default is Codex's own on both counts: no flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CodexStart {
    /// None: the model Codex picks itself.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "codex_model_or_none")]
    pub model: Option<CodexModel>,
    #[serde(skip_serializing_if = "CodexMode::is_normal", deserialize_with = "codex_mode_or_normal")]
    pub mode: CodexMode,
}

impl CodexStart {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Codex's arguments for it, each a constant of this file (`-m gpt-6.1-sol`, `-s read-only`),
    /// in the argument list form. Never `--no-daemon`: that belongs to every command line Pitwall
    /// starts Codex with (`core/terminal.rs`).
    pub fn args(self) -> Vec<&'static str> {
        let mut args = Vec::new();
        if let Some(model) = self.model {
            args.extend(["-m", model.id()]);
        }
        if self.mode == CodexMode::ReadOnly {
            args.extend(["-s", "read-only"]);
        }
        args
    }

    /// The same as part of a shell line, each argument after a space (empty for the default).
    pub fn line(self) -> String {
        self.args().iter().map(|arg| format!(" {arg}")).collect()
    }
}

/// How a session starts: the agent it goes to, and each agent's model and mode, of which only the
/// agent's own count. The default is Claude Code's own on both counts: no flag. A file that names
/// no agent (every file before Codex) is Claude's, and a Claude start is written as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Start {
    #[serde(skip_serializing_if = "Agent::is_claude")]
    pub agent: Agent,
    /// Claude's model. None: the model Claude Code picks itself.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "model_or_none")]
    pub model: Option<Model>,
    /// Claude's mode.
    #[serde(skip_serializing_if = "Mode::is_normal", deserialize_with = "mode_or_normal")]
    pub mode: Mode,
    /// Codex's model and mode.
    #[serde(skip_serializing_if = "CodexStart::is_default", deserialize_with = "codex_or_default")]
    pub codex: CodexStart,
}

/// Codex's part as a file has it; anything that doesn't fit is Codex's own choice.
fn codex_or_default<'de, D: Deserializer<'de>>(deserializer: D) -> Result<CodexStart, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).unwrap_or_default())
}

impl Start {
    /// From what was said in text (the window's choice) for Claude: each one of its list, or its
    /// default.
    pub fn of(model: Option<&str>, mode: Option<&str>) -> Self {
        Self { model: model.and_then(Model::parse), mode: mode.and_then(Mode::parse).unwrap_or_default(), ..Self::default() }
    }

    /// A Claude start, as a constant can have it.
    pub const fn claude(model: Option<Model>, mode: Mode) -> Self {
        Self { agent: Agent::Claude, model, mode, codex: CodexStart { model: None, mode: CodexMode::Normal } }
    }

    /// From what was said in text for `agent` (none: Claude, one not known: Claude too): the
    /// model and the mode of that agent's lists, or its defaults; the other agents' parts are
    /// their defaults.
    pub fn chosen(agent: Option<&str>, model: Option<&str>, mode: Option<&str>) -> Self {
        match agent.and_then(Agent::parse).unwrap_or_default() {
            Agent::Claude => Self::of(model, mode),
            Agent::Codex => Self {
                agent: Agent::Codex,
                codex: CodexStart { model: model.and_then(CodexModel::parse), mode: mode.and_then(CodexMode::parse).unwrap_or_default() },
                ..Self::default()
            },
        }
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

    /// The agent, the model and the mode are written into a command line. Only the values of the
    /// lists get there, for either agent: whatever else the window, a settings file or the board's
    /// file names (shell syntax, another of the agent's options, one that skips its approvals, a
    /// full model name, the right word in other letters) is the default, which adds nothing to the
    /// line, and an agent named in other words is Claude.
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
            "Fable",
            "claude-fable-5-1[1m]",
            "' || true #",
            "\"; rm -rf ~; \"",
            // Codex's: the ids in other letters or padded, its other sandboxes and approvals, its
            // options that skip them, retired models.
            "gpt-6.1-sol ",
            " gpt-6-luna",
            "GPT-6.1-SOL",
            "gpt-6.1-Terra",
            "gpt-6.1-sol; touch /tmp/pwned",
            "gpt-6-astra --yolo",
            "gpt-6-sol",
            "gpt-5.5",
            "o3",
            "read-only",
            "readonly",
            "ReadOnly",
            "readOnly --dangerously-bypass-approvals-and-sandbox",
            "workspace-write",
            "danger-full-access",
            "--dangerously-bypass-approvals-and-sandbox",
            "--dangerously-bypass-hook-trust",
            "--yolo",
            "--full-auto",
            "--approve-for-me",
            "never",
            "-s danger-full-access",
            "-c sandbox_mode=\"danger-full-access\"",
        ];
        for text in hostile {
            assert_eq!(Model::parse(text), None, "{text:?}");
            assert_eq!(Mode::parse(text), None, "{text:?}");
            assert_eq!(CodexModel::parse(text), None, "{text:?}");
            assert_eq!(CodexMode::parse(text), None, "{text:?}");
            // As the window says it, and as a file does: the default, with nothing on the line.
            assert_eq!(Start::of(Some(text), Some(text)), Start::default(), "{text:?}");
            let read: Start = serde_json::from_value(serde_json::json!({ "model": text, "mode": text })).unwrap();
            assert_eq!(read, Start::default(), "{text:?}");
            assert!(read.args().is_empty() && read.line().is_empty(), "{text:?}");
            // Said or read for Codex: Codex's own choice, nothing on its line either.
            let codex = Start { agent: Agent::Codex, ..Start::default() };
            let said = Start::chosen(Some("codex"), Some(text), Some(text));
            let read: Start = serde_json::from_value(serde_json::json!({ "agent": "codex", "codex": { "model": text, "mode": text } })).unwrap();
            for start in [said, read] {
                assert_eq!(start, codex, "{text:?}");
                assert!(start.codex.args().is_empty() && start.codex.line().is_empty() && start.args().is_empty(), "{text:?}");
            }
            // An agent named in other words is Claude, with Claude's default.
            assert_eq!(Start::chosen(Some(text), Some("gpt-6.1-sol"), Some("readOnly")), Start::default(), "{text:?}");
        }
        // Not text at all: the default too, and the file still reads.
        for odd in [serde_json::json!(7), serde_json::json!(null), serde_json::json!(["opus"]), serde_json::json!({ "opus": true })] {
            let read: Start = serde_json::from_value(serde_json::json!({ "model": odd, "mode": odd })).unwrap();
            assert_eq!(read, Start::default(), "{odd}");
            // Codex's part of another type, or holding one: Codex's own choice, the agent kept.
            let read: Start = serde_json::from_value(serde_json::json!({ "agent": "codex", "codex": odd })).unwrap();
            assert_eq!(read, Start { agent: Agent::Codex, ..Start::default() }, "{odd}");
            let read: Start = serde_json::from_value(serde_json::json!({ "agent": "codex", "codex": { "model": odd, "mode": odd } })).unwrap();
            assert_eq!(read, Start { agent: Agent::Codex, ..Start::default() }, "{odd}");
        }

        // Every start there is, in both forms: the words of the two lists and nothing else.
        let allowed = ["--model", "fable", "opus", "sonnet", "haiku", "--permission-mode", "plan", "acceptEdits"];
        let mut seen = std::collections::HashSet::new();
        for model in std::iter::once(None).chain(Model::ALL.into_iter().map(Some)) {
            for mode in Mode::ALL {
                let start = Start { model, mode, ..Start::default() };
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
        assert_eq!(seen.len(), 15);
        assert_eq!(Start { model: Some(Model::Sonnet), mode: Mode::Plan, ..Start::default() }.line(), " --model sonnet --permission-mode plan");
        assert_eq!(Start { model: Some(Model::Haiku), mode: Mode::AcceptEdits, ..Start::default() }.args(), ["--model", "haiku", "--permission-mode", "acceptEdits"]);

        // Every Codex start there is: the words of its two lists and nothing else, never a flag
        // that skips its approvals or its sandbox.
        let allowed = ["-m", "gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-astra", "gpt-6-luna", "-s", "read-only"];
        let mut seen = std::collections::HashSet::new();
        for model in std::iter::once(None).chain(CodexModel::ALL.into_iter().map(Some)) {
            for mode in CodexMode::ALL {
                let codex = CodexStart { model, mode };
                let start = Start { agent: Agent::Codex, codex, ..Start::default() };
                let args = codex.args();
                assert!(args.iter().all(|arg| allowed.contains(arg)), "{args:?}");
                assert_eq!(args.len(), 2 * (usize::from(model.is_some()) + usize::from(mode != CodexMode::Normal)), "{args:?}");
                let line = codex.line();
                assert_eq!(line.split(' ').filter(|word| !word.is_empty()).collect::<Vec<_>>(), args, "{line:?}");
                assert!(line.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b' ')), "{line:?}");
                // Claude's part of a Codex start says nothing.
                assert!(start.args().is_empty(), "{start:?}");
                // Written as the board's file and a project's settings hold it, and read back.
                let written = serde_json::to_value(start).unwrap();
                assert_eq!(written["agent"], "codex");
                assert_eq!(serde_json::from_value::<Start>(written).unwrap(), start);
                // And what the window names by the words of the lists is that start.
                assert_eq!(Start::chosen(Some("codex"), model.map(CodexModel::id), Some(mode.word())), start);
                assert!(seen.insert(line));
            }
        }
        assert_eq!(seen.len(), 10);
        assert_eq!(CodexStart { model: Some(CodexModel::Terra), mode: CodexMode::ReadOnly }.line(), " -m gpt-6.1-terra -s read-only");

        // A Claude start is written as before there were agents: the default as `{}`, with neither
        // an agent nor a Codex part, and Claude's words for its model and mode.
        assert_eq!(serde_json::to_value(Start::default()).unwrap(), serde_json::json!({}));
        assert_eq!(serde_json::to_value(Start::claude(Some(Model::Opus), Mode::Plan)).unwrap(), serde_json::json!({ "model": "opus", "mode": "plan" }));
        assert_eq!(Start::chosen(None, Some("opus"), Some("plan")), Start::claude(Some(Model::Opus), Mode::Plan));
        assert_eq!(Start::chosen(Some("claude"), Some("gpt-6.1-sol"), Some("readOnly")), Start::default());
    }
}
