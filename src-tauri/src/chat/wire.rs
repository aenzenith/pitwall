//! What goes over a chat's wire: the user's messages as the core builds them (the window sends
//! no JSON), the CLI's frames made fit for the window (`sanitize`), and what the core learns of
//! the process (`ChatInfo`), each read by a reader that declares its fields.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::i18n::t;

/// The most base64 an image of a message carries: the window scales a larger one first.
pub const IMAGE_BASE64_MAX: usize = 5 * 1024 * 1024;
/// The most base64 a PDF of a message carries.
pub const DOCUMENT_BASE64_MAX: usize = 32 * 1024 * 1024;
/// The most characters a message's text holds.
pub const TEXT_MAX: usize = 50_000;
/// The pictures a message may carry, as the Messages API takes them.
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];
/// The most images and documents one message carries.
const ATTACHMENTS_MAX: usize = 20;

/// A message as the window hands it over (`wire.ts` `ChatOutgoing`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatOutgoing {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub images: Vec<OutImage>,
    #[serde(default)]
    pub documents: Vec<OutDocument>,
    #[serde(default = "next")]
    pub priority: String,
    #[serde(default)]
    pub pasted: bool,
}

fn next() -> String {
    "next".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutImage {
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutDocument {
    pub media_type: String,
    pub data: String,
    #[serde(default)]
    pub name: String,
}

impl ChatOutgoing {
    /// A message of text alone (a board card's prompt).
    pub fn text(text: String) -> Self {
        Self { text, images: Vec::new(), documents: Vec::new(), priority: next(), pasted: false }
    }

    /// Refused: text over `TEXT_MAX` characters, an image over `IMAGE_BASE64_MAX` of base64 or of
    /// a kind the API doesn't take, a document that isn't a PDF, anything that isn't base64, a
    /// priority not of the three, nothing at all to send.
    pub fn checked(&self, carried: bool) -> Result<(), String> {
        if self.text.chars().count() > TEXT_MAX {
            return Err(t!("chatCore.error.textTooLong", limit = TEXT_MAX));
        }
        if !matches!(self.priority.as_str(), "next" | "now" | "later") {
            return Err(t!("chatCore.error.badMessage"));
        }
        if self.images.len() + self.documents.len() > ATTACHMENTS_MAX {
            return Err(t!("chatCore.error.badMessage"));
        }
        for image in &self.images {
            if image.data.len() > IMAGE_BASE64_MAX {
                return Err(t!("chatCore.error.imageTooLarge"));
            }
            if !IMAGE_TYPES.contains(&image.media_type.as_str()) || !is_base64(&image.data) {
                return Err(t!("chatCore.error.badMessage"));
            }
        }
        for document in &self.documents {
            if document.media_type != "application/pdf" || document.data.len() > DOCUMENT_BASE64_MAX || !is_base64(&document.data) {
                return Err(t!("chatCore.error.badMessage"));
            }
        }
        if self.text.trim().is_empty() && self.images.is_empty() && self.documents.is_empty() && !carried {
            return Err(t!("chatCore.error.badMessage"));
        }
        Ok(())
    }
}

/// Base64 as a data URL carries it: letters, digits, `+`, `/`, and `=` at its end.
fn is_base64(data: &str) -> bool {
    !data.is_empty() && data.trim_end_matches('=').bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
}

/// What a `!` line ran and printed, carried to Claude with the user's next message as Claude
/// Code's own `!` mode hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCarry {
    pub line: String,
    pub stdout: String,
    pub stderr: String,
}

impl ShellCarry {
    /// The two text blocks: the line, then what it printed.
    fn blocks(&self) -> [Value; 2] {
        [
            json!({ "type": "text", "text": format!("<bash-input>{}</bash-input>", self.line) }),
            json!({ "type": "text", "text": format!("<bash-stdout>{}</bash-stdout><bash-stderr>{}</bash-stderr>", self.stdout, self.stderr) }),
        ]
    }
}

/// The user's message as the CLI reads it on stdin: the uuid the core minted, a human's origin,
/// the priority, and its content: what `!` lines carry first, then the text, then the images and
/// documents. Pasted text is marked so where it stands (`inline_pastes`), Claude Code's way of
/// telling the user's words from what they pasted.
pub fn user_message(message: &ChatOutgoing, uuid: &str, carried: &[ShellCarry]) -> Value {
    let mut content: Vec<Value> = carried.iter().flat_map(ShellCarry::blocks).collect();
    if !message.text.is_empty() {
        content.push(json!({ "type": "text", "text": message.text }));
    }
    for image in &message.images {
        content.push(json!({ "type": "image", "source": { "type": "base64", "media_type": image.media_type, "data": image.data } }));
    }
    for document in &message.documents {
        let mut block = json!({ "type": "document", "source": { "type": "base64", "media_type": "application/pdf", "data": document.data } });
        let name: String = document.name.chars().filter(|c| !c.is_control()).take(200).collect();
        if !name.is_empty() {
            block["title"] = Value::String(name);
        }
        content.push(block);
    }
    let mut line = json!({
        "type": "user",
        "uuid": uuid,
        "session_id": "",
        "message": { "role": "user", "content": content },
        "parent_tool_use_id": null,
        "origin": { "kind": "human" },
        "priority": message.priority,
    });
    if message.pasted && !message.text.is_empty() {
        line["inline_pastes"] = json!([message.text]);
    }
    line
}

/// The text with terminal escapes and other control characters taken out (a `decision_reason`
/// may carry the colours of a hook's output); line breaks and tabs stay.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                // CSI: parameters, then one final byte.
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC and the strings: up to BEL or ST.
                Some(']' | 'P' | '_' | '^' | 'X') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The base64 of every image and document in `content` (and in the tool results it holds) taken
/// out: `"data": null` and `"omitted": true` instead. The window has what it sent; a replay or a
/// log would only carry it again.
pub fn drop_media(content: &mut Value) {
    let Some(blocks) = content.as_array_mut() else {
        return;
    };
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("image" | "document") => {
                if let Some(source) = block.get_mut("source").and_then(Value::as_object_mut) {
                    if source.get("data").is_some_and(|data| !data.is_null()) {
                        source.insert("data".into(), Value::Null);
                        if let Some(block) = block.as_object_mut() {
                            block.insert("omitted".into(), Value::Bool(true));
                        }
                    }
                }
            }
            Some("tool_result") => {
                if let Some(inner) = block.get_mut("content") {
                    drop_media(inner);
                }
            }
            _ => {}
        }
    }
}

