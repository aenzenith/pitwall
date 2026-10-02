//! A project's links (staging, production, the admin panel, the issue tracker…): addresses made
//! openable, and suggestions found in the project itself.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::resolve::parse_app_url;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinkSuggestion {
    pub name: String,
    pub url: String,
    /// Where it was found: `git`, `.env` or `package.json`.
    pub source: String,
}

/// The only schemes a link may have. Anything else (`file:`, an app's own scheme) could open a
/// local file or program, so it is never opened or suggested.
const SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// An address as typed, made openable: `staging.app.dev` becomes `https://staging.app.dev`.
/// `None` when it is empty or names a scheme other than `http`, `https` or `mailto`.
pub fn normalize(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    match scheme(url) {
        Some(scheme) => SCHEMES.iter().any(|allowed| scheme.eq_ignore_ascii_case(allowed)).then(|| url.to_string()),
        None => Some(format!("https://{url}")),
    }
}

/// The scheme an address names: `https` in `https://…`, `mailto` in `mailto:…`, `file` in
/// `file:/…`. A host with a port (`localhost:5173`, `app.test:8080/x`) names none.
fn scheme(url: &str) -> Option<&str> {
    let (head, rest) = url.split_once(':')?;
    let named = head.starts_with(|c: char| c.is_ascii_alphabetic()) && head.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    let port = rest.split(['/', '?', '#']).next().is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));

    (named && !port).then_some(head)
}

/// The web page of a git remote: `git@github.com:me/app.git` and
/// `https://token@github.com/me/app.git` both become `https://github.com/me/app`.
pub fn web_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let (host, path) = match remote.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit('@').next()?;
            // An ssh port is not the web server's.
            let host = if scheme == "http" || scheme == "https" { host } else { host.split(':').next()? };
            (host, path)
        }
        // scp-like: [user@]host:path
        None => {
            let (authority, path) = remote.split_once(':')?;
            (authority.rsplit('@').next()?, path)
        }
    };

    let path = path.trim_matches('/').trim_end_matches(".git");
    (host.contains('.') && !path.is_empty()).then(|| format!("https://{host}/{path}"))
}

/// The pages a code host has for a repository: itself, its reviews, its CI.
fn forge_pages(web: &str) -> Vec<(String, String)> {
    let host = web.trim_start_matches("https://").split('/').next().unwrap_or_default().to_string();
    let page = |suffix: &str| format!("{web}/{suffix}");

    if host.contains("github") {
        vec![("GitHub".into(), web.into()), ("Pull requests".into(), page("pulls")), ("Actions".into(), page("actions"))]
    } else if host.contains("gitlab") {
        vec![("GitLab".into(), web.into()), ("Merge requests".into(), page("-/merge_requests")), ("Pipelines".into(), page("-/pipelines"))]
    } else if host.contains("bitbucket") {
        vec![("Bitbucket".into(), web.into()), ("Pull requests".into(), page("pull-requests")), ("Pipelines".into(), page("pipelines"))]
    } else {
        vec![(host, web.into())]
    }
}

/// Addresses the project itself names: its git remote's pages, `APP_URL` in `.env`, and
/// `homepage` in `package.json`.
pub fn suggestions(path: &str) -> Vec<LinkSuggestion> {
    let mut found = Vec::new();
    let mut add = |name: String, url: String, source: &str| {
        if !found.iter().any(|s: &LinkSuggestion| s.url == url) {
            found.push(LinkSuggestion { name, url, source: source.into() });
        }
    };

    let remote = Command::new("git")
        .args(["-C", path, "remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());
    if let Some(web) = remote.as_deref().and_then(web_url) {
        for (name, url) in forge_pages(&web) {
            add(name, url, "git");
        }
    }

    let folder = Path::new(path);
    if let Some(url) = fs::read_to_string(folder.join(".env")).ok().as_deref().and_then(parse_app_url) {
        add("APP_URL".into(), url, ".env");
    }

    let homepage = fs::read_to_string(folder.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("homepage").and_then(|h| h.as_str()).map(str::to_string))
        .filter(|url| url.starts_with("http"));
    if let Some(url) = homepage {
        add("Homepage".into(), url, "package.json");
    }

    // Only what would open: a project's files can't suggest a local file or app.
    found.retain(|suggestion| normalize(&suggestion.url).as_deref() == Some(suggestion.url.as_str()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_and_mail_addresses_open() {
        for (typed, opened) in [
            ("staging.app.dev", "https://staging.app.dev"),
            ("localhost:5173/admin", "https://localhost:5173/admin"),
            (" https://app.test ", "https://app.test"),
            ("HTTP://app.test", "HTTP://app.test"),
            ("mailto:team@app.dev", "mailto:team@app.dev"),
        ] {
            assert_eq!(normalize(typed).as_deref(), Some(opened), "{typed}");
        }

        for refused in ["file:///Applications/Calculator.app", "file:/etc/passwd", "FILE://x", "vscode://file/tmp", "javascript:alert(1)", "smb://server/share", "ssh://host", ""] {
            assert_eq!(normalize(refused), None, "{refused}");
        }
    }
}
