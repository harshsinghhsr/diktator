//! Microphone capture. The cpal callback only downmixes to mono and pushes
//! into a lock-free ring buffer. A worker thread owns the stream, resamples
//! to 16 kHz, keeps the utterance, and hands 512-sample windows to the caller
//! (VAD + level meter).

use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const SAMPLE_RATE: u32 = 16_000;
/// 32 ms at 16 kHz: Silero VAD's window.
pub const WINDOW: usize = 512;
/// 5 minutes; the controller also stops at this point.
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 300;

pub fn list_input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    devices.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect()
}

fn find_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(want) = name {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.description().map(|x| x.name() == want).unwrap_or(false)) {
                return Ok(d);
            }
        }
        log::warn!("selected microphone not found; using the system default");
    }
    host.default_input_device().ok_or_else(|| anyhow!("No microphone found. Connect one and try again."))
}

pub fn downmix(interleaved: &[f32], channels: usize) -> impl Iterator<Item = f32> + '_ {
    let ch = channels.max(1);
    interleaved.chunks_exact(ch).map(move |f| f.iter().sum::<f32>() / ch as f32)
}

/// Maps RMS from -60..0 dBFS to 0..1.
pub fn level(window: &[f32]) -> f32 {
    if window.is_empty() {
        return 0.0;
    }
    let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
    if rms <= 1e-6 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[derive(Default)]
pub struct Windower {
    pending: Vec<f32>,
}

impl Windower {
    pub fn push(&mut self, samples: &[f32], on_window: &mut dyn FnMut(&[f32])) {
        self.pending.extend_from_slice(samples);
        let mut start = 0;
        while self.pending.len() - start >= WINDOW {
            on_window(&self.pending[start..start + WINDOW]);
            start += WINDOW;
        }
        self.pending.drain(..start);
    }
}

fn build_stream(device: &cpal::Device, mut prod: rtrb::Producer<f32>) -> Result<(cpal::Stream, u32)> {
    let supported = device.default_input_config().context("Could not read the microphone's audio format.")?;
    let rate = supported.sample_rate();
    let channels = (supported.channels() as usize).max(1);
    let config: cpal::StreamConfig = supported.config();
    let err_fn = |e: cpal::Error| log::warn!("audio stream error: {e}");

    macro_rules! stream_for {
        ($t:ty, $to_f32:expr) => {
            device.build_input_stream::<$t, _, _>(
                config,
                move |data: &[$t], _: &cpal::InputCallbackInfo| {
                    for frame in data.chunks_exact(channels) {
                        let s = frame.iter().map(|&x| $to_f32(x)).sum::<f32>() / channels as f32;
                        let _ = prod.push(s); // drop samples if the worker falls behind
                    }
                },
                err_fn,
                None,
            )
        };
    }

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => stream_for!(f32, |x: f32| x),
        cpal::SampleFormat::I16 => stream_for!(i16, |x: i16| x as f32 / 32_768.0),
        cpal::SampleFormat::I32 => stream_for!(i32, |x: i32| x as f32 / 2_147_483_648.0),
        other => bail!("Unsupported microphone sample format {other}."),
    }
    .context(
        "Could not open the microphone. On macOS, allow Diktator in System Settings → Privacy & Security → Microphone.",
    )?;
    stream.play().context("Could not start the microphone.")?;
    Ok((stream, rate))
}

/// Called on the audio worker thread with each 512-sample 16 kHz window.
pub type WindowFn = Box<dyn FnMut(&[f32]) + Send>;

pub struct Session {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<Vec<f32>>>>,
}

