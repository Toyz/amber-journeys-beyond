use lingo::{parse_dat, parse_value, Value};

#[test]
fn parses_property_lists() {
    let v = parse_value("[#castNum: 1590, #channel: 1, #ink: 0]").unwrap();
    assert_eq!(v.get_int("castNum"), Some(1590));
    assert_eq!(v.get_int("channel"), Some(1));
}

#[test]
fn property_lookup_ignores_case() {
    let v = parse_value("[#CastNum: 7]").unwrap();
    assert_eq!(v.get_int("castnum"), Some(7));
    assert_eq!(v.get_int("CASTNUM"), Some(7));
}

#[test]
fn rect_uses_lingo_argument_order() {
    // rect(left, top, right, bottom), which is not Director's binary order.
    let r = parse_value("rect(46, 64, 347, 356)").unwrap().as_rect().unwrap();
    assert_eq!((r.left, r.top, r.right, r.bottom), (46, 64, 347, 356));
    assert!(r.contains(100, 100));
    assert!(!r.contains(400, 100));
}

#[test]
fn distinguishes_empty_list_from_empty_property_list() {
    assert_eq!(parse_value("[]").unwrap(), Value::List(vec![]));
    assert!(matches!(parse_value("[:]").unwrap(), Value::Props(m) if m.is_empty()));
}

#[test]
fn nested_lists_and_symbols() {
    let v = parse_value(r#"[#a: [1, 2], #b: "text", #c: #sym]"#).unwrap();
    assert_eq!(v.get_list("a").unwrap().len(), 2);
    assert_eq!(v.get_str("b"), Some("text"));
    assert_eq!(v.get("c").unwrap().as_symbol(), Some("sym"));
}

#[test]
fn splits_dat_records_on_0xbc() {
    // Records are separated by 0xBC, and the file opens with a dated banner.
    let mut data = b"* 10/4/96,4:22 PM *   ".to_vec();
    data.extend_from_slice(b"[#castNum: 1]");
    data.push(0xBC);
    data.extend_from_slice(b"[#castNum: 2]");
    data.push(0xBC);
    let records = parse_dat(&data).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].get_int("castNum"), Some(1));
    assert_eq!(records[1].get_int("castNum"), Some(2));
}

#[test]
fn symbols_compare_case_insensitively() {
    let a = Value::Symbol("Forward".into());
    let b = Value::Symbol("forward".into());
    assert!(a.loosely_eq(&b));
}

// The cases below are regressions. Each is a bug that shipped and was found
// by playing the game rather than by any check, so each gets a test naming
// what went wrong.

#[test]
fn boolean_literals_are_the_integers_guards_compare_against() {
    // `setState( ..., #FrontDoorIsOpen, TRUE )` has to satisfy `= 1`. Parsed
    // as a bare word it became a symbol matching neither 1 nor 0, and the
    // door could be opened but never walked through.
    assert_eq!(parse_value("TRUE").unwrap(), Value::Int(1));
    assert_eq!(parse_value("FALSE").unwrap(), Value::Int(0));
    assert_eq!(parse_value("true").unwrap(), Value::Int(1));
    assert_eq!(parse_value("False").unwrap(), Value::Int(0));
}

#[test]
fn property_lists_keep_repeated_keys() {
    // A compound guard is two entries under one key. Storing them in a map
    // drops one clause, which unlocked every locked thing in the game.
    let v = parse_value("[#and: [#equals: [#a, 1], #equals: [#b, 2]]]").unwrap();
    let inner = v.get("and").unwrap();
    assert_eq!(inner.entries().len(), 2, "both clauses must survive");
    assert_eq!(inner.get_all("equals").len(), 2);
}

#[test]
fn integer_property_keys_parse() {
    // A movie's event track keys cues by frame number. Rejecting these failed
    // the whole enclosing list, which in one chapter was its entire sound bank.
    let v = parse_value(r#"[165: 90, 167: ["assertSound #x"], 173: 120]"#).unwrap();
    assert_eq!(v.entries().len(), 3);
    assert_eq!(v.get("165").and_then(Value::as_int), Some(90));
    assert!(v.get("167").unwrap().as_list().is_some());
}

#[test]
fn first_entry_wins_for_a_repeated_key() {
    // Lingo's own behaviour, and what `get` has to do so a guard reads the
    // clause the authors wrote first.
    let v = parse_value("[#k: 1, #k: 2]").unwrap();
    assert_eq!(v.get_int("k"), Some(1));
    assert_eq!(v.get_all("k").len(), 2);
}

#[test]
fn symbols_may_contain_a_period() {
    // The clock tables key on time of day: `#t1`, `#t1.15`, `#t1.30`. Stopping
    // the name at the period made the whole chunk unparsable, which cost every
    // state-indexed sprite in that chapter its art.
    let v = parse_value("[#t1: 635, #t1.15: 636, #t1.30: 637]").unwrap();
    assert_eq!(v.entries().len(), 3);
    assert_eq!(v.get_int("t1.15"), Some(636));
    assert_eq!(v.get_int("t1"), Some(635));
}

#[test]
fn a_period_is_part_of_a_name_only_when_a_name_continues() {
    // The lookahead exists so a period that ends a name cannot run it into
    // whatever follows. Without it, `#t1.` would absorb the period and the
    // entry separator would be read one byte late.
    let v = parse_value("[#a.b: 1, #c: 2]").unwrap();
    assert_eq!(v.get_int("a.b"), Some(1));
    assert_eq!(v.get_int("c"), Some(2));

    // A period with nothing name-like after it terminates the symbol.
    let v = parse_value("[#a, #b]").unwrap();
    assert_eq!(v.as_list().map(<[_]>::len), Some(2));
}

/// Lingo compares symbols without regard to case, and the two pressings of the
/// disc disagree: the PC location table says `bedrm_fadeIn`, the Macintosh one
/// says `bedrm_fadein`. Everything that tests a symbol has to go through this.
#[test]
fn symbols_compare_without_regard_to_case() {
    let v = Value::Symbol("bedrm_fadeIn".into());
    assert!(v.is_symbol("bedrm_fadein"));
    assert!(v.is_symbol("BEDRM_FADEIN"));
    assert!(v.is_symbol("#bedrm_fadeIn"));
    assert!(!v.is_symbol("bedrm_fadeOut"));

    // A leading hash on either side is the same symbol.
    assert!(Value::Symbol("#off".into()).is_symbol("off"));
    assert!(!Value::Symbol("off".into()).is_symbol("on"));

    // Nothing else answers to a symbol.
    assert!(!Value::String("off".into()).is_symbol("off"));
    assert!(!Value::Void.is_symbol("off"));
}

