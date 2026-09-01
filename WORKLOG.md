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

## 18. Literal references, and a correction

The literal table is five-field: a count and offset for an 8-byte record
table, plus a count and offset for the data those records point into. Each
record is a type word and an offset; each datum is a length word followed by
its bytes. Type 1 is a string.

That made a candidate I had not tested: a literal reference in bytecode is a
**byte offset into the record table**, so a multiple of 8, not an index.
Three opcodes satisfy that for every single use:

  0x44   1245 uses   100.0%
  0x4b   1025 uses    99.9%
  0x84    716 uses   100.0%

The control matters more than the score. The median script has ten
literals, so a stray operand clears the bound rarely, and the field bears
that out: below those three the next tier is 0x4c at 92% and 0x52 at 89%,
then it falls away to 40%, 20% and eventually zero. A test that everything
passes would be worthless; this one separates three opcodes from thirty by
a wide margin.

**Correction to entry 16.** I reported there that `0x84` reached variable
names, listing `tumbler`, `lock_C`, `digitStack` and `allboxes`, and said in
summary that this was the puzzle state. That reading came from the
name-index test I had already shown to be contaminated, and it was wrong:
`0x84` is a literal push. Those words are real and they are still in the
data, but as strings in the literal pool rather than as variables the
opcode names.

I also got the measurement wrong before I got it right. My first bounds test
scored every opcode against per-handler tables and reported that nothing fit
anything. The fault was the denominator: handlers with an empty table could
never contribute a success but were still counted as attempts, which pushed
every candidate below its true rate. The data had not changed between that
run and the one that found three exact fits; only the arithmetic had.

### Still not done

Locals, arguments, globals and properties are not separated - 0x4c and 0x52
are the obvious pair to chase, being the second and sixth most common
operand opcodes and sitting just under the literal threshold. Nothing is
decompiled, and the 66 handlers remain unimplemented.

## 19. Slots, and the first readable handler

Handler records carry offsets to their argument and local-variable name
lists. Parsing those was the step that grounded everything else, because the
names that came out are unmistakably real: `setGrateIsOpen(suggestion)` with
a local `currentState`, `setGammaLevel(desiredLevel)` with `currentLevel`,
`pNum` and `i`, `castCursor(cursorID)` with `whichCursor` and `cMask`. Loop
counters and meaningful parameter names are not what a wrong parse produces.

Slots are eight bytes wide, and three opcodes address them for every use in
the corpus:

  0x4b   push argument        1033 uses
  0x4c   push local           5393 uses
  0x52   set local            2577 uses

The read-to-write ratio between 0x4c and 0x52 is about two to one, which is
what reading variables more often than writing them looks like.

`0x4b` needed disambiguating, because "operand is a multiple of eight and
under some count" describes both an argument reference and a literal one,
and it had scored highly as both. The separation is in the violations rather
than the fits: across the corpus `0x4b` never once exceeds its handler's
argument count, and does exceed the literal count nine times. Every opcode in
this group is decided the same way, by the single bound it never breaks while
breaking all the others.

### The first handler that reads

With calls, argument lists, literals, arguments and locals identified, a
disassembly of `setGrateIsOpen` comes out as code: read the current state
with `getState`, compare it against the suggestion passed in, and on a
difference call `setProp` and then `updateDisplay`. That is precisely the
shape a door-state setter should have, and it is the family the wrong-scene
bug from entry 11 belongs to.

The output reading as sense is itself the check. Wrong opcode attributions do
not produce a coherent handler; they produce plausible-looking noise, which
is a distinction this project has had to learn twice.

The literal pool also preserves the developers' debug strings, left in the
shipped build.

### Still not done

The arithmetic and comparison opcodes are unidentified, and so are globals
and properties, though `0x49` and `0x85` are clearly those two from context.
Nothing is translated to source yet and the 66 handlers remain
unimplemented.

## 20. Globals, and a method that failed

`0x49` and `0x89` push globals; `0x4f` sets one. Globals are named
program-wide rather than per script, so unlike locals they index the movie's
name table directly, which is why the per-handler bounds tests never fit them.

The test used Director's naming convention, where object references start
with `o` and globals with `g`. Across the whole name table only 5.1% of names
look like that, so it discriminates: `0x49` lands on them 92.8% of the time
over 1706 uses and `0x89` 92.3%, while the argument-list opcodes land on them
0.0% of the time. The names that come out are `oStoryteller`, `oPuppeteer`,
`gCPU`. An eighteen-fold enrichment over background with a clean zero at the
other end is the shape I now look for.

### The stack-effect solve did not work

I spent most of this stretch trying to derive every opcode's stack effect from
the balance constraint, and it failed. Anchoring the known effects and solving
the rest gave a model under which only 84 of 543 handlers balanced. Testing
whether `0x42` and `0x43` differ in keeping or discarding a call's result -
a reasonable idea, since one would be an expression and the other a statement
- produced at best 78 of 543.

