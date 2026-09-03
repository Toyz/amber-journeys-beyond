//! The timed script that runs alongside a film.
//!
//! Edwin's car is the only place the game uses this, and it is most of what a
//! drive *is*. `#trackData` gives each stretch of track a film and two lists
//! of cues against that film's own clock -- one for driving alone and one with
//! the chipmunk aboard:
//!
//! ```text
//! #B: [#trackMovie: 1197,
//!      #alone:  [200: 178, 385: 195, ..., 525: #edwinLaugh, ...],
//!      #chippy: [0: 3, 76: 2, ..., 380: #yell1, ...]]
//! ```
//!
//! The key is a movie time in ticks and the value says what happens there:
//!
//!   - a symbol is a sound: `#edwinLaugh`, `#yell1`, `#getTheBear`;
//!   - a number above five is the engine's volume, so the track loop swells
//!     and falls with the gradient the car is on;
//!   - a number from one to five is a pose for the passenger's head;
//!   - a list of strings is Lingo to run, which the game uses exactly once.
//!
//! Without them a drive is a film with an engine noise flat behind it. With
//! them the car labours uphill, Chippy yells on the corners, and Edwin says
//! something about the bear.

use std::collections::HashMap;

use lingo::{parse_value, Value};

/// What happens at one point in a film.
#[derive(Clone, Debug, PartialEq)]
pub enum Cue {
    /// `soundEffect <name>`.
    Sound(String),
    /// `setLoop #trackLoop, <level>`, out of 255.
    Level(i32),
    /// A frame of the passenger's head, 1 to 5.
    Head(i32),
    /// A line of Lingo, run as written.
    Run(String),
}

impl std::fmt::Display for Cue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cue::Sound(name) => write!(f, "play {name}"),
            Cue::Level(level) => write!(f, "engine at {level}"),
            Cue::Head(frame) => write!(f, "passenger {frame}"),
            Cue::Run(line) => write!(f, "{line}"),
        }
    }
}

/// One stretch of track: its film, and the cues for each way of driving it.
#[derive(Clone, Default, Debug)]
pub struct Track {
    pub movie: u32,
    pub alone: Vec<(u32, Cue)>,
    pub with_chippy: Vec<(u32, Cue)>,
}

/// `#trackData`, by track name in lower case.
#[derive(Default)]
pub struct Tracks {
    tracks: HashMap<String, Track>,
}

impl Tracks {
    /// Picks `#trackData` out of a movie's text chunks.
    ///
    /// It shares a chunk with `#sndDurations` and `#waffleClips`, so the whole
    /// chunk is parsed and this one property taken out of it.
    pub fn from_texts(texts: &[String]) -> Tracks {
        let mut tracks = HashMap::new();
        for text in texts {
            let trimmed = text.trim();
            if !trimmed.contains("#trackData") || !trimmed.starts_with("[#") {
                continue;
            }
            let Ok(Value::Props(entries)) = parse_value(trimmed) else { continue };
            let Some((_, Value::Props(rows))) = entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("trackData"))
            else {
                continue;
            };
            for (name, value) in rows {
                let Value::Props(fields) = value else { continue };
                let mut track = Track::default();
                for (field, v) in fields {
                    match field.to_ascii_lowercase().as_str() {
                        "trackmovie" => track.movie = v.as_int().unwrap_or(0).max(0) as u32,
                        "alone" => track.alone = cues(v),
                        "chippy" => track.with_chippy = cues(v),
                        _ => {}
                    }
                }
                tracks.insert(name.to_ascii_lowercase(), track);
            }
        }
        Tracks { tracks }
    }

    /// The cues for a stretch of track, in the order they come.
    pub fn cues(&self, track: &str, with_chippy: bool) -> Vec<(u32, Cue)> {
        let Some(t) = self.tracks.get(&track.trim_start_matches('#').to_ascii_lowercase()) else {
            return Vec::new();
        };
        let mut out = if with_chippy { t.with_chippy.clone() } else { t.alone.clone() };
        // The lists are written roughly in order and not always exactly: `#A`
        // has 587 twice and 510 after 511. Sorting keeps a cue from being
        // skipped because the one before it came later.
        out.sort_by_key(|(at, _)| *at);
        out
    }

}

fn cues(value: &Value) -> Vec<(u32, Cue)> {
    let Value::Props(rows) = value else { return Vec::new() };
    rows.iter()
        .filter_map(|(at, what)| {
            let at: u32 = at.parse().ok()?;
            let cue = match what {
                Value::Symbol(s) => Cue::Sound(s.trim_start_matches('#').to_string()),
                Value::Int(n) if (1..=5).contains(n) => Cue::Head(*n),
                Value::Int(n) => Cue::Level(*n),
                Value::List(items) => Cue::Run(items.first()?.as_str()?.to_string()),
                _ => return None,
            };
            Some((at, cue))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: &str = "[#sndDurations: [#carDoorClose: 26], \
         #trackData: [#main: [#trackMovie: 1208, \
             #alone: [165: 90, 167: [\"assertSound #aCleverCar\"], 173: 120], \
             #chippy: [165: 90, 173: 120]], \
           #A: [#trackMovie: 1189, \
             #alone: [511: 255, 525: #edwinLaugh, 510: 3], \
             #chippy: [0: 3, 600: #yell3]]], \
         #waffleClips: [#c: 1218]]";

    #[test]
    fn a_drive_carries_its_own_soundtrack() {
        let tracks = Tracks::from_texts(&[CHUNK.to_string()]);
        // Alone on the A track: the engine swells, Edwin laughs, and the
        // chipmunk's head is not in it -- except that the head pose is in the
        // `#alone` list too, out of order, which is why they are sorted.
        assert_eq!(
            tracks.cues("A", false),
            vec![
                (510, Cue::Head(3)),
                (511, Cue::Level(255)),
                (525, Cue::Sound("edwinLaugh".into())),
            ]
        );

        // With him aboard it is a different list entirely.
        assert_eq!(
            tracks.cues("A", true),
            vec![(0, Cue::Head(3)), (600, Cue::Sound("yell3".into()))]
        );

        // And the one line of Lingo in the whole table.
        assert_eq!(
            tracks.cues("main", false)[1],
            (167, Cue::Run("assertSound #aCleverCar".into()))
        );
    }
}
