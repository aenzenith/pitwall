//! A take from the default microphone: its samples, mono, at the device's rate, kept in memory
//! only while it records and handed to the engine at its end; never written anywhere. The stream
//! lives on the thread that records it (`cpal`'s streams aren't `Send` everywhere).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the level goes to the window and the end of speech is looked for.
const FRAME: Duration = Duration::from_millis(50);
/// A take is at most this long; at its end it is written down all the same.
pub const LONGEST: Duration = Duration::from_secs(120);
/// In a take that ends by itself (a tap): this much quiet after speech ends it…
const QUIET_ENDS: Duration = Duration::from_millis(2000);
/// …and this much with no speech at all.
const NOTHING_ENDS: Duration = Duration::from_secs(15);

/// What the window asked of a take under way (`Take::ask`).
pub const RUN: u8 = 0;
pub const STOP: u8 = 1;
pub const CANCEL: u8 = 2;

/// Why a take ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// The user stopped it (a second tap, letting go after holding, the stop button).
    Stopped,
    /// Quiet after speech, or no speech for long, while it was ending by itself.
    Quiet,
    /// It reached `LONGEST`.
    Limit,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// No input device.
    NoDevice,
    /// The system refused the microphone, or the stream couldn't be opened.
    Failed,
}

/// The controls of a take under way, shared with the thread that records it.
#[derive(Clone)]
pub struct Controls {
    /// `RUN`, `STOP` or `CANCEL`.
    pub ask: Arc<AtomicU8>,
    /// Whether the take ends by itself on quiet (a tap; a hold ends when it is let go).
    pub by_itself: Arc<AtomicBool>,
}

impl Controls {
    pub fn new() -> Self {
        Self { ask: Arc::new(AtomicU8::new(RUN)), by_itself: Arc::new(AtomicBool::new(false)) }
    }
}

/// Records until the take ends, telling `level` the loudness (0–1) every `FRAME`. Returns the
/// take's samples (mono, at the returned rate) and why it ended.
pub fn record(controls: &Controls, level: &mut dyn FnMut(f32)) -> Result<(Vec<f32>, u32, End), Problem> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(Problem::NoDevice)?;
    let supported = device.default_input_config().map_err(|_| Problem::Failed)?;
    let rate = supported.sample_rate();
    let channels = supported.channels().max(1) as usize;
    let config: cpal::StreamConfig = supported.config();
    let sink: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 10)));
    let broken = Arc::new(AtomicBool::new(false));

    let stream = match supported.sample_format() {
        SampleFormat::F32 => open::<f32>(&device, &config, channels, &sink, &broken),
        SampleFormat::I16 => open::<i16>(&device, &config, channels, &sink, &broken),
        SampleFormat::U16 => open::<u16>(&device, &config, channels, &sink, &broken),
        SampleFormat::I32 => open::<i32>(&device, &config, channels, &sink, &broken),
        SampleFormat::F64 => open::<f64>(&device, &config, channels, &sink, &broken),
        SampleFormat::I8 => open::<i8>(&device, &config, channels, &sink, &broken),
        SampleFormat::U8 => open::<u8>(&device, &config, channels, &sink, &broken),
        _ => Err(Problem::Failed),
    }?;
    stream.play().map_err(|_| Problem::Failed)?;

    let started = Instant::now();
    let mut seen = 0usize;
    let mut ear = Ear::default();
    let end = loop {
        std::thread::sleep(FRAME);
        match controls.ask.load(Ordering::Relaxed) {
            CANCEL => break End::Cancelled,
            STOP => break End::Stopped,
            _ => {}
        }
        if broken.load(Ordering::Relaxed) {
            return Err(Problem::Failed);
        }
        let loudness = {
            let samples = sink.lock().unwrap_or_else(|poison| poison.into_inner());
            let fresh = &samples[seen.min(samples.len())..];
            seen = samples.len();
            rms(fresh)
        };
        level(meter(loudness));
        if started.elapsed() >= LONGEST {
            break End::Limit;
        }
        if ear.hear(loudness, FRAME) && controls.by_itself.load(Ordering::Relaxed) {
            break End::Quiet;
        }
    };
    drop(stream);
    let samples = std::mem::take(&mut *sink.lock().unwrap_or_else(|poison| poison.into_inner()));
    Ok((samples, rate, end))
}

fn open<T>(device: &cpal::Device, config: &cpal::StreamConfig, channels: usize, sink: &Arc<Mutex<Vec<f32>>>, broken: &Arc<AtomicBool>) -> Result<cpal::Stream, Problem>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let sink = Arc::clone(sink);
    let broken = Arc::clone(broken);
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                let mut samples = sink.lock().unwrap_or_else(|poison| poison.into_inner());
                for frame in data.chunks(channels) {
                    let sum: f32 = frame.iter().map(|&sample| f32::from_sample(sample)).sum();
                    samples.push(sum / frame.len() as f32);
                }
            },
            move |_| broken.store(true, Ordering::Relaxed),
            None,
        )
        .map_err(|_| Problem::Failed)
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// The loudness as the window's meter shows it: −60 dB and below empty, −10 dB full.
fn meter(rms: f32) -> f32 {
    let db = 20.0 * (rms + 1e-7).log10();
    ((db + 60.0) / 50.0).clamp(0.0, 1.0)
}

/// Tells when speech is over, on loudness alone: a floor follows the room's quiet, speech is well
/// over it. Whether the take holds speech at all is the gate's to say afterwards; this only ends it.
#[derive(Default)]
struct Ear {
    floor: Option<f32>,
    spoke: Duration,
    quiet: Duration,
    waited: Duration,
}

impl Ear {
    /// Whether the take should end now.
    fn hear(&mut self, loudness: f32, frame: Duration) -> bool {
        let floor = *self.floor.get_or_insert(loudness.max(1e-4));
        let speech = loudness > (floor * 3.0).max(0.01);
        if speech {
            self.spoke += frame;
            self.quiet = Duration::ZERO;
        } else {
            self.quiet += frame;
            // The floor sinks at once to a quieter room and rises slowly with a louder one.
            self.floor = Some(if loudness < floor { loudness.max(1e-4) } else { floor * 0.98 + loudness * 0.02 });
        }
        self.waited += frame;
        let heard = self.spoke >= Duration::from_millis(200);
        (heard && self.quiet >= QUIET_ENDS) || (!heard && self.waited >= NOTHING_ENDS)
    }
}

/// Whether the system lets Pitwall use the microphone, asking the user the first time (macOS);
/// elsewhere the stream itself says so.
#[cfg(target_os = "macos")]
pub fn allowed() -> bool {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, Bool};
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: &'static NSString;
    }

    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        return false;
    };
    // 0 not yet asked, 1 restricted, 2 denied, 3 allowed (AVAuthorizationStatus).
    let status: isize = unsafe { msg_send![class, authorizationStatusForMediaType: AVMediaTypeAudio] };
    match status {
        3 => true,
        0 => {
            let (tell, told) = std::sync::mpsc::channel::<bool>();
            let block = RcBlock::new(move |granted: Bool| {
                let _ = tell.send(granted.as_bool());
            });
            let _: () = unsafe { msg_send![class, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*block] };
            told.recv_timeout(Duration::from_secs(300)).unwrap_or(false)
        }
        _ => false,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn allowed() -> bool {
    true
}

