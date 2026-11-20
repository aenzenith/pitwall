//! Writes a take down as text, on this machine: the speech gate (Silero VAD) first, which keeps a
//! take with no speech in it from whisper (whisper writes made-up lines over silence and room
//! noise: "Altyazı M.K.", "Abone olmayı unutmayın"), then whisper. The model stays loaded while
//! voice messages are being sent and is let go after a few idle minutes.

use super::models::{self, Model};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

/// The rate whisper and the gate take.
pub const RATE: u32 = 16_000;

/// Less speech than this (the gate's segments summed, in centiseconds) is no speech.
const SPEECH_LEAST_CS: f32 = 30.0;

/// The words a prompt may carry (the project's name), at most.
const PROMPT_CHARS: usize = 120;

/// The languages a voice message may be spoken in: the app's (`zh-Hans` as whisper's `zh`).
pub const LANGUAGES: [&str; 7] = ["en", "tr", "de", "es", "ru", "ja", "zh"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// The model's files aren't there (deleted while it was asked for).
    Missing,
    /// whisper.cpp couldn't load or run them.
    Failed,
}

struct Loaded {
    model: Model,
    whisper: WhisperContext,
    gate: WhisperVadContext,
    used: Instant,
}

#[derive(Default)]
pub struct Engine {
    loaded: Mutex<Option<Loaded>>,
}

impl Engine {
    /// The take's text, or `None` when it holds no speech or nothing but marks (`[Müzik]`).
    pub fn transcribe(&self, dir: &Path, model: Model, samples: &[f32], language: &str, prompt: &str) -> Result<Option<String>, Problem> {
        let mut slot = self.loaded.lock().unwrap_or_else(|poison| poison.into_inner());
        if slot.as_ref().map(|loaded| loaded.model) != Some(model) {
            *slot = None;
            *slot = Some(load(dir, model)?);
        }
        let Some(loaded) = slot.as_mut() else {
            return Err(Problem::Failed);
        };
        loaded.used = Instant::now();

        let segments = loaded.gate.segments_from_samples(WhisperVadParams::new(), samples).map_err(|_| Problem::Failed)?;
        let speech: f32 = (0..segments.num_segments())
            .map(|i| segments.get_segment_end_timestamp(i).unwrap_or(0.0) - segments.get_segment_start_timestamp(i).unwrap_or(0.0))
            .sum();
        if speech < SPEECH_LEAST_CS {
            return Ok(None);
        }

        let mut state = loaded.whisper.create_state().map_err(|_| Problem::Failed)?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(if LANGUAGES.contains(&language) { language } else { "auto" }));
        params.set_n_threads(threads());
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_no_context(true);
        let prompt = prompt_of(prompt);
        if !prompt.is_empty() {
            params.set_initial_prompt(&prompt);
        }
        state.full(params, samples).map_err(|_| Problem::Failed)?;
        let text: String = state.as_iter().map(|segment| segment.to_string()).collect();
        Ok(clean(&text))
    }

    /// Lets the model go once it went unused for `idle`.
    pub fn let_go_after(&self, idle: Duration) {
        let mut slot = self.loaded.lock().unwrap_or_else(|poison| poison.into_inner());
        if slot.as_ref().is_some_and(|loaded| loaded.used.elapsed() >= idle) {
            *slot = None;
        }
    }

    /// Lets the model go at once (its file is being deleted).
    pub fn let_go(&self, model: Model) {
        let mut slot = self.loaded.lock().unwrap_or_else(|poison| poison.into_inner());
        if slot.as_ref().is_some_and(|loaded| loaded.model == model) {
            *slot = None;
        }
    }
}

fn load(dir: &Path, model: Model) -> Result<Loaded, Problem> {
    if !models::ready(dir, model) {
        return Err(Problem::Missing);
    }
    // whisper.cpp's and ggml's lines go nowhere (no log backend is on).
    whisper_rs::install_logging_hooks();
    let whisper_path = models::path(dir, model.file());
    let gate_path = models::path(dir, &models::GATE);
    let mut params = WhisperContextParameters::default();
    params.use_gpu(cfg!(target_os = "macos"));
    let whisper = WhisperContext::new_with_params(whisper_path.to_str().ok_or(Problem::Failed)?, params).map_err(|_| Problem::Failed)?;
    let mut gate_params = WhisperVadContextParams::new();
    gate_params.set_n_threads(2);
    gate_params.set_use_gpu(false);
    let gate = WhisperVadContext::new(gate_path.to_str().ok_or(Problem::Failed)?, gate_params).map_err(|_| Problem::Failed)?;
    Ok(Loaded { model, whisper, gate, used: Instant::now() })
}

fn threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get().clamp(1, 8) as i32).unwrap_or(4)
}

/// The prompt whisper is started with: the words given (the project's name), control characters
/// out, cut short.
fn prompt_of(words: &str) -> String {
    let words: String = words.chars().map(|c| if c.is_control() { ' ' } else { c }).take(PROMPT_CHARS).collect();
    words.trim().to_string()
}

/// whisper's text made a message: marks of no speech (`[Müzik çalıyor]`, `(Sokak ortam sesi)`,
/// `*öksürür*`) out, spaces made single; nothing left but punctuation is no text.
pub fn clean(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut close: Option<char> = None;
    for c in text.chars() {
        match close {
            Some(end) if c == end => close = None,
            Some(_) => {}
            None => match c {
                '[' => close = Some(']'),
                '(' => close = Some(')'),
                '*' => close = Some('*'),
                _ => out.push(c),
            },
        }
    }
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().any(char::is_alphanumeric).then_some(joined)
}

/// Whether this machine's processor runs the core as it was built: on x86-64 it takes AVX2, FMA
/// and F16C (Haswell, 2013, and later); ARM always does.
pub fn supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") && std::arch::is_x86_feature_detected!("f16c")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

/// A take at the device's rate made whisper's 16 kHz: each new sample the mean of the old ones it
/// stands for (a box filter, enough for speech), or between two of them when the device is slower.
pub fn resample(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate == RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = rate as f64 / RATE as f64;
    let length = (samples.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(length);
    for i in 0..length {
        let centre = i as f64 * ratio;
        if ratio >= 1.0 {
            let start = (centre - ratio / 2.0).max(0.0) as usize;
            let end = ((centre + ratio / 2.0).ceil() as usize).clamp(start + 1, samples.len());
            let slice = &samples[start..end];
            out.push(slice.iter().sum::<f32>() / slice.len() as f32);
        } else {
            let at = centre.floor() as usize;
            let next = (at + 1).min(samples.len() - 1);
            let part = (centre - at as f64) as f32;
            out.push(samples[at] * (1.0 - part) + samples[next] * part);
        }
    }
    out
}
