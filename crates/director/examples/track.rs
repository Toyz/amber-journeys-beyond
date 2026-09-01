//! Prints the shape of a movie's #trackData, without dumping its contents.
fn main() {
    for path in std::env::args().skip(1) {
        let m = director::Movie::open(&path).unwrap();
        for t in m.texts() {
            let Ok(v) = lingo::parse_value(t.trim()) else { continue };
            let Some(track) = v.get("trackData") else { continue };
            println!("{}", path.rsplit('/').next().unwrap());
            for (variant, body) in track.entries() {
                let keys: Vec<&str> = body.entries().iter().map(|(k, _)| k.as_str()).collect();
                println!("  #{variant}: {} entries -> {:?}", keys.len(), &keys[..keys.len().min(6)]);
                // Look inside one sub-track to see how frames map to actions.
                for (sub, frames) in body.entries().iter().take(8) {
                    if sub == "trackmovie" {
                        println!("      #{sub} = {:?}", frames.as_int());
                        continue;
                    }
                    let e = frames.entries();
                    if e.is_empty() { continue; }
                    let shape: Vec<String> = e
                        .iter()
                        .take(4)
                        .map(|(f, val)| match val {
                            lingo::Value::Int(n) => format!("{f}:hold {n}"),
                            lingo::Value::List(items) => format!("{f}:{} action(s)", items.len()),
                            other => format!("{f}:{other:?}"),
                        })
                        .collect();
                    println!("      #{sub}: {} frames  {}", e.len(), shape.join(", "));
                }
            }
            return;
        }
    }
}
