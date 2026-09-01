//! Shows the bytes around a parse failure in a text chunk.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let idx: usize = a.next().unwrap().parse().unwrap();
    let at: usize = a.next().unwrap().parse().unwrap();
    let texts = m.texts();
    let t = &texts[idx];
    let lo = at.saturating_sub(90);
    let hi = (at + 60).min(t.len());
    println!("...{}...", &t[lo..hi]);
    println!("{}^ byte {at}", " ".repeat(at - lo + 3));
}
