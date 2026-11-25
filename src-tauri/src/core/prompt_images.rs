//! Images pasted into the empty terminal panel's box for Claude's or Codex's tab
//! (`window/TerminalEmpty.vue`). An agent in a terminal sees a picture only as a file, so the core
//! writes each one into a folder of its own under `prompt-images/`, beside the settings and
//! readable by the user alone, names the files under the prompt as a given board card names its
//! images (`[Image #n] <path>`), and the agent starts with that folder among its directories
//! (`--add-dir`), so it reads them without asking. Only the bytes of an image are written
//! (`board::decode_image`: by how they start), under names the core makes. Nothing else of them
//! is kept: a folder goes once it is a day old, swept at a start and whenever another is written;
//! the agent read its files at its first turn.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::board::{decode_image, IMAGES_MAX};
use super::Core;
use crate::i18n::t;
use crate::start::{is_uuid, new_uuid};

/// The folder beside the settings that holds them, one folder per start.
pub(super) const DIR: &str = "prompt-images";

/// How long a start's images stay.
const KEPT: Duration = Duration::from_secs(24 * 60 * 60);

impl Core {
    /// `prompt` (the box's text as `first_prompt` left it) with `images` (`data:` URLs) written
    /// and named under it, and the folder they are in; as it was, and none, without images. On
    /// Windows the agent takes no first prompt on its line, so none is written there. Refused
    /// whole, with nothing written, when one isn't an image or there are too many.
    pub(super) fn with_images(&self, prompt: Option<String>, images: &[String]) -> Result<(Option<String>, Option<PathBuf>), String> {
        if images.is_empty() || cfg!(windows) {
            return Ok((prompt, None));
        }
        if images.len() > IMAGES_MAX {
            return Err(format!("at most {IMAGES_MAX} images"));
        }
        let decoded = images.iter().map(|data| decode_image(data)).collect::<Result<Vec<_>, _>>()?;

        let root = self.cfg.settings_file.with_file_name(DIR);
        sweep(&root, SystemTime::now());
        let dir = root.join(new_uuid());
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        private(&root);
        private(&dir);

        let mut text = prompt.map(|text| format!("{}\n\n", text.trim_end())).unwrap_or_default();
        text.push_str(&t!("board.imagesPrompt"));
        for (at, (kind, bytes)) in decoded.iter().enumerate() {
            let n = at + 1;
            let file = dir.join(format!("{n}.{}", kind.ext()));
            if let Err(error) = fs::write(&file, bytes) {
                let _ = fs::remove_dir_all(&dir);
                return Err(error.to_string());
            }
            text.push_str(&format!("\n[Image #{n}] {}", file.display()));
        }
        Ok((Some(text), Some(dir)))
    }
}

/// Deletes the start folders under `root` (named as the core names them) last written a day or
/// more before `now`, with what is in them; nothing else there, and nothing through a link.
pub(super) fn sweep(root: &Path, now: SystemTime) {
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        let ours = entry.file_name().to_str().is_some_and(is_uuid);
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        let old = meta.modified().ok().and_then(|at| now.duration_since(at).ok()).is_some_and(|age| age >= KEPT);
        if ours && meta.is_dir() && old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(unix)]
fn private(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn private(_dir: &Path) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::core::CoreConfig;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use std::sync::Arc;

    #[test]
    fn images_are_written_named_and_swept_after_a_day() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let core = Core::new(
            CoreConfig {
                registry_dir: tmp.path().join("registry"),
                claude_dir: tmp.path().join("claude").join("projects"),
                codex_home: tmp.path().join("codex"),
                settings_file: config.join("settings.json"),
                claude_settings: tmp.path().join("claude").join("settings.json"),
                legacy_registries: Vec::new(),
                title: "Pitwall".into(),
            },
            Arc::new(|_| {}),
        );
        let url = |bytes: &[u8]| format!("data:image/png;base64,{}", BASE64.encode(bytes));
        let png = |fill: u8| url(&[&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..], &[fill; 16][..]].concat());
        let root = config.join(DIR);

        // No images: the prompt as it was, no folder.
        assert_eq!(core.with_images(Some("fix it".into()), &[]).unwrap(), (Some("fix it".into()), None));

        // A file that isn't an image refuses the lot, and nothing is written.
        assert!(core.with_images(Some("fix it".into()), &[png(1), url(b"#!/bin/sh\nrm -rf ~\n")]).is_err());
        assert!(!root.exists());

        // Each image in a file of the core's naming, named under the text, the folder handed back.
        let (prompt, dir) = core.with_images(Some("fix it".into()), &[png(1), png(2)]).unwrap();
        let (prompt, dir) = (prompt.unwrap(), dir.unwrap());
        assert_eq!(dir.parent(), Some(root.as_path()));
        assert!(prompt.starts_with("fix it\n\n"));
        for n in [1, 2] {
            let file = dir.join(format!("{n}.png"));
            assert_eq!(fs::read(&file).unwrap()[8..], [n as u8; 16]);
            assert!(prompt.contains(&format!("[Image #{n}] {}", file.display())));
        }

        // A day on, the sweep takes the start's folder and leaves what isn't one of them.
        let stray = root.join("keep-me");
        fs::create_dir_all(&stray).unwrap();
        let day = SystemTime::now() + KEPT + Duration::from_secs(1);
        sweep(&root, SystemTime::now());
        assert!(dir.exists());
        sweep(&root, day);
        assert!(!dir.exists());
        assert!(stray.exists());
    }
}
