//! The desktop's audio device, which is the only sink with a platform behind
//! it.
//!
//! Everything about mixing lives in `audio`; this is the fifty lines that know
//! about CPAL.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

use crate::audio::{Audio, Sink};

/// A CPAL stream, held so it keeps playing.
struct CpalSink {
    _stream: Stream,
}

// A `Stream` is not `Send` on every platform, and the engine only ever holds
// this on the thread that made it.
unsafe impl Send for CpalSink {}

impl Sink for CpalSink {}

/// Opens the default output device. Returns `None` when there is no audio
/// device, which is normal in a terminal or CI and must not be fatal.
pub fn open() -> Option<Audio> {
    let device = cpal::default_host().default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let rate = supported.sample_rate();
    let channels = supported.channels();
    if supported.sample_format() != SampleFormat::F32 {
        eprintln!("audio: unsupported sample format {:?}", supported.sample_format());
        return None;
    }
    let config: StreamConfig = supported.into();

    Audio::over(rate, channels, move |mixer| {
        let err = |e| eprintln!("audio error: {e}");
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    if let Ok(mut mixer) = mixer.lock() {
                        mixer.fill(data);
                    }
                },
                err,
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(Box::new(CpalSink { _stream: stream }) as Box<dyn Sink>)
    })
}