The fault was in the method rather than the data. I was simulating stack depth
straight through code that branches, and with a median handler of fifty
instructions, jumps throughout, and 771 mid-handler returns across the corpus,
a linear walk does not describe what the stack does. Getting this properly
needs a control-flow graph with depth propagated along edges and checked at
join points, which is a larger job than the afternoon I gave it.

Worth recording that the useful finding of the stretch came out of that
failure anyway: every one of the 543 handlers ends with `0x01`, which is the
return, and that turned up only because I went looking for why the balance
model was wrong.

### Where the disassembler stands

`setGrateIsOpen` now reads as its own logic: fetch the current state from
`oStoryteller`, compare it against the argument, and on a difference drive the
animation through `oPuppeteer`, write the new state with `setProp`, and
refresh. That is the division of labour the room scripts already assume, which
is a check on the reading.

Unidentified: the arithmetic and comparison opcodes, `0x03`, `0x0f`, `0x12`,
`0x61`, `0x62`. Nothing is translated to Rust and the 66 handlers remain
unimplemented.

## 21. Comparisons and arithmetic, read rather than solved

The statistical approach ran out of road. Solving stack effects from argument
windows was better than whole-handler balance - windows are short and do not
branch - but the opcodes I most needed still sat between 50% and 72%
agreement, because a nested call inside an argument truncates the window and
corrupts the equation.

So I stopped solving and started reading. Listing handlers by length put
`suspendSounds` in view, and it contains a textbook counted loop:

     push int 1 / set local i
     push local i / push local soundChannels / call count / 0x0d / jump out
     ...
     push int 1 / push local i / 0x05 / set local i / 0x54 back

That is `repeat with i = 1 to count(soundChannels)`, and it names two
opcodes on sight: `0x05` adds, `0x54` closes the loop.

Confirming the family statistically then worked, because by then I knew what
to measure. Comparisons are defined by what consumes them, not by what
precedes them:

  0x0d 0x0e 0x0f   comparisons     followed by a jump 99%, 73%, 75%
  0x12             logical and/or  followed by a jump 93%, and preceded by a
                                   push only 6% - it consumes comparison
                                   results, not values
  0x04 0x05 0x06 0x0a   arithmetic  preceded by a push 86-97%, followed by a
                                   store

Which comparison is which is still open, so the disassembler prints them by
role rather than inventing a symbol for them. An honest `compare-c` is worth
more than a confident `=` that turns out to be `<>`, and getting that backwards
in a door handler would invert the behaviour while still looking plausible.

The lesson I want to keep is about sequencing. Reading one handler carefully
told me more in a minute than three rounds of corpus statistics, and it also
told me what the statistics should be measuring. I reached for the aggregate
method first because it feels more rigorous, but it only becomes rigorous once
you know what question to put to it.

## 22. Nine handlers ported, and a constant found by diffing

Cross-referencing the engine's own list of missing handlers against the
compiled ones gave the work inventory I should have built earlier: 65 of the
66 have bodies in the movies, and sorting them by size shows the ground is
not uniform. The smallest are one to thirty instructions; the largest,
`driveTheCar`, is 1026.

The small ones read cleanly, and a matched pair settled `0x03` in two lines:

  enableGust    push #gustEnabled / push int 1 / setState
  disableGust   push #gustEnabled / 0x03       / setState

`0x03` pushes the constant zero. Its 980 uses across the corpus had resisted
every statistical test I threw at it, and a two-line diff between handlers
that differ only in the value they store gave it up immediately. That is the
third time in this project that reading a specific pair beat measuring the
whole population.

Ported so far: `enableGust`, `disableGust`, `enableSongs`, `disableSongs`,
`freezeInventory`, `beeSwarm`, and three that turn out to be empty hooks in
the shipped build. The engine's own report moves from 66 distinct handlers
over 438 call sites to 57 over 384.

`beeSwarm` is worth a note. It reads as a one-in-three die roll gating a
sound, so the port rolls too rather than firing every time - the
intermittency is the effect. The roll is seeded from game state instead of a
system source, so a replayed save sounds the same, which the original would
not have guaranteed but which is the better behaviour for a save file.

### Polarity is still assumed

`beeSwarm` also pins the jump convention, if the reading is right: the
comparison is followed by a jump that skips the sound, so the jump must be
taken when the comparison is false, which makes `0x0f` an equality test. That
is consistent and it is what the handler ought to do, but it rests on the
sound being the rarer outcome rather than the commoner one. Every ported
handler so far is unconditional and so does not depend on it; the first
conditional handler will, and that is where it needs a harder check than
plausibility.

## 23. Polarity settled, and three more ported

The jump convention was the last thing resting on plausibility, and
`setGrateIsOpen` settles it because the handler is a mirror:

  suggestion = 0  and  currentState = 1  ->  close cue, store 0
  suggestion = 1  and  currentState = 0  ->  open cue,  store 1

