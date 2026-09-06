//! The three documents that shipped beside the game.
//!
//! `HINTS.WRI`, `MANUAL.WRI` and `README.WRI` sit in the root of the disc and
//! were meant to be opened in Windows Write. Nobody has Windows Write, and on
//! a phone there is nowhere to open them at all -- so the engine reads them
//! itself and the menu shows them.
//!
//! This is worth doing rather than shipping a transcription: the hints file
//! carries the walkthrough for the first third of the game and the three BAR
//! settings, and a player stuck at midnight on a phone has no other way to
//! reach any of it. It is also the disc's own text, which is the standard the
//! rest of this port holds to.
//!
//! Two formats, despite the shared extension:
//!
//!   - `HINTS.WRI` is RTF, in plain ASCII with backslash keywords.
//!   - `MANUAL.WRI` and `README.WRI` are Word 6 documents in an OLE2 compound
//!     file, which is to say `.doc` files that were renamed. The text is
//!     eight-bit and contiguous, which is the one mercy of that format: the
//!     piece table only matters for a document that has been edited since it
//!     was last saved whole, and these have not been.
//!
//! Neither decoder is general. They read these three files, and they say so
//! rather than pretending to be libraries.

use crate::content::Content;

/// One document, wrapped to a column.
#[derive(Clone)]
pub struct Doc {
    pub title: String,
    /// Paragraphs, each already broken into lines that fit.
    pub lines: Vec<String>,
}

/// What the disc has, in the order the menu should offer it.
///
/// Hints first. It is the one a stuck player wants, and putting the ninety
/// kilobyte manual above it would bury it.
const FILES: [(&str, &str); 3] = [
    ("HINTS", "HINTS.WRI"),
    ("MANUAL", "MANUAL.WRI"),
    ("README", "README.WRI"),
];

/// Reads and wraps every document the disc carries.
///
/// A missing or unreadable file is left out rather than reported: some
/// pressings may not carry all three, and a menu entry that opens onto an
/// error is worse than an entry that is not there.
pub fn load(content: &dyn Content, columns: usize) -> Vec<Doc> {
    // The disc's own listing rather than a guessed path, because the case of
    // the name is whatever the pressing used and an ISO is not case-folding.
    let listing = content.list();
    FILES
        .iter()
        .filter_map(|(title, want)| {
            let path = listing
                .iter()
                .find(|p| p.rsplit('/').next().is_some_and(|f| f.eq_ignore_ascii_case(want)))?;
            let bytes = content.read(path)?;
            let text = decode(&bytes)?;
            Some(Doc {
                title: (*title).into(),
                lines: wrap(&text, columns),
            })
        })
        .collect()
}

/// Picks a decoder by what the bytes actually are, not by the extension.
pub fn decode(bytes: &[u8]) -> Option<String> {
    const OLE2: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    if bytes.starts_with(b"{\\rtf") {
        Some(from_rtf(bytes))
    } else if bytes.starts_with(&OLE2) {
        from_word(bytes)
    } else {
        None
    }
}

