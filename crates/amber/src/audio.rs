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
    /// What the scripts call this sound, when they name it. A programme's
    /// takes and a movie's soundtrack are unnamed: they are distinct
    /// recordings played in turn, and must never stand in for one another.
    name: Option<String>,
    samples: Arc<Vec<i16>>,
    /// Source channel count, so mono sources feed both outputs.
    channels: u16,
    /// Fractional read position, in source frames.
    position: f64,
    /// Source frames per output frame.
    step: f64,
    gain: f32,
    looping: bool,
    /// Whether this voice occupies one of the game's four sound channels.
    ///
    /// A movie's own soundtrack does not: QuickTime plays it outside them, so
    /// it neither takes a channel nor can be crowded out of one.
    channelled: bool,
}

#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
    rate: u32,
    channels: u16,
    master: f32,
    /// Current level of the ambient bed, ramped rather than switched.
    duck: f32,
    /// Set by the scripts' `suspendSounds`, cleared by `restoreSounds`.
    suspended: bool,
}

impl Mixer {

    /// Adds a voice, or folds the request into one already playing.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        name: Option<&str>,
        key: Option<String>,
        samples: Arc<Vec<i16>>,
        channels: u16,
        step: f64,
        gain: f32,
        looping: bool,
        channelled: bool,
    ) {
        if let Some(k) = &key {
            if self.voices.iter().any(|v| v.key.as_deref() == Some(k.as_str())) {
                trace!(crate::trace::Topic::Audio, "loop {k} already playing");
                return;
            }
        } else if let Some(v) = self.voices.iter_mut().find(|v| {
            !v.looping
                && v.name
                    .as_deref()
                    .zip(name)
                    .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        }) {
            // Director plays a sound on a channel, and asking that channel for
            // the same sound again restarts it. Layering a second copy instead
            // sums one waveform with itself, which is twice the amplitude of a
            // single take rather than the modest rise two unrelated sounds
            // give, and it is the harshest way a mix can go wrong.
            trace!(crate::trace::Topic::Audio, "restart {}", name.unwrap_or(""));
            v.position = 0.0;
            v.gain = gain;
            return;
        }
        // A sound that shares a channel with one already speaking is not
        // started; the original gives up rather than finding room.
        if let Some(group) = name.and_then(exclusive_group) {
            if self
                .voices
                .iter()
                .any(|v| v.name.as_deref().and_then(exclusive_group) == Some(group))
            {
                trace!(
                    crate::trace::Topic::Audio,
                    "{} not started, {group} is already speaking",
                    name.unwrap_or("")
                );
                return;
            }
        }

        // The game mixes on four sound channels. Every chapter's schema
        // declares `#soundChannels` with exactly four, loops and effects share
        // them, and `soundEffect` gives up when none is free rather than
        // finding room. Without that cap the voices pile up: each one is
        // quiet enough on its own, and eight of them at once is not.
        const CHANNELS: usize = 4;
        if channelled && self.voices.iter().filter(|v| v.channelled).count() >= CHANNELS {
            trace!(
                crate::trace::Topic::Audio,
                "no free channel for {}, dropped",
                name.unwrap_or("(unnamed)")
            );
            return;
        }
        trace!(
            crate::trace::Topic::Audio,
            "play {} gain {gain:.2} {}ch {} frames{}",
            name.unwrap_or("(unnamed)"),
            channels,
            samples.len() / channels.max(1) as usize,
            if looping { " looping" } else { "" }
        );
        self.voices.push(Voice {
            key,
            name: name.map(str::to_string),
            samples,
            channels: channels.max(1),
            position: 0.0,
            step,
            gain,
            looping,
            channelled,
        });
    }

    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let out_channels = self.channels.max(1) as usize;
        let master = self.master;
        let frames = out.len() / out_channels;

        // The ambient bed steps back while anything is playing over it.
        //
        // A room's mix is a balance between its own background sources; it
        // says nothing about what should happen when a line of speech or a
        // sound effect arrives, and those are what the player is meant to be
        // listening to. Without this the house hum sits at the same level
        // underneath them and competes.
        //
        // The scripts ask for the same thing explicitly in twenty places, with
        // `suspendSounds` before a set piece and `restoreSounds` after. That
        // used to pull the master down, which ducked the set piece along with
        // everything else; it belongs on the bed alone.
        const DUCKED: f32 = 0.35;
        // Roughly forty milliseconds either way. Switching the level outright
        // clicks.
        let ramp = 25.0 / self.rate.max(1) as f32;
        let target = if self.suspended || self.voices.iter().any(|v| !v.looping) {
            DUCKED
        } else {
            1.0
        };
        let mut bed = vec![0.0f32; frames];
        for slot in bed.iter_mut() {
            if self.duck < target {
                self.duck = (self.duck + ramp).min(target);
            } else {
                self.duck = (self.duck - ramp).max(target);
            }
            *slot = self.duck;
        }

        self.voices.retain_mut(|voice| {
            let frames = voice.samples.len() / voice.channels.max(1) as usize;
            if frames == 0 {
                return false;
            }
            let src = voice.channels.max(1) as usize;
            // Only the ambient bed ducks; whatever is playing over it does not.
            let ducks = voice.looping;
            for (f, frame) in out.chunks_mut(out_channels).enumerate() {
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
                let level = voice.gain * master * if ducks { bed[f] } else { 1.0 };
                for (c, slot) in frame.iter_mut().enumerate() {
                    let sc = c.min(src - 1);
                    let a = voice.samples[index * src + sc] as f32;
                    let b = voice.samples[next * src + sc] as f32;
                    *slot += (a + (b - a) * frac) / 32768.0 * level;
                }
                voice.position += voice.step;
            }
            true
        });

        // Guard against summed voices clipping.
        for s in out.iter_mut() {
            *s = saturate(*s);
        }
    }
}