/// A frame of the CLI's made fit for the window, in place: a replayed user message without its
/// images' and documents' data, a permission's reason without escapes, a result without when the
/// credential ends.
pub fn sanitize(frame: &mut Value) {
    match frame.get("type").and_then(Value::as_str) {
        Some("user") if frame.get("isReplay").and_then(Value::as_bool) == Some(true) => {
            if let Some(content) = frame.pointer_mut("/message/content") {
                drop_media(content);
            }
        }
        Some("result") => {
            if let Some(fields) = frame.as_object_mut() {
                fields.remove("credential_expires_at");
            }
        }
        Some("control_request") => {
            if let Some(reason) = frame.pointer_mut("/request/decision_reason") {
                if let Some(text) = reason.as_str() {
                    *reason = Value::String(strip_ansi(text));
                }
            }
        }
        _ => {}
    }
}

/// Keys whose values never leave the core, wherever they are in a control request's answer: an
/// account's identity, an MCP server's secrets, the environment a settings file sets.
const SECRET_KEYS: [&str; 12] = ["account", "email", "emailAddress", "organization", "organizationName", "organizationUuid", "accountUuid", "oauthAccount", "headers", "env", "apiKey", "authToken"];
/// The rows of `get_status` that name the account: never shown.
const SECRET_ROWS: [&str; 3] = ["email", "organization", "account"];

/// A control request's answer made fit for the window: every `SECRET_KEYS` key taken out, and of
/// `get_status`'s sections the rows that name the account (`Email`, `Organization`). The rest of
/// the account's traces goes with `scrub_emails`.
pub fn scrub_answer(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for key in SECRET_KEYS {
                fields.remove(key);
            }
            let row = fields.get("label").and_then(Value::as_str).map(str::to_lowercase);
            if row.is_some_and(|label| SECRET_ROWS.iter().any(|secret| label.contains(secret))) {
                if let Some(value) = fields.get_mut("value") {
                    *value = Value::Null;
                }
            }
            for inner in fields.values_mut() {
                scrub_answer(inner);
            }
        }
        Value::Array(items) => {
            items.retain(|item| {
                let label = item.get("label").and_then(Value::as_str).map(str::to_lowercase);
                !label.is_some_and(|label| SECRET_ROWS.iter().any(|secret| label.contains(secret)))
            });
            for item in items {
                scrub_answer(item);
            }
        }
        _ => {}
    }
}

/// Whether `byte` may stand in an email address's local part or its domain.
fn email_byte(byte: u8, domain: bool) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-') || (!domain && matches!(byte, b'_' | b'+' | b'%'))
}

