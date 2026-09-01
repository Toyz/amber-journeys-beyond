use std::collections::HashMap;
use std::path::Path;

use crate::bitmap::{self, Bitmap};
use crate::chunk::{Endian, FourCc, Reader, Resource};
use crate::palette::Palette;
use crate::sound::{self, Sound};
use crate::{Error, Result};

const TAG_MMAP: &[u8; 4] = b"mmap";
const TAG_KEY: &[u8; 4] = b"KEY*";
const TAG_CAS: &[u8; 4] = b"CAS*";
const TAG_BITD: &[u8; 4] = b"BITD";
const TAG_CLUT: &[u8; 4] = b"CLUT";
const TAG_SND: &[u8; 4] = b"snd ";
const TAG_VWCF: &[u8; 4] = b"VWCF";
const TAG_LNAM: &[u8; 4] = b"Lnam";
const TAG_LSCR: &[u8; 4] = b"Lscr";

/// The kinds of cast member Director 5 can hold. Amber only exercises a few of
/// these, but the discriminants are the on-disk values so unknown members still
/// round-trip rather than aborting the load.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CastKind {
    Bitmap,
    FilmLoop,
    Text,
    Palette,
    Picture,
    Sound,
    Button,
    Shape,
    Movie,
    DigitalVideo,
    Script,
    RichText,
    Ole,
    Transition,
    Unknown(u32),
}

impl From<u32> for CastKind {
    fn from(v: u32) -> Self {
        match v {
            1 => CastKind::Bitmap,
            2 => CastKind::FilmLoop,
            3 => CastKind::Text,
            4 => CastKind::Palette,
            5 => CastKind::Picture,
            6 => CastKind::Sound,
            7 => CastKind::Button,
            8 => CastKind::Shape,
            9 => CastKind::Movie,
            10 => CastKind::DigitalVideo,
            11 => CastKind::Script,
            12 => CastKind::RichText,
            13 => CastKind::Ole,
            14 => CastKind::Transition,
            other => CastKind::Unknown(other),
        }
    }
}

/// Metadata for one slot of the cast. `number` is the 1-based cast number the
/// game's `.DAT` scripts refer to via `#castNum`.
#[derive(Clone, Debug)]
pub struct CastMember {
    pub number: u32,
    pub kind: CastKind,
    pub name: Option<String>,
    /// Index into the `mmap` table of the `CASt` chunk itself.
    pub resource: u32,
    /// Bitmap geometry, present for `CastKind::Bitmap`.
    pub width: u16,
    pub height: u16,
    /// Bytes per source row; may exceed `width` because Director pads to an even
    /// boundary, and the high bit is a flag rather than part of the value.
    pub pitch: u16,
    pub bit_depth: u8,
    /// Whether a digital video member plays again when it reaches its end.
    ///
    /// Director stores this on the member, not on the sprite that places it,
    /// which is why nothing in a room's own record says whether its film is
    /// scenery or a one-shot. Ninety-two of the game's movies are marked: the
    /// ceiling fans, the door scanners, the fireplaces, the bubbling. The
    /// opening, the montages and the haunts are not.
    pub loops: bool,
    /// Registration point, the anchor Director positions the sprite by.
    ///
    /// Expressed in the member's own rectangle space, not the image's, so a
    /// member whose rectangle has a non-zero origin carries that origin in
    /// here too. Subtract [`origin_x`](Self::origin_x) and
    /// [`origin_y`](Self::origin_y) to get the offset within the image.
    pub reg_x: i16,
    pub reg_y: i16,
    /// Top-left of the member's rectangle, usually zero but not always.
    pub origin_x: i16,
    pub origin_y: i16,
    /// Cast number of the custom palette this bitmap wants, if any.
    pub palette_ref: i16,
}

impl CastMember {
    fn empty(number: u32) -> Self {
        CastMember {
            number,
            kind: CastKind::Unknown(0),
            name: None,
            loops: false,
            resource: 0,
            width: 0,
            height: 0,
            pitch: 0,
            bit_depth: 0,
            reg_x: 0,
            reg_y: 0,
            origin_x: 0,
            origin_y: 0,
            palette_ref: 0,
        }
    }
}

