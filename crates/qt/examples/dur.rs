//! Prints each movie's duration, longest first.
fn main() {
    let mut rows = Vec::new();
    for path in std::env::args().skip(1) {
        let Ok(m) = qt::Movie::open(&path) else { continue };
        let Some(v) = m.track(qt::TrackKind::Video) else { continue };
        let secs = v.duration as f64 / v.timescale.max(1) as f64;
        let sound = m.track(qt::TrackKind::Sound).is_some();
        rows.push((secs, path, v.samples.len(), sound));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (secs, path, frames, sound) in rows.iter().take(12) {
        println!("  {secs:>7.1}s  {frames:>5} frames  {}{}",
            path.rsplit('/').next().unwrap_or(path),
            if *sound { "  +sound" } else { "" });
    }
}
