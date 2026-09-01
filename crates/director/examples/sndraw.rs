//! Hexdumps the start of a `snd ` resource to locate the sample data.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let cast: u32 = a.next().unwrap().parse().unwrap();
    let raw = m.sound_raw(cast).expect("no snd child");
    for off in (0..96.min(raw.len())).step_by(16) {
        let row = &raw[off..(off + 16).min(raw.len())];
        println!(
            "{off:04x}  {:<47}  {}",
            row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
            row.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect::<String>()
        );
    }
}
