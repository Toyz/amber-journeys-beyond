//! Lists cast members whose name matches any of the given substrings.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let needles: Vec<String> = a.map(|s| s.to_ascii_lowercase()).collect();
    for member in m.members() {
        if member.resource == 0 {
            continue;
        }
        let Some(name) = &member.name else { continue };
        let lower = name.to_ascii_lowercase();
        if needles.iter().any(|n| lower.contains(n.as_str())) {
            println!(
                "  #{:<6} {:<28} {:?} {}x{}",
                member.number, name, member.kind, member.width, member.height
            );
        }
    }
}
