//! The shells a new terminal can start on Windows (Command Prompt, Windows PowerShell,
//! PowerShell 7), and an administrator's shell. The window names a shell by a word of a fixed
//! list, never by a path or a command line (`Shell::parse`): where each one is installed is found
//! here when it is asked for, and that and the mode Windows' sudo runs in are all that is read for
//! it, nothing of it kept or logged. An administrator's shell opens only when the user picks one,
//! Windows asking for its permission first: in the tab through sudo when sudo runs inline, else in
//! a window of its own. Elsewhere a tab has the user's own shell, as before.

use portable_pty::CommandBuilder;
use serde::{Deserialize, Deserializer};

use super::terminal::Launch;
use super::*;

/// A shell of the list, by the word the window and the records name it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    /// Command Prompt: `%ComSpec%` (else Windows' own), what a tab ran on Windows before there was
    /// a choice.
    Cmd,
    /// Windows PowerShell, the one Windows comes with.
    Powershell,
    /// PowerShell 7, once it is installed.
    Pwsh,
}

impl Shell {
    /// In the menu's order.
    #[cfg(windows)]
    const ALL: [Self; 3] = [Self::Cmd, Self::Powershell, Self::Pwsh];

    /// One of the list, or nothing: no other text names a shell.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "cmd" => Some(Self::Cmd),
            "powershell" => Some(Self::Powershell),
            "pwsh" => Some(Self::Pwsh),
            _ => None,
        }
    }

    /// Its name as Windows gives it, in the language of the moment: the tab's title.
    pub(super) fn name(self) -> String {
        match self {
            Self::Cmd => t!("terminal.shell.cmd"),
            Self::Powershell => t!("terminal.shell.powershell"),
            Self::Pwsh => t!("terminal.shell.pwsh"),
        }
    }
}

/// A shell as a record has it: one of the list, and nothing for anything else (another type
/// too), so a value this version doesn't know costs neither the record nor its tab.
pub(super) fn shell_or_none<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Shell>, D::Error> {
    Ok(serde_json::Value::deserialize(deserializer)?.as_str().and_then(Shell::parse))
}

/// The shell of the list a shell tab runs: on Windows the one asked for, Command Prompt when none
/// was; elsewhere none (the user's own shell).
pub(super) fn on_this_system(shell: Option<Shell>) -> Option<Shell> {
    if cfg!(windows) {
        Some(shell.unwrap_or(Shell::Cmd))
    } else {
        None
    }
}

/// The shells the "+" menu offers, and where an administrator's opens.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalShells {
    /// The installed ones, in the menu's order; none where there is no choice (macOS, Linux).
    pub shells: Vec<Shell>,
    /// `inline`: an administrator's shell opens in a tab (sudo runs inline); `window`: in a window
    /// of its own. None where there is no choice.
    pub elevate: Option<&'static str>,
}

/// What came of opening a shell.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalOpened {
    /// A tab of the panel.
    Tab { view: TerminalView },
    /// An administrator's shell in a window of its own: sudo doesn't run inline.
    Window,
    /// Windows wasn't given the permission: nothing opened.
    Denied,
}

impl Core {
    /// A shell tab in the project's folder: on Windows the shell of the list picked (`shell`),
    /// elsewhere the user's own. `elevated`, an administrator's (Windows only): in the tab through
    /// sudo when sudo runs inline, else in a window of its own, Windows asking for the permission
    /// first either way. Waits for that answer, so not on the main thread.
    pub fn open_shell(self: &Arc<Self>, path: &str, shell: Option<Shell>, elevated: bool, size: Option<(u16, u16)>) -> Result<TerminalOpened, String> {
        if elevated && !sudo_inline() {
            if !Path::new(path).is_dir() {
                return Err(t!("core.error.notFolder", path = path));
            }
            let shell = on_this_system(shell).ok_or("no administrator's shell on this system")?;
            return Ok(if elevate_in_window(shell, path)? { TerminalOpened::Window } else { TerminalOpened::Denied });
        }
        let view = self.open_terminal_as(path, &Launch::Shell { shell, elevated }, None, false, size, false)?;
        Ok(TerminalOpened::Tab { view })
    }

