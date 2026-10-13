//! The phone's channel: a signal the user asked to be told about away from the Mac is sent on,
//! through ntfy, a Telegram bot or a webhook of their own.
//!
//! Nothing else in Pitwall sends anything about the user's work anywhere, so what leaves is
//! made in one place (`Message`): the project's name, what happened in the UI's words and, for
//! a permission, the tool's name. Never the project's path, a command or its name, a card's
//! title, a session's name, or anything Claude or the user wrote.
//!
//! The channel and its address are kept in `phone.json` beside the settings, readable by the
//! user alone and part of no backup; the Telegram token never goes to the UI. Nothing is sent
//! until the user turned the channel on, and then only the events ticked, at the moment chosen
//! (`When`), each project's event once a minute at most. A send is one request on a thread of
//! its own, never tried again; why it failed is kept for Settings and nothing is logged.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use ureq::http::{HeaderValue, Uri};

use super::restore::write_private;
use super::signals::{Signal, SignalKind};
use super::*;
use crate::presence::{self, Presence};

const FILE: &str = "phone.json";
/// The public ntfy server.
const NTFY: &str = "https://ntfy.sh";
const TELEGRAM: &str = "https://api.telegram.org";
/// Who asks, for a channel that wants to know: the app's name, and no version with it.
const USER_AGENT: &str = "Pitwall";
/// A request has this long, all of it.
const TIMEOUT: Duration = Duration::from_secs(10);
/// A project's event goes out once in this time at most.
const REST: Duration = Duration::from_secs(60);
/// Without input for this long, the user counts as away (`When::Idle`).
const AWAY: Duration = Duration::from_secs(5 * 60);
/// A name goes out cut to this many characters.
const NAME_LENGTH: usize = 80;
/// As much of a refusal is read as could say why.
const REPLY_BYTES: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Channel {
    #[default]
    Ntfy,
    Telegram,
    Webhook,
}

/// The moment a signal may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum When {
    /// While the screen is locked.
    Locked,
    /// After `AWAY` without input, or while the screen is locked.
    Idle,
    Always,
}

impl When {
    /// The moments this system can tell, in the order Settings offers them.
    fn offered() -> Vec<When> {
        let mut moments = Vec::new();
        if presence::TELLS_LOCKED {
            moments.push(When::Locked);
        }
        if presence::TELLS_IDLE {
            moments.push(When::Idle);
        }
        moments.push(When::Always);
        moments
    }

    /// Whether this is the moment, with the user's presence as `look` tells it. What the system
    /// can't tell isn't the moment: nothing goes on a guess.
    fn is_now(self, look: impl FnOnce() -> Presence) -> bool {
        match self {
            When::Always => true,
            When::Locked => look().locked == Some(true),
            When::Idle => {
                let seen = look();
                seen.locked == Some(true) || seen.idle.is_some_and(|idle| idle >= AWAY)
            }
        }
    }
}

/// The events offered, each with its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Events {
    asked: bool,
    permission: bool,
    finished: bool,
    crashed: bool,
    fuel: bool,
    card: bool,
}

impl Default for Events {
    fn default() -> Self {
        Self { asked: true, permission: true, finished: false, crashed: true, fuel: true, card: false }
    }
}

/// One of the events offered: what a webhook is told it is, the key of its text, and whether
/// its box is ticked.
struct Offer {
    event: &'static str,
    text: &'static str,
    ticked: bool,
}

/// A kind of signal that isn't named here has no box and never leaves.
fn offer(kind: SignalKind, events: &Events) -> Option<Offer> {
    let (event, text, ticked) = match kind {
        SignalKind::ClaudeAsking => ("claude-asking", "phone.event.asked", events.asked),
        SignalKind::ClaudePermission => ("claude-permission", "phone.event.permission", events.permission),
        SignalKind::ClaudeFinished => ("claude-finished", "phone.event.finished", events.finished),
        SignalKind::ServerCrashed => ("server-crashed", "phone.event.crashed", events.crashed),
        SignalKind::FuelHigh => ("fuel-high", "phone.event.fuel", events.fuel),
        SignalKind::CardReview => ("card-review", "phone.event.card", events.card),
        _ => return None,
    };
    Some(Offer { event, text, ticked })
}

