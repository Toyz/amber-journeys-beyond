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
