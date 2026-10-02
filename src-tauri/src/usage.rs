//! Claude's plan limits, as claude.ai's usage page shows them: the 5-hour session, the week, each
//! model's week and Extra Usage. Read from the endpoint Claude Code's own `/usage` reads, with
//! Claude Code's sign-in.
//!
//! The sign-in is borrowed, never refreshed: refreshing rotates the token and can sign Claude Code
//! out, so an expired one is only reported. The token lives in memory for one request and goes
//! nowhere else: not to disk, logs, errors or the UI.

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA: &str = "oauth-2025-04-20";
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const USER_AGENT: &str = concat!("Pitwall/", env!("CARGO_PKG_VERSION"));

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(30);
/// A token this close to its expiry counts as expired: it could lapse mid-request.
const EXPIRY_MARGIN_MS: i64 = 60_000;
/// After a request, a refresh asked for by hand waits this long, a background one `AUTO_FLOOR_MS`.
const FORCED_FLOOR_MS: i64 = 60_000;
const AUTO_FLOOR_MS: i64 = 15 * 60_000;
/// How long a 429 without a usable `Retry-After` keeps us away.
const DEFAULT_BACKOFF_MS: i64 = 5 * 60_000;
const MAX_BACKOFF_MS: i64 = 24 * 60 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ready,
    /// A refresh is running, or the first one hasn't finished yet.
    Loading,
    /// No Claude Code sign-in on this Mac.
    SignedOut,
    /// Claude Code's token has expired or was turned down; opening Claude Code refreshes it.
    Expired,
    /// The endpoint said 429; nothing is asked again before `retryAt`.
    RateLimited,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageState {
    pub status: Status,
    /// The last good reading, kept through later failures.
    pub usage: Option<Usage>,
    /// The plan Claude Code signed in with: `Pro`, `Max`, `Max 5x`, `Max 20x`, `Team`, `Enterprise`.
    pub plan: Option<String>,
    /// When `usage` was read, in ms.
    pub fetched_at: Option<i64>,
    /// While rate limited: when asking again is allowed, in ms.
    pub retry_at: Option<i64>,
    /// With `Error`, a code for the UI to word: `network`, `timeout`, `forbidden`, `server`, `http`,
    /// `badResponse`, `credentials` or `keychain`. Never server text.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// The session first, then the week, then everything else in the order the API gave it.
    pub windows: Vec<Window>,
    pub extra: Option<Extra>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// The API's key: `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`… A model limit
    /// that only comes in the `limits` list gets one of the same shape from the model's first word
    /// (`seven_day_fable`).
    pub id: String,
    /// The model's name as the API writes it ("Fable 5"), for limits from the `limits` list.
    pub name: Option<String>,
    /// Percent used, 0–100; above 100 when over the limit.
    pub used: f64,
    /// In ms; none while the window hasn't started.
    pub resets_at: Option<i64>,
}

/// Extra Usage, the pay-as-you-go spend past the plan's limits. Amounts are in the currency's
/// main unit (dollars, euros), already divided by the API's minor-unit exponent.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extra {
    pub enabled: bool,
    /// Spent this month.
    pub used_credits: f64,
    /// The monthly cap; none when there is no cap.
    pub monthly_limit: Option<f64>,
    /// Percent of the cap spent, 0–100; none without a cap.
    pub used: Option<f64>,
    /// ISO 4217 code, e.g. `USD`, `EUR`.
    pub currency: Option<String>,
}

/// Watches Claude's plan limits. Cheap to clone; every clone shares one state.
#[derive(Clone)]
pub struct UsageWatch {
    shared: Arc<Shared>,
}

struct Shared {
    inner: Mutex<Inner>,
    on_change: Box<dyn Fn() + Send + Sync>,
}

struct Inner {
    state: UsageState,
    busy: bool,
    last: Option<Attempt>,
}

#[derive(Debug, Clone, Copy)]
struct Attempt {
    at: i64,
    /// Whether it went as far as the endpoint; a sign-in check alone costs the API nothing.
    reached_api: bool,
}

