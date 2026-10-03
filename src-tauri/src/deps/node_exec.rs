//! How the migration tools of the Node world are run: each is a package of the project, started
//! through the runner of the project's package manager, which `node.rs` tells by the lock file.
//! Every spelling runs only what the project has installed: none downloads a missing package
//! (`npx` and `bunx` would, without `--no-install`), none installs first (`pnpm exec` would when
//! the packages differ from the lock file, without `verify-deps-before-run=false`), and none
//! runs a `package.json` script of the same name (`yarn run` would).

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::{read_json, MigrationsError};

/// A tool's two commands through each runner, in the order `runner` counts: npm, pnpm, yarn,
/// bun. `$status` and `$migrate` start with the tool's executable (`prisma migrate status`).
macro_rules! through_runners {
    ($status:literal, $migrate:literal) => {
        [
            $crate::deps::Commands {
                status: concat!("npx --no-install ", $status),
                migrate: concat!("npx --no-install ", $migrate),
                needs: &["npx"],
            },
            $crate::deps::Commands {
                status: concat!("pnpm --config.verify-deps-before-run=false exec ", $status),
                migrate: concat!("pnpm --config.verify-deps-before-run=false exec ", $migrate),
                needs: &["pnpm"],
            },
            $crate::deps::Commands { status: concat!("yarn exec ", $status), migrate: concat!("yarn exec ", $migrate), needs: &["yarn"] },
            $crate::deps::Commands {
                status: concat!("bunx --no-install ", $status),
                migrate: concat!("bunx --no-install ", $migrate),
                needs: &["bunx"],
            },
        ]
    };
}
pub(super) use through_runners;

/// Which of `through_runners!`'s four the project runs: its package manager's.
pub(super) fn runner(dir: &Path) -> usize {
    match super::ecosystem("npm").and_then(|node| node.detect(dir)) {
        Some("pnpm") => 1,
        Some("yarn") => 2,
        Some("bun") => 3,
        _ => 0,
    }
}

/// The project's package.json when it lists `package` in `dependencies` or `devDependencies`,
/// with the range it asks for.
pub(super) fn depends(dir: &Path, package: &str) -> Option<(Value, String)> {
    let manifest = read_json(&dir.join("package.json"))?;
    let range = ["dependencies", "devDependencies"].iter().find_map(|key| manifest.get(*key)?.get(package)?.as_str().map(str::to_string))?;
    Some((manifest, range))
}

/// A command's output without the colour codes: the tools colour theirs even into a pipe when
/// `FORCE_COLOR` is set at all, as `process::spawn_shell` sets it.
pub(super) fn plain(output: &str) -> String {
    static ESCAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());
    ESCAPE.replace_all(output, "").into_owned()
}

/// Why a status command that failed gave no list: the project hasn't installed the tool
/// (`executable`), its database refused or couldn't be reached, or anything else.
pub(super) fn failure(output: &str, executable: &str) -> MigrationsError {
    const UNREACHABLE: [&str; 14] = [
        "econnrefused",
        "enotfound",
        "eai_again",
        "etimedout",
        "ehostunreach",
        "getaddrinfo",
        "connection refused",
        "access denied",
        "password authentication failed",
        "timeout acquiring a connection",
        "unable to open database file",
        "sqlite_cantopen",
        "unknown database",
        "connectionrefusederror",
    ];
    let text = output.to_lowercase();
    let missing = [
        "npx canceled due to missing packages".to_string(),
        format!("command \"{executable}\" not found"),
        format!("command not found: {executable}"),
        format!("couldn't find the binary {executable}"),
        format!("could not find an existing '{executable}' binary"),
    ];
    if missing.iter().any(|needle| text.contains(needle)) {
        MigrationsError::ToolMissing
    } else if UNREACHABLE.iter().any(|needle| text.contains(needle)) {
        MigrationsError::Unreachable
    } else {
        MigrationsError::Failed
    }
}
