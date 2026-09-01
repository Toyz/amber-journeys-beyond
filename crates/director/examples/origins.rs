//! Counts bitmap members whose rectangle has a non-zero origin.
fn main() {
    for path in std::env::args().skip(1) {
        let m = director::Movie::open(&path).unwrap();
        let (mut total, mut offset, mut worst) = (0, 0, 0i32);
        for c in m.members() {
            if c.resource == 0 || c.kind != director::CastKind::Bitmap {
                continue;
            }
            total += 1;
            let d = (c.origin_x as i32).abs().max((c.origin_y as i32).abs());
            if d > 0 {
                offset += 1;
                worst = worst.max(d);
            }
        }
        println!(
            "  {:<18} {offset} of {total} bitmaps have a non-zero origin (worst {worst}px)",
            path.rsplit('/').next().unwrap()
        );
    }
}