impl UsageWatch {
    /// `on_change` runs, on whichever thread made the change, each time `state()` changes.
    pub fn new(on_change: impl Fn() + Send + Sync + 'static) -> Self {
        let state = UsageState { status: Status::Loading, usage: None, plan: None, fetched_at: None, retry_at: None, error: None };
        let inner = Inner { state, busy: false, last: None };
        UsageWatch { shared: Arc::new(Shared { inner: Mutex::new(inner), on_change: Box::new(on_change) }) }
    }

    pub fn state(&self) -> UsageState {
        self.shared.lock().state.clone()
    }

    /// Reads the limits again on a background thread, unless a read is running, a 429 asked us to
    /// wait, or the last request was under a minute ago (`force`, the refresh button) or under
    /// 15 minutes ago (the background refresh). The endpoint punishes polling with long waits.
    pub fn refresh(&self, force: bool) {
        let now = now_ms();
        {
            let mut inner = self.shared.lock();
            if inner.busy || !may_start(now, force, inner.last, inner.state.retry_at) {
                return;
            }
            inner.busy = true;
            inner.state.status = Status::Loading;
            inner.state.error = None;
            inner.state.retry_at = None;
        }
        (self.shared.on_change)();

        let shared = Arc::clone(&self.shared);
        let spawned = thread::Builder::new().name("usage".into()).spawn(move || {
            let report = fetch(now);
            shared.finish(now, report);
        });
        if spawned.is_err() {
            self.shared.finish(now, Report { plan: None, reached_api: false, result: Err(Problem::Failed("network")) });
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn finish(&self, started: i64, report: Report) {
        {
            let mut inner = self.lock();
            inner.busy = false;
            inner.last = Some(Attempt { at: started, reached_api: report.reached_api });
            let state = &mut inner.state;
            if report.plan.is_some() {
                state.plan = report.plan;
            }
            match report.result {
                Ok(usage) => {
                    state.status = Status::Ready;
                    state.usage = Some(usage);
                    state.fetched_at = Some(now_ms());
                }
                Err(Problem::SignedOut) => state.status = Status::SignedOut,
                Err(Problem::Expired) => state.status = Status::Expired,
                Err(Problem::RateLimited(at)) => {
                    state.status = Status::RateLimited;
                    state.retry_at = Some(at);
                }
                Err(Problem::Failed(code)) => {
                    state.status = Status::Error;
                    state.error = Some(code.to_string());
                }
            }
        }
        (self.on_change)();
    }
}

/// Whether a refresh may start: never inside a 429's wait; after a request, not for a minute
/// when forced and 15 minutes otherwise; after a check that stopped at the sign-in (signed out,
/// expired), at once when forced and after a minute otherwise.
fn may_start(now: i64, force: bool, last: Option<Attempt>, retry_at: Option<i64>) -> bool {
    if retry_at.is_some_and(|at| now < at) {
        return false;
    }
    let Some(last) = last else {
        return true;
    };
    let floor = match (last.reached_api, force) {
        (true, true) => FORCED_FLOOR_MS,
        (true, false) => AUTO_FLOOR_MS,
        (false, true) => 0,
        (false, false) => FORCED_FLOOR_MS,
    };
    now - last.at >= floor
}

struct Report {
    plan: Option<String>,
    reached_api: bool,
    result: Result<Usage, Problem>,
}

#[derive(Debug, PartialEq)]
enum Problem {
    SignedOut,
    Expired,
    RateLimited(i64),
    Failed(&'static str),
}

/// One read: Claude Code's sign-in, then the endpoint.
fn fetch(now: i64) -> Report {
    let (found, keychain_failed) = read_credentials();
    match decide(found, now) {
        Access::Use(credentials) => {
            let plan = credentials.plan.clone();
            let result = request(&credentials.access_token);
            drop(credentials);
            Report { plan, reached_api: true, result }
        }
        Access::Expired { plan } => Report { plan, reached_api: false, result: Err(Problem::Expired) },
        Access::SignedOut => {
            let problem = if keychain_failed { Problem::Failed("keychain") } else { Problem::SignedOut };
            Report { plan: None, reached_api: false, result: Err(problem) }
        }
    }
}

/// Claude Code's sign-in. Only what the request needs is kept; the refresh token is never read.
#[derive(Clone, PartialEq)]
struct Credentials {
    access_token: String,
    /// In ms.
    expires_at: Option<i64>,
    plan: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("access_token", &"<hidden>").field("expires_at", &self.expires_at).field("plan", &self.plan).finish()
    }
}

#[derive(Debug, PartialEq)]
enum Access {
    Use(Credentials),
    Expired { plan: Option<String> },
    SignedOut,
}

/// The first sign-in still valid is used as it is. One past its expiry is never refreshed (that
/// would rotate Claude Code's token and could sign it out); it only says so.
fn decide(found: Vec<Credentials>, now: i64) -> Access {
    let mut expired = None;
    for credentials in found {
        match credentials.expires_at {
            Some(at) if at - EXPIRY_MARGIN_MS <= now => {
                expired.get_or_insert(credentials.plan);
            }
            _ => return Access::Use(credentials),
        }
    }
    match expired {
        Some(plan) => Access::Expired { plan },
        None => Access::SignedOut,
    }
}

/// Where Claude Code keeps its sign-in: the macOS Keychain, else `~/.claude/.credentials.json`.
/// Also says whether the Keychain could not be read (as opposed to holding nothing).
fn read_credentials() -> (Vec<Credentials>, bool) {
    let mut found = Vec::new();
    let keychain = keychain_secret();
    if let Ok(Some(text)) = &keychain {
        found.extend(parse_credentials(text));
    }
    if let Some(home) = dirs::home_dir() {
        if let Ok(text) = std::fs::read_to_string(home.join(".claude").join(".credentials.json")) {
            found.extend(parse_credentials(&text));
        }
    }
    (found, keychain.is_err())
}

/// Claude Code's Keychain item, read with Apple's `security` tool, which doesn't prompt the way
/// a direct Keychain read from another app does. The secret comes back on a pipe, never through
/// arguments. `Ok(None)` when there is no such item.
#[cfg(target_os = "macos")]
fn keychain_secret() -> Result<Option<String>, ()> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    let mut child = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let mut stdout = child.stdout.take().ok_or(())?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        let _ = tx.send(out);
    });

    let Ok(out) = rx.recv_timeout(KEYCHAIN_TIMEOUT) else {
        // A Keychain prompt nobody answered.
        let _ = child.kill();
        let _ = child.wait();
        return Err(());
    };
    let status = child.wait().map_err(|_| ())?;
    if status.success() {
        String::from_utf8(out).map(Some).map_err(|_| ())
    } else if status.code() == Some(44) {
        // errSecItemNotFound
        Ok(None)
    } else {
        Err(())
    }
}

