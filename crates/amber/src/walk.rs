//! A terminal walkthrough, for reproducing a route without the window.
//!
//! Prints the current room with the hotspots that are live under the current
//! state, so a route can be replayed step by step and a guard that should have
//! blocked a move is visible at the point it fails.

use std::io::{BufRead, Write};
use std::path::Path;

use crate::game::Game;
use crate::script;
use crate::world::Verb;

pub fn walk(root: &Path, script_steps: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut game = Game::new(root)?;

    // `--replay <file>` takes the steps from a recording made by `play`.
    // Blank lines and comments are skipped, so a recording can be annotated
    // and trimmed down to the shortest route that still fails.
    let mut script_steps = script_steps.to_vec();
    if let Some(i) = script_steps.iter().position(|a| a == "--replay") {
        let path = script_steps
            .get(i + 1)
            .ok_or("--replay needs a file")?
            .clone();
        let text = std::fs::read_to_string(&path)?;
        script_steps = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect();
        println!("replaying {} steps from {path}", script_steps.len());
    }
    let script_steps = &script_steps[..];
    let interactive = script_steps.is_empty();

    // A recording starts where the player was, not in the opening film, and
    // the window's `--replay` already clicks through it. Doing it in only one
    // of the two front ends meant the same file walked the house in a window
    // and failed every step in the terminal, which is the drift entry 122 set
    // out to avoid. Typing `skip` still works for anyone watching it live.
    if !interactive {
        game.skip_opening();
    }

    show(&mut game);
    if interactive {
        println!("\ncommands: a verb (forward, left, right, up, down, examine, pointer),");
        println!("          a room name, `state [filter]`, `blocked`,");
        println!("          `give <item>`, `use <item>`, `set <flag> <value>`,");
        println!("          `click x y`, `inv x y`,");
        println!("          `skip`, `quit`");
    }

    let stdin = std::io::stdin();
    let mut lines: Box<dyn Iterator<Item = String>> = if interactive {
        Box::new(stdin.lock().lines().map_while(Result::ok))
    } else {
        Box::new(script_steps.iter().cloned())
    };

    // A replayed step that finds nothing is a regression, so a recording can
    // be used as a test rather than only read.
    let (mut broken, mut missed) = (0usize, 0usize);
    loop {
        if interactive {
            print!("> ");
            std::io::stdout().flush().ok();
        }
        let Some(line) = lines.next() else { break };
        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }
        if cmd == "quit" || cmd == "q" {
            break;
        }
        if !interactive {
            println!("> {cmd}");
        }

        match command(&mut game, cmd, true) {
            Step::Done => {}
            Step::Missed => missed += 1,
            Step::Broken => broken += 1,
        }
    }
    if !interactive && (broken > 0 || missed > 0) {
        println!("\n{broken} step(s) found no such room or exit, {missed} clicked nothing");
    }
    if broken > 0 {
        return Err(format!("{broken} step(s) of the recording no longer resolve").into());
    }
    Ok(())
}

/// What one replayed step did.
///
/// A click that lands on nothing is not a fault: a recording replays what a
/// player did, and players click the scenery. A step that names a room or a
/// verb the world does not have is different -- it means the world changed
/// under the recording, which is what a recording is worth testing for.
#[derive(PartialEq, Eq, Debug)]
pub(crate) enum Step {
    Done,
    Missed,
    Broken,
}

