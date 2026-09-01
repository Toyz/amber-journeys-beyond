//! Prints the top-level keys of a named text cast member.
fn main() {
    let mut a = std::env::args().skip(1);
    let m = director::Movie::open(a.next().unwrap()).unwrap();
    let want = a.next().unwrap();
    for member in m.members() {
        if member.resource == 0 {
            continue;
        }
        if member.name.as_deref() != Some(want.as_str()) {
            continue;
        }
        let Some(text) = m.text(member.number) else { return };
        // Walk the top level, printing each key and what kind of value it has.
        let b = text.as_bytes();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut i = 0usize;
        while i < b.len() {
            match b[i] {
                b'"' => in_string = !in_string,
                b'[' if !in_string => depth += 1,
                b']' if !in_string => depth -= 1,
                b'#' if !in_string && depth == 1 => {
                    let start = i + 1;
                    let mut j = start;
                    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                        j += 1;
                    }
                    // Only a key if a colon follows.
                    let mut k = j;
                    while k < b.len() && b[k] == b' ' {
                        k += 1;
                    }
                    if k < b.len() && b[k] == b':' {
                        let peek: String = text[k + 1..(k + 42).min(text.len())]
                            .chars()
                            .take(38)
                            .collect();
                        println!("  #{:<24} {}", &text[start..j], peek.replace('\n', " "));
                    }
                    i = j;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        return;
    }
    println!("no member named {want}");
}