#[cfg(not(target_os = "macos"))]
fn keychain_secret() -> Result<Option<String>, ()> {
    Ok(None)
}

/// Claude Code's credentials JSON: `{"claudeAiOauth": {"accessToken", "expiresAt" (ms),
/// "subscriptionType", "rateLimitTier", …}}`. On macOS 26 `security -w` prints a secret with a
/// newline in it as hex, so all-hex text is decoded first. None without a Claude sign-in (the item
/// can hold only MCP servers' tokens).
fn parse_credentials(text: &str) -> Option<Credentials> {
    let text = text.trim();
    let decoded;
    let text = if !text.starts_with('{') && !text.is_empty() && text.len().is_multiple_of(2) && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes: Option<Vec<u8>> = (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect();
        decoded = String::from_utf8(bytes?).ok()?;
        decoded.as_str()
    } else {
        text
    };

    let json: Value = serde_json::from_str(text).ok()?;
    let oauth = json.get("claudeAiOauth")?;
    let access_token = oauth.get("accessToken")?.as_str()?.trim();
    if access_token.is_empty() {
        return None;
    }
    let expires_at = oauth.get("expiresAt").and_then(Value::as_f64).map(|ms| ms as i64);
    let plan = plan_name(oauth.get("subscriptionType").and_then(Value::as_str), oauth.get("rateLimitTier").and_then(Value::as_str));
    Some(Credentials { access_token: access_token.to_string(), expires_at, plan })
}

