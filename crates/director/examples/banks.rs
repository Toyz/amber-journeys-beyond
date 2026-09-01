//! Reports which text chunks look like a sound bank and whether they parse.
fn main() {
    for path in std::env::args().skip(1) {
        let m = director::Movie::open(&path).unwrap();
        println!("{}", path.rsplit('/').next().unwrap());
        for (i, t) in m.texts().iter().enumerate() {
            let trimmed = t.trim();
            if !trimmed.contains("soundBank") {
                continue;
            }
            let starts = trimmed.starts_with("[#") || trimmed.starts_with("[ #");
            let parsed = lingo::parse_value(trimmed).is_ok();
            println!(
                "  chunk {i}: len {} starts-with-bracket {starts} parses {parsed}",
                t.len()
            );
            if !parsed {
                if let Err(e) = lingo::parse_value(trimmed) {
                    println!("      {e}");
                }
            }
        }
    }
}
