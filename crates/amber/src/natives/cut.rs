//! Handlers the disc carries and the game never calls.
//!
//! The audit in entry 178 turned up sixty-two handlers that nothing reaches.
//! Most are Director housekeeping -- palette tools, debug printers, `xxx`
//! leftovers. Three are finished pieces of the game that no hotspot, no
//! handler and no idle ever asks for, and they are here so they can be
//! watched: `amber play` binds them to a key.
//!
//! Nothing in the game calls any of this. It is not wired into `natives::call`
//! for that reason -- a stray `setState` must not be able to reach it.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

/// One cut handler: which chapter it belongs to, what it does, and what has
/// to be true for it to do anything.
pub struct Cut {
    pub chapter: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    /// The guard in the handler's own words, or none if it has one.
    pub needs: Option<&'static str>,
}

pub const CUT: &[Cut] = &[
    Cut {
        chapter: "EDWIN",
        name: "backSeatDriver",
        about: "Chippy nags from the passenger seat while the car sits at a junction",
        needs: Some("the chipmunk in the car (#chippyLocation = #inCar)"),
    },
    Cut {
        chapter: "EDWIN",
        name: "secretMission",
        about: "the chipmunk does something amusing in the corner of the windscreen",
        needs: None,
    },
    Cut {
        chapter: "MARGARET",
        name: "blackWings",
        about: "black wings sweep in from both sides of the stage",
        needs: None,
    },
];

/// What a chapter has, for a front end offering to show it.
pub fn in_chapter(domain: &str) -> Vec<&'static Cut> {
    CUT.iter()
        .filter(|c| c.chapter.eq_ignore_ascii_case(domain))
        .collect()
}