/// `max` + `default_claude_max_20x` → `Max 20x`; `pro` → `Pro`. The subscription type wins over
/// the rate-limit tier, which only adds Max's multiplier or stands in when the type is missing.
fn plan_name(subscription: Option<&str>, tier: Option<&str>) -> Option<String> {
    let words = |text: Option<&str>| -> Vec<String> {
        text.unwrap_or_default().to_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
    };
    let subscription_words = words(subscription);
    let tier_words = words(tier);
    let known = ["max", "pro", "team", "enterprise"];
    let base = known
        .iter()
        .find(|plan| subscription_words.iter().any(|w| w == *plan))
        .or_else(|| known.iter().find(|plan| tier_words.iter().any(|w| w == *plan)));

    let capitalized = |word: &str| {
        let mut chars = word.chars();
        chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
    };
    match base {
        Some(&"max") => {
            let multiplier = tier_words
                .iter()
                .skip_while(|w| *w != "max")
                .nth(1)
                .filter(|w| w.len() > 1 && w.ends_with('x') && w[..w.len() - 1].bytes().all(|b| b.is_ascii_digit()));
            Some(match multiplier {
                Some(multiplier) => format!("Max {multiplier}"),
                None => "Max".to_string(),
            })
        }
        Some(plan) => Some(capitalized(plan)),
        None => subscription.map(str::trim).filter(|s| !s.is_empty()).map(capitalized),
    }
}

/// `GET /api/oauth/usage` with the token as a bearer. A 401 means the token is no good any more
/// (expired early or signed out), which only Claude Code can mend.
fn request(token: &str) -> Result<Usage, Problem> {
    use ureq::http::HeaderValue;
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

    let Ok(mut authorization) = HeaderValue::from_str(&format!("Bearer {token}")) else {
        return Err(Problem::Failed("credentials"));
    };
    authorization.set_sensitive(true);

    // The system's TLS and trust store, so a company proxy's root installed on the Mac is honoured.
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(USER_AGENT)
        .build()
        .into();
    let response = agent.get(USAGE_URL).header("authorization", authorization).header("anthropic-beta", BETA).header("accept", "application/json").call();

    let mut response = match response {
        Ok(response) => response,
        Err(ureq::Error::Timeout(_)) => return Err(Problem::Failed("timeout")),
        Err(_) => return Err(Problem::Failed("network")),
    };
    match response.status().as_u16() {
        200 => {
            let body = response.body_mut().read_to_string().map_err(|_| Problem::Failed("network"))?;
            parse_usage(&body).ok_or(Problem::Failed("badResponse"))
        }
        401 => Err(Problem::Expired),
        403 => Err(Problem::Failed("forbidden")),
        429 => {
            let header = response.headers().get("retry-after").and_then(|value| value.to_str().ok());
            Err(Problem::RateLimited(retry_at(header, now_ms())))
        }
        500..=599 => Err(Problem::Failed("server")),
        _ => Err(Problem::Failed("http")),
    }
}

/// When a 429 lets us ask again: `Retry-After` in seconds or as an HTTP date, five minutes when
/// it is missing, zero or in the past (the endpoint has sent `Retry-After: 0` with a 429).
fn retry_at(header: Option<&str>, now: i64) -> i64 {
    let wait = header.map(str::trim).and_then(|value| match value.parse::<i64>() {
        Ok(seconds) => Some(seconds.saturating_mul(1000)),
        Err(_) => parse_http_date(value).map(|at| at - now),
    });
    now + wait.filter(|ms| *ms > 0).unwrap_or(DEFAULT_BACKOFF_MS).min(MAX_BACKOFF_MS)
}

