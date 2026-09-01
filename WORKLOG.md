# Work log

Reverse engineering *Amber: Journeys Beyond* (Hue Forest, 1996) into a
native Rust engine. Led by helba; log kept by me (Claude) as I
go, so the retro at the end is written from a record rather than memory.

Entries are append-only. Each notes what I was trying to do, what I found,
and what it cost.

---

## 1. Identify the target

Disc is a hybrid Mac/PC Toast build, volume `AMBER_JB`. Game is a
Director 5 projector driving four chapter movies plus a hub. Media is
QuickTime (Cinepak, 320x240) and AIFF/WAV.

## 2. The finding that set the whole approach

The `.DAT` files sitting next to each chapter turned out to be **plain
text Lingo property lists** — one per room, carrying art, ambient mix,
hotspot rectangles, guards and actions. That meant the game's logic could
be read directly and a full Director bytecode VM was not on the critical
path. This is the single decision point the rest of the project hangs off.

Scoped the vocabulary before committing: 12 hotspot verbs, 7 condition
operators, ~40 engine calls. Closed and small, so a targeted interpreter
would do.

## 3. Validating the asset pipeline

Built the container reader against a Python reference implementation, then
diffed them. Decoded a frame both ways: 307,200 of 307,200 pixels
identical. Cross-checked room counts against an independent scan — 5,957
hotspots and 2,616 stage elements matched exactly.

Cost: writing the throwaway Python first felt redundant at the time. It
paid for itself immediately — every subsequent format guess had an oracle
to check against.

## 4. Bugs found by disagreeing with my own numbers

- **44 rooms instead of 1,320.** Records are separated by byte `0xBC`, not
  a newline or NUL. A line-based reader sees one room per file. Caught
  because the count disagreed with a `grep -c` I had run earlier.
- **Zero named cast members.** The name is field 1 of the info block, not
  field 0, and the offset table starts at the block's own declared header
  length. Caught by probing rather than by reasoning — I had guessed the
  layout twice and been wrong twice, so I hexdumped it.
- **Rooms doubled to 2,645.** Every room is stored twice, once embedded in
  the movie and once external. Deduping on the full `#storageCast` triple
  only caught 44; the offsets differ between copies. Only element 0, the
  cast number, is stable. Joining on that alone fixed it.

## 5. Navigation

`goTo( #bedrm_A1 )` names a destination but the `.DAT` records are
anonymous and positional. Found the name table in an `STXT` chunk. Result:
1,325 rooms, all named, 3,697 exits with 5 unresolved.

## 6. Renderer

640x480 stage, sprites composited by channel. All 1,478 initially visible
sprites decode with no failures, whole-game check in 0.7s.

## 7. Black screen

Reported by helba. Three separate causes stacked:

1. Starting room resolved to BRICE's `DEFAULT_LOCATION` because my name
   index was global and all four chapters declare that name. First writer
   won.
2. `DEFAULT_LOCATION` is an empty placeholder with no art at all, so it
   would have been blank regardless.
3. The real opening room, `Gbhs_playIntro`, has exactly one element: a
   movie on the `#video` channel. Video is still a stub, so black is the
   literally correct output.

Fixes: domain-scoped name resolution, start location read from the
chapter's declared schema, and an explicit message plus skip-ahead rather
than a silent blank window.

Lesson worth keeping: I reported "playable" after checking that rooms
render. I had never run the actual start path. The verification I did was
real but it was not the verification that mattered.

## 8. Palette selection

Rooms render with correct geometry but wrong colour. Root cause: I read
bit depth at offset `0x16` and the palette at `0x17`; the real layout is
flags at `0x16`, depth at `0x17`, palette `i16` at `0x18`. So palettes
were being chosen by a garbage index.

Fixing that offset improved the colour but did not correct it. The real
error was one layer down: **I had been reversing every palette.** Director
stores `CLUT` entries in palette order; I had entry 0 mapping to colour
index 255.

The uncomfortable part is how long it survived. I had verified the bitmap
decoder against an independent implementation and got 307,200 of 307,200
pixels identical, and reported that as strong evidence. It was not. Both
implementations were mine and both reversed the palette, so the agreement
only proved they were consistent, not correct. The test frame was a
near-greyscale menu screen, which looks plausible either way round — the
one image in the game least able to reveal the bug.

