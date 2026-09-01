# Amber: Journeys Beyond — reimplementation

A Rust reimplementation of the engine behind *Amber: Journeys Beyond*
(Hue Forest Entertainment, 1996), so the game runs natively on current
systems instead of through Macromedia Director 5 on Windows 95.

This repository contains **no game content**. It reads the files from an
original disc, which you supply yourself.

## What the original is

The disc is a hybrid Mac/PC CD built with Toast, volume `AMBER_JB`. The
game is a Director 5 projector (`AMBER_F/AMBER_JB.EXE`) driving four
chapter movies, one per haunting, plus a hub:

| Path | Role |
| --- | --- |
| `AMBERHUB.DXR` | Hub and menu movie |
| `ROXY/ROXY.DXR` | Chapter movie, 129 MB |
| `MARGARET/MARGARET.DXR` | Chapter movie, 32 MB |
| `EDWIN/EDWIN.DXR`, `BRICE/BRICE.DXR` | Chapter movies |
| `*/MOVIES*/*.MOV` | QuickTime video, Cinepak + IMA4 ADPCM |
| `CD_DATA/`, `AMBER_F/GAMEDATA/` | AIFF and WAV audio |
| `*/[NAME]_[n].DAT` | **Room definitions, as Lingo source text** |

The projector needs six Director Xtras (`FILEIO.X32`, `SCRNUTIL.X32`,
`MOVUTILS.DLL`, `MEMORY.DLL`, `MISC_X.DLL`, `LABELDRV.DLL`). None of them
are reimplemented here; the behaviour they provided is folded into the
engine directly.

## The finding that makes this tractable

Amber does not keep its world in Director's score or in compiled Lingo.
Every one of its **1,320 rooms is a Lingo property list stored as plain
text** in the `.DAT` files, listing the room's art, its ambient mix, and
its clickable regions with their guards and actions:

```text
[#preLoad: [1591, 236], #onStage: [[#castName: "O_ENTRY2", #castNum: 1590,
 #channel: 1, #showIF: [#equals: [#always, 1]], #coords: point(320, 210),
 #ink: 0]], #Hotspots: [[#forward, rect(46, 64, 347, 356),
 ["goTo( #OfficeEwall, #forward )"], [#equals: [#always, 1]]]],
 #storageCast: [145, 1, 1089]]
```

So the navigation graph, the puzzle gating and the interaction model can
be read directly, and a full Director bytecode VM is not needed to play
the game. The action vocabulary is closed and small: twelve hotspot
verbs, seven condition operators, and about forty engine calls.

## Format notes

Things that cost time to work out, recorded so they need not be
rediscovered:

- **Endianness is per file.** `RIFX` means big-endian (Mac-authored),
  `XFIR` little-endian. This hybrid disc ships both, sometimes within one
  chapter, and the byte order applies to four-character tags too — they
  are stored reversed in little-endian movies.
- **`.DAT` records are separated by a single `0xBC` byte**, not a newline
  or a NUL. The whole file is one physical line to ordinary text tools,
  which is why a naive line-based reader finds exactly one room per file.
  Each file opens with a `* 10/4/96,4:22 PM *` banner padded with spaces.
- **Every room is stored twice.** Once as a text cast member inside the
  chapter movie, named `<RoomName>.DATA`, and again in the external `.DAT`
  files the projector streams at run time. Only the embedded copy carries
  the room's name; only the external copy carries its address. Both are
  read and then deduplicated.
- **Rooms are addressed by a `#storageCast` triple**, `[cast number, start,
  end]`. Element 0 is the cast member holding the room text and is the only
  part that is stable: the other two are byte offsets into the chapter's
  concatenated room text, so they differ between the two copies of a room.
  Join on element 0 alone.
- **The room-name table is an `STXT` chunk**, a property list grouped by
  area, mapping each name to that same triple. Without it, `goTo( #bedrm_A1
  )` cannot be resolved to anything, because the `.DAT` records are
  anonymous and positional.
- **A cast member's name is field 1 of its info block**, not field 0, and
  the field offset table begins at the offset the block's own header length
  gives. Field 0 is a script and is usually empty.
- **The state schema is also an `STXT` chunk**: every flag with its initial
  value first and its legal values after, e.g. `#knittingNeedle :
  [#atRest, #floating, #dumbWaiter, #usedUp]`. This is effectively the save
  format, declared in the data.
- **`CLUT` palettes are in palette order**, with 16-bit components of which
  only the high byte matters here. I initially had these reversed and the
  error survived a pixel-exact cross-check, because both my implementations
  shared the assumption. Wrong colour with correct geometry means the
  palette, not the decoder.