/// The usage response. Every top-level `{utilization, resets_at}` object is a window, so a limit
/// the API adds later shows without a code change; nulls are skipped, and so are code-named ones
/// (`nimbus_quill`) that sit at 0 with no reset, which the API lists before they apply. Model
/// limits that only come in the `limits` list are added unless a top-level key already has them.
/// Extra Usage prefers the `spend` block and falls back to `extra_usage`. None unless an object.
pub fn parse_usage(body: &str) -> Option<Usage> {
    let json: Value = serde_json::from_str(body).ok()?;
    let root = json.as_object()?;

    let mut windows = Vec::new();
    for (key, value) in root {
        let Some(window) = value.as_object() else {
            continue;
        };
        let Some(used) = window.get("utilization").and_then(Value::as_f64).filter(|u| u.is_finite()) else {
            continue;
        };
        if !window.contains_key("resets_at") {
            continue;
        }
        let resets_at = window.get("resets_at").and_then(Value::as_str).and_then(parse_rfc3339);
        if resets_at.is_none() && !is_period_name(key) {
            continue;
        }
        windows.push(Window { id: key.clone(), name: None, used, resets_at });
    }

    for limit in root.get("limits").and_then(Value::as_array).into_iter().flatten() {
        let model = limit.pointer("/scope/model/display_name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
        let (Some(model), Some(used)) = (model, limit.get("percent").and_then(Value::as_f64).filter(|p| p.is_finite())) else {
            continue;
        };
        let prefix = match (limit.get("group").and_then(Value::as_str), limit.get("kind").and_then(Value::as_str)) {
            (Some("session"), _) => "five_hour",
            (Some("weekly"), _) | (_, Some("weekly_scoped")) => "seven_day",
            _ => continue,
        };
        let Some(first_word) = model.split(|c: char| !c.is_alphanumeric()).find(|w| !w.is_empty()) else {
            continue;
        };
        let id = format!("{prefix}_{}", first_word.to_lowercase());
        if windows.iter().any(|w| w.id == id) {
            continue;
        }
        let resets_at = limit.get("resets_at").and_then(Value::as_str).and_then(parse_rfc3339);
        windows.push(Window { id, name: Some(model.to_string()), used, resets_at });
    }

    let rank = |id: &str| match id {
        "five_hour" => 0,
        "seven_day" => 1,
        _ => 2,
    };
    windows.sort_by_key(|w| rank(&w.id));

    let extra = root.get("spend").and_then(spend_extra).or_else(|| root.get("extra_usage").and_then(legacy_extra));
    Some(Usage { windows, extra })
}

/// `five_hour`, `seven_day`, `seven_day_opus`: a count and a unit, then maybe a model.
fn is_period_name(key: &str) -> bool {
    let mut parts = key.split('_');
    let count = parts.next().unwrap_or_default();
    let unit = parts.next().unwrap_or_default();
    !count.is_empty() && count.bytes().all(|b| b.is_ascii_alphanumeric()) && matches!(unit, "hour" | "hours" | "day" | "days" | "week" | "weeks" | "month" | "months")
}

/// `spend`: `{enabled, used: {amount_minor, currency, exponent}, limit: {…} | null}`. A cap that is
/// there but makes no sense drops the block, so it is never shown as "no cap".
fn spend_extra(spend: &Value) -> Option<Extra> {
    let spend = spend.as_object()?;
    let enabled = spend.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    let (used_credits, used_currency) = money(spend.get("used")?)?;
    let (monthly_limit, limit_currency) = match spend.get("limit") {
        None | Some(Value::Null) => (None, None),
        Some(limit) => {
            let (amount, currency) = money(limit)?;
            (Some(amount), currency)
        }
    };
    Some(Extra { enabled, used_credits, monthly_limit, used: percent_of(used_credits, monthly_limit), currency: used_currency.or(limit_currency) })
}

/// A money object: `amount_minor / 10^exponent`. None when negative or malformed.
fn money(value: &Value) -> Option<(f64, Option<String>)> {
    let amount = value.get("amount_minor")?.as_f64().filter(|a| a.is_finite() && *a >= 0.0)?;
    let exponent = match value.get("exponent") {
        None | Some(Value::Null) => 2,
        Some(exponent) => exponent.as_u64().filter(|e| *e <= 6)?,
    };
    let currency = value.get("currency").and_then(Value::as_str).map(str::to_string);
    Some((amount / 10f64.powi(exponent as i32), currency))
}

/// The older `extra_usage`: `{is_enabled, used_credits, monthly_limit, currency, decimal_places}`,
/// amounts in minor units (cents). A null limit means no cap; a negative amount drops the block.
fn legacy_extra(extra: &Value) -> Option<Extra> {
    let extra = extra.as_object()?;
    let enabled = extra.get("is_enabled").and_then(Value::as_bool).unwrap_or(false);
    let places = match extra.get("decimal_places") {
        None | Some(Value::Null) => 2,
        Some(places) => places.as_u64().filter(|p| *p <= 6)?,
    };
    let scale = 10f64.powi(places as i32);
    let amount = |key: &str| -> Option<Option<f64>> {
        match extra.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(value) => value.as_f64().filter(|a| a.is_finite() && *a >= 0.0).map(|a| Some(a / scale)),
        }
    };
    let used_credits = amount("used_credits")?.unwrap_or(0.0);
    let monthly_limit = amount("monthly_limit")?;
    let currency = extra.get("currency").and_then(Value::as_str).map(str::to_string);
    Some(Extra { enabled, used_credits, monthly_limit, used: percent_of(used_credits, monthly_limit), currency })
}