/// `text` with every email address in it put as `…`; none when it holds none.
pub fn redact_emails(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (at, _) in text.match_indices('@') {
        let start = bytes[..at].iter().rposition(|&byte| !email_byte(byte, false)).map_or(0, |before| before + 1);
        let end = bytes[at + 1..].iter().position(|&byte| !email_byte(byte, true)).map_or(bytes.len(), |after| at + 1 + after);
        let domain = text[at + 1..end].trim_end_matches('.');
        let dotted = domain.rsplit_once('.').is_some_and(|(name, top)| !name.is_empty() && top.len() >= 2 && top.bytes().all(|byte| byte.is_ascii_alphabetic()));
        if start < at && dotted && spans.last().is_none_or(|&(_, last)| last <= start) {
            spans.push((start, at + 1 + domain.len()));
        }
    }
    if spans.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for (start, end) in spans {
        out.push_str(&text[from..start]);
        out.push('…');
        from = end;
    }
    out.push_str(&text[from..]);
    Some(out)
}

/// Every email address in a control request's answer about the session or the setup (`get_status`,
/// `get_usage`, `get_settings`, `get_session_cost`…) taken out, wherever the CLI put the
/// account's: a row (an object with a `label`) that holds one goes whole, any other text holds `…`
/// in its place. Not for the answers that carry the conversation or the project's files
/// (`ChatControl::carries_content`).
pub fn scrub_emails(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(redacted) = redact_emails(text) {
                *text = redacted;
            }
        }
        Value::Array(items) => {
            items.retain(|item| !(item.get("label").is_some() && holds_email(item)));
            items.iter_mut().for_each(scrub_emails);
        }
        Value::Object(fields) => fields.values_mut().for_each(scrub_emails),
        _ => {}
    }
}

fn holds_email(value: &Value) -> bool {
    match value {
        Value::String(text) => redact_emails(text).is_some(),
        Value::Array(items) => items.iter().any(holds_email),
        Value::Object(fields) => fields.values().any(holds_email),
        _ => false,
    }
}

/// The CLI's error as the window shows it, verbatim: its first line, escapes taken out, no email
/// address and no path (a word of two folders or more, `…` instead), 200 characters at most.
pub fn short_error(text: &str) -> String {
    let text = strip_ansi(text);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let line = redact_emails(line).unwrap_or_else(|| line.to_string());
    let pathlike = |word: &str| {
        let word = word.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')' | ',' | ':' | ';'));
        let rooted = word.starts_with('/') || word.starts_with("~/") || word.starts_with("\\\\") || word.get(1..3) == Some(":\\");
        rooted && word.matches(['/', '\\']).count() >= 2
    };
    let words: Vec<&str> = line.split(' ').map(|word| if pathlike(word) { "…" } else { word }).collect();
    let mut short: String = words.join(" ").chars().take(200).collect();
    if short.is_empty() {
        short = t!("chatCore.error.refusedByCli");
    }
    short
}

/// A slash command, skill or plugin command the process can run (`wire.ts` `ChatCommand`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCommand {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAgentInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModelInfo {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_effort: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_effort_levels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_adaptive_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_fast_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_auto_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_thinking_off: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// What the core learned of the process (`wire.ts` `ChatInfo`): its `initialize` reply, then each
/// turn's `system/init`, `system/status` and `system/commands_changed`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub commands: Vec<ChatCommand>,
    pub agents: Vec<ChatAgentInfo>,
    pub models: Vec<ChatModelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_style: Option<String>,
    pub output_styles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_mode_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_mode_disabled_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_provider: Option<String>,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServerInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<PluginInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_commands: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// One field of `fields`, when it has the type asked for.
fn field<T: DeserializeOwned>(fields: &Map<String, Value>, key: &str) -> Option<T> {
    fields.get(key).and_then(|value| T::deserialize(value).ok())
}

/// Each entry of a list that reads as `T`; one that doesn't is left out.
fn entries<T: DeserializeOwned>(fields: &Map<String, Value>, key: &str) -> Option<Vec<T>> {
    fields.get(key).and_then(Value::as_array).map(|items| items.iter().filter_map(|item| T::deserialize(item).ok()).collect())
}

/// Text entries of a list, or the `name` of object entries.
fn names(fields: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    fields.get(key).and_then(Value::as_array).map(|items| {
        items.iter().filter_map(|item| item.as_str().map(str::to_string).or_else(|| item.get("name").and_then(Value::as_str).map(str::to_string))).collect()
    })
}