/// Runs one command, typed or replayed.
///
/// Shared with the window's `--replay`, so a recording drives the real game
/// through exactly the path the terminal takes -- there is no second reading
/// of a `.walk` file that could drift from this one.
pub(crate) fn command(game: &mut Game, cmd: &str, drain: bool) -> Step {
    if let Some(filter) = cmd.strip_prefix("state") {
        dump_state(game, filter.trim());
        return Step::Done;
    }
    // A click on the inventory bar, which `play` records separately because it
    // never reaches the room's hotspots.
    if let Some(rest) = cmd.strip_prefix("inv ") {
        let nums: Vec<i32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        return match nums[..] {
            [x, y] => {
                let taken = game.click_inventory(x, y, 640, 480);
                println!(
                    "  inventory ({x}, {y}): {}",
                    if taken { "taken" } else { "nothing there" }
                );
                // The bar can start a sequence -- taking the PeeK unit
                // opens it -- so the queue has to be run out here too, the
                // same as a click on the room.
                if drain {
                    settle(game);
                }
                show(game);
                if taken { Step::Done } else { Step::Missed }
            }
            _ => {
                println!("  usage: inv <x> <y>");
                Step::Broken
            }
        };
    }
    if cmd == "skip" {
        let skipped = game.skip_video();
        println!("  skip: {}", if skipped { "moved on" } else { "no movie" });
        if drain {
            settle(game);
        }
        show(game);
        return Step::Done;
    }
    // A click at a point goes through the same hit test the window uses, so an
    // overlap that resolves the wrong way can be reproduced exactly.
    if let Some(rest) = cmd.strip_prefix("click ") {
        let nums: Vec<i32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        match nums[..] {
            [x, y] => match game.hotspot_at(x, y) {
                Some((verb, bounds)) => {
                    println!("  hits {verb:?} {bounds:?}");
                    if let Some(o) = game.click(x, y) {
                        if let Some(d) = &o.destination {
                            println!("  -> {d}");
                        }
                    }
                    if drain {
                        settle(game);
                    }
                    show(game);
                }
                None => {
                    println!("  nothing at ({x}, {y})");
                    return Step::Missed;
                }
            },
            _ => {
                println!("  usage: click <x> <y>");
                return Step::Broken;
            }
        }
        return Step::Done;
    }
    // Writes what the stage looks like right now to a PNG.
    //
    // `shot` on the command line renders a room from its own declared
    // sprites, which is not the same picture: a puzzle laid out by a script
    // lives entirely on puppet channels and so does not appear there at all.
    // A recording can reach the puzzle and then ask for the frame.
    if let Some(path) = cmd.strip_prefix("shot ") {
        const W: u32 = 640;
        const H: u32 = 480;
        let mut frame = vec![0u32; (W * H) as usize];
        game.draw(&mut frame, W, H);
        game.draw_inventory(&mut frame, W, H, false);
        let mut rgba = Vec::with_capacity(frame.len() * 4);
        for px in &frame {
            rgba.extend_from_slice(&[
                (px >> 16) as u8,
                (px >> 8) as u8,
                *px as u8,
                (px >> 24) as u8,
            ]);
        }
        let path = std::path::Path::new(path.trim());
        return match crate::write_png(path, W, H, &rgba) {
            Ok(()) => {
                println!("  wrote {}", path.display());
                Step::Done
            }
            Err(e) => {
                println!("  {}: {e}", path.display());
                Step::Broken
            }
        };
    }
    if cmd == "blocked" {
        show_blocked(game);
        return Step::Done;
    }
    // Puts a flag where a long stretch of play would have put it, so the
    // second half of the game can be reached without replaying the first.
    // A symbol unless it parses as a number, which is how the data reads.
    if let Some(rest) = cmd.strip_prefix("set ") {
        let mut parts = rest.split_whitespace();
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
            println!("  usage: set <flag> <value>");
            return Step::Broken;
        };
        let value = value.trim_start_matches('#');
        let parsed = value
            .parse::<i32>()
            .map(lingo::Value::Int)
            .unwrap_or_else(|_| lingo::Value::Symbol(value.to_string()));
        game.state.set(key, parsed.clone());
        println!("  {key} = {parsed:?}");
        show(game);
        return Step::Done;
    }
    // The other half of `set`: takes a value out of a flag's list, which is
    // what `trimState` does and what a recording needs when the click that
    // would have done it is not ported yet.
    if let Some(rest) = cmd.strip_prefix("trim ") {
        let mut parts = rest.split_whitespace();
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
            println!("  usage: trim <flag> <item>");
            return Step::Broken;
        };
        let value = value.trim_start_matches('#');
        let parsed = value
            .parse::<i32>()
            .map(lingo::Value::Int)
            .unwrap_or_else(|_| lingo::Value::Symbol(value.to_string()));
        game.state.trim_item(key, &parsed);
        println!("  {key} -= {parsed:?}");
        show(game);
        return Step::Done;
    }
    // Granting an item makes the #itemInUse hotspots reachable, which is most
    // of the game's interaction and otherwise needs real progress.
    if let Some(item) = cmd.strip_prefix("give ") {
        game.state.add_inventory(item.trim());
        println!("  carrying: {}", game.state.inventory().join(", "));
        return Step::Done;
    }
    if let Some(item) = cmd.strip_prefix("use ") {
        let item = item.trim();
        game.state.stow();
        game.state.set("itemInUse", lingo::Value::Symbol(item.to_string()));
        println!("  in hand: {item}");
        show(game);
        return Step::Done;
    }

    match step(game, cmd) {
        Ok(()) => {
            if drain {
                settle(game);
            }
            show(game);
            Step::Done
        }
        Err(msg) => {
            println!("  {msg}");
            Step::Broken
        }
    }
}

/// Runs out the effect queue, reporting what it carried.
///
/// Not only sound: a sequence is films, waits and state writes interleaved,
/// and the order they come in is usually the thing worth seeing.
fn settle(game: &mut Game) {
    for line in game.settle() {
        println!("    {line}");
    }
}