/// The channel as `phone.json` keeps it, and as Settings' form sends it (there an empty
/// `telegram_token` stands for the one stored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneConfig {
    on: bool,
    channel: Channel,
    /// Each channel keeps its address while another is picked.
    ntfy_server: String,
    ntfy_topic: String,
    telegram_token: String,
    telegram_chat: String,
    webhook_url: String,
    when: When,
    events: Events,
}

impl Default for PhoneConfig {
    fn default() -> Self {
        Self {
            on: false,
            channel: Channel::default(),
            ntfy_server: NTFY.into(),
            ntfy_topic: String::new(),
            telegram_token: String::new(),
            telegram_chat: String::new(),
            webhook_url: String::new(),
            when: When::offered()[0],
            events: Events::default(),
        }
    }
}

impl PhoneConfig {
    /// As it is kept: trimmed, the ntfy server with its scheme and no slash at its end (the
    /// public one when none is named), and a moment this system can tell.
    fn tidy(mut self) -> Self {
        for field in [&mut self.ntfy_server, &mut self.ntfy_topic, &mut self.telegram_token, &mut self.telegram_chat, &mut self.webhook_url] {
            *field = field.trim().to_string();
        }
        self.ntfy_server = match self.ntfy_server.trim_end_matches('/') {
            "" => NTFY.to_string(),
            server if server.contains("://") => server.to_string(),
            server => format!("https://{server}"),
        };
        if !When::offered().contains(&self.when) {
            self.when = When::offered()[0];
        }
        self
    }

    /// Where a message goes; `Failure::Address` while the channel's address is missing or is
    /// none. `telegram` is Telegram's own address.
    fn target(&self, telegram: &str) -> Result<Target, Failure> {
        // Letters, digits and these: nothing that would lead the address elsewhere.
        let word = |text: &str, also: &[char]| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || also.contains(&c));

        let target = match self.channel {
            Channel::Ntfy => {
                let named = word(&self.ntfy_topic, &['-', '_']) && self.ntfy_topic.len() <= 64;
                // The topic is the address's last part: a server's address ends where its path does.
                let server = is_web(&self.ntfy_server) && !self.ntfy_server.contains(['?', '#']);
                (named && server).then(|| Target::Ntfy(format!("{}/{}", self.ntfy_server, self.ntfy_topic)))
            }
            Channel::Telegram => (word(&self.telegram_token, &[':', '-', '_']) && !self.telegram_chat.is_empty())
                .then(|| Target::Telegram { url: format!("{telegram}/bot{}/sendMessage", self.telegram_token), chat: self.telegram_chat.clone() }),
            Channel::Webhook => is_web(&self.webhook_url).then(|| Target::Webhook(self.webhook_url.clone())),
        };
        target.ok_or(Failure::Address)
    }
}

/// A web address with a host: `http` or `https`, nothing else.
fn is_web(url: &str) -> bool {
    url.parse::<Uri>().is_ok_and(|uri| matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some_and(|host| !host.is_empty()))
}

/// Where a message goes: the channel's address, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Ntfy(String),
    /// The address holds the token.
    Telegram {
        url: String,
        chat: String,
    },
    Webhook(String),
}

impl Target {
    /// The part of the address that is a secret, as `target` put it there: Telegram's token,
    /// ntfy's topic.
    fn secret(&self) -> Option<&str> {
        match self {
            Target::Ntfy(url) => url.rsplit('/').next(),
            Target::Telegram { url, .. } => url.rsplit("/bot").next().and_then(|rest| rest.strip_suffix("/sendMessage")),
            Target::Webhook(_) => None,
        }
    }
}

/// Why a message didn't arrive. It names no address: Telegram's holds the token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    /// The channel is off.
    Off,
    Address,
    Network,
    Timeout,
    /// The channel answered with this status and, when its reply says so, why.
    Refused(u16, Option<String>),
}