- **A bitmap's palette is a cast member number**, not an index into the
  movie's `CLUT` list, and it sits at offset `0x1a` of the bitmap's
  type-specific block. The byte before the reserved word is the bit depth,
  at `0x17`; `0x16` is flags. Resolve the palette through the cast and
  `KEY*`, the same three hops as the pixel data.
- **Room names are not unique across chapters.** All four declare a
  `DEFAULT_LOCATION`, and it is an empty placeholder with no art. A global
  name index silently resolves to whichever chapter loaded first.
- **The game opens on a movie.** Roxy's declared start, `Gbhs_playIntro`,
  places a single element on the `#video` channel and nothing on the sprite
  channels, so a correct engine with no video support renders black.
- **`BITD` uses a PackBits variant**: a control byte below `0x80`
  introduces `n + 1` literal bytes, one at or above `0x80` repeats the
  next byte `0x101 - n` times. Small images are sometimes stored flat,
  detectable by the payload already being exactly `stride * height`.
- **Rows are padded.** `pitch` from the cast member can exceed the width,
  and its top bit is a flag rather than part of the value.
- **Reaching pixel data takes three hops.** `#castNum` indexes `CAS*`,
  which yields a `CASt` resource, whose `BITD` child is named only by the
  `KEY*` table. There is no direct link from a cast member to its bitmap.
- **`rect()` in Lingo is left, top, right, bottom** — not the top, left,
  bottom, right order Director uses inside binary chunks.
- **Hotspots overlap by design.** A room-sized `#browse` region sits under
  everything, so hit testing has to resolve by verb specificity and then
  by smallest area, or every click lands on the background.
- **`pushVideo` usually takes no argument.** The room nominates its movie
  through the sprite it places on the `#video` channel.
- **Sounds are named by symbol, never by file.** The table resolving them
  is `#soundBank` inside `foreground.DATA`, a text cast member holding
  chapter configuration; `#soundVolTweaks` beside it gives per-sound gain.
  Do not find these tables by shape: every room record mentions `houseHum`
  in its ambient mix, so a shape match collects rooms instead.
- **`gCPU` is an authoring-time platform switch** left in the shipped
  data. The `#PC` branches are the more complete ones.
- **Lingo property lists are association lists.** The same key may appear
  more than once and the game depends on it: a compound guard is written
  `[#and: [#equals: [a, b], #equals: [c, d]]]`, two entries under one key.
  Storing them in a map drops half of every compound condition, and reading
  the operand as a linear list finds nothing and yields an empty `and`, which
  is vacuously true. Either mistake unlocks every locked thing in the game.
- **Property keys are not always symbols.** A movie's event track keys cues
  by frame number: `[165: 90, 167: ["assertSound #aCleverCar"]]`. Rejecting
  integer keys fails the whole enclosing list, which in one chapter was its
  entire sound bank.
- **`TRUE` and `FALSE` are the integers the guards compare against.** Parsed
  as bare words they become symbols matching neither `= 1` nor `= 0`, and
  anything set that way becomes permanently unreachable. They appear 227
  times.
- **A registration point is in the member's rectangle space**, not the
  image's. A member whose rectangle has a non-zero origin carries that origin
  in its registration point too, and 520 of the game's 3,208 bitmaps do.
- **Overlapping hotspots resolve by order, not by size.** Director takes the
  first match, and that order is where the authors expressed precedence. The
  porch offers two forward exits whose guards can both hold, and the one into
  the darkened house is both first and larger.
- **A compiled sprite write is** `push channel; push value; push property;
  0x5d 6`, with `0x5c 6` reading. Property 4 is the cast member and 33 the
  location. `puppetSprite` claims the channel first.
- **An extended `snd ` header's samples begin at offset 64**, not where a
  field-by-field walk lands. Twelve bytes early reads header as audio, and
  since unsigned eight-bit silence is 0x80, twelve zero bytes are twelve
  samples at full-scale negative: a click at the head of every sound.
- **Sound banks are per chapter and their filename extensions are not
  reliable.** Five cues are listed as `.wav` where the disc holds `.AIF`. The
  stem identifies the sound.
- **A `playerHas<Item>` flag is written when the item is taken**, not derived
  when read; the schema seeds all eight to zero. Rooms hide a taken object by
  drawing an "object gone" plate gated on that flag.

## Layout