What actually caught it was helba looking at the window and saying it was
black. Chasing that led to a room with real colour in it, and a wrong
palette is obvious the moment the picture has a green lawn in it.

Corrected in three places: the code, the README (which listed the
reversal as a *finding*, so it was actively teaching the wrong thing), and
the claim about what the cross-check proved.

## 9. State schema as the source of truth

The chapters declare their own initial state in an `STXT` chunk: every
flag, its starting value, and its legal values. Wiring that in fixed the
start location and seeds the guard conditions correctly. Worth noting that
the data describes itself well enough that hard-coding a start room would
have been the wrong instinct.

## 10. Repository

Committed. `extract/` and the disc image are excluded; the tree carries no
game content, which the README already promised and now enforces.

## 11. Known-bad: wrong scene on some moves

helba reported walking into the kitchen and getting the wrong scene.
Checked the obvious mechanical cause first - names resolving to more than
one room in the same chapter - and found exactly one (`darkup_mbrexit`), so
that is not it.

The real cause is the missing set-piece handlers. Several doors have
state-dependent destinations: the schema declares
`#kitchenDoorIsOpen : [#diningRm, #hall, #exit]`, so the room the door
leads to depends on progress that nothing currently advances. With state
seeded to initial values and never updated by the native handlers, those
moves take the first branch every time.

Worth recording because the failure is silent. The move succeeds, the
destination resolves, the room renders. Nothing looks broken; it is just
wrong. Any test that checks "does the exit resolve" passes here, which is
the same shape of false confidence as the palette cross-check in entry 8.

## 12. QuickTime and the codecs

Wrote a QuickTime reader plus Cinepak and IMA ADPCM decoders, so the engine
needs no native media library on any platform.

Four bugs, and the way each was found matters more than the bug.

**Chunk headers.** I had them as six bytes with a 32-bit length; they are
four bytes with a 16-bit length. Everything past the first chunk was
garbage. Found by dumping a real frame's structure rather than reasoning
about it.

**Codebook and vector flags.** Cinepak chunk ids are a bit field and the
flags live in the high byte: 0x0200 selects the V1 codebook, 0x0100 marks a
partial update, 0x0400 marks luma-only entries. I masked the low byte, so
every partial update was read as a full one and overwrote the inherited
codebook from index zero.

**Chroma conversion.** The standard full-range YUV to RGB coefficients are
wrong for Cinepak, which wants `r = y + 2v`, `g = y - u/2 - v`,
`b = y + 2u`.

**ADPCM predictor.** The packet header carries the predictor in its top 9
bits, quantised to a multiple of 128. Treating that as the exact state and
restarting from it at each packet drifts by up to 127. The running
predictor has to be carried, and carried across chunk boundaries too, which
means it is decoder state and not a per-call local.

### On measuring the right thing

I nearly accepted a broken decoder three times, each time because I picked
a metric that a wrong answer could pass.

First I looked at a frame and it was white, so I counted distinct colours
instead. A later frame gave 1,945 distinct colours and I took that as
working. It was noise. Noise has excellent colour variety. That is the same
mistake as entry 8, made again within the same project, three days of
lessons later.

What finally worked was luma correlation against a reference, which
separates structure from colour and cannot be faked: it read 1.000 for the
strip that was genuinely right and -0.03 for the strip that was noise, in
the same frame. That one number localised the bug to partial codebook
updates in a single measurement, after two rounds of confident wrong
guessing had got nowhere.

The other thing that changed was the oracle. In entry 3 I checked my
decoder against my own second implementation and learned nothing, because
both shared my assumptions. Here I checked against ffmpeg, which shares
none of them. Cinepak now matches it to R 0.0 G 0.0 B 0.0 on a full frame,
and the ADPCM decoder matches 4,396,096 of 4,396,096 samples exactly. Those
numbers mean something the earlier 307,200 of 307,200 did not.

## 13. Video and audio in the engine

Hooked the codecs up. Rooms that place a movie on the `#video` channel now
load and play it behind the sprite channels, and the soundtrack goes out
through cpal.

Two things the data made awkward:

**Movie names are not filenames.** A room asks for `intro.mov`, the disc
holds `INTRO.MOV`, and a third of the references end in `.multiframe`
rather than `.mov` - a marker for frame-addressed rather than linear
playback, not an extension. Treating everything after the last dot as
advisory took resolution from 165 of 196 to 191. The remaining five look
genuinely absent under those names.

