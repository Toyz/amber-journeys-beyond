//! Dumps the raw header of a `snd ` cast member so the decoder can be checked.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let cast: u32 = a.next().unwrap().parse().unwrap();
    match m.sound(cast) {
        Ok(s) => {
            let n = s.samples.len();
            let peak = s.samples.iter().map(|x| x.unsigned_abs()).max().unwrap_or(0);
            let at_clamp = s.samples.iter().filter(|&&x| x == i16::MIN || x == i16::MAX).count();
            let mean = s.samples.iter().map(|&x| x as i64).sum::<i64>() / n.max(1) as i64;
            // Zero crossings say whether this is a waveform or noise.
            let cross = s.samples.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
            println!("cast {cast}: {n} samples, {} Hz, {}ch", s.sample_rate, s.channels);
            println!("  peak {peak}, at-clamp {at_clamp} ({:.2}%), mean {mean}",
                     100.0 * at_clamp as f64 / n as f64);
            println!("  zero crossings {cross} ({:.1} per 1000 samples)",
                     1000.0 * cross as f64 / n as f64);
            println!("  first 16: {:?}", &s.samples[..16.min(n)]);
        }
        Err(e) => println!("cast {cast}: {e}"),
    }
}
