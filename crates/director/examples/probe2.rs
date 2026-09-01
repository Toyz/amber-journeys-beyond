fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let want = a.next().unwrap();
    for (n, name) in m.members_named_with(".DATA") {
        if name.to_ascii_lowercase().starts_with(&want.to_ascii_lowercase()) {
            let t = m.text(n).unwrap_or_default();
            println!("cast #{n} {name} len={}", t.len());
            println!("{}", t.chars().take(700).collect::<String>());
            return;
        }
    }
    println!("no match for {want}");
}
