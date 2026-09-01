//! Lists digital video cast members whose film is inside the movie file.
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(m) = director::Movie::open(&path) else { continue };
        for member in m.members() {
            if member.kind != director::CastKind::DigitalVideo || member.resource == 0 {
                continue;
            }
            if let Some(data) = m.embedded_movie(member.number) {
                println!(
                    "  {:<16} #{:<5} {:<24} {:>8} bytes embedded",
                    path.rsplit('/').next().unwrap_or(&path),
                    member.number,
                    member.name.clone().unwrap_or_default(),
                    data.len()
                );
            }
        }
    }
}