/// A loaded Director movie, holding the whole file in memory and indexing it.
///
/// Amber's largest movie (`ROXY.DXR`) is 129 MB, so the file is memory-mapped in
/// spirit but read eagerly here for portability; decoding is lazy and per-member.
pub struct Movie {
    data: Vec<u8>,
    endian: Endian,
    resources: Vec<Resource>,
    /// `(owner resource, child tag) -> child resource indices`, from `KEY*`.
    key: HashMap<(u32, FourCc), Vec<u32>>,
    /// Cast slot -> `CASt` resource index, from `CAS*`. Index 0 is cast number 1.
    cast_slots: Vec<u32>,
    members: Vec<CastMember>,
    by_name: HashMap<String, u32>,
    pub stage_width: u16,
    pub stage_height: u16,
}


/// Where a cast member's parts are, and which Director wrote it.
pub(crate) struct CastLayout {
    pub kind: u32,
    pub info: std::ops::Range<usize>,
    pub spec: std::ops::Range<usize>,
    /// Offset within `spec` of a bitmap's palette reference.
    pub palette_at: usize,
}

/// Works out which of the two `CASt` layouts a record uses.
///
/// Director 5 writes three `u32`s -- kind, info length, data length -- then the
/// info block and then the type-specific block. Director 4 writes a `u16` data
/// length and a `u32` info length, then the type-specific block, then the info
/// block; it has no kind field at all, because the kind is the first byte of
/// the type-specific block, followed by a flags byte.
///
/// The Macintosh release of Amber is a Director 4 movie and the PC release is
/// Director 5. Reading only the Director 5 shape turned every cast member on
/// the Macintosh disc into an unknown type: its first four bytes are the data
/// length in the high half and the top of the info length in the low half,
/// which is why they all came out as large round numbers like 1835008.
///
/// The two are told apart by which arithmetic accounts for the whole record.
/// Both are checked and neither is guessed; across Roxy's 2444 members exactly
/// one of them fits every time.
pub(crate) fn cast_layout(cd: &[u8], endian: Endian) -> Option<CastLayout> {
    let read = |at: usize, wide: bool| -> Option<usize> {
        let mut r = Reader::at(cd, endian, at);
        Some(if wide { r.u32().ok()? as usize } else { r.u16().ok()? as usize })
    };

    // Director 5: kind, info length, data length, all wide.
    if let (Some(kind), Some(info_len), Some(data_len)) =
        (read(0, true), read(4, true), read(8, true))
    {
        if cd.len() == 12 + info_len + data_len {
            let spec_start = 12 + info_len;
            return Some(CastLayout {
                kind: kind as u32,
                info: 12..12 + info_len,
                spec: spec_start..spec_start + data_len,
                palette_at: 0x1a,
            });
        }
    }

    // Director 4: a narrow data length, a wide info length, and the kind
    // inside the block. Past the kind and its flags byte the bitmap header is
    // the same as Director 5's, except that it has no `ffff` field before the
    // palette, so the reference sits two bytes earlier.
    let (data_len, info_len) = (read(0, false)?, read(2, true)?);
    if cd.len() == 6 + data_len + info_len {
        return Some(CastLayout {
            kind: u32::from(*cd.get(6)?),
            info: 6 + data_len..6 + data_len + info_len,
            spec: 8..6 + data_len,
            palette_at: 0x18,
        });
    }
    None
}