**Audio cannot be streamed from an arbitrary point.** IMA ADPCM carries
predictor state across the entire track, which is what entry 12 was about,
so seeking into the middle of a soundtrack is not a matter of jumping to an
offset. The movies are short enough that decoding the whole track on load
is cheaper than the bookkeeping to do it properly, so that is what happens.

The intro now plays where the game actually opens, which closes out the
black screen from entry 7. Its first seconds are a genuine fade from black,
so verifying it needed a seek and a pixel count rather than a look: at 60
seconds in, 76,800 pixels of the 640x480 stage are non-black, which is
exactly the footprint of a 320x240 movie centred on it. That the number
comes out exact is the check - a partially decoded or mispositioned frame
would not land on it.

No audio device is treated as normal rather than fatal, since that is the
common case over a remote session and the game is playable silently.

## 14. Ambient sound

Scripts name sounds by symbol, never by file. The resolution table turned
out to live inside `foreground.DATA`, a text cast member holding chapter
configuration rather than a room, under two keys: `#soundBank` mapping each
symbol to its source, and `#soundVolTweaks` giving per-sound gain. A source
is a filename, a `snd ` cast number, or a list of interchangeable takes the
game varies between.

The audio files are AIFF-C carrying IMA ADPCM, the same codec the movies
use, or WAV carrying unsigned 8-bit PCM.

**A shape match found the wrong thing.** My first detector looked for a
property list mentioning `houseHum` and collected 15 entries, none of which
resolved anything. Every room record also mentions `houseHum`, in its
ambient mix, so the detector was cheerfully parsing rooms. Matching the key
name `soundBank` instead took it to 181 symbols, of which 73 of the 91 the
scripts actually fire now resolve. Worth noting because the failure looked
like partial success rather than an error: 15 entries is a plausible number
for a small table.

**A peak of 32768 is not always saturation.** Three decoded loops all
reported that, which is the value a runaway ADPCM predictor produces and
which I had spent entry 12 chasing. Here it is simply correct: the sources
are 8-bit, and `(0 - 128) << 8` is exactly -32768 at full scale. The same
number means opposite things depending on the source format, so the check
has to know which it is looking at.

The remaining unresolved symbols are not missing sounds. `#BRradio` and its
siblings are playlists declared in the state schema - sequences of tunes and
announcers that cycle - so they need a sequencer rather than a file.

## 15. Radio and clock programmes

The unresolved sounds from entry 14 turned out to be a second layer of the
same table. `#BRradio` appears in two places meaning two different things:
in the sound bank it is a **group**, a nested property list of takes, and in
the state schema it is a **running order**, a list of symbols naming items
within that group.

That double meaning is why the item names repeat across groups: `#tune1` is
one file in the bedroom and another in the kitchen, so the resolution key is
the pair, not the name. My first parser dropped every group on the floor,
because it only handled property values that were strings, integers or flat
lists, and a group is a nested property list.

Telling a running order from an ordinary flag needs a test, since both are
lists of symbols in the same file. A flag's list is its legal settings, which
are distinct by definition; a programme's list is a sequence, and sequences
repeat their takes. Requiring a duplicate separates them cleanly, and it
correctly declines to treat the three single-take groups as programmes:
`#Shed: [#tune1]` is a plain loop, and the mixer holds it gaplessly rather
than the sequencer re-queuing it.

Verifying this needed stepping the running order rather than listening. Each
of the four radios alternates a long tune with a short announcer segment, and
the decoded lengths show exactly that shape: 122.3s, 18.4s, 122.3s, 6.3s,
122.3s, 14.3s. The tune entries are byte-identical because `#tune1` and
`#tune2` both point at the same file, which is a detail I would have assumed
was a bug had the sample counts not matched exactly.

## 16. Lingo bytecode: structure established, semantics partly

Source is stripped from these movies, as expected of a protected Director
build, so the 66 set-piece handlers exist only as compiled bytecode. What
survives alongside it is the name table, and it survives complete: 620 names
in Brice's movie alone, opening with `cursorOff`, `killVideo`, `pushVideo`,
`goBack`, `inState`, `trimState`, `fadeToMontage`. Those are exactly the
calls the room scripts make and that the engine currently records as
unimplemented.