impl ChatInfo {
    /// From the `initialize` reply: of its `account` only `subscriptionType` and `apiProvider` are
    /// read; the email and the organisation are skipped, never kept.
    pub fn of_initialize(reply: &Value) -> Self {
        let Some(fields) = reply.as_object() else {
            return Self::default();
        };
        let account = fields.get("account").and_then(Value::as_object);
        let account_field = |key: &str| account.and_then(|account| account.get(key)).and_then(Value::as_str).map(str::to_string);
        Self {
            version: field(fields, "claude_code_version"),
            commands: entries(fields, "commands").unwrap_or_default(),
            agents: entries(fields, "agents").unwrap_or_default(),
            models: entries(fields, "models").unwrap_or_default(),
            output_style: field(fields, "output_style"),
            output_styles: names(fields, "available_output_styles").unwrap_or_default(),
            permission_mode: field(fields, "current_permission_mode"),
            fast_mode_state: field(fields, "fast_mode_state"),
            fast_mode_disabled_reason: field(fields, "fast_mode_disabled_reason"),
            subscription_type: account_field("subscriptionType"),
            api_provider: account_field("apiProvider"),
            capabilities: names(fields, "capabilities").unwrap_or_default(),
            ..Self::default()
        }
    }

    /// What a frame of the process tells: a turn's `system/init`, a `system/status` that names
    /// the mode, a `system/commands_changed`. Whether anything changed.
    pub fn take(&mut self, frame: &Value) -> bool {
        let before = self.clone();
        let Some(fields) = frame.as_object() else {
            return false;
        };
        match (fields.get("type").and_then(Value::as_str), fields.get("subtype").and_then(Value::as_str)) {
            (Some("system"), Some("init")) => {
                if let Some(version) = field(fields, "claude_code_version") {
                    self.version = Some(version);
                }
                self.model = field(fields, "model").or(self.model.take());
                self.permission_mode = field(fields, "permissionMode").or(self.permission_mode.take());
                self.output_style = field(fields, "output_style").or(self.output_style.take());
                self.fast_mode_state = field(fields, "fast_mode_state").or(self.fast_mode_state.take());
                self.skills = names(fields, "skills").or(self.skills.take());
                self.tools = names(fields, "tools").or(self.tools.take());
                self.mcp_servers = entries(fields, "mcp_servers").or(self.mcp_servers.take());
                self.plugins = entries(fields, "plugins").or(self.plugins.take());
                self.terminal_commands = names(fields, "terminal_slash_commands").or(self.terminal_commands.take());
                self.cwd = field(fields, "cwd").or(self.cwd.take());
                self.session_id = field(fields, "session_id").or(self.session_id.take());
                for capability in names(fields, "capabilities").unwrap_or_default() {
                    if !self.capabilities.contains(&capability) {
                        self.capabilities.push(capability);
                    }
                }
            }
            (Some("system"), Some("status")) => {
                if let Some(mode) = field(fields, "permissionMode") {
                    self.permission_mode = Some(mode);
                }
            }
            (Some("system"), Some("commands_changed")) => {
                if let Some(commands) = entries(fields, "commands") {
                    self.commands = commands;
                }
            }
            _ => {}
        }
        *self != before
    }

    /// The model values a `set_model` may name: those the `initialize` reply listed.
    pub fn model_values(&self) -> Vec<String> {
        self.models.iter().map(|model| model.value.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Part of Test 5: a control's reply about the session keeps no trace of the account (the
    /// rows that name it, an email anywhere in it), and a CLI's error is shown without an email
    /// or a path.
    #[test]
    fn a_reply_keeps_no_trace_of_the_account() {
        let mut status = json!({ "sections": [{ "title": "Session", "rows": [
            { "label": "Version", "value": "2.1.325" },
            { "label": "Organization", "value": "jo@example.com's Organization" },
            { "label": "Email", "value": "jo@example.com" },
            { "label": "Signed in", "value": "Jo (jo@example.com)" },
        ] }] });
        scrub_answer(&mut status);
        scrub_emails(&mut status);
        assert_eq!(status, json!({ "sections": [{ "title": "Session", "rows": [{ "label": "Version", "value": "2.1.325" }] }] }));

        let mut usage = json!({ "oauthAccount": { "emailAddress": "jo@example.com" }, "note": "billed to jo.doe+work@mail.example.co.uk", "plan": "max" });
        scrub_answer(&mut usage);
        scrub_emails(&mut usage);
        assert_eq!(usage, json!({ "note": "billed to …", "plan": "max" }));

        let error = short_error("\u{1b}[31mMCP server failed\u{1b}[0m for jo@example.com: /Users/jo/projects/app/.mcp.json is broken\nat line 2");
        assert_eq!(error, "MCP server failed for …: … is broken");
    }
}
