//! "Open in browser" brings forward the tab that already shows the project instead of opening
//! one more. macOS only: running browsers are asked through AppleScript (the user allows it once,
//! per browser); when none has such a tab, or asking isn't allowed, the caller opens the address.

/// Address prefixes a tab can start with to show this project: each address's scheme and host
/// (with port), under both schemes and, for local servers, as `localhost` and `127.0.0.1`.
pub fn prefixes(urls: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();

    for url in urls {
        let Some((_, rest)) = url.split_once("://") else {
            continue;
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default().to_lowercase();
        if host.is_empty() {
            continue;
        }

        let mut hosts = vec![host.clone()];
        if let Some(port) = host.strip_prefix("localhost") {
            hosts.push(format!("127.0.0.1{port}"));
        } else if let Some(port) = host.strip_prefix("127.0.0.1") {
            hosts.push(format!("localhost{port}"));
        }

        for host in hosts {
            for scheme in ["http", "https"] {
                let prefix = format!("{scheme}://{host}");
                if !found.contains(&prefix) {
                    found.push(prefix);
                }
            }
        }
    }

    found
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// The asking stops after this long (a browser that doesn't answer, a permission prompt left
    /// open); the address then opens as before.
    const LIMIT: Duration = Duration::from_secs(8);

    /// Browsers Pitwall can ask: the executable inside the app (to see whether it runs), the
    /// app's name for AppleScript, and whether it speaks Chrome's or Safari's dictionary.
    const BROWSERS: [(&str, &str, bool); 6] = [
        ("/Google Chrome.app/Contents/MacOS/Google Chrome", "Google Chrome", true),
        ("/Safari.app/Contents/MacOS/Safari", "Safari", false),
        ("/Brave Browser.app/Contents/MacOS/Brave Browser", "Brave Browser", true),
        ("/Microsoft Edge.app/Contents/MacOS/Microsoft Edge", "Microsoft Edge", true),
        ("/Vivaldi.app/Contents/MacOS/Vivaldi", "Vivaldi", true),
        ("/Chromium.app/Contents/MacOS/Chromium", "Chromium", true),
    ];

    /// The tab's address matches when it is a prefix itself, or a prefix followed by a path,
    /// query or fragment (so `localhost:5173` never matches `localhost:51730`). The script's
    /// first argument says whether to reload the tab, the rest are the prefixes.
    const MATCHES: &str = r##"
on shows(u, argv)
    if u is missing value then return false
    repeat with p in rest of argv
        set p to p as text
        if u is p or u starts with (p & "/") or u starts with (p & "?") or u starts with (p & "#") then return true
    end repeat
    return false
end shows
"##;

    fn chrome_script(app: &str) -> String {
        format!(
            r#"{MATCHES}
on run argv
    tell application "{app}"
        repeat with w in windows
            set i to 0
            repeat with t in tabs of w
                set i to i + 1
                if my shows(URL of t, argv) then
                    if item 1 of argv is "reload" then tell t to reload
                    set active tab index of w to i
                    set index of w to 1
                    activate
                    return "found"
                end if
            end repeat
        end repeat
    end tell
    return ""
end run
"#
        )
    }

    fn safari_script() -> String {
        format!(
            r#"{MATCHES}
on run argv
    tell application "Safari"
        repeat with w in windows
            repeat with t in tabs of w
                if my shows(URL of t, argv) then
                    if item 1 of argv is "reload" then set URL of t to (URL of t)
                    set current tab of w to t
                    set index of w to 1
                    activate
                    return "found"
                end if
            end repeat
        end repeat
    end tell
    return ""
end run
"#
        )
    }

    /// Running processes' executables; only browsers in here are asked, so AppleScript never
    /// looks for (and offers to locate) one that isn't installed.
    fn running() -> String {
        Command::new("ps")
            .args(["-axo", "comm="])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default()
    }

    /// Runs `script` with `args`; true when it printed "found".
    fn ask(script: &str, args: &[&str]) -> bool {
        let Ok(mut child) = Command::new("osascript").arg("-").args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
            return false;
        };

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(script.as_bytes());
        }

        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < LIMIT => std::thread::sleep(Duration::from_millis(40)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
            }
        }

        child.wait_with_output().map(|out| String::from_utf8_lossy(&out.stdout).trim() == "found").unwrap_or(false)
    }

    pub fn focus_tab(prefixes: &[String], reload: bool) -> bool {
        let running = running();
        let is_running = |executable: &str| running.lines().any(|line| line.trim_end().ends_with(executable));
        let args: Vec<&str> = std::iter::once(if reload { "reload" } else { "keep" }).chain(prefixes.iter().map(String::as_str)).collect();

        BROWSERS
            .iter()
            .filter(|(executable, _, _)| is_running(executable))
            .any(|(_, app, chrome)| ask(&if *chrome { chrome_script(app) } else { safari_script() }, &args))
    }
}

/// Brings forward a browser tab showing one of `urls`, reloaded with `reload` (a server that
/// just came back); false when there is none (or asking the browsers isn't possible), and the
/// address should be opened instead.
pub fn focus_tab(urls: &[String], reload: bool) -> bool {
    let prefixes = prefixes(urls);
    if prefixes.is_empty() {
        return false;
    }

    #[cfg(target_os = "macos")]
    return mac::focus_tab(&prefixes, reload);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = reload;
        false
    }
}
