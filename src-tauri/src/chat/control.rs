//! A chat's control requests, both ways. The window sends no JSON: what it asks of the process
//! is a `ChatControl` (one of a fixed list, each checked here) and what it answers to a card is a
//! `ChatAnswer`, of which the core builds the CLI's line itself, on the request it holds.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::i18n::t;
use crate::start::{new_uuid, ChatMode};

/// The id of a request the core sends: `req_<n>_<8 hex>`, as the SDKs number theirs.
pub fn request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("req_{}_{}", NEXT.fetch_add(1, Ordering::Relaxed), &new_uuid()[..8])
}

/// A control request's line, around its body.
pub fn request_line(id: &str, body: Value) -> Value {
    json!({ "type": "control_request", "request_id": id, "request": body })
}

/// The `initialize` request: no hooks of Pitwall's, Stop that ends the turn alone (background
/// work stopped one by one), the agents' progress told, prompt suggestions as Settings say, and
/// no dialog kinds, since the window draws none yet (a kind not declared is never sent).
pub fn initialize(suggestions: bool) -> Value {
    json!({
        "subtype": "initialize",
        "hooks": null,
        "promptSuggestions": suggestions,
        "perTaskStopAffordance": true,
        "agentProgressSummaries": true,
        "supportedDialogKinds": [],
    })
}

/// Asks the CLI to name the session from `text`, the message its turn began with, as Claude Code
/// names one in its terminal: the title is kept as the session's `ai-title`, and one it has
/// already is given back as it is (`persist`). The reply's `title` is null when there is nothing
/// to name it from (a message too short, a command).
pub fn generate_title(text: &str) -> Value {
    json!({ "subtype": "generate_session_title", "description": text, "persist": true })
}

/// The dialog kinds the core declared in `initialize`, of which only it answers.
pub const DIALOG_KINDS: [&str; 0] = [];

/// What the window asks of a chat's process (`wire.ts` `ChatControl`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChatControl {
    SetMode { mode: String },
    SetModel { model: String },
    SetThinking { tokens: Option<u64> },
    SetEffort { level: String },
    SetFast { on: bool },
    /// Claude Code's Ultracode (`apply_flag_settings {ultracode}`, from 2.1.284): Claude may run
    /// a workflow of agents for a task, at the session's effort.
    SetUltracode { on: bool },
    SetAgent { agent: String },
    ContextUsage,
    Usage,
    Settings,
    ListModels,
    PermissionRules,
    Hooks,
    McpStatus,
    McpReconnect { server: String },
    McpToggle { server: String, enabled: bool },
    McpAuthenticate { server: String },
    RewindFiles { message: String, dry_run: bool },
    StopTask { task: String },
    BackgroundTasks { tool_use: Option<String> },
    TaskOutput { task: String },
    Rename { title: String },
    CancelQueued { message: String },
    FileSuggestions { query: String },
    ReloadPlugins,
    ReloadSkills,
    ReloadStyles,
    SideQuestion { text: String },
    WorkspaceDiff,
    SuggestionsPaused { paused: bool },
    Status,
    Memory,
    Skills,
    Export,
    Plan,
    SessionCost,
    SetStyle { style: String },
}

impl ChatControl {
    /// Its answer carries the conversation or the project's files (an export, a side question's
    /// answer, a diff, file names, a task's output, the plan): passed on as the CLI gave it, no
    /// email address taken out of it (`wire::scrub_emails`).
    pub fn carries_content(&self) -> bool {
        matches!(
            self,
            Self::Export | Self::SideQuestion { .. } | Self::WorkspaceDiff | Self::FileSuggestions { .. } | Self::TaskOutput { .. } | Self::BackgroundTasks { .. } | Self::Plan | Self::RewindFiles { .. }
        )
    }
}

/// What the process named in its `initialize` reply, that a control may only pick from.
pub struct Known<'a> {
    pub models: &'a [String],
    pub agents: &'a [String],
    pub styles: &'a [String],
}