    /// The terminal panel said that the last quit's or update's administrator's tabs weren't put
    /// back.
    pub fn admin_not_restored_seen(&self) {
        let seen = std::mem::take(&mut self.lock().admin_not_restored) > 0;
        if seen {
            self.notify();
        }
    }
}

/// The shells installed here and where an administrator's opens, asked of the system now.
#[cfg(windows)]
pub fn installed_shells() -> TerminalShells {
    let shells = Shell::ALL.into_iter().filter(|shell| win::program(*shell).is_some()).collect();
    TerminalShells { shells, elevate: Some(if win::sudo_inline() { "inline" } else { "window" }) }
}

#[cfg(not(windows))]
pub fn installed_shells() -> TerminalShells {
    TerminalShells { shells: Vec::new(), elevate: None }
}

/// What a shell tab starts: on Windows the shell of the list, through sudo for an
/// administrator's; elsewhere the user's own shell (`shell` is none there).
#[cfg(windows)]
pub(super) fn command(shell: Option<Shell>, elevated: bool) -> Result<CommandBuilder, String> {
    win::command(shell.unwrap_or(Shell::Cmd), elevated)
}

#[cfg(not(windows))]
pub(super) fn command(_shell: Option<Shell>, elevated: bool) -> Result<CommandBuilder, String> {
    if elevated {
        return Err("no administrator's shell on this system".into());
    }
    Ok(CommandBuilder::new_default_prog())
}

/// Sudo runs inline: an administrator's shell can open in Pitwall's own tab.
#[cfg(windows)]
fn sudo_inline() -> bool {
    win::sudo_inline()
}

#[cfg(not(windows))]
fn sudo_inline() -> bool {
    false
}

/// Opens an administrator's `shell` in a window of its own, in the project's folder: true once
/// it opened, false when the permission wasn't given.
#[cfg(windows)]
fn elevate_in_window(shell: Shell, folder: &str) -> Result<bool, String> {
    win::elevate_in_window(shell, folder)
}

#[cfg(not(windows))]
fn elevate_in_window(_shell: Shell, _folder: &str) -> Result<bool, String> {
    Err("no administrator's shell on this system".into())
}

/// What an administrator's shell opened in a window of its own is started with. Windows starts it
/// in its own folder whatever it was asked, so the shell is told to go to the project's folder
/// itself, written so that nothing in its name runs. Command Prompt: every sign but a path's own
/// letters gets a `^` before it (a `%` too, so no variable takes its place), delayed expansion
/// off (`!`). PowerShell: one single-quoted string (its quotes doubled, the curly ones too, which
/// PowerShell reads as quotes as well) in a command handed over encoded, which no command line
/// takes apart. None for a folder with a `"` or a control character in it: no path holds one.
#[cfg(any(windows, test))]
fn elevated_args(shell: Shell, folder: &str) -> Option<String> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;

    if folder.chars().any(|sign| sign == '"' || sign.is_control()) {
        return None;
    }
    Some(match shell {
        Shell::Cmd => {
            let mut escaped = String::new();
            for sign in folder.replace('/', "\\").chars() {
                if sign.is_ascii() && !sign.is_ascii_alphanumeric() && !matches!(sign, '\\' | ':' | '.' | '_' | '-') {
                    escaped.push('^');
                }
                escaped.push(sign);
            }
            format!("/E:ON /V:OFF /S /K \"cd /d {escaped}\"")
        }
        Shell::Powershell | Shell::Pwsh => {
            let mut quoted = String::from("'");
            for sign in folder.chars() {
                if matches!(sign, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
                    quoted.push(sign);
                }
                quoted.push(sign);
            }
            quoted.push('\'');
            let script = format!("Set-Location -LiteralPath {quoted}");
            let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            format!("-NoExit -EncodedCommand {}", BASE64.encode(bytes))
        }
    })
}