impl Movie {
    pub fn open(path: impl AsRef<Path>) -> Result<Movie> {
        Movie::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Movie> {
        let endian = match data.get(..4) {
            Some(b"RIFX") => Endian::Big,
            Some(b"XFIR") => Endian::Little,
            _ => return Err(Error::NotDirector),
        };

        // The `imap` sits immediately after the 12-byte RIFX header and its only
        // field we care about is the absolute offset of the resource map.
        let mut r = Reader::at(&data, endian, 0x18);
        let mmap_offset = r.u32()? as usize;

        let mut r = Reader::at(&data, endian, mmap_offset);
        if r.fourcc()? != TAG_MMAP {
            return Err(Error::MissingChunk("mmap"));
        }
        let _chunk_size = r.u32()?;
        let _header_len = r.u16()?;
        let _entry_len = r.u16()?;
        let _capacity = r.u32()?;
        let used = r.u32()? as usize;

        // Entries begin 24 bytes past the chunk header regardless of the declared
        // header length; Director 5 always writes 24 here.
        let mut r = Reader::at(&data, endian, mmap_offset + 8 + 24);
        let mut resources = Vec::with_capacity(used);
        for _ in 0..used {
            let tag = r.fourcc()?;
            let size = r.u32()?;
            let offset = r.u32()?;
            let _flags = r.u16()?;
            let _unused = r.i16()?;
            let _link = r.u32()?;
            resources.push(Resource { tag, offset, size });
        }

        let mut movie = Movie {
            data,
            endian,
            resources,
            key: HashMap::new(),
            cast_slots: Vec::new(),
            members: Vec::new(),
            by_name: HashMap::new(),
            stage_width: 640,
            stage_height: 480,
        };

        movie.load_config()?;
        movie.load_key_table()?;
        movie.load_cast_table()?;
        Ok(movie)
    }

    pub fn endian(&self) -> Endian {
        self.endian
    }

    /// Payload of one `mmap` entry, with the 8-byte chunk header stripped.
    pub fn resource_data(&self, index: u32) -> Result<&[u8]> {
        let res = self
            .resources
            .get(index as usize)
            .ok_or(Error::BadCast(index))?;
        let start = res.offset as usize + 8;
        let end = start + res.size as usize;
        self.data.get(start..end).ok_or(Error::Truncated {
            need: end,
            have: self.data.len(),
        })
    }

    pub fn find(&self, tag: &[u8; 4]) -> Option<u32> {
        self.resources
            .iter()
            .position(|r| r.tag == tag)
            .map(|i| i as u32)
    }

    fn find_all(&self, tag: &[u8; 4]) -> Vec<u32> {
        self.resources
            .iter()
            .enumerate()
            .filter(|(_, r)| r.tag == tag)
            .map(|(i, _)| i as u32)
            .collect()
    }

    fn load_config(&mut self) -> Result<()> {
        let Some(idx) = self.find(TAG_VWCF) else {
            return Ok(());
        };
        let cfg = self.resource_data(idx)?;
        let mut r = Reader::at(cfg, self.endian, 4);
        let top = r.i16()?;
        let left = r.i16()?;
        let bottom = r.i16()?;
        let right = r.i16()?;
        self.stage_width = (right - left).max(0) as u16;
        self.stage_height = (bottom - top).max(0) as u16;
        Ok(())
    }

    /// `KEY*` is what ties a cast member to its pixel data: a `CASt` resource owns
    /// a `BITD` (or `snd `) child, and only this table records which one.
    fn load_key_table(&mut self) -> Result<()> {
        let Some(idx) = self.find(TAG_KEY) else {
            return Err(Error::MissingChunk("KEY*"));
        };
        let k = self.resource_data(idx)?;
        let mut r = Reader::new(k, self.endian);
        let header_len = r.u16()? as usize;
        let entry_len = r.u16()? as usize;
        let _capacity = r.u32()?;
        let used = r.u32()? as usize;

        let mut key: HashMap<(u32, FourCc), Vec<u32>> = HashMap::new();
        for i in 0..used {
            let mut e = Reader::at(k, self.endian, header_len + i * entry_len);
            let child = e.u32()?;
            let owner = e.u32()?;
            let tag = e.fourcc()?;
            key.entry((owner, tag)).or_default().push(child);
        }
        self.key = key;
        Ok(())
    }

    fn load_cast_table(&mut self) -> Result<()> {
        let Some(idx) = self.find(TAG_CAS) else {
            return Err(Error::MissingChunk("CAS*"));
        };
        let c = self.resource_data(idx)?;
        let mut r = Reader::new(c, self.endian);
        let count = c.len() / 4;
        let mut slots = Vec::with_capacity(count);
        for _ in 0..count {
            slots.push(r.u32()?);
        }
        self.cast_slots = slots;

        let mut members = Vec::with_capacity(count);
        let mut by_name = HashMap::new();
        for (i, &res) in self.cast_slots.iter().enumerate() {
            let number = i as u32 + 1;
            if res == 0 {
                members.push(CastMember::empty(number));
                continue;
            }
            match self.parse_cast_member(number, res) {
                Ok(m) => {
                    if let Some(n) = &m.name {
                        by_name.insert(n.to_ascii_uppercase(), number);
                    }
                    members.push(m);
                }
                // A malformed member should cost us that member, not the movie.
                Err(_) => members.push(CastMember::empty(number)),
            }
        }
        self.members = members;
        self.by_name = by_name;
        Ok(())
    }

    fn parse_cast_member(&self, number: u32, res: u32) -> Result<CastMember> {
        let cd = self.resource_data(res)?;

        // Two layouts, and the disc has one of each.
        //
        // Director 5 writes three `u32`s -- kind, info length, data length --
        // then the info block and then the type-specific block. Director 4
        // writes a `u16` data length and a `u32` info length, then the
        // type-specific block, then the info block; there is no kind field,
        // because the kind is the first byte of the type-specific block.
        //
        // The Macintosh release of Amber is a Director 4 movie and the PC
        // release is Director 5. Reading only the Director 5 shape turned
        // every cast member on the Macintosh disc into an unknown type -- its
        // first four bytes are the data length in the high half and the top of
        // the info length in the low half, which is why they all came out as
        // large round numbers like 1835008 rather than as anything plausible.
        //
        // They are told apart by which arithmetic accounts for the whole
        // record. Both are checked, neither is guessed, and across Roxy's
        // 2444 members exactly one of them fits.
        let Some(layout) = cast_layout(cd, self.endian) else {
            return Err(Error::Unsupported(
                "cast member matches neither the Director 4 nor the Director 5 layout".into(),
            ));
        };
        let CastLayout {
            kind,
            info,
            spec,
            palette_at,
        } = layout;
        let info = cd.get(info.clone()).unwrap_or(&[]);
        let spec = cd.get(spec.clone()).unwrap_or(&[]);
        let kind = CastKind::from(kind);

        let mut m = CastMember::empty(number);
        m.kind = kind;
        m.resource = res;
        m.name = self.cast_member_name(info);

        // The digital video block ends with a flags byte, of which bit 4 is
        // the loop. Established by comparing members across the whole disc:
        // the four values that occur are 0x22, 0x2a, 0x32 and 0x3a, and the
        // ones carrying 0x10 are exactly the films that should run for as long
        // as the room is on screen.
        if kind == CastKind::DigitalVideo {
            m.loops = spec.last().is_some_and(|f| f & 0x10 != 0);
        }
        if kind == CastKind::Bitmap && spec.len() >= 10 {
            let mut s = Reader::new(spec, self.endian);
            // Top bit is a flag, not stride.
            m.pitch = s.u16()? & 0x7fff;
            let top = s.i16()?;
            let left = s.i16()?;
            let bottom = s.i16()?;
            let right = s.i16()?;
            m.width = (right - left).max(0) as u16;
            m.height = (bottom - top).max(0) as u16;
            m.origin_x = left;
            m.origin_y = top;
            if spec.len() >= 0x16 {
                let mut s = Reader::at(spec, self.endian, 0x12);
                m.reg_y = s.i16()?;
                m.reg_x = s.i16()?;
            }
            // Layout past the registration point: a flags byte, then the
            // depth, then a reserved word, then the palette. The palette is a
            // cast member number, not an index into the movie's `CLUT` list;
            // a value of zero or less means the built-in system palette.
            if spec.len() >= 0x18 {
                m.bit_depth = spec[0x17];
            }
            if spec.len() >= palette_at + 2 {
                let mut s = Reader::at(spec, self.endian, palette_at);
                m.palette_ref = s.i16().unwrap_or(0);
            }
            // Director omits the depth byte for 1-bit members; infer from stride.
            if m.bit_depth == 0 && m.width > 0 {
                m.bit_depth = if m.pitch >= m.width { 8 } else { 1 };
            }
        }
        Ok(m)
    }

    /// Pulls the member's name out of its info block.
    ///
    /// The info block opens with its own header length, and the offset table
    /// begins exactly there: a `u16` field count, then `count + 1` `u32`
    /// offsets delimiting the fields, then the field bytes. Field 1 is the
    /// member's name, stored as a Pascal string; field 0 is a script that is
    /// usually empty.
    fn cast_member_name(&self, info: &[u8]) -> Option<String> {
        let mut r = Reader::new(info, self.endian);
        let header_len = r.u32().ok()? as usize;

        let mut t = Reader::at(info, self.endian, header_len);
        let count = t.u16().ok()? as usize;
        // The name lives in field 1, so the table must delimit at least that.
        if count < 2 {
            return None;
        }
        let mut offsets = Vec::with_capacity(count + 1);
        for _ in 0..=count {
            offsets.push(t.u32().ok()? as usize);
        }

        let data_base = header_len + 2 + (count + 1) * 4;
        let field = info.get(data_base + offsets[1]..data_base + offsets[2])?;
        let mut f = Reader::new(field, self.endian);
        let name = f.pstring().ok()?;
        (!name.is_empty()).then_some(name)
    }

    pub fn members(&self) -> &[CastMember] {
        &self.members
    }

    pub fn member(&self, cast_number: u32) -> Option<&CastMember> {
        let m = self.members.get(cast_number.checked_sub(1)? as usize)?;
        (m.resource != 0).then_some(m)
    }

    /// Scripts address some members by name (`#castName: "O_ENTRY2"`).
    pub fn member_by_name(&self, name: &str) -> Option<&CastMember> {
        let n = self.by_name.get(&name.to_ascii_uppercase())?;
        self.member(*n)
    }

    pub(crate) fn child(&self, owner: u32, tag: &[u8; 4]) -> Option<u32> {
        self.key.get(&(owner, FourCc::new(tag)))?.first().copied()
    }

    /// Decodes a bitmap cast member into 8-bit indexed pixels.
    pub fn bitmap(&self, cast_number: u32) -> Result<Bitmap> {
        let m = self.member(cast_number).ok_or(Error::BadCast(cast_number))?;
        if m.kind != CastKind::Bitmap {
            return Err(Error::Unsupported(format!(
                "cast {cast_number} is {:?}, not a bitmap",
                m.kind
            )));
        }
        let child = self
            .child(m.resource, TAG_BITD)
            .ok_or(Error::MissingChunk("BITD"))?;
        let raw = self.resource_data(child)?;
        bitmap::decode(m, raw)
    }

    /// The QuickTime data a digital video cast member carries inside the
    /// movie, when it has any.
    ///
    /// Most of the game's films are files on the disc and a cast member only
    /// names one. Some are `MooV` chunks owned by the member itself, and those
    /// have no file at all -- which is why five referenced movies looked
    /// missing, including the one Margaret's chapter opens on.
    pub fn embedded_movie(&self, cast_number: u32) -> Option<&[u8]> {
        let m = self.member(cast_number)?;
        let child = self.child(m.resource, b"MooV")?;
        self.resource_data(child).ok()
    }

    /// The type-specific block of a cast member, for inspection.
    pub fn cast_spec(&self, cast_number: u32) -> Option<&[u8]> {
        let m = self.member(cast_number)?;
        let cd = self.resource_data(m.resource).ok()?;
        let mut r = Reader::new(cd, self.endian);
        let _kind = r.u32().ok()?;
        let info_len = r.u32().ok()? as usize;
        let data_len = r.u32().ok()? as usize;
        cd.get(12 + info_len..12 + info_len + data_len)
    }

    /// The raw bytes of a sound cast member's `snd ` chunk, for inspection.
    pub fn sound_raw(&self, cast_number: u32) -> Option<&[u8]> {
        let m = self.member(cast_number)?;
        let child = self.child(m.resource, TAG_SND)?;
        self.resource_data(child).ok()
    }

    pub fn sound(&self, cast_number: u32) -> Result<Sound> {
        let m = self.member(cast_number).ok_or(Error::BadCast(cast_number))?;
        let child = self
            .child(m.resource, TAG_SND)
            .ok_or(Error::MissingChunk("snd "))?;
        sound::decode(self.resource_data(child)?, self.endian)
    }

    /// The palette a bitmap cast member asks for, resolved through the cast.
    ///
    /// The reference is a cast member number pointing at a palette member,
    /// whose `CLUT` child holds the colours. Values of zero or less name a
    /// built-in palette, which this port approximates with the movie's first
    /// `CLUT` because every room here ships a custom one.
    pub fn palette_for_cast(&self, palette_ref: i16) -> Option<Palette> {
        if palette_ref > 0 {
            let member = self.member(palette_ref as u32)?;
            let child = self.child(member.resource, TAG_CLUT)?;
            return Some(Palette::decode(self.resource_data(child).ok()?, self.endian));
        }
        None
    }

    /// Every Lingo handler the movie defines, by name.
    ///
    /// Needed to tell a handler that exists from one that is merely implied.
    /// `setState` dispatches to `set<Flag>` for any flag whose value list has
    /// a single entry, but plenty of those flags have no such handler and take
    /// the write directly -- 29 of the 50 in this game. Reporting all 50 as
    /// missing work would be a number that cries wolf, which is no more useful
    /// than one that stays silent.
    pub fn handler_names(&self) -> Vec<String> {
        // The name table: a header offset and a count, then Pascal strings.
        // A movie can carry several and the real one is the largest.
        let names: Vec<String> = self
            .find_all(TAG_LNAM)
            .into_iter()
            .filter_map(|i| self.resource_data(i).ok())
            .max_by_key(|d| d.len())
            .map(|d| {
                let mut r = Reader::at(d, self.endian, 16);
                let (start, count) = (
                    r.u16().unwrap_or(0) as usize,
                    r.u16().unwrap_or(0) as usize,
                );
                let mut out = Vec::with_capacity(count);
                let mut p = start;
                for _ in 0..count {
                    let Some(&len) = d.get(p) else { break };
                    let end = p + 1 + len as usize;
                    let Some(bytes) = d.get(p + 1..end) else { break };
                    out.push(String::from_utf8_lossy(bytes).into_owned());
                    p = end;
                }
                out
            })
            .unwrap_or_default();

        let mut found = Vec::new();
        for i in self.find_all(TAG_LSCR) {
            let Ok(d) = self.resource_data(i) else { continue };
            if d.len() < 0x5c {
                continue;
            }
            let mut r = Reader::at(d, self.endian, 0x48);
            let count = r.u16().unwrap_or(0) as usize;
            let table = r.u32().unwrap_or(0) as usize;
            // Each entry is 42 bytes and opens with the handler's name index.
            for k in 0..count {
                let at = table + k * 42;
                if at + 42 > d.len() {
                    break;
                }
                let id = Reader::at(d, self.endian, at).u16().unwrap_or(0) as usize;
                if let Some(name) = names.get(id) {
                    found.push(name.clone());
                }
            }
        }
        found.sort();
        found.dedup();
        found
    }

    /// The movie's palettes, in `mmap` order. Bitmaps select one by `palette_ref`;
    /// index 0 is the sensible default when a member asks for the system palette.
    pub fn palettes(&self) -> Vec<Palette> {
        self.find_all(TAG_CLUT)
            .into_iter()
            .filter_map(|i| self.resource_data(i).ok())
            .map(|d| Palette::decode(d, self.endian))
            .collect()
    }

    pub fn palette_count(&self) -> usize {
        self.find_all(TAG_CLUT).len()
    }
}

const TAG_STXT: &[u8; 4] = b"STXT";

impl Movie {
    /// Every `STXT` chunk's text, in `mmap` order.
    ///
    /// Director stores styled text as a header giving the offset and length of
    /// the plain characters, followed by a run-length style table this ignores.
    /// Amber uses these chunks for more than prose: the room-name table and the
    /// state schema are both plain text sitting in an `STXT`.
    pub fn texts(&self) -> Vec<String> {
        self.find_all(TAG_STXT)
            .into_iter()
            .filter_map(|i| self.resource_data(i).ok())
            .filter_map(|d| {
                let mut r = Reader::new(d, self.endian);
                let offset = r.u32().ok()? as usize;
                let length = r.u32().ok()? as usize;
                let _style_length = r.u32().ok()?;
                let text = d.get(offset..offset.checked_add(length)?)?;
                // Mac Roman, and the parts we care about are ASCII structure.
                Some(text.iter().map(|&c| c as char).collect())
            })
            .collect()
    }
}

impl Movie {
    /// The text of a text cast member, via its `STXT` child.
    ///
    /// Amber stores some rooms this way rather than in an external `.DAT`: a
    /// text member named `Cons_CenterN.DATA` holds that room's property list,
    /// with the member's own name serving as the room name.
    pub fn text(&self, cast_number: u32) -> Option<String> {
        let m = self.member(cast_number)?;
        let child = self.child(m.resource, TAG_STXT)?;
        let d = self.resource_data(child).ok()?;
        let mut r = Reader::new(d, self.endian);
        let offset = r.u32().ok()? as usize;
        let length = r.u32().ok()? as usize;
        let _style_length = r.u32().ok()?;
        let text = d.get(offset..offset.checked_add(length)?)?;
        Some(text.iter().map(|&c| c as char).collect())
    }