/// The effort levels a chat may switch to; `auto` clears the flag (Claude Code's own choice).
const EFFORTS: [&str; 6] = ["low", "medium", "high", "xhigh", "max", "auto"];
/// The most thinking tokens a chat asks for.
const THINKING_MAX: u64 = 1_000_000;

fn refused() -> String {
    t!("chatCore.error.refused")
}

/// A UUID as written: a message's or a session's id.
pub fn is_uuid(text: &str) -> bool {
    crate::start::is_uuid(text)
}

/// An id of a task or a tool call: letters, digits, `_` and `-`.
fn is_plain_id(text: &str) -> bool {
    !text.is_empty() && text.len() <= 128 && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A name of an MCP server as the CLI lists it: no control characters, not too long.
fn plain_name(text: &str) -> Result<&str, String> {
    if text.is_empty() || text.chars().count() > 200 || text.chars().any(char::is_control) {
        return Err(refused());
    }
    Ok(text)
}

/// The body of the request `control` stands for, checked: a model, an agent and a style only of
/// those the process listed (`known`), a mode only of the chat's (never `bypassPermissions`), an
/// effort only of the list, ids only as ids; and a rewind that changes files only after a dry
/// run of the same message (`dry_runs`).
pub fn request_body(control: &ChatControl, known: &Known, dry_runs: &HashSet<String>) -> Result<Value, String> {
    let body = match control {
        ChatControl::SetMode { mode } => {
            let mode = ChatMode::parse(mode).filter(|_| mode != "normal").ok_or_else(refused)?;
            json!({ "subtype": "set_permission_mode", "mode": mode.word() })
        }
        ChatControl::SetModel { model } => {
            if model != "default" && !known.models.contains(model) {
                return Err(refused());
            }
            json!({ "subtype": "set_model", "model": model })
        }
        ChatControl::SetThinking { tokens } => {
            if tokens.is_some_and(|tokens| tokens > THINKING_MAX) {
                return Err(refused());
            }
            json!({ "subtype": "set_max_thinking_tokens", "max_thinking_tokens": tokens })
        }
        ChatControl::SetEffort { level } => {
            if !EFFORTS.contains(&level.as_str()) {
                return Err(refused());
            }
            let level = if level == "auto" { Value::Null } else { Value::String(level.clone()) };
            json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": level } })
        }
        ChatControl::SetFast { on } => json!({ "subtype": "apply_flag_settings", "settings": { "fastMode": on } }),
        ChatControl::SetUltracode { on } => json!({ "subtype": "apply_flag_settings", "settings": { "ultracode": on } }),
        ChatControl::SetAgent { agent } => {
            if !known.agents.contains(agent) {
                return Err(refused());
            }
            json!({ "subtype": "apply_flag_settings", "settings": { "agent": agent } })
        }
        ChatControl::SetStyle { style } => {
            if !known.styles.contains(style) {
                return Err(refused());
            }
            json!({ "subtype": "apply_flag_settings", "settings": { "outputStyle": style } })
        }
        ChatControl::ContextUsage => json!({ "subtype": "get_context_usage" }),
        ChatControl::Usage => json!({ "subtype": "get_usage" }),
        ChatControl::Settings => json!({ "subtype": "get_settings" }),
        ChatControl::ListModels => json!({ "subtype": "list_models" }),
        ChatControl::PermissionRules => json!({ "subtype": "list_permission_rules" }),
        ChatControl::Hooks => json!({ "subtype": "get_hooks_listing" }),
        ChatControl::McpStatus => json!({ "subtype": "mcp_status" }),
        ChatControl::McpReconnect { server } => json!({ "subtype": "mcp_reconnect", "serverName": plain_name(server)? }),
        ChatControl::McpToggle { server, enabled } => json!({ "subtype": "mcp_toggle", "serverName": plain_name(server)?, "enabled": enabled }),
        ChatControl::McpAuthenticate { server } => json!({ "subtype": "mcp_authenticate", "serverName": plain_name(server)? }),
        ChatControl::RewindFiles { message, dry_run } => {
            if !is_uuid(message) {
                return Err(refused());
            }
            if !dry_run && !dry_runs.contains(message) {
                return Err(t!("chatCore.error.rewindUnchecked"));
            }
            json!({ "subtype": "rewind_files", "user_message_id": message, "dry_run": dry_run })
        }
        ChatControl::StopTask { task } => {
            if !is_plain_id(task) {
                return Err(refused());
            }
            json!({ "subtype": "stop_task", "task_id": task })
        }
        ChatControl::BackgroundTasks { tool_use } => match tool_use {
            Some(id) if !is_plain_id(id) => return Err(refused()),
            Some(id) => json!({ "subtype": "background_tasks", "tool_use_id": id }),
            None => json!({ "subtype": "background_tasks" }),
        },
        ChatControl::TaskOutput { task } => {
            if !is_plain_id(task) {
                return Err(refused());
            }
            json!({ "subtype": "get_task_output", "task_id": task })
        }
        ChatControl::Rename { title } => {
            let title: String = title.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
            let title: String = title.trim().chars().take(200).collect();
            if title.is_empty() {
                return Err(refused());
            }
            json!({ "subtype": "rename_session", "title": title })
        }
        ChatControl::CancelQueued { message } => {
            if !is_uuid(message) {
                return Err(refused());
            }
            json!({ "subtype": "cancel_async_message", "message_uuid": message })
        }
        ChatControl::FileSuggestions { query } => {
            if query.chars().count() > 1_000 {
                return Err(refused());
            }
            json!({ "subtype": "file_suggestions", "query": query })
        }
        ChatControl::ReloadPlugins => json!({ "subtype": "reload_plugins" }),
        ChatControl::ReloadSkills => json!({ "subtype": "reload_skills" }),
        ChatControl::ReloadStyles => json!({ "subtype": "reload_output_styles" }),
        ChatControl::SideQuestion { text } => {
            if text.trim().is_empty() || text.chars().count() > super::wire::TEXT_MAX {
                return Err(refused());
            }
            json!({ "subtype": "side_question", "question": text })
        }
        ChatControl::WorkspaceDiff => json!({ "subtype": "get_workspace_diff" }),
        ChatControl::SuggestionsPaused { paused } => json!({ "subtype": "set_prompt_suggestions_paused", "paused": paused }),
        ChatControl::Status => json!({ "subtype": "get_status" }),
        ChatControl::Memory => json!({ "subtype": "get_memory_dialog" }),
        ChatControl::Skills => json!({ "subtype": "get_skills_dialog" }),
        ChatControl::Export => json!({ "subtype": "export_conversation" }),
        ChatControl::Plan => json!({ "subtype": "get_plan" }),
        ChatControl::SessionCost => json!({ "subtype": "get_session_cost" }),
    };
    Ok(body)
}

/// What the user answered to a card (`wire.ts` `ChatAnswer`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatAnswer {
    Allow {
        #[serde(default)]
        input: Option<Map<String, Value>>,
        #[serde(default)]
        always: Option<Always>,
        #[serde(default)]
        mode: Option<String>,
    },
    Deny {
        #[serde(default)]
        message: String,
        #[serde(default)]
        interrupt: bool,
    },
    Elicitation {
        action: String,
        #[serde(default)]
        content: Option<Map<String, Value>>,
    },
    Dialog {
        #[serde(default)]
        response: Value,
    },
}

