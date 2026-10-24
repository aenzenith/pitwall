//! Pure decisions: package manager, commands, URLs and output parsing.
//! Port of the extension's `src/resolve.ts`; keep the two in step.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

use crate::i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
            PackageManager::Bun => "bun",
        })
    }
}

impl PackageManager {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "npm" => Some(Self::Npm),
            "pnpm" => Some(Self::Pnpm),
            "yarn" => Some(Self::Yarn),
            "bun" => Some(Self::Bun),
            _ => None,
        }
    }
}

const LOCK_FILES: [(&str, PackageManager); 5] = [
    ("pnpm-lock.yaml", PackageManager::Pnpm),
    ("yarn.lock", PackageManager::Yarn),
    ("bun.lockb", PackageManager::Bun),
    ("bun.lock", PackageManager::Bun),
    ("package-lock.json", PackageManager::Npm),
];

/// Script names may only use these characters; keeps the shell out of it.
static SAFE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9:._-]*$").unwrap());

/// Extra arguments passed to the script: `--port`, `--port=5176`, `5176`.
static SAFE_ARG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(--?[A-Za-z0-9][A-Za-z0-9-]*(=[A-Za-z0-9._:/-]+)?|[0-9]{1,5})$").unwrap()
});

/// Colour and cursor codes (CSI), hyperlinks and titles (OSC), charset switches, stray ESCs.
static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][A-Za-z0-9]|\x1b").unwrap()
});
/// The line a dev server says its own address on: `Local:` (Vite, Next, Nuxt, Vue CLI,
/// create-react-app), `Loopback:` (webpack-dev-server).
static OWN_ADDRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:Local|Loopback):\s*(https?://\S+)").unwrap());
static ANY_LOCAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)https?://(?:localhost|127\.0\.0\.1|\[::1\]):\d+\S*").unwrap());
static PORT_TAKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)Port\s+(\d{2,5})\s+is\s+in\s+use").unwrap());
static ADDR_IN_USE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)EADDRINUSE[^\n]*").unwrap());
/// Vite took a change: `[vite] hmr update /src/App.vue`, `[vite] (client) page reload …`.
static UPDATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[vite\]\s+(?:\(\w+\)\s+)?(?:hmr update|page reload)\b").unwrap());
/// Vite run by its own name in a script: `vite`, `vite --host`, `vite dev`, `npx vite`, `remix
/// vite:dev`, Vite+'s `vp dev`; never `vitest`, `vite-node` or `vitepress`.
static RUNS_VITE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:^|[\s;&|()"'=/])(?:vite(?:$|[\s;&|()"':])|vp\s+dev\b)"#).unwrap());

/// What a dev server prints for an error: one list for the project's issue here and for the
/// output pane, which marks the same lines (`src/lib/outputLines.ts`). Each is searched for in a
/// line, whatever its case, and keeps to what both regex engines read alike.
static ERROR_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let found: Vec<String> = serde_json::from_str(include_str!("../../src/lib/errorLines.json")).expect("errorLines.json is a list of patterns");
    found.iter().map(|pattern| Regex::new(&format!("(?im)^.*{pattern}.*$")).unwrap()).collect()
});

pub fn detect_package_manager(files: &[String]) -> PackageManager {
    LOCK_FILES
        .iter()
        .find(|(lock, _)| files.iter().any(|file| file == lock))
        .map(|(_, manager)| *manager)
        .unwrap_or(PackageManager::Npm)
}

/// Rejects script names unsafe for the shell: the name goes into a command line as it is.
pub fn is_unsafe_script(script: &str) -> bool {
    !SAFE_SCRIPT.is_match(script.trim())
}

/// The command line to run. Unsafe extra arguments are dropped.
pub fn build_command(manager: PackageManager, script: &str, args: &[String]) -> Result<String, String> {
    let name = script.trim();

    if is_unsafe_script(name) {
        return Err(t!("core.error.forbiddenScript", script = script));
    }

    let safe: Vec<&str> = args.iter().map(String::as_str).filter(|arg| SAFE_ARG.is_match(arg)).collect();
    let tail = if safe.is_empty() { String::new() } else { format!(" -- {}", safe.join(" ")) };

    Ok(format!("{manager} run {name}{tail}"))
}

/// Whether `package.json` has `script`. A byte-order mark before it is no part of it: npm reads
/// past one too.
pub fn has_script(package_json: &str, script: &str) -> bool {
    script_line(package_json, script).is_some()
}