impl Session {
    pub fn start(device: Option<String>, mut on_window: WindowFn) -> Result<Session> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let worker = thread::Builder::new()
            .name("audio".into())
            .spawn(move || -> Result<Vec<f32>> {
                // 2 s of 48 kHz mono headroom between the callback and this thread.
                let (prod, mut cons) = rtrb::RingBuffer::<f32>::new(96_000);
                let (stream, rate) = match find_device(device.as_deref()).and_then(|d| build_stream(&d, prod)) {
                    Ok(v) => v,
                    Err(e) => {
                        let msg = format!("{e:#}");
                        let _ = ready_tx.send(Err(msg.clone()));
                        return Err(anyhow!(msg));
                    }
                };
                let resampler = if rate == SAMPLE_RATE {
                    None
                } else {
                    Some(
                        sherpa_onnx::LinearResampler::create(rate as i32, SAMPLE_RATE as i32)
                            .ok_or_else(|| anyhow!("could not create resampler"))?,
                    )
                };
                let _ = ready_tx.send(Ok(()));

                let mut stream = Some(stream);
                let mut out: Vec<f32> = Vec::with_capacity(SAMPLE_RATE as usize * 30);
                let mut windower = Windower::default();
                let mut raw: Vec<f32> = Vec::with_capacity(8_192);
                loop {
                    let stopping = stop_flag.load(Ordering::Relaxed);
                    if stopping {
                        drop(stream.take()); // stop the device before the final drain
                    }
                    raw.clear();
                    while let Ok(s) = cons.pop() {
                        raw.push(s);
                    }
                    let chunk = match &resampler {
                        Some(r) => r.resample(&raw, stopping),
                        None => raw.clone(),
                    };
                    if out.len() < MAX_SAMPLES {
                        let room = MAX_SAMPLES - out.len();
                        out.extend_from_slice(&chunk[..chunk.len().min(room)]);
                    }
                    windower.push(&chunk, &mut *on_window);
                    if stopping {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(out)
            })
            .context("could not start the audio thread")?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Session { stop, worker: Some(worker) }),
            Ok(Err(msg)) => {
                let _ = worker.join();
                Err(anyhow!(msg))
            }
            Err(_) => Err(anyhow!("the audio thread exited during start-up")),
        }
    }

    pub fn stop(mut self) -> Result<Vec<f32>> {
        self.stop.store(true, Ordering::Relaxed);
        let worker = self.worker.take().ok_or_else(|| anyhow!("session already stopped"))?;
        worker.join().map_err(|_| anyhow!("the audio thread panicked"))?
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        let mono: Vec<f32> = downmix(&stereo, 2).collect();
        assert_eq!(mono, vec![0.5, 0.5, 0.0]);
        let same: Vec<f32> = downmix(&[0.1, 0.2], 1).collect();
        assert_eq!(same, vec![0.1, 0.2]);
    }

    #[test]
    fn level_maps_silence_to_zero_and_full_scale_to_one() {
        assert_eq!(level(&[0.0; WINDOW]), 0.0);
        assert!((level(&[1.0; WINDOW]) - 1.0).abs() < 1e-6);
        let quiet = level(&[0.01; WINDOW]); // -40 dBFS
        assert!(quiet > 0.2 && quiet < 0.5, "{quiet}");
    }

    #[test]
    fn windower_emits_fixed_windows_and_keeps_remainder() {
        let mut w = Windower::default();
        let mut got = Vec::new();
        w.push(&vec![1.0; 700], &mut |win| got.push(win.len()));
        assert_eq!(got, vec![WINDOW]);
        w.push(&vec![1.0; 400], &mut |win| got.push(win.len()));
        assert_eq!(got, vec![WINDOW, WINDOW]);
        assert_eq!(w.pending.len(), 700 + 400 - 2 * WINDOW);
    }

    #[test]
    fn sherpa_resampler_converts_48k_to_16k() {
        let r = sherpa_onnx::LinearResampler::create(48_000, 16_000).unwrap();
        let out = r.resample(&vec![0.0; 4_800], true);
        assert!((out.len() as i64 - 1_600).abs() <= 2, "{}", out.len());
    }

    #[test]
    #[ignore = "needs a microphone and (on macOS) mic permission for the terminal"]
    fn records_from_default_microphone() {
        let windows = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = windows.clone();
        let s = Session::start(
            None,
            Box::new(move |_| {
                w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));
        let samples = s.stop().unwrap();
        assert!(samples.len() > 6_000, "{}", samples.len());
        assert!(windows.load(std::sync::atomic::Ordering::Relaxed) > 10);
    }
}
