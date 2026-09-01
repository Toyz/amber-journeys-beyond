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
    let interactive = script_steps.is_empty();

    show(&mut game);
    if interactive {
        println!("\ncommands: a verb (forward, left, right, up, down, examine, pointer),");
        println!("          a room name, `state [filter]`, `blocked`,");
        println!("          `give <item>`, `use <item>`, `quit`");
    }

    let stdin = std::io::stdin();
    let mut lines: Box<dyn Iterator<Item = String>> = if interactive {
        Box::new(stdin.lock().lines().map_while(Result::ok))
    } else {
        Box::new(script_steps.to_vec().into_iter())
    };

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

        if let Some(filter) = cmd.strip_prefix("state") {
            dump_state(&game, filter.trim());
            continue;
        }
        // A click at a point goes through the same hit test the window uses,
        // so an overlap that resolves the wrong way can be reproduced exactly.
        if let Some(rest) = cmd.strip_prefix("click ") {
            let nums: Vec<i32> = rest
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
            match nums[..] {
                [x, y] => match game.hotspot_at(x, y) {
                    Some((verb, bounds)) => {
                        println!("  hits {verb:?} {bounds:?}");
                        if let Some(o) = game.click(x, y) {
                            if let Some(d) = &o.destination {
                                println!("  -> {d}");
                            }
                        }
                        show(&mut game);
                    }
                    None => println!("  nothing at ({x}, {y})"),
                },
                _ => println!("  usage: click <x> <y>"),
            }
            continue;
        }
        if cmd == "blocked" {
            show_blocked(&game);
            continue;
        }
        // Granting an item makes the #itemInUse hotspots reachable, which is
        // most of the game's interaction and otherwise needs real progress.
        if let Some(item) = cmd.strip_prefix("give ") {
            game.state.add_inventory(item.trim());
            println!("  carrying: {}", game.state.inventory().join(", "));
            continue;
        }
        if let Some(item) = cmd.strip_prefix("use ") {
            let item = item.trim();
            game.state.stow();
            game.state
                .set("itemInUse", lingo::Value::Symbol(item.to_string()));
            println!("  in hand: {item}");
            show(&mut game);
            continue;
        }

        match step(&mut game, cmd) {
            Ok(()) => show(&mut game),
            Err(msg) => println!("  {msg}"),
        }
    }
    Ok(())
}

/// Takes one step: a verb, or a room name to jump to.
fn step(game: &mut Game, cmd: &str) -> Result<(), String> {
    if let Some(verb) = parse_verb(cmd) {
        let hit = {
            let state = &game.state;
            game.node()
                .hotspots
                .iter()
                .filter(|h| h.verb == verb && !h.actions.is_empty())
                .find(|h| state.test(&h.condition))
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
            return Err(if gated {
                format!("{cmd}: present but blocked by its guard (try `blocked`)")
            } else {
                format!("{cmd}: no such exit here")
            });
        };
        let outcome = script::run(&h.actions, &mut game.state);
        game.apply_outcome(&outcome);
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
    if !held.is_empty() {
        let hand = game.state.item_in_use().unwrap_or("nothing");
        println!("  carrying: {}   in hand: {hand}", held.join(", "));
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

    let state = &game.state;
    let mut any = false;
    for h in &game.node().hotspots {
        if h.actions.is_empty() || !state.test(&h.condition) {
            continue;
        }
        let _speculative = crate::trace::Probe::begin();
        let mut probe = state.clone();
        let dest = script::run(&h.actions, &mut probe)
            .destination
            .unwrap_or_else(|| "-".into());
        println!("  {:<10} -> {dest}", format!("{:?}", h.verb).to_lowercase());
        any = true;
    }
    if !any {
        println!("  (no live exits)");
    }
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
    for (key, value) in game.state.entries() {
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