/// Sounds that must never overlap one another.
///
/// `ghostCalls` walks the four sound channels for the one the last call used,
/// asks `soundBusy` whether it is still running, and gives up if it is. So two
/// ghosts never speak at once, however often the room asks. Without that the
/// calls pile up on each other and the result is not speech.
///
/// Keyed by the naming convention because that is how the calls are addressed
/// in the first place: `BCALL1` to `BCALL11`, `ECALL1` to `ECALL12`,
/// `MCALL1` to `MCALL10`, built by the handler from a ghost's initial.
fn exclusive_group(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_uppercase();
    let call = ["BCALL", "ECALL", "MCALL"]
        .iter()
        .any(|p| n.strip_prefix(p).is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit())));
    call.then_some("ghostCall")
}

/// Keeps the summed mix inside full scale without hard clipping.
///
/// Everything below the knee passes through untouched, so ordinary material is
/// unaffected; above it the curve bends smoothly and approaches full scale
/// without ever crossing it. A hard clamp squares off the peaks instead, and
/// squared-off peaks are the crunch that gives a stacked mix away -- speech
/// suffers worst, because its peaks are frequent and short.
fn saturate(x: f32) -> f32 {
    const KNEE: f32 = 0.7;
    let magnitude = x.abs();
    if magnitude <= KNEE {
        return x;
    }
    let over = magnitude - KNEE;
    let headroom = 1.0 - KNEE;
    (KNEE + headroom * (over / (over + headroom))).copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(looping: bool, gain: f32) -> Voice {
        Voice {
            key: looping.then(|| "houseHum".to_string()),
            name: Some("houseHum".to_string()),
            // A steady full-scale tone, long enough to outlast the ramp, so a
            // level change is easy to read off.
            samples: Arc::new(vec![32767i16; 1 << 16]),
            channels: 1,
            position: 0.0,
            step: 1.0,
            gain,
            looping,
            channelled: true,
        }
    }

    fn mixer(voices: Vec<Voice>) -> Mixer {
        Mixer {
            voices,
            rate: 44100,
            channels: 1,
            master: 1.0,
            duck: 1.0,
            suspended: false,
        }
    }

    /// Runs long enough for the ramp to settle, and reports the final level.
    fn settled(m: &mut Mixer) -> f32 {
        let mut out = vec![0.0f32; 2048];
        for _ in 0..8 {
            m.fill(&mut out);
        }
        out[out.len() - 1]
    }

    #[test]
    fn the_bed_plays_at_its_own_level_when_nothing_is_over_it() {
        let mut m = mixer(vec![voice(true, 0.5)]);
        assert!((settled(&mut m) - 0.5).abs() < 0.01);
    }

    #[test]
    fn the_bed_steps_back_while_a_one_shot_plays() {
        // The room's balance says nothing about what should happen when speech
        // arrives, and speech is what the player is meant to hear.
        let mut quiet = mixer(vec![voice(true, 0.5)]);
        let alone = settled(&mut quiet);

        let mut with_speech = mixer(vec![voice(true, 0.5), voice(false, 0.0)]);
        let ducked = settled(&mut with_speech);
        assert!(ducked < alone * 0.5, "bed {ducked} should be well under {alone}");
    }

    #[test]
    fn what_plays_over_the_bed_does_not_duck_itself() {
        // The bug this replaced pulled the master down, which quietened the
        // set piece along with the background.
        let mut m = mixer(vec![voice(false, 0.5)]);
        assert!((settled(&mut m) - 0.5).abs() < 0.01);
    }

    #[test]
    fn suspend_holds_the_bed_down_on_its_own() {
        let mut m = mixer(vec![voice(true, 0.5)]);
        m.suspended = true;
        assert!(settled(&mut m) < 0.25);
    }

    #[test]
    fn the_duck_is_ramped_rather_than_switched() {
        // A level that changes in one sample clicks.
        let mut m = mixer(vec![voice(true, 0.5), voice(false, 0.0)]);
        let mut out = vec![0.0f32; 64];
        m.fill(&mut out);
        let steps: Vec<f32> = out.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        assert!(
            steps.iter().all(|d| *d < 0.01),
            "duck moves too fast: {:?}",
            &steps[..4]
        );
    }

    fn pcm() -> Arc<Vec<i16>> {
        Arc::new(vec![32767i16; 1 << 16])
    }

    #[test]
    fn the_same_sound_asked_for_twice_restarts_rather_than_layering() {
        // Director plays a sound on a channel; asking that channel for it
        // again restarts it. Two copies of one waveform sum coherently, which
        // is twice the amplitude, not the modest rise two unrelated sounds
        // give -- the harshest way a mix can go wrong.
        let mut m = mixer(vec![]);
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 1);
    }

    #[test]
    fn a_restart_returns_the_sound_to_its_beginning() {
        let mut m = mixer(vec![]);
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.voices[0].position = 500.0;
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 0.5, false, true);
        assert_eq!(m.voices[0].position, 0.0);
        assert_eq!(m.voices[0].gain, 0.5, "the new request sets the level");
    }

    #[test]
    fn different_sounds_still_play_together() {
        let mut m = mixer(vec![]);
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.start(Some("breakerSwitch"), None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 2);
    }

    #[test]
    fn unnamed_one_shots_never_stand_in_for_one_another() {
        // A programme's takes and a movie's soundtrack are distinct
        // recordings played in turn. Folding them together would drop all but
        // the first.
        let mut m = mixer(vec![]);
        m.start(None, None, pcm(), 1, 1.0, 1.0, false, true);
        m.start(None, None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 2);
    }

    #[test]
    fn only_four_sounds_play_at_once() {
        // Every chapter declares `#soundChannels` with exactly four, loops and
        // effects share them, and the game gives up on a sound when none is
        // free rather than finding room.
        let mut m = mixer(vec![]);
        for i in 0..8 {
            m.start(Some(&format!("s{i}")), None, pcm(), 1, 1.0, 1.0, false, true);
        }
        assert_eq!(m.voices.len(), 4);
    }

    #[test]
    fn a_movie_soundtrack_takes_no_channel_and_is_never_crowded_out() {
        // QuickTime plays it outside the four, so a busy room must not stop
        // a film being heard.
        let mut m = mixer(vec![]);
        for i in 0..4 {
            m.start(Some(&format!("s{i}")), None, pcm(), 1, 1.0, 1.0, false, true);
        }
        m.start(None, None, pcm(), 1, 1.0, 1.0, false, false);
        assert_eq!(m.voices.len(), 5);
        assert!(m.voices.last().is_some_and(|v| !v.channelled));
    }

    #[test]
    fn a_channel_freed_by_a_sound_ending_can_be_used_again() {
        let mut m = mixer(vec![]);
        for i in 0..4 {
            m.start(Some(&format!("s{i}")), None, pcm(), 1, 1.0, 1.0, false, true);
        }
        m.voices.remove(0);
        m.start(Some("late"), None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 4);
        assert!(m.voices.iter().any(|v| v.name.as_deref() == Some("late")));
    }

    #[test]
    fn a_loop_already_running_is_left_where_it_is() {
        // Re-entering a room must not restart its ambience, or the seam is
        // audible on every move.
        let mut m = mixer(vec![]);
        m.start(Some("houseHum"), Some("houseHum".into()), pcm(), 1, 1.0, 1.0, true, true);
        m.voices[0].position = 900.0;
        m.start(Some("houseHum"), Some("houseHum".into()), pcm(), 1, 1.0, 0.2, true, true);
        assert_eq!(m.voices.len(), 1);
        assert_eq!(m.voices[0].position, 900.0);
    }

    use super::saturate;

    #[test]
    fn quiet_material_passes_through_untouched() {
        for x in [-0.7, -0.5, 0.0, 0.25, 0.7] {
            assert_eq!(saturate(x), x, "{x} is below the knee");
        }
    }

    #[test]
    fn a_stacked_mix_stays_inside_full_scale() {
        // The living room asks for nearly three times full scale.
        for x in [1.0, 2.0, 2.82, 50.0] {
            assert!(saturate(x) < 1.0, "{x} saturated to {}", saturate(x));
            assert!(saturate(-x) > -1.0);
        }
    }

    #[test]
    fn the_curve_is_monotone_so_it_does_not_fold() {
        // A saturator that turns back on itself inverts loud peaks, which
        // sounds far worse than the clipping it replaced.
        let mut previous = f32::NEG_INFINITY;
        for i in 0..2000 {
            let x = -5.0 + i as f32 * 0.005;
            let y = saturate(x);
            assert!(y > previous, "not monotone at {x}");
            previous = y;
        }
    }

    #[test]
    fn it_is_continuous_at_the_knee() {
        let below = saturate(0.7 - 1e-4);
        let above = saturate(0.7 + 1e-4);
        assert!((above - below).abs() < 1e-3, "step at the knee");
    }
}

pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    // Held to keep the stream alive; dropping it stops playback. A silent
    // mixer has none: it exists so the audio path can be exercised and
    // reported on without a device, which is the only way to see what a room
    // is actually asking the mixer for.
    _stream: Option<Stream>,
    rate: u32,
}

impl Audio {
    /// A mixer with no output, for inspecting what a room asks to hear.
    pub fn silent() -> Audio {
        Audio {
            mixer: Arc::new(Mutex::new(Mixer {
                voices: Vec::new(),
                rate: 44100,
                channels: 2,
                master: 1.0,
                duck: 1.0,
                suspended: false,
            })),
            _stream: None,
            rate: 44100,
        }
    }

    /// Every voice the mixer is holding, for reporting.
    pub fn voices(&self) -> Vec<String> {
        let Ok(mixer) = self.mixer.lock() else {
            return Vec::new();
        };
        mixer
            .voices
            .iter()
            .map(|v| {
                format!(
                    "{:<18} gain {:.2} {}{}",
                    v.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    v.gain,
                    if v.looping { "looping" } else { "one-shot" },
                    if v.channelled { "" } else { ", off-channel" }
                )
            })
            .collect()
    }
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
            // The bed starts at full and ducks when something plays over it.
            duck: 1.0,
            suspended: false,
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
            _stream: Some(stream),
            rate,
        })
    }

    /// Queues a sound. `source_rate` is the rate the samples were decoded at.
    /// A `key` names a loop so it can be stopped later; pass `None` for a
    /// one-shot. Starting a loop that is already running is a no-op, so a room
    /// re-entered does not stack a second copy of its ambience.
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &self,
        name: Option<&str>,
        key: Option<String>,
        samples: Arc<Vec<i16>>,
        source_rate: u32,
        channels: u16,
        gain: f32,
        looping: bool,
        channelled: bool,
    ) {
        if samples.is_empty() {
            return;
        }
        let Ok(mut mixer) = self.mixer.lock() else {
            return;
        };
        let step = source_rate.max(1) as f64 / self.rate as f64;
        mixer.start(name, key, samples, channels, step, gain, looping, channelled);
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
        if crate::trace::enabled(crate::trace::Topic::Audio) {
            let dropped: Vec<&str> = mixer
                .voices
                .iter()
                .filter_map(|v| v.key.as_deref())
                .filter(|k| !wanted.iter().any(|(n, _)| n == k))
                .collect();
            if !dropped.is_empty() {
                trace!(crate::trace::Topic::Audio, "stop {}", dropped.join(", "));
            }
            let bed: Vec<String> = wanted
                .iter()
                .map(|(n, g)| format!("{n} {:.0}%", g * 100.0))
                .collect();
            trace!(
                crate::trace::Topic::Audio,
                "bed [{}] total {:.2}",
                bed.join(", "),
                wanted.iter().map(|(_, g)| g).sum::<f32>()
            );
        }
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

    /// Scales every voice.
    pub fn set_master(&self, gain: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.master = gain.clamp(0.0, 1.0);
        }
    }

    /// Holds the ambient bed down until it is released, for the scripts'
    /// `suspendSounds` and `restoreSounds`.
    pub fn set_suspended(&self, suspended: bool) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.suspended = suspended;
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}