/// "Always": which of the request's own suggestions, to where.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Always {
    pub suggestions: Vec<usize>,
    pub destination: String,
}

/// Where "always" may write a rule: this project for the user, the project shared, every
/// project, or this session.
const DESTINATIONS: [&str; 4] = ["localSettings", "projectSettings", "userSettings", "session"];
/// The suggestions whose place the user picks.
const PLACED: [&str; 3] = ["addRules", "addDirectories", "setMode"];

/// The fields of a tool's input the user may edit in its card; the rest is the request's own.
fn editable(tool: &str) -> &'static [&'static str] {
    match tool {
        "Edit" => &["new_string"],
        "MultiEdit" => &["edits"],
        "Write" => &["content"],
        "NotebookEdit" => &["new_source"],
        "AskUserQuestion" => &["answers", "annotations", "response"],
        "ExitPlanMode" => &["plan"],
        _ => &[],
    }
}

/// The input the tool runs with: the request's own, with only the fields of `editable` the user
/// changed laid over it (of MultiEdit's edits, each one's `new_string` alone). Any other field,
/// or any field of another tool, refuses the answer.
fn overlaid(tool: &str, original: &Value, edited: Option<&Map<String, Value>>) -> Result<Value, String> {
    let mut input = original.clone();
    let Some(edited) = edited.filter(|edited| !edited.is_empty()) else {
        return Ok(input);
    };
    let allowed = editable(tool);
    let Some(fields) = input.as_object_mut() else {
        return Err(refused());
    };
    for (key, value) in edited {
        if !allowed.contains(&key.as_str()) {
            return Err(refused());
        }
        match (tool, key.as_str()) {
            ("MultiEdit", "edits") => {
                let (Some(theirs), Some(ours)) = (value.as_array(), fields.get_mut("edits").and_then(Value::as_array_mut)) else {
                    return Err(refused());
                };
                if theirs.len() != ours.len() {
                    return Err(refused());
                }
                for (edit, own) in theirs.iter().zip(ours.iter_mut()) {
                    let Some(text) = edit.get("new_string").and_then(Value::as_str) else {
                        return Err(refused());
                    };
                    if let Some(own) = own.as_object_mut() {
                        own.insert("new_string".into(), Value::String(text.to_string()));
                    }
                }
            }
            ("AskUserQuestion", "answers" | "annotations") if !value.is_object() => return Err(refused()),
            (_, "answers" | "annotations") => {
                fields.insert(key.clone(), value.clone());
            }
            _ if !value.is_string() => return Err(refused()),
            _ => {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(input)
}

/// The `response` of the core's answer to the CLI's request `request` (its body, as the CLI sent
/// it): built from the user's `answer` on that request alone. An allow carries the whole input
/// (`overlaid`), "always" only copies of the request's own `permission_suggestions`, each placed
/// where the user picked (a mode only in this session), and a mode to go on in as one `setMode`
/// more; a deny the user's words.
/// An answer of the wrong kind for the request refuses.
pub fn answer_body(request: &Value, answer: &ChatAnswer) -> Result<Value, String> {
    let subtype = request.get("subtype").and_then(Value::as_str).unwrap_or_default();
    match (subtype, answer) {
        ("can_use_tool", ChatAnswer::Allow { input, always, mode }) => {
            let tool = request.get("tool_name").and_then(Value::as_str).unwrap_or_default();
            let original = request.get("input").cloned().unwrap_or_else(|| json!({}));
            let updated_input = overlaid(tool, &original, input.as_ref())?;
            let mut updates: Vec<Value> = Vec::new();
            if let Some(always) = always {
                if request.get("suppress_always_allow_rule").and_then(Value::as_bool) == Some(true) || !DESTINATIONS.contains(&always.destination.as_str()) {
                    return Err(refused());
                }
                let suggested = request.get("permission_suggestions").and_then(Value::as_array).cloned().unwrap_or_default();
                for &index in &always.suggestions {
                    let mut entry = suggested.get(index).cloned().ok_or_else(refused)?;
                    let kind = entry.get("type").and_then(Value::as_str);
                    // A mode kept in a settings file would be that file's default mode: a mode
                    // goes to this session alone.
                    if kind == Some("setMode") && always.destination != "session" {
                        return Err(refused());
                    }
                    let placed = kind.is_some_and(|kind| PLACED.contains(&kind));
                    if placed {
                        if let Some(fields) = entry.as_object_mut() {
                            fields.insert("destination".into(), Value::String(always.destination.clone()));
                        }
                    }
                    if !updates.contains(&entry) {
                        updates.push(entry);
                    }
                }
            }
            if let Some(mode) = mode {
                let mode = ChatMode::parse(mode).filter(|_| mode != "normal").ok_or_else(refused)?;
                updates.push(json!({ "type": "setMode", "mode": mode.word(), "destination": "session" }));
            }
            let mut body = json!({ "behavior": "allow", "updatedInput": updated_input });
            if !updates.is_empty() {
                body["updatedPermissions"] = Value::Array(updates);
            }
            Ok(body)
        }
        ("can_use_tool", ChatAnswer::Deny { message, interrupt }) => {
            let message = if message.trim().is_empty() { t!("chatCore.denied") } else { message.chars().take(super::wire::TEXT_MAX).collect() };
            Ok(json!({ "behavior": "deny", "message": message, "interrupt": interrupt }))
        }
        ("elicitation", ChatAnswer::Elicitation { action, content }) => {
            if !matches!(action.as_str(), "accept" | "decline" | "cancel") {
                return Err(refused());
            }
            let mut body = json!({ "action": action });
            if let (Some(content), "accept") = (content, action.as_str()) {
                body["content"] = Value::Object(content.clone());
            }
            Ok(body)
        }
        ("request_user_dialog", ChatAnswer::Dialog { response }) => {
            let kind = request.get("dialog_kind").and_then(Value::as_str).unwrap_or_default();
            if !DIALOG_KINDS.contains(&kind) {
                return Err(refused());
            }
            Ok(response.clone())
        }
        _ => Err(refused()),
    }
}

/// The line that answers the CLI's request `id` with `response`.
pub fn answer_line(id: &str, response: Value) -> Value {
    json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } })
}

