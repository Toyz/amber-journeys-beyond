//! Lists the text chunks and text cast members that mention a needle.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let needle = a.next().unwrap_or_else(|| "houseHum".into());

    println!("-- STXT chunks --");
    for (i, t) in m.texts().iter().enumerate() {
        if t.contains(&needle) {
            println!("  [{i}] len={} starts: {:?}", t.len(), t.chars().take(110).collect::<String>());
        }
    }
    println!("-- text cast members --");
    for member in m.members() {
        if member.resource == 0 || member.kind != director::CastKind::Text {
            continue;
        }
        if let Some(t) = m.text(member.number) {
            if t.contains(&needle) {
                println!(
                    "  #{} {:?} len={} starts: {:?}",
                    member.number,
                    member.name,
                    t.len(),
                    t.chars().take(110).collect::<String>()
                );
            }
        }
    }
}
