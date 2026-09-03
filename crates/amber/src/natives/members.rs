//! Scripts that belong to a cast member rather than to a room.
//!
//! Director lets a cast member carry its own handlers, and a click on a sprite
//! showing that member runs them before the room sees it. The game has
//! twenty-eight, and until now none were read: the dispatch in `Game::click`
//! knew about the telegram's tiles by channel number and nothing else.
//!
//! They matter because they are the only way to work a few things. The PeeK
//! unit's readouts are the clearest case -- the scan playback is a `mouseDown`
//! on the member `TXT-tonal ready`, and with it missing there is no click
//! anywhere in the shipped data that reads a tonal residue. `full.walk` had to
//! do it by hand, which is the debt entry 145 recorded.

use lingo::Value;

use crate::script::{Effect, Outcome};
use crate::state::State;

/// `(chapter, member name, handler)`, matched on the member a sprite shows.
///
/// Cast names are matched without case, as everywhere else.
const MEMBER_SCRIPTS: &[(&str, &str, &str)] = &[
    ("ROXY", "TXT-tonal ready", "readtonalresidue"),
];

/// The handler for the member a sprite is showing, if it has one.
pub fn script_for(domain: &str, member: &str) -> Option<&'static str> {
    MEMBER_SCRIPTS
        .iter()
        .find(|(chapter, name, _)| {
            chapter.eq_ignore_ascii_case(domain) && name.eq_ignore_ascii_case(member)
        })
        .map(|(_, _, handler)| *handler)
}

pub fn call(name: &str, _args: &[Value], state: &mut State, out: &mut Outcome) -> bool {
    match name {
        // on mouseDown  -- cast 'TXT-tonal ready'
        //   camSprite = 44
        //   whichKnob = getState( oStoryteller, #DoorWithScanUnit )
        //   PKscan = #None
        //   if whichKnob = #kitchenOutside    then PKscan = #PkPatioScan
        //   if whichKnob = #bathroomInside    then PKscan = #PkBathroomScan
        //   if whichKnob = #margaretRmOutside then PKscan = #Pk40sScan
        //   if whichKnob = #boatHouseOutside  then PKscan = #PkBoathouseScan
        //   if PKscan = #None
        //     then put ">>> mouseDown( cast 'TXT-tonal ready' ): I don't have
        //               a scan listed for this knob.."
        //     else trimState( #tonalResidueRemaining, PKscan )
        //   set the visible of sprite camSprite = TRUE
        //
        // Playing back a tonal residue. The unit knows which knob it was
        // attached to and takes that scan off the list of the four; the film
        // of the playback is on sprite 44, which the last line shows.
        //
        // This is the click `full.walk` could not make. Four knobs in the
        // house carry a scan and the PeeK unit reads one of them back, and
        // without this handler the whole chain -- residue read, psionic waves
        // present, the telephone ringing -- had no reachable start.
        "readtonalresidue" => {
            const KNOBS: [(&str, &str); 4] = [
                ("kitchenOutside", "PkPatioScan"),
                ("bathroomInside", "PkBathroomScan"),
                ("margaretRmOutside", "Pk40sScan"),
                ("boatHouseOutside", "PkBoathouseScan"),
            ];
            let knob = state.get("DoorWithScanUnit");
            let Some((_, scan)) = KNOBS.iter().find(|(k, _)| knob.is_symbol(k)) else {
                // The original says so and carries on, and so does this: a
                // knob with no scan listed is not an error, it is a readout
                // with nothing to play.
                crate::trace!(
                    crate::trace::Topic::Script,
                    "no scan listed for knob {knob:?}"
                );
                return true;
            };
            state.trim_item(
                "tonalResidueRemaining",
                &Value::Symbol((*scan).into()),
            );
            out.effects.push(Effect::SpriteVisible {
                channel: 44,
                visible: true,
            });
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
    fn the_readout_plays_back_the_scan_for_the_knob_it_was_on() {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all(
            "tonalResidueRemaining",
            ["PkPatioScan", "PkBathroomScan", "Pk40sScan", "PkBoathouseScan"]
                .map(|s| Value::Symbol(s.into()))
                .to_vec(),
        );
        state.set_all(
            "DoorWithScanUnit",
            vec![Value::Symbol("kitchenOutside".into())],
        );

        let mut out = Outcome::default();
        assert!(call("readtonalresidue", &[], &mut state, &mut out));
        assert!(
            !state
                .get_all("tonalResidueRemaining")
                .iter()
                .any(|v| v.is_symbol("PkPatioScan")),
            "the patio scan is read and comes off the list"
        );
        assert_eq!(state.get_all("tonalResidueRemaining").len(), 3);
    }

    #[test]
    fn a_knob_with_no_scan_listed_is_not_an_error() {
        let mut state = State::new();
        state.set_all("gChapter", vec![Value::Symbol("ROXY".into())]);
        state.set_all("DoorWithScanUnit", vec![Value::Symbol("nowhere".into())]);
        let mut out = Outcome::default();
        assert!(call("readtonalresidue", &[], &mut state, &mut out));
        assert!(out.effects.is_empty());
    }

    #[test]
    fn the_readout_is_found_by_the_member_a_sprite_shows() {
        assert_eq!(
            script_for("ROXY", "txt-tonal ready"),
            Some("readtonalresidue")
        );
        assert_eq!(script_for("MARGARET", "TXT-tonal ready"), None);
        assert_eq!(script_for("ROXY", "TXT-blank"), None);
    }
}
