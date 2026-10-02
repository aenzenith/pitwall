//! The language Pitwall speaks. The texts live in `src/locales/*.json`, shared with the UI; the
//! core reads the same files for notifications, errors, issues and its own output lines.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Every language there is a catalog for, in the order Settings lists them.
pub const LANGUAGES: [&str; 7] = ["en", "tr", "de", "es", "ru", "ja", "zh-Hans"];

const SOURCES: [(&str, &str); 7] = [
    ("en", include_str!("../../src/locales/en.json")),
    ("tr", include_str!("../../src/locales/tr.json")),
    ("de", include_str!("../../src/locales/de.json")),
    ("es", include_str!("../../src/locales/es.json")),
    ("ru", include_str!("../../src/locales/ru.json")),
    ("ja", include_str!("../../src/locales/ja.json")),
    ("zh-Hans", include_str!("../../src/locales/zh-Hans.json")),
];

static CURRENT: RwLock<&'static str> = RwLock::new("en");

type Catalog = HashMap<String, String>;

fn catalogs() -> &'static HashMap<&'static str, Catalog> {
    static CATALOGS: OnceLock<HashMap<&'static str, Catalog>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(code, raw)| {
                let entries: HashMap<String, serde_json::Value> = serde_json::from_str(raw).unwrap_or_default();
                // Plural entries (objects) are the UI's; the core only uses plain texts.
                let texts = entries.into_iter().filter_map(|(key, value)| Some((key, value.as_str()?.to_string()))).collect();
                (*code, texts)
            })
            .collect()
    })
}

/// The first of the user's preferred languages (`tr-TR`, `zh-Hans-CN`, `pt_BR`) there is a
/// catalog for; English when there is none.
fn pick(tags: impl IntoIterator<Item = String>) -> &'static str {
    for tag in tags {
        let tag = tag.replace('_', "-").to_lowercase();
        let mut parts = tag.split('-');
        let primary = parts.next().unwrap_or_default();

        if primary == "zh" {
            let rest: Vec<&str> = parts.collect();
            // Traditional Chinese (Taiwan, Hong Kong, Macau) has no catalog yet.
            if !rest.contains(&"hans") && rest.iter().any(|part| matches!(*part, "hant" | "tw" | "hk" | "mo")) {
                continue;
            }
            return "zh-Hans";
        }

        if let Some(code) = LANGUAGES.iter().find(|code| **code == primary) {
            return code;
        }
    }

    "en"
}

/// The system's language, read once: macOS asks to restart apps when it changes. Tests speak
/// English whatever the machine's language is.
pub fn system() -> &'static str {
    static SYSTEM: OnceLock<&'static str> = OnceLock::new();
    SYSTEM.get_or_init(|| if cfg!(test) { "en" } else { pick(sys_locale::get_locales()) })
}

/// The language for the Settings value: a language code, or `system`.
pub fn resolve(setting: &str) -> &'static str {
    LANGUAGES.iter().find(|code| **code == setting).copied().unwrap_or_else(system)
}

pub fn set(language: &'static str) {
    *CURRENT.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = language;
}

pub fn current() -> &'static str {
    *CURRENT.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The text for `key` in the current language (English when it has none, the key itself when
/// English has none either), with `{name}` placeholders filled from `params`.
pub fn text(key: &str, params: &[(&str, String)]) -> String {
    let catalogs = catalogs();
    let found = catalogs.get(current()).and_then(|catalog| catalog.get(key)).or_else(|| catalogs.get("en")?.get(key));
    let mut text = found.cloned().unwrap_or_else(|| key.to_string());

    for (name, value) in params {
        text = text.replace(&format!("{{{name}}}"), value);
    }

    text
}

/// `t!("core.issue.notResponding", port = port)`: [`text`] with named parameters.
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::text($key, &[])
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::text($key, &[$((stringify!($name), $value.to_string())),+])
    };
}

pub(crate) use t;