/// Takes one step: a verb, or a room name to jump to.
fn step(game: &mut Game, cmd: &str) -> Result<(), String> {
    // `pointer 2` takes the second live hotspot carrying that verb, which the
    // room listing numbers. A bare verb is the first, as it always was.
    let (word, nth) = match cmd.split_once(char::is_whitespace) {
        Some((w, rest)) => match rest.trim().parse::<usize>() {
            Ok(n) if n >= 1 => (w, n),
            _ => (cmd, 1),
        },
        None => (cmd, 1),
    };
    if let Some(verb) = parse_verb(word) {
        let hit = {
            let state = &game.state;
            game.node()
                .hotspots
                .iter()
                .filter(|h| h.verb == verb && !h.actions.is_empty())
                .filter(|h| state.test(&h.condition))
                .nth(nth - 1)
                .cloned()
        };
        let Some(h) = hit else {
            // Say whether the affordance exists but is gated, since that is
            // exactly the case worth reporting.
            let gated = game
                .node()
                .hotspots
                .iter()
                .any(|h| h.verb == verb && !h.actions.is_empty());
            let live = {
                let state = &game.state;
                game.node()
                    .hotspots
                    .iter()
                    .filter(|h| h.verb == verb && !h.actions.is_empty())
                    .filter(|h| state.test(&h.condition))
                    .count()
            };
            return Err(if nth > 1 && live > 0 {
                format!("{cmd}: only {live} live here")
            } else if gated {
                format!("{cmd}: present but blocked by its guard (try `blocked`)")
            } else {
                format!("{cmd}: no such exit here")
            });
        };
        // Through the same path a click takes, so a sequence stops at each
        // of its waits instead of running to the end in one instant.
        game.begin(&h.actions);
        return Ok(());
    }

    // A chapter name goes to that chapter's opening, the same as `play`.
    if let Some(domain) = game
        .world
        .domains
        .keys()
        .find(|d| d.eq_ignore_ascii_case(cmd))
        .cloned()
    {
        game.enter_chapter(&domain);
        game.start_room_video();
        return Ok(());
    }

    // Otherwise treat it as a room name.
    let from = game.node().domain.clone();
    match game.world.resolve(cmd, Some(&from)) {
        Some(i) => {
            // Jumping across chapters skips the entry that would have seeded
            // this one, and an unseeded chapter reads every flag as void.
            let domain = game.world.nodes[i].domain.clone();
            if domain != from {
                game.seed_chapter(&domain);
            }
            game.jump_to(i);
            game.start_room_video();
            Ok(())
        }
        None => Err(format!("{cmd}: not a verb or a known room")),
    }
}

fn parse_verb(s: &str) -> Option<Verb> {
    Some(match s.to_ascii_lowercase().as_str() {
        "forward" | "f" => Verb::Forward,
        "left" | "l" => Verb::Left,
        "right" | "r" => Verb::Right,
        "up" | "u" => Verb::Up,
        "down" | "d" => Verb::Down,
        "examine" | "x" => Verb::Examine,
        "pointer" | "p" => Verb::Pointer,
        "nextpage" | "n" => Verb::NextPage,
        // The three that were missing. `browse` is how a close-up is backed
        // out of and appears in most rooms, and the two dials turn Margaret's
        // wireless -- so a recording could not say them, and every route that
        // needed one had to be written as a jump to a room name instead.
        "browse" | "b" => Verb::Browse,
        "rotateleft" => Verb::RotateLeft,
        "rotateright" => Verb::RotateRight,
        _ => return None,
    })
}