/// What `script` runs, as `package.json` writes it.
pub fn script_line(package_json: &str, script: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(package_json.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|value| value.get("scripts")?.get(script.trim())?.as_str().map(str::to_string))
}

/// Whether a script's command line starts Vite (`RUNS_VITE`).
pub fn runs_vite(script_line: &str) -> bool {
    RUNS_VITE.is_match(script_line)
}

pub fn strip_ansi(text: &str) -> String {
    ANSI.replace_all(text, "").into_owned()
}

fn trim_url(url: &str) -> String {
    url.trim_end_matches(['.', ',', ';', ')', ']']).to_string()
}

/// A local URL a dev server printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalUrl {
    pub url: String,
    /// Said on the server's own address line (`OWN_ADDRESS`); else a bare URL anywhere in the
    /// output, which may be another's: a proxy's target, another process of the script.
    pub labelled: bool,
}

/// The local URL a dev server printed, preferring its own address line.
pub fn extract_local_url(output: &str) -> Option<LocalUrl> {
    let clean = strip_ansi(output);

    if let Some(captures) = OWN_ADDRESS.captures(&clean) {
        return Some(LocalUrl { url: trim_url(&captures[1]), labelled: true });
    }

    ANY_LOCAL.find(&clean).map(|found| LocalUrl { url: trim_url(found.as_str()), labelled: false })
}

/// Whether a line says the server took a change (Vite's `hmr update`, `page reload`): an error
/// it printed before was mended, or comes again right after this line.
pub fn is_update_line(line: &str) -> bool {
    UPDATE.is_match(&strip_ansi(line))
}

/// `APP_URL` from a `.env` file, quotes and trailing comments removed.
pub fn parse_app_url(env: &str) -> Option<String> {
    for line in env.lines() {
        let Some(rest) = line.trim_start().strip_prefix("APP_URL") else {
            continue;
        };
        let Some(raw) = rest.trim_start().strip_prefix('=') else {
            continue;
        };

        let mut value = raw.trim().to_string();
        let quoted = value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\'')));

        if quoted {
            value = value[1..value.len() - 1].to_string();
        } else {
            value = value.split('#').next().unwrap_or("").trim().to_string();
        }

        if (value.starts_with("http://") || value.starts_with("https://")) && !value.contains(char::is_whitespace) {
            return Some(trim_url(&value));
        }
    }

    None
}

/// Herd's default: the folder name in kebab case plus `.test`.
pub fn herd_fallback_url(folder_name: &str) -> String {
    let mut host = String::new();
    let mut dash = false;

    for ch in folder_name.trim().to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            host.push(ch);
            dash = false;
        } else if !dash {
            host.push('-');
            dash = true;
        }
    }

    format!("https://{}.test", host.trim_matches('-'))
}

/// `server: { port: 5180 }` from a Vite config. Only the first port inside the `server`
/// block counts, so a port in a comment or another block is ignored.
pub fn parse_vite_port(source: &str) -> Option<u16> {
    static BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
    static LINE_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)(^|\s)//[^\n]*").unwrap());
    static SERVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"server\s*:\s*\{").unwrap());
    static PORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bport\s*:\s*(\d{2,5})\b").unwrap());

    let without_block = BLOCK_COMMENT.replace_all(source, "");
    let clean = LINE_COMMENT.replace_all(&without_block, "$1");
    let start = SERVER.find(&clean)?.start();
    let end = clean.char_indices().map(|(i, _)| i).find(|&i| i >= start + 600).unwrap_or(clean.len());

    PORT.captures(&clean[start..end])?[1].parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortConflict {
    pub port: u16,
    /// The server died (`EADDRINUSE`) rather than moving on by itself.
    pub fatal: bool,
}

pub fn detect_port_conflict(output: &str) -> Option<PortConflict> {
    let clean = strip_ansi(output);

    if let Some(captures) = PORT_TAKEN.captures(&clean) {
        return Some(PortConflict { port: captures[1].parse().unwrap_or(0), fatal: false });
    }

    ADDR_IN_USE.find(&clean).map(|found| PortConflict { port: port_from_address(found.as_str()), fatal: true })
}

/// The port in `... in use 127.0.0.1:5173`, read after the last colon so the address's own
/// numbers don't leak in.
fn port_from_address(line: &str) -> u16 {
    static AFTER_COLON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":(\d{2,5})\b").unwrap());
    static BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{4,5})\b").unwrap());

    if let Some(last) = AFTER_COLON.captures_iter(line).last() {
        return last[1].parse().unwrap_or(0);
    }

    BARE.captures(line).and_then(|c| c[1].parse().ok()).unwrap_or(0)
}