#[cfg(test)]
mod call_tests {
    use super::*;

    fn pcm() -> Arc<Vec<i16>> {
        Arc::new(vec![32767i16; 1 << 16])
    }

    fn mixer() -> Mixer {
        Mixer {
            voices: Vec::new(),
            rate: 44100,
            channels: 1,
            master: 1.0,
            duck: 1.0,
            suspended: false,
        }
    }

    #[test]
    fn two_ghosts_never_speak_at_once() {
        let mut m = mixer();
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.start(Some("MCALL1"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.start(Some("BCALL3"), None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 1, "the first call holds the channel");
    }

    #[test]
    fn a_call_can_start_once_the_last_has_finished() {
        let mut m = mixer();
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.voices.clear();
        m.start(Some("MCALL1"), None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 1);
    }

    #[test]
    fn ordinary_sounds_are_not_grouped_with_the_calls() {
        let mut m = mixer();
        m.start(Some("MCALL7"), None, pcm(), 1, 1.0, 1.0, false, true);
        m.start(Some("breakerSwitch"), None, pcm(), 1, 1.0, 1.0, false, true);
        assert_eq!(m.voices.len(), 2);
    }

    #[test]
    fn the_group_is_the_call_names_and_not_anything_that_starts_like_one() {
        assert_eq!(exclusive_group("MCALL7"), Some("ghostCall"));
        assert_eq!(exclusive_group("bcall11"), Some("ghostCall"));
        assert_eq!(exclusive_group("ECALL12"), Some("ghostCall"));
        assert_eq!(exclusive_group("MCALLBACK"), None);
        assert_eq!(exclusive_group("breakerSwitch"), None);
    }
}
