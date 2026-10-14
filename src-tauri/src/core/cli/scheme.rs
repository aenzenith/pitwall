//! `pitwall://` links, for Raycast, Shortcuts and scripts. Any app and any web page can open
//! one, so a link says one of four things and nothing else:
//!
//! `pitwall://start?project=<name>`, `stop`, `restart`, `show` (and `show` alone, for the
//! window). The name is a listed project's, whole. Whatever else a link holds (another word, a
//! path, a second parameter, a fragment, a name that isn't written out as one) makes it no link
//! at all: nothing of it is read further, kept, shown or said.

use super::*;

const SCHEME: &str = "pitwall://";
/// A project's name in a link is no longer than this, once it is read back.
const NAME_MAX: usize = 200;

/// What a link asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verb {
    /// Start, stop or restart the project's dev server.
    Act(Action),
    /// Bring Pitwall's window up, on the project when one is named.
    Show,
}

impl Verb {
    pub(super) fn parse(word: &str) -> Option<Self> {
        match word {
            "show" => Some(Self::Show),
            word => Action::parse(word).map(Self::Act),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Act(Action::Start) => "start",
            Self::Act(Action::Stop) => "stop",
            Self::Act(Action::Restart) => "restart",
            Self::Show => "show",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Link {
    pub(super) action: Verb,
    /// The project's name, as the link gives it.
    pub(super) project: Option<String>,
}

/// A name as a link carries it: every byte but letters, digits and `-._~` as `%XX`.
fn escaped(name: &str) -> String {
    name.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) { char::from(byte).to_string() } else { format!("%{byte:02X}") }).collect()
}

/// A name read back out of a link. None when it isn't written as one: a character that means
/// something else in a link (`&`, `=`, `#`, `?`, `/`, `+`, a space…) standing for itself, a `%`
/// with no two hex digits after it, bytes that are no text, a control character, or none or too
/// many characters at all.
fn name(written: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(written.len());
    let mut rest = written.bytes();

    while let Some(byte) = rest.next() {
        match byte {
            b'%' => {
                let digits = [rest.next()?, rest.next()?];
                let hex = std::str::from_utf8(&digits).ok().filter(|hex| hex.bytes().all(|digit| digit.is_ascii_hexdigit()))?;
                bytes.push(u8::from_str_radix(hex, 16).ok()?);
            }
            byte if byte.is_ascii_alphanumeric() || b"-._~!*'(),;:@$".contains(&byte) => bytes.push(byte),
            _ => return None,
        }
    }

    let name = String::from_utf8(bytes).ok()?;
    let fits = !name.is_empty() && name.chars().count() <= NAME_MAX && !name.chars().any(char::is_control);
    fits.then_some(name)
}

/// The link `url` is, if it is one: the four words, each with a project's name, and `show`
/// without one.
pub(super) fn parse(url: &str) -> Option<Link> {
    let rest = url.strip_prefix(SCHEME)?;
    let (word, project) = match rest.split_once('?') {
        Some((word, query)) => (word, Some(name(query.strip_prefix("project=")?)?)),
        None => (rest, None),
    };
    let action = Verb::parse(word)?;

    // To start or stop something, a link says what.
    (action == Verb::Show || project.is_some()).then_some(Link { action, project })
}

/// The link that asks `action` of the project named `project`.
pub(super) fn text(action: Verb, project: Option<&str>) -> String {
    match project {
        Some(project) => format!("{SCHEME}{}?project={}", action.word(), escaped(project)),
        None => format!("{SCHEME}{}", action.word()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any app or page can open a link: only the four words pass, each on a project's name
    /// written out as one, and everything else is no link.
    #[test]
    fn only_the_four_words_on_a_name_are_a_link() {
        let link = |action, project: Option<&str>| Some(Link { action, project: project.map(str::to_string) });

        assert_eq!(parse("pitwall://start?project=web-harbor42"), link(Verb::Act(Action::Start), Some("web-harbor42")));
        assert_eq!(parse("pitwall://stop?project=web-harbor42"), link(Verb::Act(Action::Stop), Some("web-harbor42")));
        assert_eq!(parse("pitwall://restart?project=web-harbor42"), link(Verb::Act(Action::Restart), Some("web-harbor42")));
        assert_eq!(parse("pitwall://show?project=web-harbor42"), link(Verb::Show, Some("web-harbor42")));
        assert_eq!(parse("pitwall://show"), link(Verb::Show, None));

        let no_links = [
            // Other words: nothing runs a command, writes a card or reads anything through a link.
            "pitwall://run?project=web",
            "pitwall://run?project=web&command=rm",
            "pitwall://card?project=web",
            "pitwall://logs?project=web",
            "pitwall://open?project=web",
            "pitwall://edit?project=web",
            "pitwall://quiet",
            "pitwall://status",
            "pitwall://START?project=web",
            "pitwall://",
            // The three that act say on what.
            "pitwall://start",
            "pitwall://stop",
            "pitwall://restart",
            "pitwall://start?",
            "pitwall://start?project=",
            // More than the one parameter, or another one.
            "pitwall://start?project=web&project=api",
            "pitwall://start?project=web&x=1",
            "pitwall://start?path=/Users/x/web",
            "pitwall://start?x=1&project=web",
            "pitwall://show?",
            "pitwall://show?page=settings",
            // A path, a fragment, a port, somebody's name before the word.
            "pitwall://start/?project=web",
            "pitwall://start/web",
            "pitwall://show/",
            "pitwall://show#top",
            "pitwall://start?project=web#x",
            "pitwall://start:80?project=web",
            "pitwall://user@start?project=web",
            "pitwall:///start?project=web",
            // A name that isn't written out as one.
            "pitwall://start?project=web site",
            "pitwall://start?project=web+site",
            "pitwall://start?project=a/b",
            "pitwall://start?project=..%2",
            "pitwall://start?project=%zz",
            "pitwall://start?project=%FF%FE",
            "pitwall://start?project=web%0Arm",
            "pitwall://start?project=web%00",
            "pitwall://start?project=a=b",
            "pitwall://start?project=a?b",
            // Another scheme, or none.
            "pitwall:start?project=web",
            "Pitwall://start?project=web",
            "http://start?project=web",
            " pitwall://show",
            "start?project=web",
            "",
        ];
        for url in no_links {
            assert_eq!(parse(url), None, "{url}");
        }
        let long = format!("pitwall://start?project={}", "a".repeat(NAME_MAX + 1));
        assert_eq!(parse(&long), None);
    }

    /// A row's "Copy link" gives a link that says the same project back, whatever its name
    /// holds.
    #[test]
    fn a_copied_link_names_its_project_again() {
        for name in ["web-harbor42", "my project", "çöktü & co", "a/b?c=d#e", "100%", "日本語 プロジェクト", "o'brien (old)", "x+y"] {
            for action in [Verb::Act(Action::Start), Verb::Act(Action::Stop), Verb::Act(Action::Restart), Verb::Show] {
                let url = text(action, Some(name));
                assert!(url.is_ascii() && !url.contains(' '), "{url}");
                assert_eq!(parse(&url), Some(Link { action, project: Some(name.to_string()) }), "{url}");
                // And as the system hands it over (`RunEvent::Opened`): read as a URL first.
                assert_eq!(tauri::Url::parse(&url).unwrap().as_str(), url);
            }
        }
        assert_eq!(text(Verb::Act(Action::Restart), Some("web-harbor42")), "pitwall://restart?project=web-harbor42");
        assert_eq!(parse(&text(Verb::Show, None)), Some(Link { action: Verb::Show, project: None }));
    }
}
