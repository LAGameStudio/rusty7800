//! Audio output for the native GUI: TIA samples to the OS device.
//!
//! The core renders mono `i16` at 31_400 Hz (`TIA_AUDIO_CLOCK`); the
//! device runs whatever it runs (usually 44.1/48 kHz `f32`). [`Pump`]
//! bridges the two with linear resampling across calls (fractional phase
//! preserved), silence on underrun, and a 2-second cap so a fast
//! emulator can't build latency. [`AudioOut`] owns the cpal stream +
//! pump; [`start`] fails gracefully (no device on CI/headless) and the
//! app keeps running silent.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Seconds of backlog before old samples are dropped.
const CAP_SECS: u32 = 2;

/// Fractional linear resampler + jitter buffer (all pure logic, unit
/// tested below; the cpal callback only calls [`Pump::render`]).
pub struct Pump {
    buf: VecDeque<f32>,
    /// Read position in source-sample units (fractional part preserved).
    phase: f64,
    step: f64,
    cap: usize,
}

impl Pump {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Pump {
            buf: VecDeque::new(),
            phase: 0.0,
            step: in_rate as f64 / out_rate.max(1) as f64,
            cap: in_rate as usize * CAP_SECS as usize,
        }
    }

    /// Queue emulator samples (`i16` -> `f32` mono).
    pub fn push_i16(&mut self, samples: &[i16]) {
        self.buf.extend(samples.iter().map(|s| *s as f32 / 32768.0));
        if self.buf.len() > self.cap {
            let drop = self.buf.len() - self.cap;
            self.buf.drain(..drop);
            self.phase = (self.phase - drop as f64).max(0.0);
        }
    }

    /// Render `out.len()` device samples (silence past the buffered end).
    pub fn render(&mut self, out: &mut [f32]) {
        for o in out.iter_mut() {
            let i = self.phase.floor() as usize;
            if i + 1 >= self.buf.len() {
                *o = 0.0; // underrun: hold position, emit silence
                continue;
            }
            let frac = (self.phase - i as f64) as f32;
            *o = self.buf[i] * (1.0 - frac) + self.buf[i + 1] * frac;
            self.phase += self.step;
        }
        let consumed = (self.phase.floor() as usize).min(self.buf.len().saturating_sub(1));
        self.buf.drain(..consumed);
        self.phase -= consumed as f64;
    }
}

/// Live output: stream handle (kept alive) + shared pump.
pub struct AudioOut {
    _stream: cpal::Stream,
    pump: Arc<Mutex<Pump>>,
    pub out_rate: u32,
}

impl AudioOut {
    /// Feed one frame's emulator samples to the device queue.
    pub fn feed(&self, samples: &[i16]) {
        if let Ok(mut p) = self.pump.lock() {
            p.push_i16(samples);
        }
    }
}

/// Open the default output device. `in_rate` is the core render rate
/// (31_400 Hz); every supported sample format is accepted.
pub fn start(in_rate: u32) -> Result<AudioOut, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no default output device".to_string())?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let out_rate = config.sample_rate;
    let pump = Arc::new(Mutex::new(Pump::new(in_rate, out_rate)));
    let build = |p: Arc<Mutex<Pump>>| {
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if let Ok(mut pump) = p.lock() {
                pump.render(data);
            } else {
                data.fill(0.0);
            }
        }
    };
    let err_fn = |err| eprintln!("rusty7800-gui audio: {err}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_output_stream(config, build(pump.clone()), err_fn, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_output_stream(config, convert_fn::<i16>(pump.clone()), err_fn, None)
        }
        cpal::SampleFormat::U16 => {
            device.build_output_stream(config, convert_fn::<u16>(pump.clone()), err_fn, None)
        }
        f => return Err(format!("unsupported sample format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(AudioOut {
        _stream: stream,
        pump,
        out_rate,
    })
}

/// Adapter for non-`f32` device formats (render float, convert per sample).
fn convert_fn<T>(pump: Arc<Mutex<Pump>>) -> impl FnMut(&mut [T], &cpal::OutputCallbackInfo)
where
    T: cpal::Sample + cpal::FromSample<f32>,
{
    let mut scratch = Vec::<f32>::new();
    move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        scratch.resize(data.len(), 0.0);
        if let Ok(mut p) = pump.lock() {
            p.render(&mut scratch);
        } else {
            scratch.fill(0.0);
        }
        for (d, s) in data.iter_mut().zip(scratch.iter()) {
            *d = T::from_sample(*s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pumped(src: &[f32], in_rate: u32, out_rate: u32, n: usize) -> Vec<f32> {
        let mut p = Pump::new(in_rate, out_rate);
        p.buf.extend(src.iter().copied());
        let mut out = vec![0.0; n];
        p.render(&mut out);
        out
    }

    #[test]
    fn identity_rate_passes_through() {
        let got = pumped(&[0.0, 0.5, -0.5, 1.0, 0.25, 0.0], 8, 8, 4);
        assert_eq!(got, vec![0.0, 0.5, -0.5, 1.0]);
    }

    #[test]
    fn upsample_interpolates_linearly() {
        // 1 Hz source at 2 Hz out: 0, 0.5, 1, then 0.5 (phase 1.5 sits
        // between the trailing 1 and 0).
        let got = pumped(&[0.0, 1.0, 0.0], 1, 2, 4);
        assert!((got[0] - 0.0).abs() < 1e-6, "{got:?}");
        assert!((got[1] - 0.5).abs() < 1e-6, "{got:?}");
        assert!((got[2] - 1.0).abs() < 1e-6, "{got:?}");
        assert!((got[3] - 0.5).abs() < 1e-6, "{got:?}");
    }

    #[test]
    fn underrun_emits_silence_and_holds() {
        let mut p = Pump::new(2, 2);
        p.buf.extend([0.75, 0.0]);
        let mut out = [9.0; 4];
        p.render(&mut out);
        assert_eq!(out[0], 0.75);
        assert_eq!(&out[1..], &[0.0; 3]);
        // Position held with one neighbor kept: resume replays the kept
        // neighbor, then silence (a lone sample has no future neighbor to
        // interpolate toward), then the sample once a follower arrives.
        p.push_i16(&[16384]);
        let mut out2 = [9.0; 1];
        p.render(&mut out2);
        assert_eq!(out2[0], 0.0);
        let mut out3 = [9.0; 1];
        p.render(&mut out3);
        assert_eq!(out3[0], 0.0);
        p.push_i16(&[0]);
        let mut out4 = [9.0; 1];
        p.render(&mut out4);
        assert!((out4[0] - 0.5).abs() < 1e-6, "{out4:?}");
    }

    #[test]
    fn backlog_cap_drops_oldest() {
        let mut p = Pump::new(10, 10); // cap = 20 samples
        p.push_i16(&[0; 30]);
        p.push_i16(&[16384; 5]);
        assert!(p.buf.len() <= 20, "len {}", p.buf.len());
        assert!(p.phase >= 0.0);
    }
}