/// The text of an RTF file.
///
/// Not a renderer. Keywords are dropped, `\par` becomes a line break, and the
/// groups that exist only to carry settings -- the font and colour tables,
/// and anything marked `\*` as ignorable -- are skipped whole, because their
/// contents are font names and would otherwise be read as the first paragraph.
fn from_rtf(bytes: &[u8]) -> String {
    let src = bytes;
    let mut out = String::new();
    let mut i = 0;
    // How deep inside a group that is being skipped, and what depth it began
    // at, so nested groups inside it are skipped too.
    let mut depth = 0i32;
    let mut skip_from: Option<i32> = None;

    while i < src.len() {
        match src[i] {
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if skip_from.is_some_and(|d| depth < d) {
                    skip_from = None;
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                // An escaped brace, backslash, or a hex byte.
                match src.get(i) {
                    Some(c @ (b'{' | b'}' | b'\\')) => {
                        if skip_from.is_none() {
                            out.push(*c as char);
                        }
                        i += 1;
                        continue;
                    }
                    Some(b'\'') => {
                        let hex = src.get(i + 1..i + 3).and_then(|h| {
                            u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
                        });
                        if let Some(byte) = hex {
                            if skip_from.is_none() {
                                out.extend(cp1252(byte));
                            }
                            i += 3;
                            continue;
                        }
                    }
                    // `\*` marks a group whose contents are not text.
                    Some(b'*') => {
                        skip_from = skip_from.or(Some(depth));
                        i += 1;
                        continue;
                    }
                    _ => {}
                }

                let start = i;
                while src.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
                    i += 1;
                }
                let word = std::str::from_utf8(&src[start..i]).unwrap_or_default().to_string();
                // A numeric parameter, which may be negative.
                if src.get(i) == Some(&b'-') {
                    i += 1;
                }
                while src.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                // Exactly one space after a keyword is part of the keyword.
                if src.get(i) == Some(&b' ') {
                    i += 1;
                }
                if skip_from.is_none() {
                    match word.as_str() {
                        "par" | "line" | "sect" | "page" => out.push('\n'),
                        "tab" => out.push('\t'),
                        // The tables at the top of every RTF are the font
                        // names and the palette. Read as text they come out
                        // as "MS Sans Serif;Symbol;System;" ahead of the
                        // title, which is what a naive strip produces.
                        "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" => {
                            skip_from = Some(depth);
                        }
                        _ => {}
                    }
                }
            }
            // Literal newlines in the file are formatting, not text: RTF
            // breaks lines wherever it likes and says `\par` when it means it.
            b'\r' | b'\n' => i += 1,
            c => {
                if skip_from.is_none() {
                    out.extend(cp1252(c));
                }
                i += 1;
            }
        }
    }
    out
}

/// The text of a Word 6 document inside an OLE2 compound file.
///
/// The `WordDocument` stream opens with the FIB, whose `fcMin` and `fcMac` at
/// offsets 0x18 and 0x1C bracket the text. Both these files are `nFib` 101,
/// which is Word 6, where a character is a byte.
fn from_word(bytes: &[u8]) -> Option<String> {
    let doc = ole_stream(bytes, "WordDocument")?;
    let (from, to) = (u32le(&doc, 0x18)? as usize, u32le(&doc, 0x1c)? as usize);
    if from >= to || to > doc.len() {
        return None;
    }
    let mut out = String::new();
    let mut field = 0i32;
    for &byte in &doc[from..to] {
        match byte {
            // A field: `\x13 instruction \x14 result \x15`. The instruction is
            // machinery -- "embed Paint.Picture Object1" is the manual's first
            // line otherwise -- and the result, where there is one, is text.
            0x13 => field += 1,
            0x14 => field -= 1,
            0x15 => field = field.min(0),
            _ if field > 0 => {}
            // Word's paragraph mark, and the placeholders for a picture and a
            // cell boundary.
            0x0d => out.push('\n'),
            0x07 | 0x0b => out.push('\n'),
            0x01 | 0x02 | 0x08 => {}
            c => out.extend(cp1252(c)),
        }
    }
    Some(out)
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// One named stream out of an OLE2 compound file.
///
/// Only as much of the format as these two documents need: a single FAT
/// reachable from the header's own 109-entry list, and a stream long enough
/// not to live in the mini-FAT. Both hold for a Word document of any size,
/// because the text stream is the big one.
fn ole_stream(bytes: &[u8], want: &str) -> Option<Vec<u8>> {
    let sector = 1usize << u16le(bytes, 0x1e)?;
    if sector < 64 || sector > 4096 {
        return None;
    }
    let fat_sectors = u32le(bytes, 0x2c)? as usize;
    let dir_start = u32le(bytes, 0x30)? as usize;

    // The header's DIFAT: where the FAT itself lives. A file needing more
    // than 109 FAT sectors is over 400 MB, which no document on this disc is.
    let mut fat: Vec<u32> = Vec::new();
    for n in 0..fat_sectors.min(109) {
        let at = u32le(bytes, 0x4c + n * 4)? as usize;
        let start = 512 + at * sector;
        let block = bytes.get(start..start + sector)?;
        fat.extend(block.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())));
    }

    let read = |first: usize, limit: usize| -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut at = first;
        // Bounded by the file's own sector count, so a FAT that loops back on
        // itself stops rather than running until memory does.
        let most = bytes.len() / sector + 2;
        for _ in 0..most {
            if at >= 0xffff_fffa {
                break;
            }
            let start = 512 + at * sector;
            out.extend_from_slice(bytes.get(start..start + sector)?);
            if out.len() >= limit {
                break;
            }
            at = *fat.get(at)? as usize;
        }
        out.truncate(limit.min(out.len()));
        Some(out)
    };

    let directory = read(dir_start, usize::MAX)?;
    for entry in directory.chunks_exact(128) {
        let used = u16le(entry, 0x40)? as usize;
        if used < 4 || used > 64 {
            continue;
        }
        // UTF-16, and the length counts the terminator.
        let name: String = entry[..used - 2]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .filter_map(|u| char::from_u32(u as u32))
            .collect();
        // 2 is a stream; 1 is a storage and 5 the root.
        if entry[0x42] != 2 || name != want {
            continue;
        }
        let first = u32le(entry, 0x74)? as usize;
        let size = u32le(entry, 0x78)? as usize;
        return read(first, size);
    }
    None
}

