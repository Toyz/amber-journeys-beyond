//! Exercises the QuickTime reader and both codecs against a real movie.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: probe <file.mov> [out.png]")?;
    let movie = qt::Movie::open(&path)?;
    for t in &movie.tracks {
        println!(
            "{:?} codec={} {}x{} rate={} ch={} spp={} samples={} timescale={}",
            t.kind,
            String::from_utf8_lossy(&t.codec),
            t.width,
            t.height,
            t.sample_rate,
            t.channels,
            t.samples_per_packet,
            t.samples.len(),
            t.timescale
        );
    }

    if let Some(v) = movie.track(qt::TrackKind::Video) {
        let syncs = v.samples.iter().filter(|s| s.sync).count();
        println!("video: {syncs} keyframes of {}", v.samples.len());
        let animation = &v.codec == b"rle ";
        let mut dec = qt::Cinepak::new(v.width as usize, v.height as usize);
        let mut anim = qt::rle::Rle::new(
            v.width as usize,
            v.height as usize,
            v.palette.unwrap_or([[0; 3]; 256]),
        );
        println!("  codec {} depth {} palette {}", String::from_utf8_lossy(&v.codec), v.depth,
                 if v.palette.is_some() { "yes" } else { "no" });
        // Start from the keyframe at or before the requested frame, since
        // inter-frames need their predecessors.
        let want: usize = std::env::args()
            .nth(3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let target = want.min(v.samples.len().saturating_sub(1));
        let first = v.sync_before(target);
        let n = target - first + 1;
        for i in first..=target {
            let data = movie.sample_data(v, i).ok_or("no sample data")?;
            if animation { anim.decode(data, v.depth)?; } else { dec.decode(data)?; }
        }
        println!("  decoded frames {first}..={target}");
        // Statistics beat eyeballing for judging whether a decoder works: a
        // stuck decoder produces one colour, a working one produces many.
        let f = if animation { anim.frame() } else { dec.frame() };
        let mut hist = std::collections::HashSet::new();
        let mut sum = 0u64;
        for px in f.chunks_exact(4) {
            hist.insert((px[0], px[1], px[2]));
            sum += px[0] as u64 + px[1] as u64 + px[2] as u64;
        }
        println!(
            "decoded {n} frames, {}x{}, {} distinct colours, mean level {}",
            if animation { v.width as usize } else { dec.width },
            dec.height,
            hist.len(),
            sum / (f.len() as u64 / 4 * 3)
        );
        if let Some(out) = std::env::args().nth(2) {
            if animation {
                write_png(&out, v.width as u32, v.height as u32, anim.frame())?;
            } else {
                write_png(&out, dec.width as u32, dec.height as u32, dec.frame())?;
            }
            println!("wrote {out}");
        }
    }

    if let Some(a) = movie.track(qt::TrackKind::Sound) {
        let mut pcm = Vec::new();
        for i in 0..a.samples.len().min(400) {
            if let Some(d) = movie.sample_data(a, i) {
                if qt::pcm::handles(&a.codec) {
                    pcm.extend(qt::pcm::decode(&a.codec, a.sample_bits, d));
                } else {
                    pcm.extend(qt::decode_ima4(d, a.channels));
                }
            }
        }
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let nonzero = pcm.iter().filter(|&&s| s != 0).count();
        println!("audio: {} samples, peak {peak}, {nonzero} non-silent", pcm.len());
    }
    Ok(())
}

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    fn crc32(data: &[u8]) -> u32 {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
        c ^ 0xffff_ffff
    }
    fn stored(data: &[u8]) -> Vec<u8> {
        let mut o = vec![0x78, 0x01];
        for (i, blk) in data.chunks(65535).enumerate() {
            o.push(((i + 1) * 65535 >= data.len()) as u8);
            o.extend_from_slice(&(blk.len() as u16).to_le_bytes());
            o.extend_from_slice(&(!(blk.len() as u16)).to_le_bytes());
            o.extend_from_slice(blk);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        o.extend_from_slice(&((b << 16) | a).to_be_bytes());
        o
    }
    let chunk = |t: &[u8; 4], d: &[u8]| {
        let mut c = Vec::new();
        c.extend_from_slice(&(d.len() as u32).to_be_bytes());
        c.extend_from_slice(t);
        c.extend_from_slice(d);
        c.extend_from_slice(&crc32(&[&t[..], d].concat()).to_be_bytes());
        c
    };
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&w.to_be_bytes());
    hdr.extend_from_slice(&h.to_be_bytes());
    hdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut raw = Vec::new();
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(b"\x89PNG\r\n\x1a\n")?;
    f.write_all(&chunk(b"IHDR", &hdr))?;
    f.write_all(&chunk(b"IDAT", &stored(&raw)))?;
    f.write_all(&chunk(b"IEND", &[]))?;
    Ok(())
}