impl Failure {
    fn text(&self) -> String {
        match self {
            Failure::Off => t!("core.phone.error.off"),
            Failure::Address => t!("core.phone.error.address"),
            Failure::Network => t!("core.phone.error.network"),
            Failure::Timeout => t!("core.phone.error.timeout"),
            Failure::Refused(status, None) => t!("core.phone.error.refused", status = status),
            Failure::Refused(status, Some(reason)) => t!("core.phone.error.refusedWhy", status = status, reason = reason),
        }
    }
}

/// What goes out about one thing that happened, and all that does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Message {
    /// The project's name; none when it is about no project (the fuel, a test).
    project: Option<String>,
    /// What happened, as a webhook is told.
    event: &'static str,
    /// A permission's tool.
    tool: Option<String>,
    /// What happened in the UI's language, the tool in brackets.
    what: String,
    /// The app's name, which stands where a project's would when there is none.
    app: String,
}

impl Message {
    /// Of a signal only these are taken: its project's name, its kind and, of a permission, the
    /// tool. Its path stays here, and so does any other kind's `detail` (a command's name).
    fn of(signal: &Signal, offer: &Offer, app: &str) -> Self {
        let tool = signal.detail.as_deref().filter(|_| signal.kind == SignalKind::ClaudePermission).map(plain).filter(|tool| !tool.is_empty());
        let what = match &tool {
            Some(tool) => format!("{} ({tool})", t!(offer.text)),
            None => t!(offer.text),
        };
        let project = Some(plain(&signal.name)).filter(|name| !name.is_empty());
        Self { project, event: offer.event, tool, what, app: app.to_string() }
    }

    /// "Send a test" in Settings.
    fn test(app: &str) -> Self {
        Self { project: None, event: "test", tool: None, what: t!("core.phone.test"), app: app.to_string() }
    }

    /// Whose it is: the project's name, or the app's.
    fn title(&self) -> &str {
        self.project.as_deref().unwrap_or(&self.app)
    }

    /// The one line: `paddock — Claude needs permission (Bash)`.
    fn text(&self) -> String {
        format!("{} — {}", self.title(), self.what)
    }
}

/// A name as it goes out: one line, cut to what a notification shows. What would end the line
/// where it is shown, or turn it round, goes with the control characters: Unicode's line and
/// paragraph separators, and the marks that set a text's direction.
fn plain(name: &str) -> String {
    let stays = |c: &char| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let line: String = name.chars().filter(stays).take(NAME_LENGTH).collect();
    line.trim().to_string()
}

/// A title as a header's value: as it is when it is plain ASCII, else as an encoded word
/// (RFC 2047), which ntfy reads.
fn header(title: &str) -> HeaderValue {
    let encoded = || HeaderValue::from_str(&format!("=?UTF-8?B?{}?=", BASE64.encode(title))).unwrap_or(HeaderValue::from_static(""));
    HeaderValue::from_str(title).ok().filter(|_| title.is_ascii()).unwrap_or_else(encoded)
}

/// One request to the channel, waited for and never made again. ntfy gets the project's name as
/// its title and what happened as its text; Telegram the one line; a webhook each part by name.
fn send(target: &Target, message: &Message) -> Result<(), Failure> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

    // The system's TLS and trust store, as for Claude's limits (`usage.rs`). A redirect isn't
    // followed, it is an answer like any other: the message goes to the address the user gave
    // or nowhere.
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .user_agent(USER_AGENT)
        .build()
        .into();

    let answer = match target {
        Target::Ntfy(url) => {
            let request = agent.post(url).header("title", header(message.title())).header("content-type", "text/plain; charset=utf-8");
            request.send(message.what.as_str())
        }
        Target::Telegram { url, chat } => {
            // No preview: for one, Telegram would open a project named like a site.
            let body = json!({ "chat_id": chat, "text": message.text(), "link_preview_options": { "is_disabled": true } });
            agent.post(url).header("content-type", "application/json").send(body.to_string())
        }
        Target::Webhook(url) => {
            let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            let body = json!({ "project": message.project, "event": message.event, "tool": message.tool, "text": message.text(), "at": at });
            agent.post(url).header("content-type", "application/json").send(body.to_string())
        }
    };

    let mut answer = match answer {
        Ok(answer) => answer,
        Err(ureq::Error::Timeout(_)) => return Err(Failure::Timeout),
        Err(ureq::Error::BadUri(_) | ureq::Error::Http(_)) => return Err(Failure::Address),
        Err(_) => return Err(Failure::Network),
    };
    if answer.status().is_success() {
        return Ok(());
    }
    let why = answer.body_mut().with_config().limit(REPLY_BYTES).read_to_string().ok().and_then(|reply| reason(&reply));
    // A reply that says the address back would put its secret into Settings: the status alone then.
    let why = why.filter(|why| target.secret().is_none_or(|secret| !why.contains(secret)));
    Err(Failure::Refused(answer.status().as_u16(), why))
}

