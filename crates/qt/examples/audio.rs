//! Compares the first decoded audio against a reference dump.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap();
    let movie = qt::Movie::open(&path)?;
    let a = movie.track(qt::TrackKind::Sound).ok_or("no sound track")?;
    println!(
        "codec={} rate={} ch={} spp={} bpp={} chunks={}",
        String::from_utf8_lossy(&a.codec),
        a.sample_rate,
        a.channels,
        a.samples_per_packet,
        a.bytes_per_packet,
        a.samples.len()
    );
    let d = movie.sample_data(a, 0).ok_or("no chunk 0")?;
    println!("chunk 0: {} bytes, {} packets", d.len(), d.len() / 34);
    println!("first 8 header words: {:02x?}", &d[..16.min(d.len())]);
    let pcm = qt::decode_ima4(d, a.channels);
    println!("decoded {} samples", pcm.len());
    println!("first 16: {:?}", &pcm[..16.min(pcm.len())]);
    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    println!("peak {peak}");
    Ok(())
}