#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use portable_pty::CommandBuilder;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};

    use super::{elevated_args, Shell};

    /// `SW_SHOWNORMAL`: the window as it opens by itself.
    const SW_SHOWNORMAL: i32 = 1;
    /// Sudo's `Enabled` when it runs inline, in the console it was started from.
    const SUDO_INLINE: u32 = 3;

    /// Windows' folder, by its whole path.
    fn windows_folder() -> PathBuf {
        std::env::var_os("SystemRoot").map(PathBuf::from).filter(|folder| folder.is_absolute()).unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
    }

    /// Command Prompt by its whole path: `%ComSpec%` when it is one and the file is there, else
    /// Windows' own. Never a bare name, which Windows would look for in the folder it starts it
    /// in (the project's) first.
    fn command_prompt() -> PathBuf {
        std::env::var_os("ComSpec").map(PathBuf::from).filter(|file| file.is_absolute() && file.is_file()).unwrap_or_else(|| windows_folder().join(r"System32\cmd.exe"))
    }

    /// Where the shell is, by its whole path, when it is installed.
    pub fn program(shell: Shell) -> Option<PathBuf> {
        match shell {
            Shell::Cmd => Some(command_prompt()),
            Shell::Powershell => Some(windows_folder().join(r"System32\WindowsPowerShell\v1.0\powershell.exe")).filter(|file| file.is_file()),
            Shell::Pwsh => pwsh(),
        }
    }

    /// PowerShell 7: where its installer puts it, else the first `pwsh.exe` in a folder of the
    /// PATH (the Store's is an alias there, which only the link's own data tells of).
    fn pwsh() -> Option<PathBuf> {
        let installed = std::env::var_os("ProgramFiles").map(|folder| Path::new(&folder).join(r"PowerShell\7\pwsh.exe")).filter(|file| file.is_absolute() && file.is_file());
        installed.or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH")?).filter(|folder| folder.is_absolute()).map(|folder| folder.join("pwsh.exe")).find(|file| file.symlink_metadata().is_ok())
        })
    }

    /// Sudo, by its whole path in Windows' own folder.
    fn sudo() -> PathBuf {
        windows_folder().join(r"System32\sudo.exe")
    }

    /// The shell an administrator's runs: by its whole path, or not at all.
    fn elevated_program(shell: Shell) -> Result<PathBuf, String> {
        program(shell).filter(|program| program.is_absolute()).ok_or_else(|| format!("{shell:?} isn't installed"))
    }

    /// Sudo is on and runs inline (Settings › System › For developers).
    pub fn sudo_inline() -> bool {
        let enabled = windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Sudo").and_then(|key| key.get_u32("Enabled"));
        enabled.is_ok_and(|mode| mode == SUDO_INLINE) && sudo().is_file()
    }

    /// The shell, or sudo with it for an administrator's: sudo asks Windows for the permission,
    /// and the shell it starts runs in the tab, in the folder the tab started in.
    pub fn command(shell: Shell, elevated: bool) -> Result<CommandBuilder, String> {
        if !elevated {
            return Ok(CommandBuilder::new(program(shell).ok_or_else(|| format!("{shell:?} isn't installed"))?));
        }
        let mut command = CommandBuilder::new(sudo());
        command.arg(elevated_program(shell)?);
        Ok(command)
    }

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Windows' own "Run as administrator": its permission dialog first, then the shell in a
    /// window of its own. Waits for the answer.
    pub fn elevate_in_window(shell: Shell, folder: &str) -> Result<bool, String> {
        let arguments = elevated_args(shell, folder).ok_or_else(|| format!("not a folder: {folder}"))?;
        let program = elevated_program(shell)?;
        let (verb, file, parameters, directory) = (wide(OsStr::new("runas")), wide(program.as_os_str()), wide(OsStr::new(&arguments)), wide(OsStr::new(folder)));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: directory.as_ptr(),
            nShow: SW_SHOWNORMAL,
            ..Default::default()
        };
        // SAFETY: the struct's size is set, and every string it points to is NUL-terminated and
        // outlives the call.
        if unsafe { ShellExecuteExW(&mut info) } != 0 {
            return Ok(true);
        }
        // SAFETY: reads the calling thread's last error, which the failed call set.
        match unsafe { GetLastError() } {
            ERROR_CANCELLED => Ok(false),
            error => Err(std::io::Error::from_raw_os_error(error as i32).to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;

    /// The script an encoded PowerShell command runs.
    fn decoded(arguments: &str) -> String {
        let encoded = arguments.strip_prefix("-NoExit -EncodedCommand ").unwrap_or_else(|| panic!("{arguments}"));
        let bytes = BASE64.decode(encoded).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        String::from_utf16(&units).unwrap()
    }

    /// An administrator's shell in a window of its own is told its folder on its command line:
    /// nothing in the folder's name may run there with the administrator's rights. For Command
    /// Prompt every sign that means something to it is escaped (a `%` too, so no variable takes
    /// its place); for PowerShell the folder is one single-quoted string, every quote of it
    /// doubled; and a name no path can have gives no command line at all.
    #[test]
    fn nothing_in_a_folders_name_runs_in_an_administrators_shell() {
        // Each sign Command Prompt reads, alone in a name, escaped.
        for sign in [' ', '%', '!', '&', '|', '^', '<', '>', '(', ')', '\'', ';', ',', '=', '@', '`', '$', '[', ']', '{', '}', '~', '+', '#'] {
            assert_eq!(elevated_args(Shell::Cmd, &format!(r"C:\a{sign}b")).as_deref(), Some(format!(r#"/E:ON /V:OFF /S /K "cd /d C:\a^{sign}b""#).as_str()), "{sign:?}");
        }
        // A whole name: a variable, a command after it, a path's own signs and letters left as
        // they are, forward slashes as Windows writes them.
        assert_eq!(
            elevated_args(Shell::Cmd, r"C:/Users/Ali Veli/100%PATH%&calc!x!^(it's)/çalışma.d_1-2").as_deref(),
            Some(r#"/E:ON /V:OFF /S /K "cd /d C:\Users\Ali^ Veli\100^%PATH^%^&calc^!x^!^^^(it^'s^)\çalışma.d_1-2""#),
        );

        // PowerShell's own quotes, the curly ones too, doubled in one literal string; nothing of
        // PowerShell's syntax is read in it.
        for shell in [Shell::Powershell, Shell::Pwsh] {
            let arguments = elevated_args(shell, "C:\\p\\it's \u{2018}q\u{2019} \u{201a}r\u{201b} $env:X `n $(calc); calc").unwrap();
            assert_eq!(decoded(&arguments), "Set-Location -LiteralPath 'C:\\p\\it''s \u{2018}\u{2018}q\u{2019}\u{2019} \u{201a}\u{201a}r\u{201b}\u{201b} $env:X `n $(calc); calc'");
            assert_eq!(decoded(&elevated_args(shell, r"C:\Users\alex\paddock").unwrap()), r"Set-Location -LiteralPath 'C:\Users\alex\paddock'");
        }

        // A quote or a control character ends the string or the line: no command line for either.
        for shell in [Shell::Cmd, Shell::Powershell, Shell::Pwsh] {
            for name in [r#"C:\a" & calc & ""#, "C:\\a\r\ncalc", "C:\\a\u{7}b", "C:\\a\u{1b}b"] {
                assert_eq!(elevated_args(shell, name), None, "{shell:?} {name:?}");
            }
        }
    }
}