/// Why the channel refused, as its reply says: Telegram's `description`, ntfy's `error`.
fn reason(reply: &str) -> Option<String> {
    let reply: Value = serde_json::from_str(reply).ok()?;
    let why = ["description", "error"].iter().find_map(|field| reply.get(field)?.as_str())?;
    Some(plain(why)).filter(|why| !why.is_empty())
}

/// A topic nobody can guess: on a public ntfy server its name is all that keeps others from
/// reading what goes to it. `pitwall-` and 80 bits of the system's randomness, in base 32.
fn fresh_topic() -> String {
    const LETTERS: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

    let mut topic = String::from("pitwall-");
    // Five bytes are eight letters of five bits each.
    for half in random().chunks(5) {
        let bits = half.iter().fold(0u64, |bits, byte| bits << 8 | u64::from(*byte));
        for letter in (0..8).rev() {
            topic.push(char::from(LETTERS[(bits >> (letter * 5) & 31) as usize]));
        }
    }
    topic
}

/// Ten bytes nobody can guess: from the system's generator, and where that is no file, from
/// the keys std draws from it for its hashers.
fn random() -> [u8; 10] {
    use std::hash::BuildHasher;

    let mut bytes = [0u8; 10];

    #[cfg(unix)]
    {
        if fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut bytes)).is_ok() {
            return bytes;
        }
    }

    for (at, part) in bytes.chunks_mut(8).enumerate() {
        let drawn = std::collections::hash_map::RandomState::new().hash_one((at, std::process::id(), now_ms())).to_le_bytes();
        part.copy_from_slice(&drawn[..part.len()]);
    }
    bytes
}

/// The channel as Settings shows it. It has no field for the Telegram token: only whether one
/// is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    on: bool,
    channel: Channel,
    ntfy_server: String,
    ntfy_topic: String,
    telegram_token: bool,
    telegram_chat: String,
    webhook_url: String,
    when: When,
    events: Events,
    /// The moments this system can tell, in the order they are offered.
    moments: Vec<When>,
    /// What a permission wait in the first listed project would send, word for word.
    sample: String,
    /// Why the last message didn't arrive; none once one did.
    failure: Option<String>,
}

struct State {
    config: PhoneConfig,
    /// Telegram's address. A test puts its own listener's here.
    telegram: String,
    /// When each project's event last went to the channel.
    sent: HashMap<(String, SignalKind), Instant>,
    /// Why the last message didn't arrive; none once one did.
    failure: Option<Failure>,
}

impl State {
    /// A file that isn't there or doesn't parse is a channel that is off.
    fn load(file: &Path) -> Self {
        let config: PhoneConfig = fs::read_to_string(file).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default();
        Self { config: config.tidy(), telegram: TELEGRAM.into(), sent: HashMap::new(), failure: None }
    }
}

/// The channel as the core holds it: apart from `inner`, so a signal never waits on that lock,
/// and shared with the thread a send runs on. Read from its file the first time it is needed.
#[derive(Default)]
pub(super) struct Phone(Arc<Mutex<Option<State>>>);

