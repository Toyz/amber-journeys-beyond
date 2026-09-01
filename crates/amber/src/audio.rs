//! Audio output.
//!
//! The engine mixes into one interleaved stream: the current movie's soundtrack
//! plus any one-shot effects the scripts fire. Everything is resampled to the
//! device rate with linear interpolation, which is ample for 22 kHz source
//! material and avoids pulling in a resampler.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// One playing sound.
struct Voice {
    samples: Arc<Vec<i16>>,
    /// Source channel count, so mono sources feed both outputs.
    channels: u16,
    /// Fractional read position, in source frames.
    position: f64,
    /// Source frames per output frame.
    step: f64,
    gain: f32,
    looping: bool,
}

#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
    rate: u32,
    channels: u16,
}

impl Mixer {
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let out_channels = self.channels.max(1) as usize;

        self.voices.retain_mut(|voice| {
            let frames = voice.samples.len() / voice.channels.max(1) as usize;
            if frames == 0 {
                return false;
            }
            for frame in out.chunks_mut(out_channels) {
                let index = voice.position as usize;
                if index >= frames {
                    if !voice.looping {
                        return false;
                    }
                    voice.position = 0.0;
                    continue;
                }
                // Linear interpolation between adjacent source frames.
                let frac = (voice.position - index as f64) as f32;
                let src = voice.channels.max(1) as usize;
                for (c, slot) in frame.iter_mut().enumerate() {
                    let sc = c.min(src - 1);
                    let a = voice.samples[index * src + sc] as f32;
                    let b = voice
                        .samples
                        .get((index + 1) * src + sc)
                        .copied()
                        .unwrap_or(voice.samples[index * src + sc]) as f32;
                    *slot += (a + (b - a) * frac) / 32768.0 * voice.gain;
                }
                voice.position += voice.step;
            }
            true
        });

        // Guard against summed voices clipping.
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    // Held to keep the stream alive; dropping it stops playback.
    _stream: Stream,
    rate: u32,
}

impl Audio {
    /// Opens the default output device. Returns `None` when there is no audio
    /// device, which is normal in a terminal or CI and must not be fatal.
    pub fn open() -> Option<Audio> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let rate = supported.sample_rate();
        let channels = supported.channels();
        let config: StreamConfig = supported.clone().into();

        let mixer = Arc::new(Mutex::new(Mixer {
            voices: Vec::new(),
            rate,
            channels,
        }));

        let m = Arc::clone(&mixer);
        let err = |e| eprintln!("audio error: {e}");
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    if let Ok(mut mixer) = m.lock() {
                        mixer.fill(data);
                    }
                },
                err,
                None,
            ),
            other => {
                eprintln!("audio: unsupported sample format {other:?}");
                return None;
            }
        }
        .ok()?;

        stream.play().ok()?;
        Some(Audio {
            mixer,
            _stream: stream,
            rate,
        })
    }

    /// Queues a sound. `source_rate` is the rate the samples were decoded at.
    pub fn play(
        &self,
        samples: Arc<Vec<i16>>,
        source_rate: u32,
        channels: u16,
        gain: f32,
        looping: bool,
    ) {
        if samples.is_empty() {
            return;
        }
        let Ok(mut mixer) = self.mixer.lock() else {
            return;
        };
        mixer.voices.push(Voice {
            samples,
            channels: channels.max(1),
            position: 0.0,
            step: source_rate.max(1) as f64 / self.rate as f64,
            gain,
            looping,
        });
    }

    /// Stops everything, used when a room change cuts the previous scene.
    pub fn stop_all(&self) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.voices.clear();
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}
