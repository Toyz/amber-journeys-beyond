//! Lists a movie's digital video cast members with their numbers.
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let m = director::Movie::open(&path).unwrap();
    for member in m.members() {
        if member.kind != director::CastKind::DigitalVideo || member.resource == 0 {
            continue;
        }
        println!(
            "  #{:<5} {:<26} loops={}",
            member.number,
            member.name.clone().unwrap_or_default(),
            member.loops
        );
    }
}