Three identifiers flip together across the two branches: the animation cue,
the stored value, and the constants compared against. That is not an appeal
to which outcome seems likelier; it is the same fact stated three ways in the
same handler. So `0x0f` is equality, `0x12` is a logical and, and the jump is
taken when the condition is false.

`shedAutoSlam` confirms it independently: read the shed door state, compare to
1, and on the branch that runs when it is open, set it to 0 and change the
ambience. A door that slams itself shut only when it is currently open is the
right behaviour, and the opposite polarity would make it slam only when
already closed.

Thirty-six handlers share the door-setter shape, found structurally rather
than by name. They are not called from room scripts though - the scripts call
a dispatcher - so porting them alone would not move the engine. Ported
instead were the small handlers the scripts do call directly:
`shedAutoSlam`, `stashClick` and `curseWeeds`. The report moves from 57
distinct handlers over 384 call sites to 54 over 357.

### Two tool faults worth recording

`tools/disasm.py` overwrote `sys.argv` at import, to satisfy a module that
reads it at import time, and so ignored its own command line and disassembled
the same handler whatever it was asked for. I only noticed because a lookup
for a handler that does not exist returned a listing for one that does. A
tool that silently answers a different question than the one asked is worse
than one that fails.

It also printed the contents of the literal pool, which is game text and
serves no purpose in a listing. It now prints lengths instead.

## 24. The property-assignment statement, and the kitchen

Chasing the wrong-scene report led somewhere useful, though not where I
expected. Room scripts do not call the compiled door setters at all: they
call `setState( oStoryteller, #FrontDoorIsOpen, FALSE )` directly, which the
engine already handles. So the thirty-six door setters decoded in entry 23,
while correctly read, are reached only through a dispatcher and porting them
would not have moved anything. Worth knowing before spending a day on them.

What the kitchen doorway actually does is this:

  set the queuedSound of oPuppeteer = #swingingDoorOpen
  goTo( #HallKitchenEntryOpen, #forward )

The first line is Lingo's property-assignment statement, which the engine was
recording as an unimplemented native and dropping. Thirty of the thirty-four
uses across the whole game are exactly this shape, queuing a sound on the
puppeteer so that a move carries its own effect - the door you hear while
walking through a doorway is queued by the hotspot that walks you through it.
Implementing that one form took the report from 54 handlers over 357 call
sites to 54 over 327, and the sound count rose by exactly thirty.

### A second tool that answered the wrong question

While checking the fix I noticed `shot` reporting a 2991-frame movie for a
room that has no movie at all. `cmd_shot` never called `start_room_video`, so
every screenshot carried the startup movie over the top of whatever room was
asked for. In `play` the same path is covered, because moves go through
`apply`, so this was confined to the tool.

That is the second time in two entries that a diagnostic quietly answered a
different question than the one put to it, after `disasm.py` ignoring its own
command line. Both were caught by a number looking wrong rather than by
anything failing. When the tools are how you see the problem, a tool that
lies is worse than one that breaks.

The patch attempt also failed safe: I asserted the code I meant to replace
was present, and it was not, because I had misremembered an error message.
The script refused rather than writing a mangled file.

## 25. The guard bug, found from a precise repro

helba reported walking out the front door, round the back, and straight
through the kitchen's rear door, which should have been locked. That route
was exact enough to trace, and the fault was not in that door at all.

Compound guards nest as `[#and: [#equals: [a, b], #equals: [c, d]]]`. Two
things were wrong with how that was read.

The property list was stored in a `BTreeMap`, and both clauses are keyed
`#equals`, so the second silently replaced the first. Lingo property lists
are association lists and permit repeated keys; the game relies on it. 247 of
the 381 compound guards in the data take that form.

Worse, the `#and` branch read its operand with `as_list()`, but the operand is
a property list, not a linear one. That returned nothing, so every compound
guard became `And([])` - and an empty `all()` is true. **Every `#and` and
`#or` guard in the game was passing unconditionally.**

The fix was to model property lists as ordered pairs with duplicates kept, and
to read compound operands out of their entries. The kitchen door now blocks,
and its full guard reads as it should: the scan device on the knob and the
door state together.

The scale shows up elsewhere too. Sprites visible at the initial state fell
from 1489 to 1374: 115 sprites were being drawn because their `#showIF`
passed vacuously. Nothing had looked wrong, because a room with too much in
it still renders.

### A CLI, because a bug you cannot re-run is a bug you cannot fix

helba could not reproduce the route without the window. There is now
`amber walk`, which steps through rooms in the terminal, prints the exits
that are live under the current state, and takes a `blocked` command listing
the hotspots that exist but whose guards currently fail, with the guard
printed. Routes can be passed as arguments so a repro is a one-liner.

That command is what confirmed the fix, and it would have found this bug
weeks earlier than the renderer did. Worth building the instrument before
needing it, not after.
