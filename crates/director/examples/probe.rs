fn main() {
    let m = director::Movie::open(std::env::args().nth(1).unwrap()).unwrap();
    let named: Vec<_> = m.members().iter().filter(|x| x.name.is_some()).collect();
    println!("members with names: {}", named.len());
    for x in named.iter().take(8) {
        println!("  #{} {:?} {:?}", x.number, x.kind, x.name);
    }
    let d = m.members_named_with(".DATA");
    println!(".DATA members: {}", d.len());
    for (n, name) in d.iter().take(3) {
        let t = m.text(*n);
        println!("  #{n} {name} textlen={:?}", t.as_ref().map(|s| s.len()));
        if let Some(t) = t { println!("    starts: {:?}", &t.chars().take(90).collect::<String>()); }
    }
}
