# Amber: Journeys Beyond — reimplementation

A Rust reimplementation of the engine behind *Amber: Journeys Beyond*
(Hue Forest Entertainment, 1996), so the game runs natively on current
systems instead of through Macromedia Director 5 on Windows 95.

This repository contains **no game content**. It reads the files from an
original disc, which you supply yourself.

## What the original is

The game shipped as two separate releases, and neither disc is complete
on its own.

The **PC release** is one CD: pure ISO9660, volume `AMBER_JB`, 676 files,
no Apple partition map or HFS volume anywhere. The **Macintosh release**
is two CDs, `AMBER_A` and `AMBER_B`, as bare HFS volumes with no ISO9660
track. They are not the same disc pressed two ways.

The differences that matter:

| | PC | Macintosh |
| --- | --- | --- |
| Discs | 1 | 2 |
| Movies on disc | 278 | 283 |
| Referenced movies that resolve | 191 of 196 | 195 of 196 |
| Sound files loose on disc | 325 | 183 |

The PC release references five films it does not ship: `40sINTRO.mov`
(Margaret's opening), `MEewall.mov`, and the three scan-unit films
`ST-CPU-LED`, `UH-BATHKNOBSCAN-ON1` and `UH-MARGKNOBSCAN-ON1`. Its cast
entries still point at `C:\AMBER building\AJBDISC1\...`, so it was
assembled from the two-disc layout and those films were dropped to fit
one CD. The Macintosh release has all five and is missing only
`tuner_bg.mov`, which only the PC build uses. Its extra sounds live
inside its installer rather than loose on the disc, but every sound the
game actually references resolves on both.

**This port favours the Macintosh release**, and reads the PC disc too.
Where a platform test appears in the Lingo -- `if gCPU = #Mac` -- the
Macintosh arm is taken; the movies are `RIFX`, which is the Macintosh
byte order, and on that build the films carry their own audio where the
PC build plays it separately.

The game itself is a Director 5 projector (`AMBER_F/AMBER_JB.EXE` on the
PC) driving four chapter movies, one per haunting, plus a hub:

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

Point the tools at a directory holding a disc's contents. Because
neither release is complete, `AMBER_FALLBACK` names further directories
to fill the gaps -- a `:`-separated list, searched in order after the
directory given on the command line:

```sh
# the Macintosh release, with the PC disc filling what it lacks
AMBER_FALLBACK=extract ./target/release/amber play mac_game
```

Both indexes are first-match-wins, so the directory on the command line
always beats a fallback. Nothing is copied and no tree is modified,
which keeps each release honest about what it actually shipped.

The Macintosh discs are StuffIt-wrapped HFS images; `tools/hfs.py`
reads them:

```sh
unar amber_aimage.sit && unar amber_bimage.sit
python3 tools/hfs.py "AMBER_A*image" list
python3 tools/hfs.py "AMBER_B*image" extract mac_game
```

Seven of Roxy's endgame films are 468-byte stubs on disc B and real on
disc A, so when merging the two take the larger copy.



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

## Hearing a room without hearing it

`mix` runs the audio path against a mixer with no output and prints what it is
holding, so a room that sounds wrong can be examined rather than described:

```
amber mix extract bedrm_boxes 440 190
```

It reports the ambient bed with the gain each source actually gets, then what
each click adds. The gains are the room's `#earShot` level times the game's own
`soundVolTweaks` trim, which is why a clock that reads 19% in the room plays at
3%.

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

## Recording a session

`play` can write everything it is told to do as a walkthrough script, so a
route can be handed over and replayed exactly:

```
AMBER_RECORD=/tmp/run.walk amber play extract
amber walk extract --replay /tmp/run.walk
AMBER_TRACE=all amber walk extract --replay /tmp/run.walk
```

The file is plain text -- a room name to start, then `click x y`, `inv x y`
and `skip` -- so trimming the tail is how a route is cut down to the shortest
one that still fails. Blank lines and `#` comments are ignored, and the
recorder writes the current room as a comment before each click.

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
- Every handler the room scripts name is ported -- no unported verbs and no
  unported setters: the ghost telephone,
  Chippy, the office laptop, the ice white-out, the music boxes, the boat and
  the car
- Transitions: `goTo`'s second argument is the transition for that move, so a
  turn is a chunky quarter-second wipe and a step forward is a dissolve
- The game's own cursors, read from the cast rather than drawn
- The ghosts telephone once the Amber headgear is on, working through their
  recordings on a rota

Not yet done:

- `usePeekUnit`, the PeeK unit's own interface: a modal screen with its own
  event loop, opened from the inventory bar rather than from a room script, so
  no tally counts it. The hint book tells a new player to click it in their
  first minute
- The rest of `idle`: the menu bar, `cursorDance`, and the `ripple` that runs
  after five seconds of no input
- Save and load, though the state schema in the data is effectively the save
  format already
- The movie event track, which keys cues to frames. Its structure is read but
  its bare integers are ambiguous, so it is left rather than guessed
- A cursor's hot spot, which is not read yet, so the pointers centre instead

## Licence

MIT. See [LICENSE](LICENSE).

That covers this engine -- the Rust crates, the Python tools, and the notes.
It does not and cannot cover *Amber: Journeys Beyond* itself.

## Legal

Reverse engineering for interoperability. No original code or content is
reproduced or redistributed here; you need your own copy of the disc.

Everything in this repository was written by reading a disc that was bought,
and none of it contains any of the game's own code, art, sound or film. The
game remains the property of its authors. If you want to run this, find a
copy.

## The log

[WORKLOG.md](WORKLOG.md) is an append-only record of the whole port, written
as it happened. It is long, and it is where the reasoning lives -- including
the mistakes, which are the useful part: a decoder that read a greyscale film
as colour for three rounds, a coverage test that could not fail, an effect
emitted a hundred and four times and acted on none, and a handler ported,
tested, and never once reached.