pub fn call(name: &str, args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    match name.to_ascii_lowercase().as_str() {
        // on backSeatDriver
        //   if getState( #chippyLocation ) <> #inCar then return
        //   if idle( 2 ) <> 1 then return
        //   if inState( #utterancesRemaining, #makeaDecision ) then say #makeaDecision
        //   else if inState( #utterancesRemaining, #impressMe ) then say #impressMe
        //   else if inState( #utterancesRemaining, #smartyPants ) then say #smartyPants
        //   else say #stopForDirections
        //   assertSound thisOne : wait #soundStop
        //   if thisOne = #impressMe then startSound #imThinking : wait #soundStop
        //
        // Chippy, waiting for you to pick a direction. Four remarks in order,
        // each taken off `#utterancesRemaining` as the car's own comments are,
        // and the second of them is followed by him thinking about it.
        //
        // It is `carComments`' twin and it is never called: `carComments` runs
        // when a drive ends and nothing at all runs while the car stands at a
        // hub. The lines are on the disc.
        "backseatdriver" => {
            if !state.get("chippyLocation").is_symbol("inCar") {
                return true;
            }
            const REMARKS: [&str; 3] = ["makeaDecision", "impressMe", "smartyPants"];
            let remaining = state.get_all("utterancesRemaining").to_vec();
            let has = |what: &str| remaining.iter().any(|v| v.is_symbol(what));
            let said = REMARKS
                .iter()
                .find(|r| has(r))
                .copied()
                .unwrap_or("stopForDirections");

            out.effects.push(Effect::PlaySound {
                name: said.into(),
                loudness: None,
            });
            out.effects.push(Effect::WaitForSound(said.into()));
            if said == "impressMe" {
                out.effects.push(Effect::PlaySound {
                    name: "imThinking".into(),
                    loudness: None,
                });
                out.effects.push(Effect::WaitForSound("imThinking".into()));
            }
            out.redraw = true;
        }

        // on secretMission
        //   set the visible of sprite 44 = FALSE : updateStage : wait 20
        //   puppetSprite 45, 1
        //   set the castNum of sprite 45 = 936      -- chipamus.mov
        //   set the loc     of sprite 45 = point( 500, 400 )
        //   set the visible of sprite 45 = TRUE : updateStage
        //   ... hold until a mouseDown ...
        //   set the loc of sprite 45 = point( 1000, 1000 ) : puppetSprite 45, 0
        //
        // An easter egg with no way in. `chipamus.mov` is 76 by 72 -- the
        // chipmunk on his own, amusing himself -- parked in the bottom right
        // of the windscreen and held there until the player clicks.
        "secretmission" => {
            const PASSENGER: u8 = 45;
            const CHIPAMUS: u32 = 936;
            out.effects.push(Effect::SpriteVisible { channel: 44, visible: false });
            out.effects.push(Effect::WaitTicks(20));
            out.effects.push(Effect::PuppetSprite { channel: PASSENGER, on: true });
            out.effects.push(Effect::SpriteCast { channel: PASSENGER, cast: CHIPAMUS });
            out.effects.push(Effect::SpriteLoc { channel: PASSENGER, x: 500, y: 400 });
            out.effects.push(Effect::PlayOverlay { channel: PASSENGER });
            out.effects.push(Effect::WaitForClick);
            out.effects.push(Effect::SpriteLoc { channel: PASSENGER, x: 1000, y: 1000 });
            out.effects.push(Effect::PuppetSprite { channel: PASSENGER, on: false });
            out.effects.push(Effect::SpriteVisible { channel: 44, visible: true });
            out.redraw = true;
        }

        // on blackWings inOrOut
        //   leftWing = 38 : rightWing = 40
        //   both puppeted, both pointed at oPuppeteer's #blackWing
        //   if inOrOut = #out then left starts at h 70,  right at 570
        //                     else left starts at h 30,  right at 670
        //   repeat five times: move each twenty pixels towards the other,
        //                      updateStage
        //
        // Two black wings closing across the stage from the sides. Margaret's,
        // and nothing in her chapter or anywhere else asks for them.
        "blackwings" => {
            const LEFT: u8 = 38;
            const RIGHT: u8 = 40;
            let out_wards = args.first().is_some_and(|v| v.is_symbol("out"));
            let (mut left, mut right) = if out_wards { (70, 570) } else { (30, 670) };
            for channel in [LEFT, RIGHT] {
                out.effects.push(Effect::PuppetSprite { channel, on: true });
                out.effects.push(Effect::SpriteCastNamed {
                    channel,
                    name: "blackWing".into(),
                });
            }
            for _ in 0..5 {
                out.effects.push(Effect::SpriteLoc { channel: LEFT, x: left, y: 13 });
                out.effects.push(Effect::SpriteLoc { channel: RIGHT, x: right, y: 13 });
                out.effects.push(Effect::WaitTicks(2));
                left += 20;
                right -= 20;
            }
            out.redraw = true;
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chippy_works_down_his_list_of_complaints() {
        let mut s = State::new();
        s.set_all("chippyLocation", vec![Value::Symbol("inCar".into())]);
        let say = |s: &mut State| {
            let mut out = Outcome::default();
            assert!(call("backSeatDriver", &[], s, &mut out));
            out.effects.iter().find_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
        };

        s.set_all(
            "utterancesRemaining",
            ["makeaDecision", "impressMe", "smartyPants"]
                .map(|u| Value::Symbol(u.into()))
                .to_vec(),
        );
        assert_eq!(say(&mut s).as_deref(), Some("makeaDecision"));

        // With the first spent he moves on, and that one he follows up.
        s.set_all(
            "utterancesRemaining",
            ["impressMe", "smartyPants"].map(|u| Value::Symbol(u.into())).to_vec(),
        );
        let mut out = Outcome::default();
        call("backSeatDriver", &[], &mut s, &mut out);
        let said: Vec<String> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::PlaySound { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(said, vec!["impressMe", "imThinking"]);

        // And when he has said everything, he asks you to stop and ask.
        s.set_all("utterancesRemaining", vec![]);
        assert_eq!(say(&mut s).as_deref(), Some("stopForDirections"));
    }

    #[test]
    fn he_says_nothing_when_he_is_not_in_the_car() {
        let mut s = State::new();
        s.set_all("chippyLocation", vec![Value::Symbol("waiting".into())]);
        let mut out = Outcome::default();
        assert!(call("backSeatDriver", &[], &mut s, &mut out));
        assert!(out.effects.is_empty());
    }

    #[test]
    fn the_wings_close_from_both_sides() {
        let mut s = State::new();
        let mut out = Outcome::default();
        assert!(call("blackWings", &[Value::Symbol("out".into())], &mut s, &mut out));
        let left: Vec<i32> = out
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::SpriteLoc { channel: 38, x, .. } => Some(*x),
                _ => None,
            })
            .collect();
        assert_eq!(left, vec![70, 90, 110, 130, 150]);
    }

    #[test]
    fn each_chapter_knows_what_it_is_hiding() {
        assert_eq!(in_chapter("EDWIN").len(), 2);
        assert_eq!(in_chapter("MARGARET").len(), 1);
        assert!(in_chapter("ROXY").is_empty());
        // Every one of them answers to its own name.
        for cut in CUT {
            let mut s = State::new();
            let mut out = Outcome::default();
            assert!(call(cut.name, &[], &mut s, &mut out), "{}", cut.name);
        }
    }
}
