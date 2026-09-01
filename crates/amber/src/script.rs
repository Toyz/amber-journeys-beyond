//! Interpreter for the handful of Lingo calls Amber's hotspots invoke.
//!
//! The rooms never run arbitrary Lingo: the action strings are drawn from a
//! closed vocabulary of about twenty calls, all of which either move the player,
//! read or write a flag, or shuffle inventory. That makes a small
//! call-and-arguments evaluator sufficient, with no need for the Director
//! bytecode VM that the `Lscr` chunks would otherwise demand.

use lingo::{parse_value, Value};

use crate::state::State;

/// A deferred effect produced by a room's actions, in the order the script
/// listed them. The game loop plays these back; the script layer only decides
/// what happens, never how it is presented.
#[derive(Clone, PartialEq, Debug)]
pub enum Effect {
    /// Play a QuickTime movie, from `pushVideo`. The name is almost always
    /// absent: the room nominates its movie through the sprite it places on the
    /// `#video` channel, and `pushVideo` just starts whatever is loaded there.
    PlayVideo(Option<String>),
    /// Stop whatever movie is playing, from `killVideo`.
    StopVideo,
    /// A one-shot sound effect or a voice line.
    PlaySound { name: String, loudness: Option<String> },
    /// Start an ambient loop at the given volume, 0-255.
    StartLoop { name: String, volume: Option<i32> },
    /// Stop an ambient loop, optionally fading.
    StopLoop { name: String, fade: bool },
    /// Duck or restore the whole ambient bed.
    SuspendSounds { fade: bool },
    RestoreSounds { fade: bool },
    /// Block for a number of ticks (a tick is 1/60 s).
    WaitTicks(u32),
    /// Block until the current movie finishes.
    WaitForVideo,
    /// Block until a named sound finishes.
    WaitForSound(String),
    /// Cross-fade to one of the five montage sequences.
    FadeToMontage(i32),
    /// Hide the cursor for the next scripted beat.
    CursorOff,
    /// Claim or release a sprite channel for script control.
    PuppetSprite { channel: u8, on: bool },
    /// Point a script-controlled channel at a cast member.
    SpriteCast { channel: u8, cast: u32 },
    /// Move a script-controlled channel.
    SpriteLoc { channel: u8, x: i32, y: i32 },
    /// Show or hide a script-controlled channel.
    SpriteVisible { channel: u8, visible: bool },
    /// Point a channel at a cast the chapter names rather than numbers, as
    /// `getProp(oPuppeteer, #doorStatic)` does. Resolved when applied, since
    /// the table belongs to the chapter and handlers do not carry one.
    SpriteCastNamed { channel: u8, name: String },
    /// Point a channel at one of an inventory item's icons, by position.
    SpriteCastIcon { channel: u8, item: String, index: usize },
    /// Write a flag in timeline order, for a value that must land between two
    /// waits rather than when the handler ran.
    SetState { key: String, value: Value },
    /// Remove an item from a list-valued flag, in timeline order.
    ///
    /// A handler's effects are played back later while its state writes happen
    /// as it runs, so anything that must follow a wait has to be an effect.
    /// The haunts turn on this: their movie is shown only while the haunt is
    /// still in the pool, so trimming it before the movie plays consumes the
    /// haunt without ever showing it.
    TrimState { key: String, item: Value },
    /// A puzzle or set-piece handler that still lives in Director bytecode.
    Native { name: String, args: Vec<Value> },
}

/// What running a hotspot's actions asked the game to do next.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// Destination room symbol, from `goTo`.
    pub destination: Option<String>,
    /// Movement flavour, which selects the transition movie (`#turnLeft`, ...).
    pub transition: Option<String>,
    /// Set when the player crossed into another chapter.
    pub new_domain: Option<String>,
    /// True when the script asked to return to the previous room.
    pub go_back: bool,
    /// True if the stage needs redrawing without a move.
    pub redraw: bool,
    /// Set by `showCreditScreen`.
    pub credits: bool,
    /// Timeline of audio, video and pacing effects, in script order.
    pub effects: Vec<Effect>,
    /// Re-run this action while the button stays down.
    ///
    /// Director's dials spin for as long as `stillDown` reports the mouse
    /// held, polling inside the handler. This engine acts on the release edge,
    /// so the handler takes one step and asks to be run again, which the front
    /// end does on an interval until the button comes up.
    pub repeat_while_held: bool,
    /// Statements we could not parse at all, kept for diagnostics.
    pub unhandled: Vec<String>,
}

