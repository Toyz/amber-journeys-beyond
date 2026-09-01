//! Audio output.
//!
//! The engine mixes into one interleaved stream: the current movie's soundtrack
//! plus any one-shot effects the scripts fire. Everything is resampled to the
//! device rate with linear interpolation, which is ample for 22 kHz source
//! material and avoids pulling in a resampler.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// One playing sound. Loops carry the name the scripts know them by, so
/// `endLoop #houseHum` can stop the right one; one-shots carry none.
struct Voice {
    key: Option<String>,
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
    master: f32,
}

impl Mixer {
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let out_channels = self.channels.max(1) as usize;
        let master = self.master;

        self.voices.retain_mut(|voice| {
            let frames = voice.samples.len() / voice.channels.max(1) as usize;
            if frames == 0 {
                return false;
            }
            let src = voice.channels.max(1) as usize;
            for frame in out.chunks_mut(out_channels) {
                if voice.position >= frames as f64 {
                    if !voice.looping {
                        return false;
                    }
                    // Carry the fractional remainder across the seam. Resetting
                    // to zero instead drops part of a sample every lap, and
                    // skipping the output frame leaves a hole: on a
                    // three-second ambient loop that is an audible tick every
                    // three seconds, for as long as the room is occupied.
                    voice.position -= frames as f64;
                }
                let index = voice.position as usize;
                let frac = (voice.position - index as f64) as f32;
                // Interpolate into the start of the loop rather than off the
                // end of the buffer, so the seam is continuous.
                let next = if index + 1 < frames {
                    index + 1
                } else if voice.looping {
                    0
                } else {
                    index
                };
                for (c, slot) in frame.iter_mut().enumerate() {
                    let sc = c.min(src - 1);
                    let a = voice.samples[index * src + sc] as f32;
                    let b = voice.samples[next * src + sc] as f32;
                    *slot += (a + (b - a) * frac) / 32768.0 * voice.gain * master;
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
            master: 1.0,
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
    /// A `key` names a loop so it can be stopped later; pass `None` for a
    /// one-shot. Starting a loop that is already running is a no-op, so a room
    /// re-entered does not stack a second copy of its ambience.
    pub fn play(
        &self,
        key: Option<String>,
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
        if let Some(k) = &key {
            if mixer.voices.iter().any(|v| v.key.as_deref() == Some(k.as_str())) {
                return;
            }
        }
        mixer.voices.push(Voice {
            key,
            samples,
            channels: channels.max(1),
            position: 0.0,
            step: source_rate.max(1) as f64 / self.rate as f64,
            gain,
            looping,
        });
    }

    /// Makes the set of playing loops match `wanted`, which is `(name, gain)`.
    ///
    /// A room's ambience is not just which loops play but how loud each is:
    /// the house hum sits at 224 indoors, drops through 160 and 96 nearer the
    /// doors, and reaches 0 out on the grounds. Starting loops without ever
    /// stopping or re-levelling them leaves every loop the player has ever
    /// triggered running at full volume, stacked, for the rest of the session.
    ///
    /// Loops already playing keep their position so the sound is continuous
    /// across a move; only their gain changes.
    pub fn set_loops(&self, wanted: &[(String, f32)]) {
        let Ok(mut mixer) = self.mixer.lock() else {
            return;
        };
        mixer.voices.retain(|v| match &v.key {
            Some(k) => wanted.iter().any(|(n, _)| n == k),
            // One-shots are not loops and are left alone.
            None => true,
        });
        for voice in mixer.voices.iter_mut() {
            if let Some(k) = &voice.key {
                if let Some((_, gain)) = wanted.iter().find(|(n, _)| n == k) {
                    voice.gain = *gain;
                }
            }
        }
    }

    /// Names of the loops currently playing.
    pub fn playing_loops(&self) -> Vec<String> {
        match self.mixer.lock() {
            Ok(mixer) => mixer.voices.iter().filter_map(|v| v.key.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Stops one named loop.
    pub fn stop(&self, key: &str) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.voices.retain(|v| v.key.as_deref() != Some(key));
        }
    }

    /// Stops everything, used when a room change cuts the previous scene.
    pub fn stop_all(&self) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.voices.clear();
        }
    }

    /// Stops every one-shot but leaves ambient loops running, which is what a
    /// plain room change wants.
    pub fn stop_oneshots(&self) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.voices.retain(|v| v.key.is_some());
        }
    }

    /// Scales every voice, for the script-driven duck and restore.
    pub fn set_master(&self, gain: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.master = gain.clamp(0.0, 1.0);
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}
