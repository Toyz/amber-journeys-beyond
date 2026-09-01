//! Hexdumps a bitmap cast member's type-specific block.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    for n in a.filter_map(|s| s.parse::<u32>().ok()) {
        let Some(spec) = m.cast_spec(n) else {
            println!("cast {n}: none");
            continue;
        };
        println!("cast {n}: {} bytes", spec.len());
        for off in (0..spec.len().min(32)).step_by(16) {
            let row = &spec[off..(off + 16).min(spec.len())];
            println!(
                "  {off:04x}  {}",
                row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
            );
        }
    }
}