fn show(game: &mut Game) {
    let node = game.node();
    let name = node.name.clone().unwrap_or_else(|| format!("#{}", node.index));
    let art = node
        .sprites
        .iter()
        .find(|s| matches!(s.channel, crate::world::Channel::Sprite(1)))
        .and_then(|s| s.cast_name.clone())
        .unwrap_or_else(|| "-".into());
    println!("\n{} / {name}   [{art}]", node.domain);
    if let Some(m) = game.video() {
        println!("  movie: {m}");
    }
    // The ambient mix is per room, so showing it makes a loop that should
    // have stopped on the way out visible without needing to hear it.
    let held = game.state.inventory();
    let hand = game.state.item_in_use();
    // Also when the bag is empty. Picking the PeeK unit up is `useInventory`,
    // which puts it in the hand without adding it to the bag -- the game adds
    // it when it is stowed -- so the first thing the player ever holds was
    // reported as holding nothing at all.
    if !held.is_empty() || hand.is_some() {
        // With the slot each one occupies, because the bar has seven fixed
        // places rather than a queue: a recording clicks a slot, and which
        // slot an item is in is not the order it was picked up in.
        let bag: Vec<String> = game
            .state
            .slots()
            .map(|(slot, item)| format!("{slot}:{item}"))
            .collect();
        println!(
            "  carrying: {}   in hand: {}",
            if bag.is_empty() { "(nothing)".into() } else { bag.join(" ") },
            hand.unwrap_or("nothing"),
        );
    }
    let mix = game.ambience();
    if mix.is_empty() {
        println!("  ambience: (silent)");
    } else {
        let parts: Vec<String> = mix
            .iter()
            .map(|(n, level)| format!("{n} {:.0}%", level * 100.0))
            .collect();
        println!("  ambience: {}", parts.join(", "));
    }

    let mut any = false;
    // Numbered per verb, because naming a verb takes the first live hotspot
    // that carries it and rooms routinely offer several. The open package on
    // the porch offers two pointers: one reads the letter and one leaves, and
    // typing `pointer` takes the letter for ever. `pointer 2` takes the other.
    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (index, h) in game.node().hotspots.clone().iter().enumerate() {
        if h.actions.is_empty() || !game.state.test(&h.condition) {
            continue;
        }
        let verb = format!("{:?}", h.verb).to_lowercase();
        let nth = seen.entry(verb.clone()).or_default();
        *nth += 1;
        let label = if *nth > 1 { format!("{verb} {nth}") } else { verb.clone() };

        let _speculative = crate::trace::Probe::begin();
        let mut probe = game.state.clone();
        let dest = script::run(&h.actions, &mut probe)
            .destination
            .unwrap_or_else(|| "-".into());
        match click_point(game, index, h) {
            Some((cx, cy)) => println!("  {label:<11} -> {dest:<24} click {cx} {cy}"),
            // A region entirely underneath others cannot be clicked at all, so
            // say so rather than printing a point that lands somewhere else.
            None => println!("  {label:<11} -> {dest:<24} (covered; use `{label}`)"),
        }
        any = true;
    }
    if !any {
        println!("  (no live exits)");
    }
}

/// A point inside a hotspot that the hit test actually resolves *to* it.
///
/// The middle of the region is the obvious answer and is usually right, but
/// hotspots overlap and the first in the room's list wins, so the middle of a
/// region lying under a wider one belongs to the wider one. Printing that
/// point against this row is a lie a recording then acts on -- which is how a
/// route out of the mailbox turned into a click straight back into it.
fn click_point(game: &Game, index: usize, h: &crate::world::Hotspot) -> Option<(i32, i32)> {
    let b = &h.bounds;
    let mid = |lo: i32, hi: i32| (lo + hi) / 2;
    let holding = game.state.item_in_use().is_some();
    let hits = |x: i32, y: i32| {
        let state = &game.state;
        game.node().hit_index(x, y, holding, |c| state.test(c)) == Some(index)
    };
    // The middle first, then the four quarter points and the corners just
    // inside the edge: enough to find a sliver of a region that is only
    // partly covered, without searching every pixel of a 640 by 480 rect.
    let xs = [mid(b.left, b.right), b.left + 2, b.right - 2, mid(b.left, mid(b.left, b.right)), mid(mid(b.left, b.right), b.right)];
    let ys = [mid(b.top, b.bottom), b.top + 2, b.bottom - 2, mid(b.top, mid(b.top, b.bottom)), mid(mid(b.top, b.bottom), b.bottom)];
    for y in ys {
        for x in xs {
            if b.contains(x, y) && hits(x, y) {
                return Some((x, y));
            }
        }
    }
    None
}

/// Lists the hotspots that exist but whose guards currently fail.
fn show_blocked(game: &Game) {
    let state = &game.state;
    let mut any = false;
    for h in &game.node().hotspots {
        if h.actions.is_empty() || state.test(&h.condition) {
            continue;
        }
        println!(
            "  {:<10} blocked by {:?}",
            format!("{:?}", h.verb).to_lowercase(),
            h.condition
        );
        any = true;
    }
    if !any {
        println!("  nothing here is blocked");
    }
}

fn dump_state(game: &Game, filter: &str) {
    let mut shown = 0;
    // `itemInUse` is not in the property store: it has its own field, because
    // what is in the hand is not one of a flag's declared settings. Dumping
    // the store alone printed the schema's list of every item that could ever
    // be held, headed by `#None`, whatever was actually in the hand -- which
    // reads as "holding nothing" and is why picking up the PeeK unit looked
    // like it had failed when it had not.
    if "iteminuse".contains(&filter.to_ascii_lowercase()) {
        println!("  iteminuse = {:?}", game.state.item_in_use());
        shown += 1;
    }
    for (key, value) in game.state.entries() {
        if key == "iteminuse" {
            continue;
        }
        if !filter.is_empty() && !key.contains(&filter.to_ascii_lowercase()) {
            continue;
        }
        println!("  {key} = {value:?}");
        shown += 1;
    }
    if shown == 0 {
        println!("  no state matching {filter:?}");
    }
}
