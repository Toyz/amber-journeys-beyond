//! Reports the loop points the game's `snd ` members declare.
fn main() {
    let mut declared = 0;
    let mut total = 0;
    for path in std::env::args().skip(1) {
        let Ok(m) = director::Movie::open(&path) else { continue };
        for member in m.members() {
            if member.kind != director::CastKind::Sound || member.resource == 0 {
                continue;
            }
            let Ok(s) = m.sound(member.number) else { continue };
            total += 1;
            if let Some((a, b)) = s.loop_points {
                declared += 1;
                let frames = s.samples.len() / s.channels.max(1) as usize;
                println!(
                    "{:<22} {:>8} frames, sustain {a}..{b} ({:.0}%)",
                    member.name.clone().unwrap_or_default(),
                    frames,
                    100.0 * (b - a) as f64 / frames.max(1) as f64
                );
            }
        }
    }
    println!("{declared} of {total} sounds declare a sustain");
}