fn percent_of(used: f64, limit: Option<f64>) -> Option<f64> {
    limit.filter(|l| *l > 0.0).map(|l| used / l * 100.0)
}

/// `2026-09-05T12:59:59.966454+00:00` or `2025-01-15T10:00:00Z`, in ms.
fn parse_rfc3339(text: &str) -> Option<i64> {
    let b = text.trim().as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let number = |from: usize, to: usize| -> Option<i64> {
        let digits = &b[from..to];
        digits.iter().all(u8::is_ascii_digit).then(|| digits.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let mut i = 19;
    let mut millis = 0;
    if b[i] == b'.' {
        let start = i + 1;
        i = start;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        let digits = &b[start..i.min(start + 3)];
        millis = digits.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')) * 10i64.pow(3 - digits.len() as u32);
    }
    let offset_minutes = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        sign @ (b'+' | b'-') if i + 6 == b.len() && b[i + 3] == b':' => {
            let minutes = number(i + 1, i + 3)? * 60 + number(i + 4, i + 6)?;
            if *sign == b'-' {
                -minutes
            } else {
                minutes
            }
        }
        _ => return None,
    };

    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1000 + millis)
}

/// An HTTP date, `Sun, 06 Nov 1994 08:49:37 GMT`, in ms.
fn parse_http_date(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [_, day, month, year, time, zone] = parts.as_slice() else {
        return None;
    };
    if !zone.eq_ignore_ascii_case("GMT") && !zone.eq_ignore_ascii_case("UTC") {
        return None;
    }
    let month = MONTHS.iter().position(|m| month.eq_ignore_ascii_case(m))? as i64 + 1;
    let (day, year): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let mut clock = time.split(':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    if clock.next().is_some() || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) * 1000)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real response, September 2026 (anonymised in usage-monitor-for-claude's API reference):
    /// legacy model keys null or set, code-named quotas idle, Extra Usage on without a cap.
    const SEPTEMBER_2026: &str = r#"{
        "five_hour": {"utilization": 48.0, "resets_at": "2026-09-05T12:59:59.966454+00:00", "limit_dollars": null, "used_dollars": null, "remaining_dollars": null, "locked_reason": null},
        "seven_day": {"utilization": 64.0, "resets_at": "2026-09-11T20:59:59.966479+00:00", "limit_dollars": null, "used_dollars": null, "remaining_dollars": null, "locked_reason": null},
        "seven_day_oauth_apps": null,
        "seven_day_opus": null,
        "seven_day_sonnet": {"utilization": 2.0, "resets_at": "2026-09-11T20:59:59.966479+00:00", "limit_dollars": null, "used_dollars": null, "remaining_dollars": null, "locked_reason": null},
        "seven_day_cowork": null,
        "seven_day_omelette": null,
        "tangelo": null,
        "iguana_necktie": null,
        "nimbus_quill": {"utilization": 0.0, "resets_at": null, "limit_dollars": null, "used_dollars": null, "remaining_dollars": null, "locked_reason": null},
        "extra_usage": {"is_enabled": true, "monthly_limit": null, "used_credits": 0.0, "utilization": null, "currency": "EUR", "decimal_places": 2, "disabled_reason": null, "user_disabled": false},
        "limits": [
            {"kind": "session", "group": "session", "percent": 48, "severity": "normal", "resets_at": "2026-09-05T12:59:59.966454+00:00", "scope": null, "is_active": true},
            {"kind": "weekly_all", "group": "weekly", "percent": 64, "severity": "normal", "resets_at": "2026-09-11T20:59:59.966479+00:00", "scope": null, "is_active": false}
        ],
        "spend": {"used": {"amount_minor": 0, "currency": "EUR", "exponent": 2}, "limit": null, "percent": 0, "enabled": true, "balance": null},
        "member_dashboard_available": false
    }"#;

    #[test]
    fn a_real_response_reads_as_its_windows_and_extra_usage() {
        let usage = parse_usage(SEPTEMBER_2026).unwrap();

        let ids: Vec<&str> = usage.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_sonnet"], "nulls and the idle code-named quota are left out");
        assert_eq!(usage.windows[0].used, 48.0);
        assert_eq!(usage.windows[0].resets_at, Some(1_788_613_199_966));
        assert_eq!(usage.windows[2].used, 2.0);
        assert_eq!(usage.extra, Some(Extra { enabled: true, used_credits: 0.0, monthly_limit: None, used: None, currency: Some("EUR".into()) }));
    }

    #[test]
    fn model_limits_from_any_key_or_the_limits_list_show_once() {
        let body = r#"{
            "seven_day": {"utilization": 10, "resets_at": "2026-07-02T10:59:59Z"},
            "five_hour": {"utilization": 23.5, "resets_at": null},
            "seven_day_opus": {"utilization": 60.0, "resets_at": "2026-07-02T11:00:00Z"},
            "seven_day_haiku": {"utilization": 104.0, "resets_at": "2026-07-02T11:00:00Z"},
            "tangelo": {"utilization": 7.0, "resets_at": "2026-07-03T00:00:00-02:30"},
            "limits": [
                {"kind": "weekly_scoped", "group": "weekly", "percent": 60, "resets_at": "2026-07-02T11:00:00Z", "scope": {"model": {"id": null, "display_name": "Opus"}}},
                {"kind": "weekly_scoped", "group": "weekly", "percent": 17, "resets_at": "2026-07-02T11:00:00Z", "scope": {"model": {"id": null, "display_name": "Fable 5"}}},
                {"kind": "weekly_scoped", "group": "weekly", "percent": 50, "scope": {"model": {"display_name": ""}}},
                {"kind": "weekly_scoped", "group": "weekly", "resets_at": "2026-07-02T11:00:00Z", "scope": {"model": {"display_name": "Sonnet"}}}
            ],
            "spend": {"enabled": true, "used": {"amount_minor": 125, "currency": "USD", "exponent": 2}, "limit": {"amount_minor": 1000, "currency": "USD", "exponent": 2}},
            "extra_usage": {"is_enabled": true, "used_credits": 541, "monthly_limit": 2000, "decimal_places": 2}
        }"#;
        let usage = parse_usage(body).unwrap();

        let ids: Vec<&str> = usage.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_opus", "seven_day_haiku", "tangelo", "seven_day_fable"]);
        assert_eq!(usage.windows[0].resets_at, None, "a session not started yet still shows");
        assert_eq!(usage.windows[3].used, 104.0, "over the limit stays over 100");
        assert_eq!(usage.windows[4].resets_at, Some(1_783_045_800_000), "the offset is applied");
        let fable = &usage.windows[5];
        assert_eq!((fable.name.as_deref(), fable.used, fable.resets_at), (Some("Fable 5"), 17.0, Some(1_782_990_000_000)));
        assert_eq!(usage.extra, Some(Extra { enabled: true, used_credits: 1.25, monthly_limit: Some(10.0), used: Some(12.5), currency: Some("USD".into()) }));
    }

    #[test]
    fn extra_usage_amounts_are_cents_and_a_bad_cap_is_never_no_cap() {
        let legacy = |extra: &str| parse_usage(&format!(r#"{{"extra_usage": {extra}}}"#)).unwrap().extra;

        let capped = legacy(r#"{"is_enabled": true, "used_credits": 2672, "monthly_limit": 5000}"#).unwrap();
        assert_eq!((capped.used_credits, capped.monthly_limit, capped.used), (26.72, Some(50.0), Some(53.44)));
        assert_eq!(legacy(r#"{"is_enabled": true, "used_credits": 541, "monthly_limit": -2000}"#), None);
        assert_eq!(legacy(r#"{"is_enabled": true, "used_credits": -541, "monthly_limit": 2000}"#), None);
        assert!(!legacy(r#"{"is_enabled": false, "used_credits": null, "monthly_limit": null}"#).unwrap().enabled);

        // A spend block whose cap is broken gives way to the older block instead of reading uncapped.
        let body = r#"{
            "spend": {"enabled": true, "used": {"amount_minor": 541, "exponent": 2}, "limit": {"amount_minor": -2000, "exponent": 2}},
            "extra_usage": {"is_enabled": true, "used_credits": 541, "monthly_limit": 2000}
        }"#;
        assert_eq!(parse_usage(body).unwrap().extra.unwrap().monthly_limit, Some(20.0));
        assert_eq!(parse_usage("[]"), None);
    }

    #[test]
    fn a_rate_limit_waits_as_long_as_it_is_told_and_never_zero() {
        let now = 1_000_000_000_000;
        assert_eq!(retry_at(Some("120"), now), now + 120_000);
        assert_eq!(retry_at(Some("0"), now), now + DEFAULT_BACKOFF_MS);
        assert_eq!(retry_at(None, now), now + DEFAULT_BACKOFF_MS);
        assert_eq!(retry_at(Some("soon"), now), now + DEFAULT_BACKOFF_MS);
        let date = parse_http_date("Sun, 09 Sep 2001 01:46:40 GMT").unwrap();
        assert_eq!(date, now);
        assert_eq!(retry_at(Some("Sun, 09 Sep 2001 02:46:40 GMT"), now), now + 3_600_000);
    }

    const KEYCHAIN: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-fixture-token","refreshToken":"sk-ant-ort01-fixture-refresh","expiresAt":1790000000000,"scopes":["user:inference","user:profile","user:sessions:claude_code"],"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"},"mcpOAuth":{}}"#;

    #[test]
    fn claude_codes_sign_in_reads_as_token_expiry_and_plan() {
        let credentials = parse_credentials(KEYCHAIN).unwrap();
        assert_eq!(credentials.access_token, "sk-ant-oat01-fixture-token");
        assert_eq!(credentials.expires_at, Some(1_790_000_000_000));
        assert_eq!(credentials.plan.as_deref(), Some("Max 20x"));
        assert!(!format!("{credentials:?}").contains("fixture"), "the token never reaches a log line");

        // macOS 26's `security -w` prints a secret with a newline in it as hex.
        let pretty = KEYCHAIN.replace(",\"mcpOAuth\"", ",\n\"mcpOAuth\"");
        let hex: String = pretty.bytes().map(|b| format!("{b:02x}")).collect();
        assert_eq!(parse_credentials(&format!("{hex}\n")), Some(credentials));

        assert_eq!(parse_credentials(r#"{"mcpOAuth":{"server":{"accessToken":"x"}}}"#), None, "only MCP servers' tokens");
        let pro = parse_credentials(r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1,"subscriptionType":"pro","rateLimitTier":"default_claude_ai"}}"#);
        assert_eq!(pro.unwrap().plan.as_deref(), Some("Pro"));
        assert_eq!(plan_name(None, Some("default_claude_max_5x")).as_deref(), Some("Max 5x"));
    }

    #[test]
    fn an_expired_sign_in_is_reported_never_refreshed() {
        let now = 1_790_000_000_000;
        let signed_in = |expires_at: Option<i64>, plan: &str| Credentials { access_token: "token".into(), expires_at, plan: Some(plan.into()) };

        let expired = signed_in(Some(now - 1), "Max");
        let expiring = signed_in(Some(now + 30_000), "Max");
        assert_eq!(decide(vec![expired.clone()], now), Access::Expired { plan: Some("Max".into()) });
        assert_eq!(decide(vec![expiring], now), Access::Expired { plan: Some("Max".into()) }, "lapsing within the minute counts as expired");

        // The Keychain's token expired, but the file holds a valid one: that one is used as it is.
        let valid = signed_in(Some(now + 3_600_000), "Pro");
        assert_eq!(decide(vec![expired, valid.clone()], now), Access::Use(valid));
        assert_eq!(decide(vec![signed_in(None, "Pro")], now), Access::Use(signed_in(None, "Pro")), "no expiry: the server decides");
        assert_eq!(decide(Vec::new(), now), Access::SignedOut);
    }
}