Solid so far:

- The `Lscr` header layout, confirmed by arithmetic rather than assumption:
  the literal data offset plus its count lands exactly on the chunk size.
- The handler table, 42 bytes per record, giving each handler its name, its
  argument and variable counts, and its bytecode extent. **543 handlers**
  across the five movies, with real names like `setGrateIsOpen` and
  `setConservatoryDoorIsOpen` - which is the family the kitchen bug lives in.
- The instruction framing: opcodes below 0x40 take no operand, 0x40 to 0x7f
  take one byte, 0x80 and above take two. Every one of the 543 handlers
  decodes to exactly its declared length, and all 3,533 jump targets land on
  instruction boundaries with none misaligned.
- `0x93` and `0x95` are jumps. 2,448 operands, none of which escapes its own
  handler, while every other operand-taking opcode escapes routinely.

### Two contaminated measurements, in one sitting

I tried to classify opcodes by whether their operands were valid name
indices. With 1,211 names in the table, nearly any small operand qualifies,
so almost everything scored as "name-index" and the measurement said
nothing.

Narrowing to "does it resolve to a verb the game actually calls" looked
better and was worse. The first few entries of the name table are themselves
the commonest verbs, so an opcode whose operand is a small integer - an
argument count, say - resolves to `cursorOn` or `killVideo` and scores 84%.
`0x42`, whose operand never exceeds 4, scored 84% by this test and is not a
name reference at all.

What settled it was a constraint a wrong answer cannot satisfy: a jump
operand has to land inside its own handler. That separated `0x93` and `0x95`
from everything else at 2,448 to nil, and it overturned my earlier reading,
which had `0x95` as a call because its operands happened to resolve to
plausible handler names.

Three times now in this project the useful test has been the one with a
hard constraint in it - a pixel count that must be exact, a sample count
that must match, an offset that cannot leave its region - and three times
the misleading one has been a plausibility score. I do not think that is a
coincidence any more.

### Not done

The opcode table is not finished. The push and call opcodes cluster
coherently by what they reference - `0x85` and `0x45` reach property and
symbol names, `0x84` and `0x81` reach variables including the puzzle state
(`tumbler`, `lock_C`, `digitStack`, `allboxes`), `0x57` and `0x97` reach
built-ins - but individual opcodes are not yet pinned down, and nothing is
decompiled. The 66 handlers remain unimplemented.

## 17. Opcode families, found by algebra and confirmed by a control

Rather than guess at an opcode table, I set the problem up as a constraint:
every handler must leave the stack as it found it, so each of the 543
handlers gives one equation over the opcodes it contains. With 75 distinct
opcodes that system is heavily over-determined.

It did not solve outright - rank 71 of 75 - but the null space was the
useful part. Its vectors shared an exact pattern: a set of opcodes at +1
against `0x42` and `0x43` at -1. Read as an equation, that says every call
is paired with exactly one argument-list push, and the reason the system
could not separate them is that they are perfectly correlated. The failure
to solve was itself the finding.

Testing that pairing directly gave 86%, with every discrepancy in one
direction: more argument lists than calls. So the call set was incomplete
rather than wrong. Asking what actually follows an argument-list push named
the missing ones, and they referenced exactly the sort of names a call
should: `setState`, `getState`, `DoHotspots`.

  call opcodes      0x1e 0x1f 0x46 0x56 0x57 0x63 0x66 0x86 0x97 0xa3 0xa6
  argument lists    0x42 0x43

With those, the pairing holds for 539 of 543 handlers.

Two things make me trust this more than the earlier attempts. The argument
counts carried by `0x42` and `0x43` are distributed like function arities -
1209 calls with no arguments, 3531 with one, 2694 with two, 688 with three,
tailing off - which is not what an unrelated field looks like. And I ran a
negative control: adding `0x41`, which is not a call, to the set drops the
pairing from 99.3% to 29.5%. A test that cannot fail proves nothing, so I
wanted to watch this one fail on a wrong answer before believing it on a
right one. That is the check the two contaminated measurements in entry 16
never had.

### Still not done

Individual stack effects are not separated, because the correlation that
revealed the families also prevents the algebra from splitting them. Nothing
is decompiled and the 66 handlers remain unimplemented. What exists now is
the frame, the handler inventory, and the call structure.
