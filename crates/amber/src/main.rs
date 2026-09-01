//! Command-line front end for the Amber reimplementation.
//!
//! Until the renderer lands this doubles as the verification harness: it loads
//! the real game data and reports what parsed, which is how the format work is
//! kept honest.

mod audio;
mod game;
mod locations;
mod media;
mod player;
mod render;
mod schema;
mod sound;
mod script;
mod state;
mod world;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use director::Movie;
use world::{Channel, World};

fn usage() -> ExitCode {
    eprintln!(
        "usage: amber <command> <game-dir> [args]

commands:
  info      <dir>              summarise the installed game data
  rooms     <dir> [domain]     list rooms and their exits
  room      <dir> <domain> <n> dump one room in full
  cast      <dir> <movie.dxr>  list a movie's cast members
  export    <dir> <movie.dxr> <cast#> <out.png>
                               decode one bitmap cast member
  play      <dir> [room]       open the game window
  shot      <dir> <room> <out.png>
                               render one room headlessly
  sfx       <dir> [name]       decode a named sound, or sample many
  verify    <dir>              parse everything and report failures"
    );
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        return usage();
    };
    let Some(dir) = args.get(1).map(PathBuf::from) else {
        return usage();
    };

    let result = match cmd {
        "info" => cmd_info(&dir),
        "rooms" => cmd_rooms(&dir, args.get(2).map(String::as_str)),
        "room" => match (args.get(2), args.get(3).and_then(|n| n.parse().ok())) {
            (Some(d), Some(n)) => cmd_room(&dir, d, n),
            _ => return usage(),
        },
        "cast" => match args.get(2) {
            Some(m) => cmd_cast(&dir.join(m)),
            None => return usage(),
        },
        "export" => match (args.get(2), args.get(3).and_then(|n| n.parse().ok()), args.get(4)) {
            (Some(m), Some(n), Some(out)) => cmd_export(&dir.join(m), n, Path::new(out)),
            _ => return usage(),
        },
        "play" => render::play(&dir, args.get(2).map(String::as_str)),
        "shot" => match (args.get(2), args.get(3)) {
            (Some(room), Some(out)) => cmd_shot(&dir, room, Path::new(out)),
            _ => return usage(),
        },
        "sfx" => cmd_sfx(&dir, args.get(2).map(String::as_str)),
        "verify" => cmd_verify(&dir),
        _ => return usage(),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type Res = Result<(), Box<dyn std::error::Error>>;

fn cmd_info(dir: &Path) -> Res {
    let world = World::load(dir)?;
    println!("rooms: {}", world.len());
    let mut names: Vec<_> = world.domains.iter().collect();
    names.sort();
    for (name, (start, end)) in names {
        println!("  {name:<10} {:>4} rooms", end - start);
    }

    let named = world.nodes.iter().filter(|n| n.name.is_some()).count();
    println!("named rooms: {named} ({} aliases)", world.by_name.len());

    // Every destination a hotspot can reach, and whether it resolves.
    let (mut ok, mut miss) = (0usize, 0usize);
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    for node in &world.nodes {
        for h in &node.hotspots {
            let mut probe = state::State::new();
            if let Some(dest) = script::run(&h.actions, &mut probe).destination {
                if world.resolve(&dest, Some(&node.domain)).is_some() {
                    ok += 1;
                } else {
                    miss += 1;
                    *missing.entry(dest).or_default() += 1;
                }
            }
        }
    }
    println!("exits resolving: {ok}, unresolved: {miss}");
    if !missing.is_empty() {
        let mut worst: Vec<_> = missing.iter().collect();
        worst.sort_by_key(|(_, v)| std::cmp::Reverse(**v));
        println!("top unresolved destinations:");
        for (k, v) in worst.iter().take(8) {
            println!("  {k:<28} {v:>4}");
        }
    }

    // Names that resolve to more than one room in the same chapter are a
    // correctness hazard: `resolve` has to pick one, so a move can land in the
    // wrong scene with nothing looking broken.
    let mut ambiguous: Vec<(&String, usize)> = world
        .by_name
        .iter()
        .filter_map(|(name, rooms)| {
            let same_chapter = world.domains.values().any(|&(start, end)| {
                rooms.iter().filter(|&&i| i >= start && i < end).count() > 1
            });
            same_chapter.then_some((name, rooms.len()))
        })
        .collect();
    ambiguous.sort();
    println!("ambiguous names: {}", ambiguous.len());
    for (name, n) in ambiguous.iter().take(10) {
        println!("  {name:<28} {n} rooms");
    }

    // Every movie a room can ask for, and whether the file is present.
    let index = media::MovieIndex::build(dir);
    let mut wanted: BTreeMap<String, usize> = BTreeMap::new();
    for node in &world.nodes {
        for s in &node.sprites {
            if matches!(s.channel, world::Channel::Video) {
                if let Some(n) = &s.cast_name {
                    *wanted.entry(n.clone()).or_default() += 1;
                }
            }
        }
    }
    let missing: Vec<&String> = wanted.keys().filter(|n| index.find(n).is_none()).collect();
    println!(
        "movies: {} on disc, {} referenced, {} unresolved",
        index.len(),
        wanted.len(),
        missing.len()
    );
    for m in missing.iter().take(8) {
        println!("  missing {m}");
    }

    // Sound coverage: every symbol the scripts fire, and whether it resolves.
    {
        let mut game = game::Game::new(dir)?;
        let mut wanted: BTreeMap<String, usize> = BTreeMap::new();
        for i in 0..game.world.nodes.len() {
            for h in &game.world.nodes[i].hotspots {
                let mut probe = state::State::new();
                for e in script::run(&h.actions, &mut probe).effects {
                    let name = match e {
                        script::Effect::PlaySound { name, .. } => name,
                        script::Effect::StartLoop { name, .. } => name,
                        _ => continue,
                    };
                    *wanted.entry(name).or_default() += 1;
                }
            }
            for s in &game.world.nodes[i].sprites {
                if matches!(s.channel, world::Channel::Sound) {
                    if let Some(n) = &s.cast_name {
                        *wanted.entry(n.trim_start_matches('#').to_string()).or_default() += 1;
                    }
                }
            }
        }
        let resolved = wanted
            .keys()
            .filter(|n| game.sounds.source(n).is_some())
            .count();
        println!(
            "sounds: {} files on disc, {} symbols tabulated, {} of {} referenced resolve",
            game.sounds.file_count(),
            game.sounds.len(),
            resolved,
            wanted.len()
        );
        let unresolved: Vec<&String> = wanted
            .keys()
            .filter(|n| game.sounds.source(n).is_none())
            .collect();
        for n in unresolved.iter().take(6) {
            println!("  unresolved {n}");
        }
        let missing = game.sounds.missing();
        if !missing.is_empty() {
            println!("  {} symbols name a file not on the disc", missing.len());
        }
    }

    let sprites: usize = world.nodes.iter().map(|n| n.sprites.len()).sum();
    let hotspots: usize = world.nodes.iter().map(|n| n.hotspots.len()).sum();
    let live: usize = world
        .nodes
        .iter()
        .flat_map(|n| &n.hotspots)
        .filter(|h| !h.actions.is_empty())
        .count();
    println!("sprites: {sprites}");
    println!("hotspots: {hotspots} ({live} with actions)");

    let mut channels: BTreeMap<String, usize> = BTreeMap::new();
    for s in world.nodes.iter().flat_map(|n| &n.sprites) {
        let key = match s.channel {
            Channel::Sprite(n) => format!("sprite {n}"),
            Channel::Sound => "sound".into(),
            Channel::Video => "video".into(),
            Channel::None => "none".into(),
        };
        *channels.entry(key).or_default() += 1;
    }
    println!("channels:");
    for (k, v) in channels {
        println!("  {k:<10} {v:>5}");
    }
    Ok(())
}

fn cmd_rooms(dir: &Path, domain: Option<&str>) -> Res {
    let world = World::load(dir)?;
    for node in &world.nodes {
        if domain.is_some_and(|d| !node.domain.eq_ignore_ascii_case(d)) {
            continue;
        }
        let art = node
            .sprites
            .iter()
            .find(|s| matches!(s.channel, Channel::Sprite(1)))
            .and_then(|s| s.cast_name.clone())
            .unwrap_or_else(|| "-".into());
        let exits: Vec<String> = node
            .hotspots
            .iter()
            .filter(|h| !h.actions.is_empty())
            .filter_map(|h| {
                let mut probe = state::State::new();
                script::run(&h.actions, &mut probe).destination
            })
            .collect();
        println!(
            "{:<9} {:>4}  {:<16} -> {}",
            node.domain,
            node.index,
            art,
            if exits.is_empty() {
                "(none)".into()
            } else {
                exits.join(", ")
            }
        );
    }
    Ok(())
}

fn cmd_room(dir: &Path, domain: &str, index: usize) -> Res {
    let world = World::load(dir)?;
    let node = world
        .nodes
        .iter()
        .find(|n| n.domain.eq_ignore_ascii_case(domain) && n.index == index)
        .ok_or("no such room")?;

    println!("{} room {}", node.domain, node.index);
    if let Some((lib, first, last)) = node.storage_cast {
        println!("  storage cast: library {lib}, members {first}-{last}");
    }
    println!("  preload: {:?}", node.preload);
    println!("  sprites:");
    for s in &node.sprites {
        println!(
            "    ch {:?} cast {} {:?} ink {} vol {:?}",
            s.channel, s.cast_number, s.cast_name, s.ink, s.volume
        );
    }
    println!("  hotspots:");
    for h in &node.hotspots {
        println!(
            "    {:?} {:?} {:?}",
            h.verb,
            (h.bounds.left, h.bounds.top, h.bounds.right, h.bounds.bottom),
            h.actions
        );
    }
    if !node.ambience.is_empty() {
        println!("  ambience: {:?}", node.ambience);
    }
    Ok(())
}

fn cmd_cast(path: &Path) -> Res {
    let movie = Movie::open(path)?;
    println!(
        "{}: {}x{} stage, {} cast slots, {} palettes",
        path.display(),
        movie.stage_width,
        movie.stage_height,
        movie.members().len(),
        movie.palette_count()
    );
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for m in movie.members() {
        if m.resource == 0 {
            continue;
        }
        *kinds.entry(format!("{:?}", m.kind)).or_default() += 1;
    }
    for (k, v) in &kinds {
        println!("  {k:<14} {v:>5}");
    }
    Ok(())
}

fn cmd_export(movie_path: &Path, cast: u32, out: &Path) -> Res {
    let movie = Movie::open(movie_path)?;
    let bmp = movie.bitmap(cast)?;
    let palettes = movie.palettes();
    let palette = palettes.first().cloned().unwrap_or_default();
    let rgba = bmp.to_rgba(&palette, None);
    write_png(out, bmp.width as u32, bmp.height as u32, &rgba)?;
    println!(
        "wrote {} ({}x{}) from cast {}",
        out.display(),
        bmp.width,
        bmp.height,
        cast
    );
    Ok(())
}

/// Renders one room to a PNG without opening a window, so the compositor can be
/// exercised in a terminal or in CI.
fn cmd_shot(dir: &Path, room: &str, out: &Path) -> Res {
    let mut game = game::Game::new(dir)?;
    // "start" renders wherever the game actually opens, which is the case worth
    // checking when the window comes up blank.
    if !room.eq_ignore_ascii_case("start") {
        game.room = game
            .world
            .resolve(room, None)
            .ok_or_else(|| format!("no room named {room}"))?;
    }

    const W: u32 = 640;
    const H: u32 = 480;
    let mut frame = vec![0u32; (W * H) as usize];
    // Let a movie reach a frame with content in it; the opening seconds of
    // most are a fade from black and would make a misleading screenshot. The
    // wait is in movie time, not wall clock, so it does not depend on how long
    // loading took.
    let seek: f64 = std::env::var("AMBER_SEEK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    if let Some(player) = &mut game.player {
        println!(
            "  movie {}x{}, {} frames, audio {} samples at {} Hz",
            player.width,
            player.height,
            player.frame_count(),
            player.audio.len(),
            player.audio_rate
        );
        player.seek_seconds(seek);
    } else if game.video().is_some() {
        println!("  movie {} did not load", game.video().unwrap_or(""));
    }
    game.draw(&mut frame, W, H);

    // The framebuffer is BGRA-in-a-u32; the writer wants straight RGBA bytes.
    let mut rgba = Vec::with_capacity(frame.len() * 4);
    for px in &frame {
        rgba.extend_from_slice(&[
            (px >> 16) as u8,
            (px >> 8) as u8,
            *px as u8,
            (px >> 24) as u8,
        ]);
    }
    write_png(out, W, H, &rgba)?;

    let node = game.node();
    println!(
        "{} / {} -> {}  ({} sprites drawn, {} hotspots)",
        node.domain,
        node.name.clone().unwrap_or_default(),
        out.display(),
        game.visible().len(),
        node.hotspots.len()
    );
    Ok(())
}

/// Decodes sounds and reports whether they carry signal, which is the check
/// that a format was actually understood rather than merely parsed.
fn cmd_sfx(dir: &Path, name: Option<&str>) -> Res {
    let mut game = game::Game::new(dir)?;

    let names: Vec<String> = match name {
        Some(n) => vec![n.to_string()],
        None => {
            let mut all: Vec<String> = game
                .world
                .nodes
                .iter()
                .flat_map(|n| &n.sprites)
                .filter(|s| matches!(s.channel, world::Channel::Sound))
                .filter_map(|s| s.cast_name.clone())
                .map(|n| n.trim_start_matches('#').to_string())
                .collect();
            all.sort();
            all.dedup();
            all.truncate(12);
            all
        }
    };

    let (mut ok, mut silent, mut failed) = (0, 0, 0);
    for n in &names {
        match game.sound(n) {
            Some((pcm, rate, channels)) => {
                let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
                let secs = pcm.len() as f32 / (rate.max(1) * channels.max(1) as u32) as f32;
                println!(
                    "  {n:<20} {:>9} samples  {rate:>5} Hz  {channels}ch  {secs:>5.1}s  peak {peak}",
                    pcm.len()
                );
                if peak == 0 {
                    silent += 1;
                } else {
                    ok += 1;
                }
            }
            None => {
                println!("  {n:<20} did not resolve");
                failed += 1;
            }
        }
    }
    println!("{ok} with signal, {silent} silent, {failed} unresolved");
    Ok(())
}

fn cmd_verify(dir: &Path) -> Res {
    let world = World::load(dir)?;
    let mut unhandled: BTreeMap<String, usize> = BTreeMap::new();
    let mut effects: BTreeMap<String, usize> = BTreeMap::new();
    let mut destinations = 0usize;
    let mut no_destination = 0usize;

    for node in &world.nodes {
        for h in &node.hotspots {
            if h.actions.is_empty() {
                continue;
            }
            let mut probe = state::State::new();
            let out = script::run(&h.actions, &mut probe);
            if out.destination.is_some() {
                destinations += 1;
            } else if out.new_domain.is_none() && !out.redraw && !out.credits {
                no_destination += 1;
            }
            for u in out.unhandled {
                let name = u.split('(').next().unwrap_or(&u).trim().to_string();
                *unhandled.entry(name).or_default() += 1;
            }
            for e in out.effects {
                let key = match e {
                    script::Effect::Native { name, .. } => format!("native:{name}"),
                    other => format!("{other:?}").split(['{', '(']).next().unwrap_or("?").trim().to_string(),
                };
                *effects.entry(key).or_default() += 1;
            }
        }
    }

    // Decode every sprite every room can show, to find art that will not
    // resolve at run time. This opens all four chapter movies, so it is the
    // slowest check here.
    {
        let mut game = game::Game::new(dir)?;
        let (mut drawn, mut failed) = (0usize, 0usize);
        let mut bad_rooms = 0usize;
        for i in 0..game.world.nodes.len() {
            game.room = i;
            let before = failed;
            for (_, cast, _) in game.visible() {
                if game.has_art(cast) {
                    drawn += 1;
                } else {
                    failed += 1;
                }
            }
            if failed > before {
                bad_rooms += 1;
            }
        }
        println!("sprites decoding:    {drawn} ok, {failed} failed ({bad_rooms} rooms affected)");
    }

    println!("rooms parsed:        {}", world.len());
    println!("actions with a move: {destinations}");
    println!("actions, no move:    {no_destination}");
    let native: usize = effects.iter().filter(|(k, _)| k.starts_with("native:")).count();
    let native_calls: usize = effects
        .iter()
        .filter(|(k, _)| k.starts_with("native:"))
        .map(|(_, v)| *v)
        .sum();
    println!("engine effects:");
    for (k, v) in effects.iter().filter(|(k, _)| !k.starts_with("native:")) {
        println!("  {k:<24} {v:>5}");
    }
    println!("native handlers:     {native} distinct, {native_calls} call sites");
    if unhandled.is_empty() {
        println!("unhandled calls:     none");
    } else {
        println!("unhandled calls:");
        for (k, v) in &unhandled {
            println!("  {k:<24} {v:>5}");
        }
    }
    Ok(())
}

/// Minimal PNG writer, so exporting art needs no image dependency.
fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, e) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
        }
        c ^ 0xffff_ffff
    }

    /// Stored-mode deflate: no compression, but a valid zlib stream, which keeps
    /// the exporter dependency-free.
    fn deflate_stored(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        for (i, block) in data.chunks(65535).enumerate() {
            let last = (i + 1) * 65535 >= data.len();
            out.push(if last { 1 } else { 0 });
            out.extend_from_slice(&(block.len() as u16).to_le_bytes());
            out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
            out.extend_from_slice(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        out.extend_from_slice(&((b << 16) | a).to_be_bytes());
        out
    }

    let mut chunk = |tag: &[u8; 4], data: &[u8]| {
        let mut c = Vec::with_capacity(data.len() + 12);
        c.extend_from_slice(&(data.len() as u32).to_be_bytes());
        c.extend_from_slice(tag);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc32(&[&tag[..], data].concat()).to_be_bytes());
        c
    };

    let mut hdr = Vec::new();
    hdr.extend_from_slice(&w.to_be_bytes());
    hdr.extend_from_slice(&h.to_be_bytes());
    hdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA

    let mut raw = Vec::with_capacity((w * h * 4 + h) as usize);
    for y in 0..h as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }

    let mut f = std::fs::File::create(path)?;
    f.write_all(b"\x89PNG\r\n\x1a\n")?;
    f.write_all(&chunk(b"IHDR", &hdr))?;
    f.write_all(&chunk(b"IDAT", &deflate_stored(&raw)))?;
    f.write_all(&chunk(b"IEND", &[]))?;
    Ok(())
}