```
crates/director  Director 5 movie reader: RIFX container, mmap resource
                 table, KEY*/CAS* indices, BITD bitmaps, CLUT palettes,
                 Mac `snd ` resources
crates/lingo     Parser for Lingo literals and for the .DAT room files
crates/qt        QuickTime demuxer, Cinepak and IMA ADPCM decoders,
                 both verified sample- and pixel-exact against ffmpeg
crates/amber     The engine:
                   world, locations, schema   rooms and their state
                   script, natives/           the action interpreter and the
                                              handlers ported per chapter
                   game, render, cursor       stage, window and input
                   audio, sound               mixing and the sound banks
                   media, player              movie lookup and playback
                   inventory, presentation    the bar and the named casts
                   walk                       the terminal walkthrough
```

## Use

Point the tools at a directory holding the disc's contents:

```sh
cargo build --release
./target/release/amber play   <game-dir> [room]   open the game window
./target/release/amber walk   <game-dir> [steps]  walk it in the terminal
./target/release/amber info   <game-dir>          summarise the data
./target/release/amber rooms  <game-dir> [domain] list rooms and exits
./target/release/amber room   <game-dir> ROXY 78  dump one room
./target/release/amber shot   <game-dir> <room> out.png
./target/release/amber sfx    <game-dir> [name]   decode sounds
./target/release/amber cast   <game-dir> ROXY/ROXY.DXR
./target/release/amber export <game-dir> ROXY/ROXY.DXR 566 out.png
./target/release/amber verify <game-dir>          parse everything
```

`play` opens the game window. `shot` renders one room headlessly, which is
how the compositor is checked without a display.

`verify` is the regression harness: it parses every room and every action
script and reports anything the interpreter does not understand, along with
the handlers still unimplemented.

`walk` is how a bug report gets reproduced. It prints the exits live under the
current state, and takes `blocked` to list the hotspots whose guards are
failing and why, `click x y` to run the same hit test the window uses, and
`give`/`use` to put an item in hand. A route can be passed as arguments, so a
report becomes a one-liner.

## Tracing

The engine logs nothing unless asked. `AMBER_TRACE` selects topics by name,
comma separated, or `all`:

```
AMBER_TRACE=room,script amber walk extract Gaz_lockCU "click 293 248"
AMBER_TRACE=all AMBER_TRACE_FILE=/tmp/run.log amber play extract
```

| topic | what it records |
|--------|--------------------------------------------------|
| room | room changes, with the area and hotspot count |
| script | handler dispatch, and handlers still unported |
| state | flag writes: `set`, `add`, `trim` |
| sprite | sprites that were asked to draw and could not |
| audio | loops started and stopped, and the mix per room |
| video | movies opened, and movies with no file |

Records carry the frame and the room, because "what was on screen when this
happened" is the first question every time. A leading `~` marks a speculative
run: the walkthrough lists a room's exits by running each hotspot against a
copy of the state, and `verify` sweeps the whole game the same way. Those calls
handlers and write flags exactly as a real click does, on a copy that is thrown
away.

Naming a topic that does not exist reports the ones that do rather than
silently tracing nothing.

## State

The game is playable: you can walk the house, open doors, pick things up and
use them, and the ghosts telephone. Most of the puzzle machinery is not
implemented yet.

Working:

- Director 5 container parsing, both byte orders
- Cast, palette and bitmap decoding
- QuickTime with Cinepak and IMA ADPCM, both verified against ffmpeg: a full
  frame matching to zero mean error, and 4,396,096 of 4,396,096 audio samples
  bit-exact
- All 1,325 rooms load and every one resolves to a name
- Navigation: 3,697 exits, of which 5 do not resolve
- Rendering: rooms composite to a 640x480 stage, with script-controlled
  sprite channels layered over them
- Audio: room ambience mixed per room, sound effects, voice cues, movie
  soundtracks, and the radio and clock programmes sequenced. Every one of the
  104 sound symbols the scripts fire resolves
- Video: room movies play, and space skips one
- Inventory: the bar, picking things up and using them on the scene
- Hotspot guards, including the compound conditions, so locked things stay
  locked
- 21 set-piece handlers ported from the compiled Lingo, including the ghost
  telephone, Chippy, the office laptop and the ice white-out

Not yet done:

- 45 set-piece handlers at 143 call sites, mostly the puzzle machinery: the
  combination locks, the radio dial, the weather vane, the whirligig, the
  telegram. The interpreter records each as `Effect::Native` so the
  surrounding timeline stays intact and the count stays honest
- Save and load, though the state schema in the data is effectively the save
  format already
- The movie event track, which keys cues to frames. Its structure is read but
  its bare integers are ambiguous, so it is left rather than guessed
- Cursor art. The pointers are drawn shapes standing in for the game's own
  1-bit cursors, which need `castCursor` decoded first
- The transitions `setTransition` selects

## Legal

Reverse engineering for interoperability. No original code or content is
reproduced or redistributed here; you need your own copy of the disc.
