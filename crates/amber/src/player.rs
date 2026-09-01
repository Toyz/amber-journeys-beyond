//! Playback state for the movie a room places on its `#video` channel.

use std::path::Path;
use std::time::{Duration, Instant};

use std::sync::Arc;

use qt::{Cinepak, Ima4Decoder, Movie, TrackKind};
use qt::rle::Rle;

/// Which codec a movie's video track uses.
///
/// Half the disc is Cinepak and half is Apple Animation, and the player had
/// been handing every frame to the Cinepak decoder: a hundred and thirty-three
/// movies decoded to a black rectangle without any error, because a decoder
/// given the wrong format does not know that is what happened.
enum Video {
    Cinepak(Cinepak),
    Animation { rle: Rle, depth: u16 },
}

impl Video {
    fn decode(&mut self, data: &[u8]) {
        let _ = match self {
            Video::Cinepak(c) => c.decode(data),
            Video::Animation { rle, depth } => rle.decode(data, *depth),
        };
    }

    fn frame(&self) -> &[u8] {
        match self {
            Video::Cinepak(c) => c.frame(),
            Video::Animation { rle, .. } => rle.frame(),
        }
    }

    fn size(&self, declared: (u16, u16)) -> (u32, u32) {
        match self {
            // Cinepak's own header is authoritative: it can disagree with the
            // container, and it is the decoder that sized its buffer.
            Video::Cinepak(c) => (c.width as u32, c.height as u32),
            Video::Animation { .. } => (declared.0 as u32, declared.1 as u32),
        }
    }
}

pub struct VideoPlayer {
    movie: Movie,
    decoder: Video,
    pub width: u16,
    pub height: u16,
    /// Whether the movie restarts when it reaches its end.
    looping: bool,
    /// The part of the movie to play, in the track's own timescale.
    ///
    /// Director addresses a segment in ticks and the scripts hand over pairs
    /// like `[36, 60]`; the conversion happens where the timescale is known.
    segment: Option<(u64, u64)>,
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
                // The soundtrack was being run through the ADPCM decoder
                // whatever its codec, and seventeen of the game's are not
                // ADPCM. Decoding `raw ` that way is not distorted audio, it
                // is noise at full scale.
                let mut decoder = Ima4Decoder::new();
                let mut pcm = Vec::new();
                for i in 0..sound.samples.len() {
                    if let Some(d) = movie.sample_data(sound, i) {
                        if qt::pcm::handles(&sound.codec) {
                            pcm.extend(qt::pcm::decode(&sound.codec, sound.sample_bits, d));
                        } else {
                            pcm.extend(decoder.decode(d, sound.channels));
                        }
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
            decoder: match &video.codec {
                b"rle " => Video::Animation {
                    rle: Rle::new(
                        width as usize,
                        height as usize,
                        // An indexed track without a table is a broken file;
                        // a black palette at least keeps the geometry right.
                        video.palette.unwrap_or([[0; 3]; 256]),
                    ),
                    depth: video.depth,
                },
                _ => Video::Cinepak(Cinepak::new(width as usize, height as usize)),
            },
            movie,
            width,
            height,
            current: usize::MAX,
            frame_count,
            timescale: timescale.max(1),
            started: Instant::now(),
            finished: frame_count == 0,
            // Scenery until a script says otherwise.
            looping: true,
            segment: None,
            audio,
            audio_rate,
            audio_channels,
        };
        player.seek(0);
        Some(player)
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
        let mut now = (self.started.elapsed().as_secs_f64() * self.timescale as f64) as u64;
        // Inside a segment the clock starts at its first frame, and the movie
        // is over when it reaches the last.
        if let Some((from, to)) = self.segment {
            now += from;
            if now >= to {
                self.finished = true;
                return false;
            }
        }
        let target = match video.sample_at(now) {
            Some(i) => i,
            None => 0,
        };
        if target >= self.frame_count.saturating_sub(1)
            && now > 0
            && now >= video.duration.max(1)
        {
            if self.looping {
                // Start the clock again rather than counting on from a
                // timestamp that is already past the end.
                self.started = Instant::now();
                self.current = usize::MAX;
                return self.seek(0);
            }
            // Hold the last frame rather than blanking when the movie ends.
            self.finished = true;
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
            self.decoder.decode(data);
        }
        self.current = target;
        true
    }

    /// Plays the movie again from the start when it reaches the end.
    ///
    /// A movie the room places on its video channel is scenery: the scan
    /// unit's dial, a fan, a monitor. Director keeps a QuickTime sprite
    /// running for as long as the frame holds it, so those animate for as
    /// long as the player stands there. One that a script is waiting on with
    /// `wait #videoStop` must be allowed to end instead, or the wait never
    /// clears.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Plays only the part of the movie between two times, given in ticks.
    ///
    /// Margaret's music boxes are five performances in one film: the script
    /// names each as a pair of ticks, `[0, 32]` through `[100, 124]`, which
    /// land four ticks inside the keyframes at every thirty-two so the seams
    /// do not show. Playing the whole film for each box would play all five.
    pub fn play_segment(&mut self, from_ticks: u32, to_ticks: u32) {
        let to_track = |ticks: u32| ticks as u64 * self.timescale as u64 / 60;
        let (from, to) = (to_track(from_ticks), to_track(to_ticks));
        self.segment = Some((from, to));
        self.looping = false;
        self.finished = false;
        self.started = Instant::now();
        self.current = usize::MAX;
        if let Some(video) = self.movie.track(TrackKind::Video) {
            if let Some(i) = video.sample_at(from) {
                self.seek(i);
            }
        }
    }

    /// The part of the soundtrack belonging to the segment being played.
    ///
    /// One film holds all five music box performances, and its soundtrack
    /// holds all five tunes. Handing the whole track to the mixer plays every
    /// tune whichever box was opened, which is both wrong and loud enough to
    /// bury the box's own click.
    pub fn audio_for_segment(&self) -> Arc<Vec<i16>> {
        let Some((from, to)) = self.segment else {
            return Arc::clone(&self.audio);
        };
        let channels = self.audio_channels.max(1) as usize;
        let frames = self.audio.len() / channels;
        let at = |t: u64| {
            let seconds = t as f64 / self.timescale.max(1) as f64;
            ((seconds * self.audio_rate as f64) as usize).min(frames)
        };
        let (a, b) = (at(from), at(to));
        if b <= a {
            return Arc::new(Vec::new());
        }
        Arc::new(self.audio[a * channels..b * channels].to_vec())
    }

    pub fn frame(&self) -> &[u8] {
        self.decoder.frame()
    }

    /// The dimensions of the buffer `frame()` returns, which are the
    /// decoder's own and can differ from the container's declared size.
    pub fn frame_size(&self) -> (u32, u32) {
        self.decoder.size((self.width, self.height))
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
