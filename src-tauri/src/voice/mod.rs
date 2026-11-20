//! Voice messages in the chat: the user speaks, Pitwall writes it down on this machine and the
//! chat sends it as text. The microphone is open only while a take records; the take stays in
//! memory, goes to the engine at its end and is dropped; no sound is written or sent anywhere.
//! The one thing fetched over the network is a model's file, once, by its fixed address.
//!
//! The window drives a take by its id (`voice_start`, `voice_by_itself`, `voice_stop`,
//! `voice_cancel`) and hears of it on the `voice` event: its level while it records, then
//! `transcribing`, then `done` with the text (or none: no speech) or `failed`.

pub mod capture;
pub mod engine;
pub mod models;

use models::{Fetch, Model};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// A loaded model is let go after this long without a voice message.
const IDLE: Duration = Duration::from_secs(5 * 60);
/// The level is sent this often at most (the meter needs no more).
const LEVEL_EVERY: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    Level { id: String, level: f32 },
    Transcribing { id: String },
    /// `text` is `None` when the take held no speech; `end` is `stopped`, `quiet` or `limit`.
    Done { id: String, text: Option<String>, end: &'static str },
    /// `problem`: `denied`, `noDevice`, `failed`, `missing`.
    Failed { id: String, problem: &'static str },
    Cancelled { id: String },
    Download { model: Model, done: u64, total: u64 },
    /// `problem`: `network`, `http`, `size`, `hash`, `write`, or none when it went well or was cancelled.
    Downloaded { model: Model, ok: bool, problem: Option<&'static str> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    pub model: Model,
    pub ready: bool,
    /// The model's size with the gate, as the download costs it.
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Whether this machine can write voice messages down (`engine::supported`).
    pub supported: bool,
    pub models: Vec<ModelState>,
    pub downloading: Option<Downloading>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Downloading {
    pub model: Model,
    pub done: u64,
    pub total: u64,
}

struct Take {
    id: String,
    controls: capture::Controls,
}

struct Pull {
    model: Model,
    cancel: Arc<AtomicBool>,
    done: u64,
    total: u64,
}

struct Inner {
    dir: PathBuf,
    engine: engine::Engine,
    take: Mutex<Option<Take>>,
    pull: Mutex<Option<Pull>>,
}

#[derive(Clone)]
pub struct Voice {
    inner: Arc<Inner>,
}

impl Voice {
    /// The models live in `dir` (`<app data>/voice`).
    pub fn new(dir: PathBuf) -> Self {
        let voice = Self { inner: Arc::new(Inner { dir, engine: engine::Engine::default(), take: Mutex::new(None), pull: Mutex::new(None) }) };
        let idle = Arc::downgrade(&voice.inner);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            match idle.upgrade() {
                Some(inner) => inner.engine.let_go_after(IDLE),
                None => break,
            }
        });
        voice
    }

    pub fn status(&self) -> Status {
        let dir = &self.inner.dir;
        let downloading = lock(&self.inner.pull).as_ref().map(|pull| Downloading { model: pull.model, done: pull.done, total: pull.total });
        Status {
            supported: engine::supported(),
            models: Model::ALL
                .iter()
                .map(|&model| ModelState { model, ready: models::ready(dir, model), bytes: model.file().bytes + models::GATE.bytes })
                .collect(),
            downloading,
        }
    }

    /// Starts fetching the model's files (and the gate's) unless they are there; one download at a time.
    pub fn download(&self, app: &AppHandle, model: Model) -> Result<(), String> {
        let missing = models::missing(&self.inner.dir, model);
        if missing.is_empty() {
            emit(app, Event::Downloaded { model, ok: true, problem: None });
            return Ok(());
        }
        let total: u64 = missing.iter().map(|file| file.bytes).sum();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut pull = lock(&self.inner.pull);
            if pull.is_some() {
                return Err("busy".into());
            }
            *pull = Some(Pull { model, cancel: Arc::clone(&cancel), done: 0, total });
        }
        let inner = Arc::clone(&self.inner);
        let app = app.clone();
        std::thread::spawn(move || {
            let mut before = 0u64;
            let mut told = Instant::now() - Duration::from_secs(1);
            let mut ended = Fetch::Done;
            for file in missing {
                ended = models::download(&inner.dir, file, &cancel, &mut |done| {
                    let done = before + done;
                    if let Some(pull) = lock(&inner.pull).as_mut() {
                        pull.done = done;
                    }
                    if told.elapsed() >= Duration::from_millis(200) {
                        told = Instant::now();
                        emit(&app, Event::Download { model, done, total });
                    }
                });
                if ended != Fetch::Done {
                    break;
                }
                before += file.bytes;
            }
            *lock(&inner.pull) = None;
            let event = match ended {
                Fetch::Done => Event::Downloaded { model, ok: true, problem: None },
                Fetch::Cancelled => Event::Downloaded { model, ok: false, problem: None },
                Fetch::Failed(problem) => Event::Downloaded { model, ok: false, problem: Some(problem) },
            };
            emit(&app, event);
        });
        Ok(())
    }

    pub fn cancel_download(&self) {
        if let Some(pull) = lock(&self.inner.pull).as_ref() {
            pull.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Deletes the model's file (and the gate's, once no model is left); refused while it is being fetched.
    pub fn delete(&self, model: Model) -> Result<(), String> {
        if lock(&self.inner.pull).as_ref().is_some_and(|pull| pull.model == model) {
            return Err("busy".into());
        }
        self.inner.engine.let_go(model);
        models::delete(&self.inner.dir, model);
        Ok(())
    }

    /// Starts a take for the window's `id`, a take under way elsewhere being cancelled first (one
    /// microphone). `language` is whisper's code, `prompt` the project's name.
    pub fn start(&self, app: &AppHandle, id: String, model: Model, language: String, prompt: String) -> Result<(), String> {
        if !engine::supported() {
            return Err("unsupported".into());
        }
        if !models::ready(&self.inner.dir, model) {
            return Err("missing".into());
        }
        let controls = capture::Controls::new();
        {
            let mut take = lock(&self.inner.take);
            if let Some(before) = take.take() {
                before.controls.ask.store(capture::CANCEL, Ordering::Relaxed);
            }
            *take = Some(Take { id: id.clone(), controls: controls.clone() });
        }
        let inner = Arc::clone(&self.inner);
        let app = app.clone();
        std::thread::spawn(move || {
            let ended = run(&inner, &app, &id, &controls, model, &language, &prompt);
            let mut take = lock(&inner.take);
            if take.as_ref().is_some_and(|take| take.id == id) {
                *take = None;
            }
            drop(take);
            emit(&app, ended);
        });
        Ok(())
    }

    /// The take ends by itself on quiet from now on (it was a tap, not a hold).
    pub fn by_itself(&self, id: &str) {
        if let Some(take) = lock(&self.inner.take).as_ref().filter(|take| take.id == id) {
            take.controls.by_itself.store(true, Ordering::Relaxed);
        }
    }

    pub fn stop(&self, id: &str) {
        self.ask(id, capture::STOP);
    }

    pub fn cancel(&self, id: &str) {
        self.ask(id, capture::CANCEL);
    }

    fn ask(&self, id: &str, what: u8) {
        if let Some(take) = lock(&self.inner.take).as_ref().filter(|take| take.id == id) {
            take.controls.ask.store(what, Ordering::Relaxed);
        }
    }
}

/// Records the take and writes it down; what it comes to is the event the window hears last.
fn run(inner: &Inner, app: &AppHandle, id: &str, controls: &capture::Controls, model: Model, language: &str, prompt: &str) -> Event {
    let failed = |problem| Event::Failed { id: id.to_string(), problem };
    if !capture::allowed() {
        return failed("denied");
    }
    let mut told = Instant::now() - LEVEL_EVERY;
    let recorded = capture::record(controls, &mut |level| {
        if told.elapsed() >= LEVEL_EVERY {
            told = Instant::now();
            emit(app, Event::Level { id: id.to_string(), level });
        }
    });
    let (samples, rate, end) = match recorded {
        Ok(take) => take,
        Err(capture::Problem::NoDevice) => return failed("noDevice"),
        Err(capture::Problem::Failed) => return failed(if cfg!(target_os = "macos") { "failed" } else { "denied" }),
    };
    let end = match end {
        capture::End::Cancelled => return Event::Cancelled { id: id.to_string() },
        capture::End::Stopped => "stopped",
        capture::End::Quiet => "quiet",
        capture::End::Limit => "limit",
    };
    emit(app, Event::Transcribing { id: id.to_string() });
    let samples = engine::resample(&samples, rate);
    match inner.engine.transcribe(&inner.dir, model, &samples, language, prompt) {
        Ok(text) => Event::Done { id: id.to_string(), text, end },
        Err(engine::Problem::Missing) => failed("missing"),
        Err(engine::Problem::Failed) => failed("failed"),
    }
}

fn emit(app: &AppHandle, event: Event) {
    let _ = app.emit("voice", event);
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// whisper's code for the language a voice message is spoken in: the setting's, or with `app`
/// the window's (`zh-Hans` as `zh`); one it doesn't know is left to whisper (`auto`).
pub fn language_of(setting: &str, window: &str) -> String {
    let word = if setting == "app" { window } else { setting };
    let base = word.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    if engine::LANGUAGES.contains(&base.as_str()) {
        base
    } else {
        "auto".into()
    }
}