/// The first meaningful error line in the output, shortened for the row.
pub fn detect_error_line(output: &str, max: usize) -> Option<String> {
    let clean = strip_ansi(output);

    for pattern in ERROR_PATTERNS.iter() {
        let Some(found) = pattern.find(&clean) else {
            continue;
        };

        let line = found.as_str().split_whitespace().collect::<Vec<_>>().join(" ");

        if line.is_empty() {
            continue;
        }

        if line.chars().count() > max {
            return Some(format!("{}…", line.chars().take(max - 1).collect::<String>()));
        }

        return Some(line);
    }

    None
}

pub fn port_from_url(url: &str) -> Option<u16> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let port = authority.rsplit_once(':').map(|(_, port)| port);

    match port {
        Some(port) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => port.parse().ok(),
        _ if url.starts_with("https://") => Some(443),
        _ => Some(80),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn shell_metacharacters_never_reach_the_command() {
        assert!(is_unsafe_script("dev; rm -rf ~"));
        assert!(is_unsafe_script("dev && curl x"));
        assert!(is_unsafe_script("$(whoami)"));

        let command = build_command(
            PackageManager::Pnpm,
            "dev",
            &args(&["--port", "5176", "; rm -rf ~", "$(id)", "--host=0.0.0.0"]),
        )
        .unwrap();

        assert_eq!(command, "pnpm run dev -- --port 5176 --host=0.0.0.0");
    }

    #[test]
    fn package_manager_comes_from_the_lock_file() {
        assert_eq!(detect_package_manager(&args(&["pnpm-lock.yaml", "package-lock.json"])), PackageManager::Pnpm);
        assert_eq!(detect_package_manager(&args(&["bun.lock"])), PackageManager::Bun);
        assert_eq!(detect_package_manager(&args(&["README.md"])), PackageManager::Npm);
    }

    #[test]
    fn vite_port_only_counts_inside_the_server_block() {
        assert_eq!(parse_vite_port("export default { server: { port: 5180 } }"), Some(5180));
        assert_eq!(parse_vite_port("preview: { port: 4000 }, server: { host: true, port: 5190 }"), Some(5190));
        assert_eq!(parse_vite_port("// server: { port: 1111 }\nexport default {}"), None);
        assert_eq!(parse_vite_port("/* server: { port: 2222 } */ server: { port: 3333 }"), Some(3333));
    }

    #[test]
    fn port_conflicts_are_told_apart() {
        assert_eq!(
            detect_port_conflict("Port 5173 is in use, trying another one..."),
            Some(PortConflict { port: 5173, fatal: false })
        );
        assert_eq!(
            detect_port_conflict("Error: listen EADDRINUSE: address already in use 127.0.0.1:5174"),
            Some(PortConflict { port: 5174, fatal: true })
        );
        assert_eq!(detect_port_conflict("ready in 300 ms"), None);
    }

    #[test]
    fn urls_and_app_url() {
        assert_eq!(
            extract_local_url("\x1b[32m  ➜  Local:   http://localhost:5173/\x1b[0m").map(|found| found.url),
            Some("http://localhost:5173/".into())
        );
        assert_eq!(parse_app_url("APP_NAME=x\nAPP_URL=\"https://shop.test\" \n"), Some("https://shop.test".into()));
        assert_eq!(parse_app_url("APP_URL=https://a.test # note"), Some("https://a.test".into()));
        assert_eq!(parse_app_url("APP_URL="), None);
        assert_eq!(herd_fallback_url("My Shop_v2"), "https://my-shop-v2.test");
        assert_eq!(port_from_url("http://localhost:5174/"), Some(5174));
    }

    #[test]
    fn terminal_codes_never_reach_the_output() {
        let line = "\x1b[32m➜\x1b[39m  \x1b[1mLocal\x1b[22m: \x1b]8;;http://a.test\x07http://a.test\x1b]8;;\x07 \x1b[?25l\x1b(B";

        assert_eq!(strip_ansi(line), "➜  Local: http://a.test ");
    }

    #[test]
    fn error_lines_are_shortened() {
        assert_eq!(
            detect_error_line("ok\nError: Cannot find module 'vite'\nmore", 90),
            Some("Error: Cannot find module 'vite'".into())
        );
        assert!(detect_error_line(&format!("SyntaxError: {}", "x".repeat(200)), 20).unwrap().ends_with('…'));
        assert_eq!(detect_error_line("all good", 90), None);
    }
}
