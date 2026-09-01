fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    for n in a.filter_map(|s| s.parse::<u32>().ok()) {
        match m.member(n) {
            Some(c) => println!(
                "  cast {n:<6} {:<22} {}x{}  reg=({},{})  pitch={} depth={}",
                c.name.clone().unwrap_or_default(), c.width, c.height, c.reg_x, c.reg_y,
                c.pitch, c.bit_depth
            ),
            None => println!("  cast {n}: absent"),
        }
    }
}