/// The bytes above 0x7f that these documents actually use.
///
/// Windows-1252 rather than Latin-1, because the difference is exactly the
/// punctuation a word processor inserts: curly quotes, an em dash, and the
/// trademark sign that follows the game's name on every other line.
fn cp1252(byte: u8) -> Option<char> {
    Some(match byte {
        0x91 | 0x92 => '\'',
        0x93 | 0x94 => '"',
        0x95 => '*',
        0x96 | 0x97 => '-',
        0x85 => '.',
        0xa9 => 'C',
        0xae => 'R',
        // The trademark sign, which follows the game's name on every other
        // line. Dropped rather than transliterated: "Journeys BeyondT" is
        // worse than "Journeys Beyond", and the text spells it out in full
        // where it matters.
        0x99 => return None,
        // Control characters are structure, not text. The hints file ends
        // with a stray NUL, and a document with an unprintable in it draws a
        // hole in the middle of a word.
        0x00..=0x08 | 0x0b..=0x1f | 0x7f => return None,
        0x09 | 0x0a | 0x0d => byte as char,
        0x20..=0x7e => byte as char,
        _ => ' ',
    })
}

/// Breaks text into lines no wider than `columns`.
///
/// Blank lines are kept -- they are the paragraph breaks, and a manual run
/// together is not a manual -- but runs of them are collapsed, because a word
/// processor leaves a great many behind.
pub fn wrap(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(8);
    let mut out: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let para = para.replace('\t', "    ");
        let para = para.trim_end();
        if para.trim().is_empty() {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let mut line = String::new();
        for word in para.split_whitespace() {
            // A word longer than the column is cut rather than allowed to run
            // off the edge, which on a 640 wide stage is most of a sentence.
            if word.len() > columns {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
                for part in word.as_bytes().chunks(columns) {
                    out.push(String::from_utf8_lossy(part).into_owned());
                }
                continue;
            }
            if line.is_empty() {
                line = word.to_string();
            } else if line.len() + 1 + word.len() <= columns {
                line.push(' ');
                line.push_str(word);
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    while out.first().is_some_and(String::is_empty) {
        out.remove(0);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc() -> Option<Box<dyn Content>> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        root.is_dir().then(|| crate::iso::open(&root).expect("extract/ is not a game"))
    }

    /// The hints file is RTF, and the first thing in it must be the heading
    /// rather than the font table.
    #[test]
    fn the_hints_read_as_the_hints() {
        let Some(content) = disc() else { return };
        let docs = load(content.as_ref(), 60);
        let hints = docs.iter().find(|d| d.title == "HINTS").expect("no hints on the disc");
        let text = hints.lines.join("\n");

        assert!(
            !text.contains("MS Sans Serif"),
            "the font table came through as text:\n{}",
            &text[..text.len().min(200)]
        );
        assert!(hints.lines[0].contains("HINTS"), "first line is {:?}", hints.lines[0]);
        // The three BAR settings, which is the single most asked-for line in
        // the file and the reason it is worth showing at all.
        assert!(
            text.contains("6, 5, and 8"),
            "the BAR settings are not in the hints"
        );
        assert!(text.contains("crowbar"), "the crowbar hint is missing");
    }

    /// The manual and the readme are Word 6 inside an OLE2 file.
    #[test]
    fn the_word_documents_read_as_prose() {
        let Some(content) = disc() else { return };
        let docs = load(content.as_ref(), 60);

        let manual = docs.iter().find(|d| d.title == "MANUAL").expect("no manual on the disc");
        let text = manual.lines.join("\n");
        assert!(
            !text.contains("Paint.Picture"),
            "a field instruction came through as text: {:?}",
            &text[..text.len().min(120)]
        );
        assert!(text.contains("AMBER"), "the manual does not name the game");
        assert!(manual.lines.len() > 100, "the manual is {} lines", manual.lines.len());

        let readme = docs.iter().find(|d| d.title == "README").expect("no readme on the disc");
        assert!(
            readme.lines.join("\n").contains("ReadMe"),
            "the readme does not read like one"
        );
    }

    /// Wrapping keeps every word and never exceeds the column.
    #[test]
    fn wrapping_fits_the_column_and_drops_nothing() {
        let text = "one two three four five\n\n\n\nsix seven";
        let lines = wrap(text, 12);
        assert!(lines.iter().all(|l| l.chars().count() <= 12), "{lines:?}");
        assert_eq!(
            lines.join(" ").split_whitespace().collect::<Vec<_>>(),
            ["one", "two", "three", "four", "five", "six", "seven"]
        );
        // One blank between the paragraphs, not four.
        assert_eq!(lines.iter().filter(|l| l.is_empty()).count(), 1, "{lines:?}");
    }

    /// A word too long for the column is cut rather than left to run off.
    #[test]
    fn an_overlong_word_is_broken() {
        let lines = wrap("supercalifragilistic", 8);
        assert!(lines.iter().all(|l| l.chars().count() <= 8), "{lines:?}");
        assert_eq!(lines.concat(), "supercalifragilistic");
    }
}

#[cfg(test)]
mod eyeball {
    /// Prints the documents as the menu will show them.
    ///
    /// `cargo test -p amber --release -- --ignored --nocapture eyeball` --
    /// there is no assertion worth writing about whether prose reads well, so
    /// this is here to be looked at rather than to pass.
    /// The same three documents, read out of an ISO rather than a directory.
    ///
    /// This is what the phone does: the APK carries the disc image and the
    /// engine reads it where it lies. A path that works over a directory and
    /// not over an image would leave the reader empty on the one front end
    /// that has nowhere else to open a document.
    #[test]
    #[ignore]
    fn from_an_image() {
        let iso = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../android/assets/amber.iso");
        if !iso.is_file() {
            println!("no image to read");
            return;
        }
        let content = crate::iso::open(&iso).expect("the image did not open");
        let docs = super::load(content.as_ref(), 49);
        for doc in &docs {
            println!("{}: {} lines, first {:?}", doc.title, doc.lines.len(), doc.lines.first());
        }
        assert_eq!(docs.len(), 3, "the image did not yield all three documents");
    }

    #[test]
    #[ignore]
    fn read_them() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extract");
        let Ok(content) = crate::iso::open(&root) else { return };
        for doc in super::load(content.as_ref(), 58) {
            println!("\n===== {} ({} lines) =====", doc.title, doc.lines.len());
            for line in doc.lines.iter().take(28) {
                println!("|{line}");
            }
        }
    }
}