    /// Cast members whose name ends in `suffix`, as `(cast number, name)`.
    pub fn members_named_with(&self, suffix: &str) -> Vec<(u32, String)> {
        self.members
            .iter()
            .filter(|m| m.resource != 0)
            .filter_map(|m| {
                let name = m.name.as_ref()?;
                name.to_ascii_uppercase()
                    .ends_with(&suffix.to_ascii_uppercase())
                    .then(|| (m.number, name.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod cast_layout_tests {
    use super::*;

    /// The two records are the same cast member -- Roxy's `O_ENTRY2`, 600 by
    /// 300 -- as each release actually stores it, trimmed to the header and
    /// the type-specific block. They are what taught me the difference.
    #[test]
    fn the_two_directors_are_told_apart_by_their_arithmetic() {
        // Director 5: kind, info length, data length, then info, then spec.
        let mut d5 = Vec::new();
        d5.extend_from_slice(&1u32.to_be_bytes()); // Bitmap
        d5.extend_from_slice(&4u32.to_be_bytes()); // info length
        d5.extend_from_slice(&28u32.to_be_bytes()); // data length
        d5.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // the info block
        d5.extend_from_slice(&[
            0x82, 0x58, 0x00, 0x00, 0x00, 0x00, 0x01, 0x2c, 0x02, 0x58, 0x43, 0x0c, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x96, 0x01, 0x2c, 0x00, 0x08, 0xff, 0xff, 0x03, 0x51,
        ]);

        let l = cast_layout(&d5, Endian::Big).expect("Director 5 record");
        assert_eq!(l.kind, 1);
        assert_eq!(&d5[l.info.clone()], &[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(d5[l.spec.clone()][0..2], [0x82, 0x58]); // the row stride
        assert_eq!(l.palette_at, 0x1a);
        assert_eq!(
            i16::from_be_bytes([d5[l.spec.start + 0x1a], d5[l.spec.start + 0x1b]]),
            849
        );

        // Director 4: data length, info length, then the spec -- opening with
        // the kind and a flags byte -- then the info.
        let mut d4 = Vec::new();
        d4.extend_from_slice(&28u16.to_be_bytes()); // data length
        d4.extend_from_slice(&4u32.to_be_bytes()); // info length
        d4.extend_from_slice(&[0x01, 0x00]); // kind, flags
        d4.extend_from_slice(&[
            0x82, 0x58, 0x00, 0x00, 0x00, 0x00, 0x01, 0x2c, 0x02, 0x58, 0xff, 0xf4, 0xff, 0xf4,
            0x01, 0x38, 0x02, 0x64, 0x00, 0x96, 0x01, 0x2c, 0x00, 0x08, 0x03, 0x42,
        ]);
        d4.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // the info block, last

        let l = cast_layout(&d4, Endian::Big).expect("Director 4 record");
        assert_eq!(l.kind, 1);
        assert_eq!(&d4[l.info.clone()], &[0xde, 0xad, 0xbe, 0xef]);
        // The kind and flags are not part of the header the bitmap reader sees.
        assert_eq!(d4[l.spec.clone()][0..2], [0x82, 0x58]);
        assert_eq!(l.palette_at, 0x18);
        assert_eq!(
            i16::from_be_bytes([d4[l.spec.start + 0x18], d4[l.spec.start + 0x19]]),
            834
        );
    }

    #[test]
    fn a_record_that_is_neither_is_refused_rather_than_guessed() {
        // Lengths that account for nothing: the whole point is that a wrong
        // reading used to sail through and produce an unknown cast type.
        let junk = vec![0xff; 40];
        assert!(cast_layout(&junk, Endian::Big).is_none());
    }
}