/// One parsed `name(arg, arg, ...)` call.
struct Call {
    name: String,
    args: Vec<Value>,
}

/// Splits `body` on commas that sit outside brackets and quotes.
fn split_args(body: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut start = 0usize;
    for (i, &c) in body.as_bytes().iter().enumerate() {
        match c {
            b'"' => in_string = !in_string,
            b'(' | b'[' if !in_string => depth += 1,
            b')' | b']' if !in_string => depth -= 1,
            b',' if !in_string && depth == 0 => {
                args.push(body[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !body[start..].trim().is_empty() {
        args.push(body[start..].to_string());
    }
    args
}

fn to_values(raw: &[String]) -> Vec<Value> {
    raw.iter()
        .map(|a| a.trim())
        .map(|a| parse_value(a).unwrap_or_else(|_| Value::Symbol(a.to_string())))
        .collect()
}

/// Splits a statement into its verb and arguments.
///
/// Lingo accepts both call syntax and command syntax for the same handler, and
/// the game data uses each freely: `goTo( #Foo, #forward )` and
/// `setLoop #garage, 180` are both single calls. Trailing `--` comments are
/// stripped first, since the authors left many in place mid-line.
fn parse_call(src: &str) -> Option<Call> {
    let mut src = src.trim();
    // Strip a trailing comment, taking care not to cut inside a string.
    if let Some(i) = find_comment(src) {
        src = src[..i].trim();
    }
    if src.is_empty() {
        return None;
    }

    // Call syntax: the name is everything before the first parenthesis, provided
    // that parenthesis closes at the end of the statement.
    if let Some(open) = src.find('(') {
        if src.ends_with(')') {
            let name = src[..open].trim();
            if !name.is_empty() && !name.contains(' ') {
                return Some(Call {
                    name: name.to_ascii_lowercase(),
                    args: to_values(&split_args(&src[open + 1..src.len() - 1])),
                });
            }
        }
    }

    // Command syntax: the verb is the first word, the rest is its argument list.
    let (name, rest) = match src.find(char::is_whitespace) {
        Some(i) => (&src[..i], src[i..].trim()),
        None => (src, ""),
    };
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(Call {
        name: name.to_ascii_lowercase(),
        args: to_values(&split_args(rest)),
    })
}

/// Splits `set the <property> of <object> = <value>` into property and value.
///
/// The separator is `=` or, in a few places, the word `to`. Both spellings are
/// Lingo and the data uses each.
fn parse_property_assignment(src: &str) -> Option<(String, String)> {
    parse_assignment(src).map(|(p, _, v)| (p, v))
}

/// Splits `set the <property> of <target> = <value>` into all three parts.
///
/// The target matters for sprite properties: `set the loc of sprite 39` names
/// the channel being moved, and dropping it leaves the assignment unusable.
fn parse_assignment(src: &str) -> Option<(String, String, String)> {
    let lower = src.to_ascii_lowercase();
    let rest = lower.strip_prefix("set the ")?;
    let offset = src.len() - rest.len();

    // Property name runs up to " of ".
    let of = rest.find(" of ")?;
    let property = src[offset..offset + of].trim().to_string();
    let _ = &property;

    // Value follows the assignment operator.
    let tail = &rest[of + 4..];
    let (sep, width) = match (tail.find('='), tail.find(" to ")) {
        (Some(i), Some(j)) if j < i => (j, 4),
        (Some(i), _) => (i, 1),
        (None, Some(j)) => (j, 4),
        (None, None) => return None,
    };
    let value_start = offset + of + 4 + sep + width;
    let value = src.get(value_start..)?.trim().to_string();
    let target = src.get(offset + of + 4..value_start.saturating_sub(width))?
        .trim()
        .to_string();
    if property.is_empty() || value.is_empty() {
        return None;
    }
    Some((property, target, value))
}

/// Reads the channel from a target like `sprite 39`.
fn channel_of(target: &str) -> Option<u8> {
    let rest = target.trim().strip_prefix("sprite")?;
    rest.trim().parse().ok()
}

/// Turns a sprite property assignment into the matching effect.
fn push_sprite_effect(property: &str, channel: u8, value: &str, out: &mut Outcome) {
    let parsed = parse_value(value).ok();
    match property.to_ascii_lowercase().as_str() {
        "castnum" => {
            if let Some(cast) = parsed.as_ref().and_then(Value::as_int) {
                out.effects.push(Effect::SpriteCast {
                    channel,
                    cast: cast.max(0) as u32,
                });
            }
        }
        "loc" => {
            if let Some((x, y)) = parsed.as_ref().and_then(Value::as_point) {
                out.effects.push(Effect::SpriteLoc { channel, x, y });
            }
        }
        // Other sprite properties are presentation details the renderer does
        // not model; recording them keeps the count honest.
        _ => out.effects.push(Effect::Native {
            name: format!("set the {property} of sprite"),
            args: Vec::new(),
        }),
    }
}

/// Finds the start of a trailing `--` comment, ignoring one inside a string.
fn find_comment(src: &str) -> Option<usize> {
    let b = src.as_bytes();
    let mut in_string = false;
    let mut i = 0;
    while i + 1 < b.len() {
        match b[i] {
            b'"' => in_string = !in_string,
            b'-' if !in_string && b[i + 1] == b'-' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Resolves an argument that may itself be a `getState` lookup.
fn eval_arg(arg: &Value, state: &State) -> Value {
    // A nested call survives parsing as a bare symbol; re-read it here.
    if let Value::Symbol(s) = arg {
        if s.contains('(') {
            if let Some(call) = parse_call(s) {
                if call.name == "getstate" {
                    if let Some(key) = call.args.last().and_then(|v| v.as_str()) {
                        return state.get(key);
                    }
                }
            }
        }
    }
    arg.clone()
}

/// Reads an argument as a bare symbol name, dropping any leading `#`.
fn symbol_of(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.trim_start_matches('#').to_string())
}

/// True when an argument names a fade, e.g. `#fadeOut` or `#slowFade`.
fn is_fade(args: &[Value]) -> bool {
    args.iter()
        .filter_map(|a| a.as_str())
        .any(|s| s.to_ascii_lowercase().contains("fade"))
}

/// Splits a single-line `if <condition> then <statement>` into its two halves.
///
/// The data uses this only for short guards, most often a platform check
/// (`if gCPU = #PC then ...`) or a flag test, so a textual split is enough and
/// no general expression parser is needed.
fn split_if(src: &str) -> Option<(String, String)> {
    let lower = src.to_ascii_lowercase();
    if !lower.trim_start().starts_with("if ") {
        return None;
    }
    // Find the `then` that separates condition from body.
    let then = lower.match_indices("then").find(|(i, _)| {
        let before = lower[..*i].chars().last();
        let after = lower[i + 4..].chars().next();
        before.is_some_and(char::is_whitespace)
            && after.map_or(true, |c| c.is_whitespace())
    })?;
    let cond = src[lower.find("if ")? + 3..then.0].trim().to_string();
    let body = src[then.0 + 4..].trim().to_string();
    Some((cond, body))
}

/// Evaluates the small conditions that appear in single-line `if` guards.
///
/// Returns `None` when the condition is not one of the recognised shapes, which
/// the caller treats as "run the body anyway": these guards gate presentation,
/// not progress, so failing open keeps the game moving.
fn eval_condition(cond: &str, state: &State) -> Option<bool> {
    // `gCPU` is the authoring-time platform switch. This port behaves as the
    // Windows build, whose branches are the more complete ones in this data.
    let lower = cond.to_ascii_lowercase();
    if lower.contains("gcpu") {
        let is_pc = lower.contains("#pc");
        return Some(if lower.contains("<>") { !is_pc } else { is_pc });
    }

    // `getState( oStoryteller, #key ) = #value` and its negation.
    let (op, negate) = if let Some(i) = cond.find("<>") {
        (i, true)
    } else if let Some(i) = cond.find('=') {
        (i, false)
    } else {
        return None;
    };
    let lhs = eval_arg(&parse_value(cond[..op].trim()).ok()?, state);
    let rhs_text = cond[op + if negate { 2 } else { 1 }..].trim();
    let rhs = eval_arg(&parse_value(rhs_text).ok()?, state);
    Some(lhs.loosely_eq(&rhs) != negate)
}

/// Runs a room's action list against `state`, returning what should happen next.
pub fn run(actions: &[String], state: &mut State) -> Outcome {
    let mut out = Outcome::default();
    for line in actions {
        exec(line, state, &mut out);
    }
    out
}

/// Executes one statement, appending its effects to `out`.
fn exec(line: &str, state: &mut State, out: &mut Outcome) {
    let line = line.trim();
    if line.is_empty() || line.starts_with("--") {
        return;
    }

    // Single-line conditionals wrap a normal statement.
    if let Some((cond, body)) = split_if(line) {
        if eval_condition(&cond, state).unwrap_or(true) {
            exec(&body, state, out);
        }
        return;
    }

    let Some(call) = parse_call(line) else {
        out.unhandled.push(line.to_string());
        return;
    };
    let args: Vec<Value> = call.args.iter().map(|a| eval_arg(a, state)).collect();
    let arg = |i: usize| args.get(i).cloned();
    let name_arg = |i: usize| args.get(i).and_then(symbol_of);
    let int_arg = |i: usize| args.get(i).and_then(Value::as_int);

    match call.name.as_str() {
        // -- navigation ------------------------------------------------------
        // `#destination` is the authors' placeholder for "wherever the browse
        // region would lead", and is not a real room.
        "goto" => {
            if let Some(d) = name_arg(0).filter(|d| !d.eq_ignore_ascii_case("destination")) {
                out.destination = Some(d);
            }
            out.transition = name_arg(1).or(out.transition.take());
        }
        "goback" => out.go_back = true,
        "enternewdomain" => out.new_domain = name_arg(0),
        "settransition" => out.transition = name_arg(0).or(out.transition.take()),
        "showcreditscreen" => out.credits = true,

        // -- state -----------------------------------------------------------
        // `setState` takes the object, the key and the value; the object is
        // always `oStoryteller`, so only the trailing pair matters.
        // `addState( #list, #item )` adds to a set rather than replacing the
        // flag, the counterpart of trimState.
        "addstate" if args.len() >= 2 => {
            let key = args[args.len() - 2].as_str().unwrap_or_default().to_string();
            if !key.is_empty() {
                state.add_item(&key, args[args.len() - 1].clone());
            }
        }
        "setstate" | "addstate" | "setprop" => {
            if args.len() >= 2 {
                let key = args[args.len() - 2].as_str().unwrap_or_default().to_string();
                if !key.is_empty() {
                    state.set(&key, args[args.len() - 1].clone());
                }
            }
        }
        // `trimState( #list, #item )` removes an item from a list; the
        // one-argument form drops a flag outright.
        "trimstate" => match args.len() {
            0 => {}
            1 => {
                if let Some(k) = args[0].as_str() {
                    state.trim(k);
                }
            }
            _ => {
                if let Some(k) = args[args.len() - 2].as_str() {
                    let item = args[args.len() - 1].clone();
                    state.trim_item(k, &item);
                }
            }
        },
        // Bare reads matter only when nested in another call, which `eval_arg`
        // has already resolved by this point.
        "getstate" | "getprop" | "instate" | "nothing" | "idle" => {}

        // -- inventory -------------------------------------------------------
        "addinventory" => {
            if let Some(i) = name_arg(0) {
                state.add_inventory(&i);
            }
        }
        "deleteinventory" => {
            if let Some(i) = name_arg(0) {
                state.delete_inventory(&i);
            }
        }
        "useinventory" => {
            if let Some(i) = name_arg(0) {
                state.set("itemInUse", Value::Symbol(i));
            }
        }
        "stowinventory" => state.stow(),

        // -- presentation ----------------------------------------------------
        // Lingo's property-assignment statement, `set the X of Y = Z`.
        //
        // Almost every use in the room scripts queues a sound on the puppeteer,
        // which is how a move carries its own sound effect: the door heard on
        // the way through a doorway is queued by the hotspot that walks you
        // through it. Queuing and playing amount to the same thing here,
        // because the move follows immediately.
        "set" => {
            let text = line.trim();
            match parse_property_assignment(text) {
                Some((property, value)) if property.eq_ignore_ascii_case("queuedSound") => {
                    out.effects.push(Effect::PlaySound {
                        name: value.trim_start_matches('#').to_string(),
                        loudness: None,
                    });
                }
                // A sprite property names the channel it acts on, which the
                // renderer holds as a script-controlled puppet layer.
                Some(_) => match parse_assignment(text) {
                    Some((property, target, value)) if channel_of(&target).is_some() => {
                        let channel = channel_of(&target).unwrap();
                        push_sprite_effect(&property, channel, &value, out);
                    }
                    _ => out.effects.push(Effect::Native {
                        name: "set".into(),
                        args: Vec::new(),
                    }),
                },
                None => out.unhandled.push(text.to_string()),
            }
        }

        "updatedisplay" | "updatestage" => out.redraw = true,
        "cursoroff" => out.effects.push(Effect::CursorOff),
        "fadetomontage" => out.effects.push(Effect::FadeToMontage(int_arg(0).unwrap_or(1))),

        // -- video -----------------------------------------------------------
        "pushvideo" => out.effects.push(Effect::PlayVideo(name_arg(0))),
        "killvideo" => out.effects.push(Effect::StopVideo),

        // -- audio -----------------------------------------------------------
        // `assertSound` is a voice line, `soundEffect` and `startSound` are
        // one-shots; they differ in mixing priority, not in scheduling.
        "assertsound" | "soundeffect" | "startsound" | "playsting" => {
            if let Some(n) = name_arg(0) {
                out.effects.push(Effect::PlaySound {
                    name: n,
                    loudness: name_arg(1),
                });
            }
        }
        "setloop" => {
            if let Some(n) = name_arg(0) {
                out.effects.push(Effect::StartLoop {
                    name: n,
                    volume: int_arg(1),
                });
            }
        }
        "endloop" => {
            if let Some(n) = name_arg(0) {
                out.effects.push(Effect::StopLoop {
                    name: n,
                    fade: is_fade(&args[1..]),
                });
            }
        }
        "suspendsounds" => out.effects.push(Effect::SuspendSounds { fade: is_fade(&args) }),
        "restoresounds" => out.effects.push(Effect::RestoreSounds { fade: is_fade(&args) }),

        // -- pacing ----------------------------------------------------------
        "wait" => {
            let effect = match arg(0).as_ref() {
                Some(Value::Int(ticks)) => Effect::WaitTicks(*ticks as u32),
                Some(v) => match symbol_of(v).unwrap_or_default().to_ascii_lowercase().as_str() {
                    "videostop" => Effect::WaitForVideo,
                    "soundstop" => Effect::WaitForSound(name_arg(1).unwrap_or_default()),
                    _ => Effect::WaitTicks(0),
                },
                None => Effect::WaitTicks(0),
            };
            out.effects.push(effect);
        }

        // Set-piece handlers whose bodies are Director bytecode. Those that
        // have been decoded and ported run here; the rest are recorded so the
        // timeline stays intact and the engine's report stays honest about
        // what is still missing.
        _ => {
            if !crate::natives::call(&call.name, &args, state, out) {
                out.effects.push(Effect::Native {
                    name: call.name.clone(),
                    args,
                });
            }
        }
    }
}
