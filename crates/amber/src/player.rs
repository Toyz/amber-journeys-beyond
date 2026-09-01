//! Playback state for the movie a room places on its `#video` channel.

use std::path::Path;
use std::time::{Duration, Instant};

use qt::{Cinepak, Ima4Decoder, Movie, TrackKind};

pub struct VideoPlayer {
    movie: Movie,
    decoder: Cinepak,
    pub width: u16,
    pub height: u16,
    /// Index of the frame currently in `decoder`.
    current: usize,
    frame_count: usize,
    /// Track timescale, in units per second.
    timescale: u32,
    started: Instant,
    pub finished: bool,
    /// Decoded audio for the whole movie, if it has a sound track. Shared so
    /// the mixer can hold it without copying a track that runs to megabytes.
    pub audio: std::sync::Arc<Vec<i16>>,
    pub audio_rate: u32,
    pub audio_channels: u16,
}

impl VideoPlayer {
    /// Opens a movie and decodes its audio up front.
    ///
    /// Audio is decoded eagerly because IMA ADPCM carries predictor state across
    /// the whole track, so it cannot be decoded from an arbitrary point; the
    /// movies are short enough that holding the PCM is cheaper than the
    /// bookkeeping to stream it.
    pub fn open(path: &Path) -> Option<VideoPlayer> {
        let movie = Movie::open(path).ok()?;
        let video = movie.track(TrackKind::Video)?;
        let (width, height) = (video.width, video.height);
        let frame_count = video.samples.len();
        let timescale = video.timescale;

        let (audio, audio_rate, audio_channels) = match movie.track(TrackKind::Sound) {
            Some(sound) => {
                let mut decoder = Ima4Decoder::new();
                let mut pcm = Vec::new();
                for i in 0..sound.samples.len() {
                    if let Some(d) = movie.sample_data(sound, i) {
                        pcm.extend(decoder.decode(d, sound.channels));
                    }
                }
                (
                    std::sync::Arc::new(pcm),
                    sound.sample_rate,
                    sound.channels,
                )
            }
            None => (std::sync::Arc::new(Vec::new()), 0, 0),
        };

        let mut player = VideoPlayer {
            decoder: Cinepak::new(width as usize, height as usize),
            movie,
            width,
            height,
            current: usize::MAX,
            frame_count,
            timescale: timescale.max(1),
            started: Instant::now(),
            finished: frame_count == 0,
            audio,
            audio_rate,
            audio_channels,
        };
        player.seek(0);
        Some(player)
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Advances to whichever frame the wall clock now calls for.
    ///
    /// Returns true when the displayed frame changed, so the caller only
    /// redraws when there is something new.
    pub fn tick(&mut self) -> bool {
        if self.finished {
            return false;
        }
        let Some(video) = self.movie.track(TrackKind::Video) else {
            self.finished = true;
            return false;
        };
        let now = (self.started.elapsed().as_secs_f64() * self.timescale as f64) as u64;
        let target = match video.sample_at(now) {
            Some(i) => i,
            None => 0,
        };
        if target >= self.frame_count.saturating_sub(1) && now > 0 {
            // Hold the last frame rather than blanking when the movie ends.
            self.finished = target >= self.frame_count.saturating_sub(1)
                && now >= video.duration.max(1);
        }
        if target == self.current {
            return false;
        }
        self.seek(target)
    }

    /// Decodes up to `target`, starting from the keyframe it depends on.
    ///
    /// Cinepak inter-frames reference their predecessor, so a jump backwards or
    /// a long jump forwards has to replay from a keyframe; stepping forward by
    /// one frame costs one decode.
    fn seek(&mut self, target: usize) -> bool {
        let Some(video) = self.movie.track(TrackKind::Video) else {
            return false;
        };
        if target >= self.frame_count {
            return false;
        }
        let start = if self.current != usize::MAX && target > self.current {
            self.current + 1
        } else {
            video.sync_before(target)
        };
        for i in start..=target {
            let Some(data) = self.movie.sample_data(video, i) else {
                continue;
            };
            // A frame that fails to decode leaves the previous one on screen,
            // which is far less jarring than a black flash.
            let _ = self.decoder.decode(data);
        }
        self.current = target;
        true
    }

    pub fn frame(&self) -> &[u8] {
        self.decoder.frame()
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Jumps to a position in movie time, independent of the wall clock.
    pub fn seek_seconds(&mut self, seconds: f64) {
        let ticks = (seconds * self.timescale as f64) as u64;
        let target = self
            .movie
            .track(TrackKind::Video)
            .and_then(|v| v.sample_at(ticks))
            .unwrap_or(0);
        self.current = usize::MAX;
        self.seek(target);
        self.started = Instant::now() - Duration::from_secs_f64(seconds.max(0.0));
    }
}
