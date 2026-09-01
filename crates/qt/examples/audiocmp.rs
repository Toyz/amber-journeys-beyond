//! Decodes a whole sound track and compares it to a raw PCM reference.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut a = std::env::args().skip(1);
    let movie = qt::Movie::open(a.next().unwrap())?;
    let reference = std::fs::read(a.next().unwrap())?;
    let track = movie.track(qt::TrackKind::Sound).ok_or("no sound track")?;

    let mut decoder = qt::Ima4Decoder::new();
    let mut pcm = Vec::new();
    for i in 0..track.samples.len() {
        if let Some(d) = movie.sample_data(track, i) {
            pcm.extend(decoder.decode(d, track.channels));
        }
    }
    let refs: Vec<i16> = reference
        .as_chunks::<2>().0.iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    let n = pcm.len().min(refs.len());
    let exact = (0..n).filter(|&i| pcm[i] == refs[i]).count();
    let err: u64 = (0..n).map(|i| (pcm[i] as i32 - refs[i] as i32).unsigned_abs() as u64).sum();
    println!(
        "mine {} samples, reference {}, compared {n}",
        pcm.len(),
        refs.len()
    );
    println!(
        "exact {exact}/{n} ({:.2}%), mean abs error {:.2}",
        100.0 * exact as f64 / n as f64,
        err as f64 / n as f64
    );

    // Where does it first diverge, and is divergence aligned to a packet?
    if let Some(first) = (0..n).find(|&i| pcm[i] != refs[i]) {
        let spp = track.samples_per_packet as usize;
        println!(
            "first divergence at sample {first} (packet {}, offset {} within packet)",
            first / spp,
            first % spp
        );
        let lo = first.saturating_sub(4);
        println!("  mine {:?}", &pcm[lo..(first + 8).min(n)]);
        println!("  ref  {:?}", &refs[lo..(first + 8).min(n)]);
    }

    // Accuracy of the first packet alone, which isolates the codec from framing.
    let spp = track.samples_per_packet as usize;
    let head = spp.min(n);
    let head_exact = (0..head).filter(|&i| pcm[i] == refs[i]).count();
    println!("first packet: {head_exact}/{head} exact");
    Ok(())
}