fn locked(phone: &Mutex<Option<State>>) -> MutexGuard<'_, Option<State>> {
    phone.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Core {
    fn phone_file(&self) -> PathBuf {
        self.cfg.settings_file.with_file_name(FILE)
    }

    fn with_phone<T>(&self, run: impl FnOnce(&mut State) -> T) -> T {
        run(locked(&self.phone.0).get_or_insert_with(|| State::load(&self.phone_file())))
    }

    /// Hands `signal` to the phone's channel when the user asked for that; true when it was
    /// handed over. Returns at once: the send is on its own thread.
    pub(super) fn relay(&self, signal: &Signal) -> bool {
        self.relay_seen(signal, presence::now)
    }

    /// `relay`, with the user's presence as `look` tells it.
    fn relay_seen(&self, signal: &Signal, look: impl FnOnce() -> Presence) -> bool {
        let handed = self.with_phone(|phone| {
            let config = &phone.config;
            let offer = offer(signal.kind, &config.events).filter(|offer| config.on && offer.ticked)?;

            let now = Instant::now();
            let key = (signal.path.clone(), signal.kind);
            if phone.sent.get(&key).is_some_and(|last| now.duration_since(*last) < REST) || !config.when.is_now(look) {
                return None;
            }
            let target = match config.target(&phone.telegram) {
                Ok(target) => target,
                Err(failure) => {
                    phone.failure = Some(failure);
                    return None;
                }
            };

            phone.sent.retain(|_, last| now.duration_since(*last) < REST);
            phone.sent.insert(key, now);
            Some((target, Message::of(signal, &offer, &self.cfg.title)))
        });
        let Some((target, message)) = handed else {
            return false;
        };

        let phone = Arc::clone(&self.phone.0);
        thread::spawn(move || {
            let failure = send(&target, &message).err();
            if let Some(phone) = locked(&phone).as_mut() {
                phone.failure = failure;
            }
        });
        true
    }

    /// The channel as Settings shows it.
    pub fn phone_view(&self) -> PhoneView {
        let name = self.snapshot().projects.into_iter().next().map(|project| project.name).unwrap_or_default();
        let waits = Signal { kind: SignalKind::ClaudePermission, path: String::new(), name, detail: Some("Bash".into()) };

        self.with_phone(|phone| {
            let config = &phone.config;
            let sample = offer(waits.kind, &config.events).map(|offer| Message::of(&waits, &offer, &self.cfg.title).text()).unwrap_or_default();
            PhoneView {
                on: config.on,
                channel: config.channel,
                ntfy_server: config.ntfy_server.clone(),
                ntfy_topic: config.ntfy_topic.clone(),
                telegram_token: !config.telegram_token.is_empty(),
                telegram_chat: config.telegram_chat.clone(),
                webhook_url: config.webhook_url.clone(),
                when: config.when,
                events: config.events,
                moments: When::offered(),
                sample,
                failure: phone.failure.as_ref().map(Failure::text),
            }
        })
    }

    /// Settings' form takes hold and is written, readable by the user alone. An empty Telegram
    /// token keeps the one stored; turned on for ntfy without a topic, it gets a fresh one.
    pub fn phone_set(&self, form: PhoneConfig) -> Result<PhoneView, String> {
        let file = self.phone_file();
        let written = self.with_phone(|phone| {
            let mut config = form.tidy();
            if config.telegram_token.is_empty() {
                config.telegram_token = phone.config.telegram_token.clone();
            }
            if config.on && config.channel == Channel::Ntfy && config.ntfy_topic.is_empty() {
                config.ntfy_topic = fresh_topic();
            }
            if config == phone.config {
                return Ok(());
            }

            // Another address, or off: what the last message failed on says nothing of it.
            if !config.on || config.target(&phone.telegram) != phone.config.target(&phone.telegram) {
                phone.failure = None;
            }
            phone.config = config;
            let json = serde_json::to_string_pretty(&phone.config).map_err(std::io::Error::other)?;
            write_private(&file, &json)
        });

        written.map_err(|error| t!("core.phone.error.save", error = error))?;
        Ok(self.phone_view())
    }

    /// "Send a test": one message through the channel as it is set, waited for. The error's
    /// text when it didn't arrive. Nothing goes while the channel is off.
    pub fn phone_test(&self) -> Result<(), String> {
        let Some(target) = self.with_phone(|phone| phone.config.on.then(|| phone.config.target(&phone.telegram))) else {
            return Err(Failure::Off.text());
        };

        let sent = target.and_then(|target| send(&target, &Message::test(&self.cfg.title)));
        self.with_phone(|phone| phone.failure = sent.clone().err());
        sent.map_err(|failure| failure.text())
    }

    /// A fresh ntfy topic, for the field in Settings.
    pub fn phone_topic(&self) -> String {
        fresh_topic()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{channel, Receiver};

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);
    /// Long enough for a request that was made to arrive.
    const QUIET: Duration = Duration::from_millis(400);
    const TOKEN: &str = "123456789:AAH-pit_wall-TOKEN";

    fn core_in(tmp: &Path) -> Arc<Core> {
        Core::new(
            CoreConfig {
                registry_dir: tmp.join("registry"),
                claude_dir: tmp.join("claude"),
                settings_file: tmp.join("config").join("settings.json"),
                claude_settings: tmp.join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        )
    }

    /// A listener on 127.0.0.1 that answers every request with `status` and hands over what it
    /// was sent, head and body as they came.
    fn listener(status: u16, reply: &'static str) -> (String, Receiver<String>) {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", socket.local_addr().unwrap());
        let (heard, requests) = channel();

        thread::spawn(move || {
            for stream in socket.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    let read = reader.read_line(&mut line).unwrap_or(0);
                    request.push_str(&line);
                    if read == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                request.push_str(&String::from_utf8_lossy(&body));

                let _ = write!(reader.get_mut(), "HTTP/1.1 {status} Answer\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len());
                if heard.send(request).is_err() {
                    break;
                }
            }
        });
        (address, requests)
    }

    /// A request as the listener heard it: its first line, its head in lower case, its body.
    fn parts(request: &str) -> (String, String, String) {
        let (head, body) = request.split_once("\r\n\r\n").unwrap();
        (head.lines().next().unwrap().to_string(), head.to_ascii_lowercase(), body.to_string())
    }

    /// The promise: of a signal, only its project's name, what happened and a permission's tool
    /// leave the Mac, each as one plain line whatever it holds. Its path and another kind's
    /// `detail` (a command's name) stay, on every channel, and Telegram's token is in the address
    /// it belongs to and nowhere else.
    #[test]
    fn only_the_name_the_event_and_a_permissions_tool_leave() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());
        let (address, requests) = listener(200, "{\"ok\":true}");
        let path = tmp.path().join("clients").join("paddock").to_string_lossy().into_owned();
        core.with_phone(|phone| phone.telegram = address.clone());

        let permission = Signal { kind: SignalKind::ClaudePermission, path: path.clone(), name: "paddock".into(), detail: Some("Bash".into()) };
        // A kind whose `detail` is no tool: were it sent, a command's name would be out. Its
        // name isn't ASCII, which a header can't carry as it is.
        let crashed = Signal { kind: SignalKind::ServerCrashed, path: path.clone(), name: "şehir-rehberi".into(), detail: Some("deploy:production".into()) };
        // A name and a tool's name that try to be more than names: a line's end and a header of
        // their own, a line separator, a direction turned round. Its project is another one.
        let other = tmp.path().join("clients").join("other").to_string_lossy().into_owned();
        let hostile = Signal {
            kind: SignalKind::ClaudePermission,
            path: other,
            name: "pad\r\nX-Injected: yes\u{2028}\u{202e}\0\u{7f}dock".into(),
            detail: Some("Bash\r\nX-Tool: 1\u{2028}\u{202e}".into()),
        };
        let (asks, fell) = (t!("phone.event.permission"), t!("phone.event.crashed"));
        assert!(!asks.contains("phone.") && !fell.contains("phone."), "the texts are in the catalog");

        for channel in [Channel::Ntfy, Channel::Telegram, Channel::Webhook] {
            let config = PhoneConfig {
                on: true,
                channel,
                ntfy_server: address.clone(),
                ntfy_topic: "pitwall-topic".into(),
                telegram_token: TOKEN.into(),
                telegram_chat: "42".into(),
                webhook_url: format!("{address}/hook"),
                when: When::Always,
                events: Events::default(),
            };
            core.phone_set(config).unwrap();
            // Each channel in a minute of its own.
            core.with_phone(|phone| phone.sent.clear());

            assert!(core.relay_seen(&permission, Presence::default), "{channel:?}");
            let first = requests.recv_timeout(WAIT).expect("the permission goes out");
            assert!(core.relay_seen(&crashed, Presence::default), "{channel:?}");
            let second = requests.recv_timeout(WAIT).expect("the crash goes out");
            assert!(core.relay_seen(&hostile, Presence::default), "{channel:?}");
            let third = requests.recv_timeout(WAIT).expect("the hostile one goes out");

            // The hostile names arrive as plain words in the places of the plain ones: no line
            // more in the head, nothing left that ends a line or turns it round.
            assert_eq!(parts(&third).1.lines().count(), parts(&first).1.lines().count(), "{channel:?}: {third:?}");
            assert!(third.contains("padX-Injected: yesdock") && third.contains("(BashX-Tool: 1)"), "{channel:?}: {third:?}");
            assert!(!third.contains(['\u{2028}', '\u{202e}', '\0', '\u{7f}']) && !parts(&third).2.contains(['\r', '\n']), "{channel:?}: {third:?}");

            for request in [&first, &second, &third] {
                assert!(!request.contains(&path) && !request.contains("clients"), "{channel:?} sent the path: {request}");
                assert!(!request.contains("deploy"), "{channel:?} sent a command's name: {request}");
                assert_eq!(request.matches(TOKEN).count(), usize::from(channel == Channel::Telegram), "{channel:?}: {request}");
            }
            assert!(!second.contains("Bash"), "{channel:?}: a tool goes with a permission only");

            let (line, head, body) = parts(&first);
            let (line_2, head_2, body_2) = parts(&second);
            match channel {
                Channel::Ntfy => {
                    assert_eq!(line, "POST /pitwall-topic HTTP/1.1");
                    assert!(head.contains("\r\ntitle: paddock\r\n"), "{head}");
                    assert_eq!(body, format!("{asks} (Bash)"));
                    assert!(head_2.contains(&format!("\r\ntitle: =?utf-8?b?{}?=\r\n", BASE64.encode("şehir-rehberi").to_ascii_lowercase())), "{head_2}");
                    assert_eq!(body_2, fell);
                }
                Channel::Telegram => {
                    assert_eq!(line, format!("POST /bot{TOKEN}/sendMessage HTTP/1.1"));
                    let sent: Value = serde_json::from_str(&body).unwrap();
                    assert_eq!((sent["chat_id"].as_str(), sent["text"].as_str()), (Some("42"), Some(format!("paddock — {asks} (Bash)").as_str())));
                    assert_eq!(serde_json::from_str::<Value>(&body_2).unwrap()["text"], format!("şehir-rehberi — {fell}"));
                }
                Channel::Webhook => {
                    assert_eq!(line, "POST /hook HTTP/1.1");
                    let sent: Value = serde_json::from_str(&body).unwrap();
                    let at = sent["at"].as_str().expect("a time");
                    let text = format!("paddock — {asks} (Bash)");
                    assert_eq!(sent, json!({ "project": "paddock", "event": "claude-permission", "tool": "Bash", "text": text, "at": at }));
                    let sent: Value = serde_json::from_str(&body_2).unwrap();
                    assert_eq!((&sent["project"], &sent["event"], &sent["tool"]), (&json!("şehir-rehberi"), &json!("server-crashed"), &Value::Null));
                    let _ = line_2;
                }
            }
        }
    }

    /// The gate: nothing goes while the channel is off, at another moment than the one chosen,
    /// for an event whose box isn't ticked, or for a kind that has no box at all; and what went
    /// doesn't go again within the minute.
    #[test]
    fn nothing_leaves_unless_the_user_asked_for_it() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());
        let (address, requests) = listener(200, "");

        let all = Events { asked: true, permission: true, finished: true, crashed: true, fuel: true, card: true };
        // Set as it is, whatever moments this system offers: the gate is the same on each.
        let set = |on: bool, when: When, events: Events| {
            let config = PhoneConfig { on, channel: Channel::Webhook, webhook_url: address.clone(), when, events, ..PhoneConfig::default() };
            core.with_phone(|phone| phone.config = config);
        };
        let signal = |kind: SignalKind| Signal { kind, path: "/work/paddock".into(), name: "paddock".into(), detail: Some("Bash".into()) };
        let here = || Presence { locked: Some(false), idle: Some(Duration::from_secs(20)) };
        let away = || Presence { locked: Some(true), idle: Some(AWAY) };

        // Off, as it is until the user turns it on.
        set(false, When::Always, all);
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeAsking), away));
        assert!(core.phone_test().is_err());

        // For a locked screen, and the user is at the Mac; or the system doesn't say.
        set(true, When::Locked, all);
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeAsking), here));
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeAsking), Presence::default));
        set(true, When::Idle, all);
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeAsking), here));
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeAsking), Presence::default));

        // The boxes that start unticked.
        set(true, When::Always, Events::default());
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeFinished), away));
        assert!(!core.relay_seen(&signal(SignalKind::CardReview), away));

        // The kinds that have no box.
        set(true, When::Always, all);
        for kind in [SignalKind::ClaudeLongTurn, SignalKind::CommandDone, SignalKind::CommandFailed, SignalKind::DepsWaiting] {
            assert!(!core.relay_seen(&signal(kind), away), "{kind:?}");
        }
        assert!(requests.recv_timeout(QUIET).is_err(), "something was sent");

        // The same channel does send once all of it holds: the silence above was the gate's.
        set(true, When::Locked, all);
        assert!(core.relay_seen(&signal(SignalKind::ClaudeAsking), away));
        assert!(requests.recv_timeout(WAIT).is_ok());
        // And not again within the minute.
        assert!(!core.relay_seen(&signal(SignalKind::ClaudeAsking), away));
        assert!(requests.recv_timeout(QUIET).is_err(), "sent twice in a minute");
    }

    /// The address is the user's secret: its file is theirs alone to read, the token never comes
    /// back to the UI (nor in a refusal's words), and a backup has none of it.
    #[test]
    fn the_address_is_kept_private() {
        let tmp = tempfile::tempdir().unwrap();
        let core = core_in(tmp.path());
        let file = tmp.path().join("config").join(FILE);

        let form = PhoneConfig {
            on: true,
            channel: Channel::Telegram,
            telegram_token: TOKEN.into(),
            telegram_chat: "42".into(),
            ntfy_topic: "pitwall-private".into(),
            ..PhoneConfig::default()
        };
        let view = core.phone_set(form.clone()).unwrap();
        assert!(!serde_json::to_string(&view).unwrap().contains(TOKEN));
        assert!(!serde_json::to_string(&core.phone_view()).unwrap().contains(TOKEN));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // A refusal that says the request back, as a server does of a path it doesn't know: its
        // reason would be the token's way into Settings.
        let (address, _requests) = listener(404, "{\"ok\":false,\"description\":\"Not Found: /bot123456789:AAH-pit_wall-TOKEN/sendMessage\"}");
        core.with_phone(|phone| phone.telegram = address);
        let refused = core.phone_test().unwrap_err();
        assert!(!refused.contains(TOKEN) && !serde_json::to_string(&core.phone_view()).unwrap().contains(TOKEN), "{refused}");

        // The form comes back without a token, as the UI has none to send: the stored one stays.
        let view = core.phone_set(PhoneConfig { telegram_token: String::new(), telegram_chat: "43".into(), ..form }).unwrap();
        assert!(view.telegram_token);
        let kept: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!((kept["telegramToken"].as_str(), kept["telegramChat"].as_str()), (Some(TOKEN), Some("43")));

        // A core started on that folder reads it back.
        let again = core_in(tmp.path());
        assert!(matches!(again.with_phone(|phone| phone.config.target(TELEGRAM)), Ok(Target::Telegram { url, .. }) if url.contains(TOKEN)));

        let backup = tmp.path().join("backup.json");
        core.backup_write(&backup, BackupParts { settings: true, projects: true, board: true, activity: true }).unwrap();
        let backup = fs::read_to_string(backup).unwrap();
        assert!(!backup.contains(TOKEN) && !backup.contains("pitwall-private") && !backup.contains("telegram"), "{backup}");
    }
}