/// The line that answers the CLI's request `id` with an error: a request Pitwall serves nothing
/// for (`hook_callback`, `mcp_message`, a subtype it doesn't know).
pub fn error_line(id: &str, error: &str) -> Value {
    json!({ "type": "control_response", "response": { "subtype": "error", "request_id": id, "error": error } })
}

/// The requests of the CLI's a card answers (the rest get `error_line` at once).
pub fn is_card(subtype: &str) -> bool {
    matches!(subtype, "can_use_tool" | "elicitation" | "request_user_dialog")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGE: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    /// Tests 4 and 8: what the window asks of a process is one of the fixed controls (a kind not
    /// among them doesn't parse), each building only its listed subtype; a model, an agent, a
    /// style only of those the process listed, no `bypassPermissions`, and a rewind that changes
    /// files only after a dry run of the same message.
    #[test]
    fn a_control_names_only_what_the_process_listed() {
        let (models, agents, styles) = (vec!["haiku".to_string()], vec!["reviewer".to_string()], vec!["Explanatory".to_string()]);
        let known = Known { models: &models, agents: &agents, styles: &styles };
        let checked: HashSet<String> = [MESSAGE.to_string()].into();
        let body = |control: serde_json::Value, dry_runs: &HashSet<String>| -> Result<String, String> {
            let control: ChatControl = serde_json::from_value(control).map_err(|error| error.to_string())?;
            request_body(&control, &known, dry_runs).map(|body| body["subtype"].as_str().unwrap().to_string())
        };
        let none = HashSet::new();

        for kind in ["initialize", "hookCallback", "setPermissionMode", "canUseTool"] {
            assert!(body(json!({ "kind": kind }), &none).is_err(), "{kind} parsed");
        }
        for (control, subtype) in [
            (json!({ "kind": "setMode", "mode": "acceptEdits" }), "set_permission_mode"),
            (json!({ "kind": "setModel", "model": "haiku" }), "set_model"),
            (json!({ "kind": "setModel", "model": "default" }), "set_model"),
            (json!({ "kind": "setAgent", "agent": "reviewer" }), "apply_flag_settings"),
            (json!({ "kind": "setStyle", "style": "Explanatory" }), "apply_flag_settings"),
            (json!({ "kind": "setEffort", "level": "auto" }), "apply_flag_settings"),
            (json!({ "kind": "rewindFiles", "message": MESSAGE, "dryRun": true }), "rewind_files"),
            (json!({ "kind": "status" }), "get_status"),
        ] {
            assert_eq!(body(control.clone(), &none).as_deref(), Ok(subtype), "{control}");
        }
        for control in [
            json!({ "kind": "setMode", "mode": "bypassPermissions" }),
            json!({ "kind": "setModel", "model": "opus" }),
            json!({ "kind": "setAgent", "agent": "someone" }),
            json!({ "kind": "setStyle", "style": "Pirate" }),
            json!({ "kind": "setEffort", "level": "turbo" }),
            json!({ "kind": "stopTask", "task": "../x" }),
            json!({ "kind": "rewindFiles", "message": "not-a-uuid", "dryRun": true }),
            json!({ "kind": "rewindFiles", "message": MESSAGE, "dryRun": false }),
        ] {
            assert!(body(control.clone(), &none).is_err(), "{control} was sent");
        }
        assert_eq!(body(json!({ "kind": "rewindFiles", "message": MESSAGE, "dryRun": false }), &checked).as_deref(), Ok("rewind_files"));
    }

    /// Part of Test 3: an answer is built on the CLI's own request: the input runs as the request
    /// had it but for the fields the user may edit in its card ("Yes" never runs another command),
    /// "always" names only suggestions the request made, never where it was refused, and only to
    /// one of the places listed, a mode only to this session. A deny without words still goes
    /// (Skip, "keep planning").
    #[test]
    fn an_answer_is_built_on_the_request_alone() {
        let request = json!({
            "subtype": "can_use_tool",
            "tool_name": "Bash",
            "input": { "command": "ls" },
            "permission_suggestions": [{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "ls:*" }], "behavior": "allow", "destination": "localSettings" }],
        });
        let allow = |input: Option<Value>, always: Option<(Vec<usize>, &str)>| ChatAnswer::Allow {
            input: input.and_then(|input| input.as_object().cloned()),
            always: always.map(|(suggestions, destination)| Always { suggestions, destination: destination.into() }),
            mode: None,
        };
        assert!(answer_body(&request, &allow(Some(json!({ "command": "rm -rf /" })), None)).is_err());
        assert!(answer_body(&request, &allow(None, Some((vec![1], "userSettings")))).is_err());
        assert!(answer_body(&request, &allow(None, Some((vec![0], "policySettings")))).is_err());
        let mut suppressed = request.clone();
        suppressed["suppress_always_allow_rule"] = Value::Bool(true);
        assert!(answer_body(&suppressed, &allow(None, Some((vec![0], "session")))).is_err());
        // A suggested mode goes to this session alone: in a settings file it would be its default.
        let write = json!({
            "subtype": "can_use_tool",
            "tool_name": "Write",
            "input": { "file_path": "/p/a", "content": "a" },
            "permission_suggestions": [{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }],
        });
        for place in ["localSettings", "projectSettings", "userSettings"] {
            assert!(answer_body(&write, &allow(None, Some((vec![0], place)))).is_err(), "a mode went to {place}");
        }
        let body = answer_body(&write, &allow(None, Some((vec![0], "session")))).unwrap();
        assert_eq!(body["updatedPermissions"], json!([{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }]));

        let body = answer_body(&request, &allow(None, None)).unwrap();
        assert_eq!(body, json!({ "behavior": "allow", "updatedInput": { "command": "ls" } }));
        let edit = json!({ "subtype": "can_use_tool", "tool_name": "Edit", "input": { "file_path": "/p/a", "old_string": "a", "new_string": "b" } });
        let body = answer_body(&edit, &allow(Some(json!({ "new_string": "c" })), None)).unwrap();
        assert_eq!(body["updatedInput"], json!({ "file_path": "/p/a", "old_string": "a", "new_string": "c" }));
        assert!(answer_body(&edit, &allow(Some(json!({ "file_path": "/etc/hosts" })), None)).is_err());

        let skip = answer_body(&request, &ChatAnswer::Deny { message: String::new(), interrupt: false }).unwrap();
        assert_eq!(skip["behavior"], "deny");
        assert!(!skip["message"].as_str().unwrap().is_empty());
        assert!(answer_body(&request, &ChatAnswer::Elicitation { action: "accept".into(), content: None }).is_err());
    }
}
