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

## 26. Three faults from one play session

helba played for a few minutes and turned up three distinct bugs, two of
which no amount of static checking would have found.

**The programme sequencer spun.** `tick_program` advanced the playlist index
and then returned early when an item failed to resolve, without ever setting
the next due time. So the programme was due again on the following frame, and
raced through its running order at the frame rate, re-firing every item that
did resolve dozens of times a second. Heard as the same sound over and over.
The fix advances the clock before anything can fail, and a programme whose
items all fail now stops rather than polling for ever.

Worth noting what made it possible: eleven of the ninety-one sound symbols do
not resolve, which I had recorded as an acceptable gap. It was, right up until
an unresolved item became a spin instead of a silence. A known gap and a
loop are individually fine and together are not.

**The video blit crashed.** Cinepak resizes its own buffer when a frame header
disagrees with the container, but the player kept reporting the container's
dimensions, so the blit was told 320x240 while the buffer had shrunk. The
player now reports the decoder's live size, and the blit clips against the
actual slice rather than trusting either, so a future mismatch costs pixels
instead of the process.

**The opening could not be skipped.** Rooms carried entirely by a movie have
no live exit, because the original advances them from script when the movie
ends. Space now stops the movie, and where the room has nothing else to show,
moves on: the opening leads to `Gbhs_gameEntry`, which the name table made
easy to find sitting next to `Gbhs_playIntro`. This is a deliberate departure
- the original had no skip either - and it is marked as such in the code.

## 27. The hovering drone: twelve bytes

helba reported the repeating sound was still there after the spin fix, and
described it as a loud hovering noise. That description was the clue: a spin
sounds like stutter, a drone sounds like something periodic and wrong.

Every ambient loop decoded with a peak of exactly 32768, which is the clamp
value. I had noted that in entry 14 and explained it away: eight-bit sources
reach full scale, and `(0 - 128) << 8` is exactly -32768, so the number was
consistent with correct decoding. It was also consistent with a bug, and I
did not check which.

The check took one dump. Both sounds began with exactly twelve samples at
-32768 and then real audio. Unsigned eight-bit silence is 0x80, not 0x00, so
twelve zero bytes at the head of the samples are not audio at all - they are
header being read as sound.

These carry an extended sound header, and its sample data begins at offset 64,
not at 52 where a field-by-field walk lands. Every offset was confirmed
against the data rather than assumed: the frame count at +22 reads 70364,
which is exactly the decoded length; the sample size at +48 reads 8; the audio
visibly starts at +64.

So every sound in the game opened with a full-scale click. On the house hum,
a three-second loop, that is a thump every three seconds for as long as the
player stands in the room.

What the fix exposes is how badly the defect had hidden itself. The peaks now
read 3328 for the grounds, 6084 for the garage, 9216 for the computer loop:
real dynamic range, quiet ambiences actually quiet. Before, all nine reported
32768, because twelve bad samples set the maximum for every one of them. The
measurement I was using to check the sounds was itself dominated by the bug.

That is the fourth time in this project a plausible explanation for a wrong
number has cost me more than checking would have. The pattern is specific
enough now to name: when a statistic comes out at exactly a boundary value,
the boundary is the thing to investigate, not to rationalise.

## 28. Ambience that followed the player outdoors

helba reported the house hum continuing after leaving the study, and still
playing out on the grounds, and guessed it was missing game logic. It was not:
the logic was present in the data and the engine was ignoring it.

Ambient loops were started on entering a room and never stopped. There was no
retirement step at all, so every loop the player triggered kept running for
the session, stacking. The per-room mix was thrown away too: `#earShot` gives
`houseHum` at 224 indoors, 160 and 96 nearer the doors, and 0 out on the
grounds. Across Roxy's chapter that key is 0 in 290 rooms and 224 in 252, so
the fade as the player steps outside is a designed effect and half the rooms
in the game specify it.

The fix makes the playing set match the room's mix on every move: loops the
new room does not want stop, loops it shares keep their position so the sound
stays continuous, and their gain is reset to the new room's level.

`walk` now prints each room's ambient mix, which is how this was checked
without listening: the study asks for the hum at 88% and the grounds ask for
silence.

### On the diagnosis

The report came with a guess attached - that this was missing game logic - and
it was a reasonable guess, since most of what remains unimplemented is exactly
that. It was worth checking anyway, and the check was cheap: look at what the
data asks for before concluding the data does not ask for anything. Three of
the last four bugs helba has found were in code I had written, not in logic I
had yet to write, and I would have been slower to each of them if I had taken
the accompanying guess at face value.

## 29. Inventory, and 800 hotspots that were unreachable

Asked what was next, the honest answer came from counting rather than
preference. `#itemInUse` is the single largest hotspot class in the game at
800 of them, and every one requires the player to be holding something. With
no way to choose what is held, all 800 were unreachable. That is the
difference between walking round the house and playing.

The movie carries an `inventory.DATA` cast member giving each item a pair of
icons, plain and lit, at 67 pixels square: eight items, from the scan device
to the crowbar. The lit icon marks what is in hand.

The bar draws along the bottom of the stage, which is clear of the art since
rooms are at most 452 pixels tall on a 480 stage and centred. It takes first
refusal on a click, so picking up an item does not also walk the player
through whatever hotspot lies underneath it.

`walk` gained `give` and `use`, which is how this was checked without playing
to the point where an item is found: standing at the kitchen's rear door with
the scan device in hand, two `#itemInUse` hotspots appear that were not there
before. That door is the one helba walked through when it should have been
locked, and the scan unit is what the guard was waiting for.

### What "next" should mean

I nearly reached for the largest remaining handler, `chippyCries` at 70 call
sites, because the list was already sorted that way. Sorting by call sites
measures how often something is invoked, not whether the game is playable
without it. 800 beats 70, and no amount of set-piece work would have mattered
while the player could not pick anything up.

## 30. Sprite registration, and a fix built on a bad measurement

helba sent a screenshot of the front door close-up rendering as mismatched
vertical bands. The room stacks three 600x300 plates - the doorway lit, the
same unlit, and the closed doors as an overlay - and they were landing tens of
pixels apart.

I got this wrong once before getting it right, and the way it went wrong is
the point. A regex I wrote to read the sprites' `#coords` reported `NONE` for
all three, so I concluded there was no anchor and changed the unanchored
fallback to centre the image. The regex was faulty; every one of those sprites
carries `#coords: point(320, 210)`. The change was harmless but its
justification was invented, and I committed it before checking the reading
against anything.

Tracing the actual placements gave the real answer in one run:

  cast 2181  reg=(336,212) -> (-16, -2)     wrong
  cast 1910  reg=(300,150) -> ( 20, 60)     right

Dumping both members' rectangles explained the difference. Cast 1910's is
`(0, 0, 300, 600)`, origin at zero. Cast 2181's is `(62, 36, 362, 636)`: the
same 600x300 size at a non-zero origin. And its registration point, (336, 212),
is exactly (36 + 300, 62 + 150).

**The registration point is expressed in the member's rectangle space, not the
image's.** Members whose rectangle starts at zero are unaffected, which is why
this went unnoticed: most do. Subtracting the origin puts both plates at
(20, 60), which is where the known-good room's plate sits.

520 of the game's 3208 bitmaps carry a non-zero origin, with offsets reaching
471 pixels. Every one of them was being drawn in the wrong place whenever it
appeared.

The lesson is not about Director. It is that I trusted a measurement I had
just written, from a regex over text, without checking it against a second
source. The trace I eventually added took two minutes and answered the
question exactly.

## 31. TRUE was not 1

The front door could be opened and still not walked through. The walkthrough
put the cause on screen in one step: after clicking the door, both of the
room's pointer hotspots reported as blocked, one waiting for
`frontDoorIsOpen = 0` and the other for `= 1`. A value that satisfies neither
is not a boolean at all.

The action is `setState( oStoryteller, #FrontDoorIsOpen, TRUE )`. The parser
treated a bare word as a symbol, so the door entered a state named TRUE while
every guard that reads it compares against 1. Anything set that way became
permanently unreachable. The literals appear 227 times across the room
scripts, so this was not confined to one door.

Mapping TRUE and FALSE to 1 and 0 fixes it, and the door now opens and admits
the player.

That the `blocked` command found this in a single step is the argument for
having built it. The same bug through the window is a door that will not open
and no way to ask why.

## 32. Cursors, and knowing when not to be faithful

helba asked for proper cursors, partly to see the hotspots. The game's own
cursors are 1-bit image and mask pairs at cast 2500 onward, addressed as
`2500 + (cursorID - 6000) * 2`, and which cursor a verb gets is decided inside
`castCursor` and its callers rather than in the room data. Using the original
art therefore means decoding another handler first.

The cursors drawn here are mine: an arrow per direction, a lens for examine, a
blunt pointer for operables, crosshairs while an item is in hand. What matters
for playing is that the pointer says what a click will do, and that does not
require the original bitmaps. Tab outlines the live hotspots, which answers
the question behind the request more directly than any cursor: whether an exit
is missing or merely hard to find.

Recorded because it is a departure. Everything else in this project has been
faithful to the disc, and when the original art is reachable it should replace
this. The stand-in is the cheap thing that unblocks play now, not the right
long-term answer, and the code says so.

## 33. Order, not area

helba walked into the darkened house and arrived in the lit one, and read it
as unfinished game state. It was not: the state was correct throughout and
the fault was in how I chose between overlapping hotspots.

The porch offers two forward exits whose guards can both hold at once:

  Forward (79, 57, 526, 363) -> DarkDn_Entry2   lights off and door open
  Forward (90, 62, 502, 358) -> HallNwall       door open

My hit test broke ties between same-verb hotspots by smallest area, and the
lit rectangle is the smaller of the two at 121,952 pixels against 136,782. So
every click in the overlap chose the lit house.

Director checks hotspots in the order the room lists them and takes the first
match. That order is the authors' expression of precedence and it put the
darkened exit first. Area was my own invention, introduced to make a small
`#examine` target beat the room-sized `#browse` beneath it - but verb priority
already does that, so the area rule was doing nothing except quietly
overriding the data.

The walkthrough had been right all along, because it takes the first matching
hotspot, which is why the dark path held there and broke in the window. Two
tools disagreeing was the signal; I should have noticed sooner that I was
reading them as one confirming the other rather than as one contradicting it.

`walk` now takes `click <x> <y>`, which runs the same hit test the window
uses, so an overlap that resolves the wrong way can be reproduced exactly
rather than inferred.

## 34. Waits, and why nothing in the world moved

helba asked where the in-world animations were - the power switch, the
computer starting. They were never going to appear, because the engine ran
each hotspot's action list straight through inside one frame.

The emergency switch shows the pattern:

  setState( oStoryteller, #eSwitchInUse, TRUE )
  updateDisplay( oPuppeteer )
  wait #videoStop
  setState( oStoryteller, #eSwitchInUse, FALSE )
  killVideo

The flag makes the movie sprite eligible, the redraw brings it on screen, and
the wait holds until it has played. Run without honouring the wait, the flag
goes up and comes down in the same frame and the movie is killed before a
single frame of it is drawn. Every animated interaction in the game is built
this way, and 143 hotspot sequences contain a wait, 55 of them on a movie.

Hotspot scripts are now a queue rather than a batch. Actions run one at a
time until one asks to wait; the wait is held across frames and the rest of
the sequence resumes when it clears. A room with no movie treats a video wait
as already satisfied, so a missing asset cannot stall a sequence for ever.

Worth noting the shape of the mistake, because it is the same one as the
ambient loops that never stopped and the stage that never recomposed. In each
case the engine did the thing the script asked for and then failed to do the
part the script did not have to ask for, because Director did it. The scripts
are written against a runtime that redraws on its own, retires its own loops,
and blocks on its own waits. Anywhere the data is silent is somewhere I have
to supply behaviour rather than take the silence as meaning nothing happens.

## 35. One unrecognised key form cost a chapter's audio

helba asked for the missing voice events. Six of Edwin's 33 voice cues would
not resolve, and the cause turned out to be two faults stacked.

Lingo permits any value as a property key, and the game keys a movie's event
track by frame number:

  #alone: [165: 90, 167: ["assertSound #aCleverCar"], 173: 120]

The parser accepted only `#symbol:` and `"string":` keys. Meeting `165` it
read a list element, hit the colon, and failed the enclosing list - which was
Edwin's entire sound bank, 10,644 bytes of it.

The failure hid well. Cues that Edwin shares with another chapter still
resolved from that chapter's bank, so the score read 80 of 91 rather than
"one chapter has no audio at all". A partial number looked like a small gap.

Underneath that, the bank's filename extensions are unreliable: five cues are
listed as `.wav` where the disc holds `.AIF`. The stem is what identifies the
sound, so the extension is now retried, the same lesson as `.multiframe`.

Sound coverage goes from 181 symbols and 80 of 91 references to 259 symbols
and 104 of 104.

### What the track data is, and why it is not implemented

With the parse fixed, `#trackData` is readable. Each variant pairs a movie
with two parallel timelines, `#alone` and `#chippy`, according to whether the
companion is present, and both are keyed by frame number.

Entries take two forms. A list is unambiguous - `167: ["assertSound
#aCleverCar"]` fires that cue at frame 167. A bare integer is not: `165: 90`,
`0: 3` and `411: 175` are all consistent with a duration, a target frame, or a
pose index, and the small values in the companion track look like a different
quantity from the large ones in the solo track.

Implementing on a guess would produce a ride sequence that plays and is wrong,
which is worse than one that does not play, because it looks finished. Left
until the meaning is established rather than inferred.

## 37. The ghost telephone

`ghostCalls` was the largest thing left at 57 call sites, and it is the
mechanic the game is named for: the dead telephone the player.

  possibleCallLists = [#allGhosts, #Brice_entry, #Margaret_entry,
                       #Edwin_entry, #Brice_warm, ..., #None]
  if getPos(possibleCallLists, suggestion) = 0 then exit
  if suggestion = #Brice_warm then
    if inState(#ghostsRemaining, #Brice) then [#Brice, #nobody, #nobody]

The padding is the weighting, and reading it that way is what made the
handler portable. An entry call is a bare `[#Ghost]` and always lands; a warm
call adds two `#nobody` entries and lands once in three; a cool call adds
three and lands once in four. `#allGhosts` collects whichever ghosts remain
and pads with three. A ghost already dealt with never enters the list, so
`#ghostsRemaining` both gates the calls and thins them as the game is solved.

The calls themselves are external files named by initial rather than symbols
in the sound bank, which is why nothing resolved when I looked for them
there: Brice has eleven, Edwin twelve, Margaret ten. Sound lookup now falls
back to treating an unknown name as a filename, which reaches them without a
special case.

Native handlers fall from 52 distinct over 315 call sites to 51 over 258.

### A note on the tooling

Three patch attempts this session failed their own assertion because I wrote
the expected code from memory rather than reading it first. Each time the
guard refused rather than writing a mangled file, and each time reading the
function took ten seconds. The assertion is doing work I should not be
needing it to do.

## 38. Chippy, and a line I chose not to follow literally

`chippyCries` was the last of the big handlers at 70 call sites. Chippy calls
for help from out of sight until freed, and the roll makes it occasional:
`random(6) <= 2` normally, and `<= 6` when the script asks for `#loud`, which
always sounds.

Its last two lines are the interesting part.

  nextPlea = getLast(pleaList)
  setState(oStoryteller, #distantPleas, nextPlea)

`getLast` is not defined as a handler in any of the five movies, so it is
Lingo's built-in, which returns the last element of a list rather than a list.
Read literally, the pool of eight pleas is replaced by a single symbol after
the first cry, nothing can be indexed out of it again, and Chippy falls silent
for the rest of the chapter.

I could not settle which the original did, and the two readings are not
symmetric. Following it literally produces behaviour that is obviously broken
across seventy call sites; rotating the pool produces what a list of eight
consumed one at a time is evidently for. I rotated, and said so in the code,
because this is a judgement rather than a reading and the next person should
be able to see that it was made.

Native handlers fall from 51 distinct over 258 call sites to 50 over 188. The
three biggest handlers in the game - ghostCalls, chippyCries and the laptop
pair - are now done, and what remains is a long tail of puzzle machinery.

## 39. The tail begins

Three of the small handlers, taken together because they share a shape:
read a state key, choose a line, sound it, wait for it to finish.

`snowBlind`, at 26 call sites the largest of the remainder, is the white-out
on the ice: a gust, four montage steps out and back, the player turned round,
and a remark if it happened within sight of the house. The two anchor
handlers pick between two lines on the boat's facing and where Teddy is.

`iceAnchorComments` has a detail worth keeping. When the boat faces the wrong
way the handler never assigns its local at all and then sounds it unset,
which in Lingo is silence. The port returns early instead of sounding an empty
name, which is the same behaviour by a clearer route; sounding a void would
have meant a lookup failure in the engine rather than a deliberate nothing.

Native handlers fall from 50 distinct over 188 call sites to 47 over 155. From
438 sites when the bytecode work began, that is nearly two thirds retired.

## 40. Margaret's chapter, and a wall worth naming

Margaret's module was the conspicuous gap, so I went at it and got one
handler. The other five stopped against the same thing, and finding out what
that thing was mattered more than the handler did.

`newDoorStatic` and `initRadioDial` drive sprite channels directly:
`puppetSprite` to take a channel over, then assigning that sprite's cast
member and position frame by frame. The engine draws the sprites a room
declares in its `#onStage` list and has no path for imperative control of a
channel at all.

Rather than guess how much of the remainder that blocks, I counted:

  needing puppetSprite   14 handlers,  53 call sites
  portable as-is         33 handlers, 113 call sites

So the tail is not one long grind of forty-odd decode jobs. It is two thirds
ordinary porting and one third waiting on a single renderer capability, and
the blocked third includes most of the puzzle machinery - the radio dial, the
whirligig, the door static - because a puzzle is exactly the kind of thing
that animates a channel under script control.

That reframes the work. Adding puppet channels to the renderer is one piece
of engine work that unblocks fifty-three call sites at once, and it is worth
doing before grinding through the thirty-three that do not need it.

`resetBoxPuzzle` is ported: stop the video and empty `#boxList`, so the boxes
can be worked through again from the start.

## 41. Puppet channels, built halfway on purpose

Director lets a script take a sprite channel away from the score with
`puppetSprite` and then drive it directly, setting its cast member and
position frame by frame. The engine had no path for that, and entry 40
counted the cost: 14 handlers over 53 call sites blocked on it.

The layer is now there. Claimed channels are held apart from the room's
`#onStage` list and composited over it in channel order, a move hands them all
back, and the interpreter turns `set the castNum of sprite 39` and `set the
loc of sprite 39` into effects that drive them.

What is not there is the bytecode side, and the reason is worth recording. A
compiled sprite write looks like

  push int 45 / push local loopClip / push int 4 / 0x5d 6

Three operands and an opcode operand, and which of them selects the property
is not established. Guessing would put the right cast member in the wrong
channel, or the right channel at the wrong position, and both look like a
puzzle that is subtly broken rather than one that is unimplemented.

So the layer is driven by the six assignments the room scripts spell out in
text, where the property is named and nothing is inferred. That is a thin
exercise - four `loc` writes and one `castNum`, all on channel 39 - but it is
real, and it means the infrastructure is proven before the ambiguous half is
wired to it.

The 53 blocked call sites are still blocked. What changed is that they are now
blocked on one identified question, the property index mapping, rather than on
a missing capability.

## 42. The sprite property mapping

Entry 41 left the puppet layer built but unwired, because a compiled sprite
write pushes three operands and it was not established which selected the
property. That is now settled.

My first test was worthless and worth recording as such. I checked whether
each operand position held a live cast member; every position passed at 100%,
because 2,385 of the movie's ~2,400 cast slots are live and almost any small
integer qualifies. A test everything passes measures nothing, which is the
same contamination as the name-index attempt in entry 16.

Ranges discriminate where membership did not:

  1 back   494 values, 11 distinct, 2..33     the property index
  2 back   the value, mostly non-literal      what is written
  3 back   123 values, 9 distinct, 4..45      the channel

The channel is forced rather than inferred. Its literals are 44, 45, 39 and
30, which is exactly the set `puppetSprite` claims elsewhere in the corpus.
That correspondence is not something an arbitrary operand would produce.

The properties are then named by what is written to them:

  4    fed by getProp, getAt and cast lookups   castNum
  33   fed only by point()                      loc
  15, 25   only ever 0 or 1                     flags
  13   only ever 70

A property whose values are all `point()` results can only be a location, and
one fed by cast lookups can only be a cast reference. Both readings are
independent of the other. `0x5c 6` is the reading counterpart of `0x5d 6`, so
sprite properties can now be both set and queried.

With that, `newDoorStatic` reads plainly: claim channel 45, point it at the
door static loop, hide it, position it relative to the origin, start the
looping static and push the video.

It is not ported yet. The cast it points at comes from `getProp(oPuppeteer,
#doorStatic)`, and the engine does not model the puppeteer's property table -
though the value is sitting in the chapter's `foreground.DATA` config, which
lists `#doorStatic` among the presentation cast numbers. That table is the
next piece, and it is a lookup rather than a question.

## 43. The presentation table, and a port that did not use it

Handlers reach for cast members by name - `getProp(oPuppeteer, #doorStatic)` -
and each chapter's `foreground.DATA` holds the answer. The table is now
loaded per chapter and checked against the config it came from: Margaret's
`doorStatic` resolves to 1075, Roxy's `Headgear` to 973, and Edwin's and
Brice's credit screens to 1056 and 727.

With it, `newDoorStatic` is ported: claim channel 45, point it at the static
loop, prepare it hidden, start the looping static, duck the ambience and run
the movie over it. Seven call sites.

I nearly shipped it broken. The first version claimed the channel and never
set its cast, so the plate would have been claimed and empty - the whole
point of building the table, skipped in the handler the table was built for.
The cause was structural rather than careless: `natives::call` takes the state
and the outcome, not the game, and the table hangs off the game, so the
lookup simply was not reachable from where I was writing.

Reaching through would have meant handing every handler the whole game. The
effect carries the name instead and the game resolves it when applying, which
keeps handlers to state and effects and puts the chapter's table where the
chapter's data already is.

Worth noticing that the missing lookup did not fail. It compiled, the tests
passed, the handler count fell by seven, and every number I habitually check
said the work was done. What caught it was rereading the port next to the
disassembly it claimed to implement.

## 44. The peek alert, and the same mistake twice

`peekAlert` pulses the peek unit in the inventory bar to say it has something
to show: twelve cycles on sprite channel 7, five ticks apart, alternating
between two glow icons. The guard forced a new opcode along the way - the
handler returns when the alert is disabled *or* the unit is not carried, which
makes `0x13` an or against `0x12`'s and.

The item's three icons explain something noticed weeks ago and left alone. The
inventory table lists two casts for every item and three for the peek unit;
the third is this brighter glow. A detail that looked like an inconsistency in
the data was the data being precise about something not yet implemented.

I wrote the port referencing icons by invented names, `PeekGlowHigh` and
`PeekGlowLow`, which exist nowhere. Nothing would have resolved, the channel
would have stayed empty, and no pulse would have appeared. It compiled, the
tests passed, and the handler count fell by exactly twelve.

That is the second time in three entries: `newDoorStatic` claimed a channel
and never set its cast, and this named casts that do not exist. Both times the
count fell correctly, which is what makes the mistake comfortable to make. The
count measures that a handler stopped being reported, not that it does
anything.

The icons are addressed by position now, as the original's `getAt` reads them,
and the inventory table keeps every cast an item lists rather than the first
two.

## 45. A check for the mistake I keep making

Twice now I have written a handler that compiles, drops the unimplemented
count and does nothing, because it names a cast or an icon that does not
exist. Both times every number I habitually check said the work was done.
Rereading the port against its disassembly caught both, but that is a habit,
not a mechanism.

`verify` now walks every ported handler and checks that each cast, icon and
sound it reaches for actually resolves.

The first version was worthless, and the control is what proved it. I drove
the handlers through their hotspots, which only exercises the ones whose
guards hold at the start of the game - almost none of them. `peekAlert`
returns immediately unless the alert is enabled and the unit carried, so
pointing it at a deliberately invalid icon changed nothing and the check
still reported a clean run.

Calling the handlers directly, with permissive state so they run their
bodies rather than returning at the guard, took the effects checked from
none that matter to 164.

That turned up something worth knowing rather than a bug. `natives::call`
dispatches by name across every chapter module, so Margaret's door static is
reachable from Roxy's rooms, where Roxy's presentation table has no such cast
and never will. A reference is therefore only reported when it fails in every
chapter it ran in.

With the control repeated on the corrected version, a wrong icon index shows
up as 24 dangling references and a right one as none. The check can fail,
which is the only reason its passing means anything.

## 46. The pools were never lists

Porting two haunt handlers turned up a fault under them both.

`trimState` is always called as `trimState( #hauntsRemaining, #gazebo2 )`: a
list and an item to take out of it. All 17 calls in the room scripts have that
shape, and so does every call in the bytecode. My implementation removed the
flag *named by the last argument*, so it deleted a flag called `gazebo2` and
left the haunt pool untouched. Nothing ever left the list.

Fixing that exposed the larger one. `#hauntsRemaining` was seeded as
`Symbol("knifeShadow")` rather than as a list, because seeding takes each
flag's first declared value and the schema writes a pool exactly as it writes
an enumeration of legal settings. Nothing in the schema distinguishes

  #hauntsRemaining : [#knifeShadow, #ghostBrushingHair, ...]     a pool
  #doorIsOpen : [0, 1]                                           the settings

so the shape has to come from use. A flag that has items trimmed from it, or
that is tested with `#includes` or `#lacks`, holds a list. Six do:
`hauntsRemaining`, `tunedIn`, `ghostsRemaining`, `panelGuess`,
`cameraFeedbackRemaining` and `utterancesRemaining`.

`ghostsRemaining` is the one that stings. The ghost telephone gates on it, and
when I ported that handler I wrote a fallback for the case where the list was
not yet seeded, treating every ghost as still present. That fallback was
running every time, and it was covering the bug rather than handling an edge
case. A defensive branch that is always taken is not defensive; it is the
whole behaviour, unexamined.

Both haunts now play once and leave the pool, and the house runs out of things
to do as the player sees them.

## 47. Effects were deferred, state was not

helba reported the haunts playing but never being seen: the pool shrank, the
haunt was consumed, and no movie appeared.

The haunt's movie is a room sprite gated on the haunt still being pending:

  margMir.mov   ch #video   [#includes: [#hauntsRemaining, #ghostBrushingHair]]

and the hotspot moves first, then triggers the haunt, so the movie belongs to
the room the player has just walked into. All of that was right. What was
wrong was mine: a handler's effects are queued and played back later, but its
state writes happened as it ran. So `testForMargGhost` queued the movie and
then trimmed the haunt immediately, and by the time the queue reached the
movie its guard was false.

The haunt was consumed without ever being shown, which is the worst version
of this failure: the pool shrank, so the state said it had happened.

`Effect::TrimState` and `Effect::SetState` put state writes on the same
timeline as everything else. Anything that must land between two waits is now
queued rather than written.

Checking the other ports for the same fault found it once more.
`backAwayFromLaptop` sets the montage to an intermediate value, holds a
second, then settles back; both writes were immediate and the wait between
them deferred, so it flipped straight through and the hold showed nothing.

Two ports, same mistake, and neither would have been caught by the reference
check from entry 45: every name resolved, every cast existed. The check
answers whether a handler points at real things, not whether it does them in
the right order.

## 48. The movie was drawing underneath

helba reported the lake haunt playing its sound but never appearing. The
guard was fine, the movie resolved, the engine reported it live. It was
drawing behind the room.

The room carries two full-scene plates on channels 1 and 2. `draw` composited
the movie first and the plates over it, so the haunt played every time,
underneath six hundred by three hundred pixels of boathouse.

The comment above that code was mine, and it asserted the rule as fact: the
movie sits behind the sprite channels, which is how the game frames video
inside static scenery. I wrote that while the intro was the only video room I
had tested, and the intro has no plates at all, so the ordering it needed was
unobservable there. One example, a confident comment, and every haunt in the
game invisible.

It also means the Margaret haunt from entry 47 was never fixed by that entry.
The trim ordering was a real bug and worth fixing, but the movie would still
have been hidden underneath the bureau. I reported it as fixed on the strength
of the guard being live, which is not the same as the picture being visible,
and helba had to come back twice.

The order is now plates, then the movie, then the script-controlled channels.

## 49. The two lake ghosts

`assertEdwinGhost` plays the second lake ghost, and its guard is the exact
complement of the one on the room's own `lakegst2` sprite:

  handler   fires when   lakeGhost2 pending and carrying the crowbar
                         and lakeGhost still pending
  sprite    shows when   lakeGhost2 pending and (no crowbar
                         or lakeGhost already gone)

Between them every case is covered and they never both fire. Reading the two
side by side is what made the handler safe to port: on its own the guard looks
arbitrary, and paired with the sprite it is obviously a partition.

The trim is queued rather than written, for the reason entry 47 established -
the movie is gated on the haunt still being in the pool, so consuming it as
the handler runs hides the film it belongs to.

## 50. The control panel, and addState

`panelButton` toggles a button in or out of `#panelGuess` and checks the set
after every press: all of A1, A2, B2 and B3 down, and neither A3 nor B1. A
wrong button resets nothing, it simply keeps the check from passing until it
is pressed again.

Porting it turned up a fault of the same shape as `trimState` in entry 46.
`addState( #panelGuess, #A1 )` adds to a set, and the interpreter was routing
it to a plain write, so the set held only whichever button was pressed last
and four-of-four could never be true. The panel would have looked like a
puzzle with no solution, which is exactly the failure that reads as the
player's problem rather than the engine's.

Both halves of the pair are now list operations. Worth noting the two were
written at different times and neither was checked against the other; a set
you can add to but not remove from, or the reverse, should have looked wrong
on its face.

Also mapped what is left, by the capability each handler needs rather than by
size:

  movie on a puppet channel     9 handlers, 24 sites
  puppet channel, bitmap only   2 handlers,  3 sites
  no channel work              23 handlers, 59 sites

Two thirds of the remainder needs nothing that does not already exist. The
blocked quarter is one capability again - a puppet channel that can hold a
digital video member rather than a bitmap - and it covers the whirligig, the
radio dial and the car.

## 51. Two more capabilities, named rather than worked around

`adjustLockSettings` stopped the batch. It scans the sprite channels to find
the digit it is adjusting, starts a timer, and then spins the wheel for as
long as `stillDown` says the button is held. This engine acts on the release
edge and has no notion of a button being held, so there is nothing to poll.

Rather than port a lock that turns one notch per click and call it done, I
counted what else wants the same thing:

  polls the held mouse    4 handlers, 16 sites
  does not               29 handlers, 64 sites

So the remaining 77 sites now have two named blockers rather than a vague
sense of difficulty: 16 want a held-mouse input model and 24 want a movie on
a puppet channel. Both are engine work of a few hours that unblocks a group,
and neither is discovered by porting handlers one at a time until one refuses.

`enterBubbleChamber` went in from the clear group. The descent is two montage
steps with the underwater loop brought up between them and faded out on
arrival, so the movement is carried by the sound rather than the picture -
which is worth knowing, because if the loop fails to resolve the scene reads
as a plain cut.

## 52. Held-mouse repeat

The dials spin for as long as `stillDown` reports the button held, polling
inside the handler. This engine acts on the release edge, so instead of
inverting that, a handler takes one step and sets `repeat_while_held`; the
front end re-runs the same action on an interval until the button comes up.
The first repeat waits longer than the rest, which is what the original does
with its lag timer: a click turns one notch and a hold spins.

That covers 16 call sites across the lock, the algorithm dials, the bar
settings and the car.

`adjustLockSettings` still is not ported, and the mechanism was not the last
thing it needed. Each wheel is `#lock_A`, `#lock_B`, `#lock_C`, and the schema
declares each with a single value rather than a range, so what a digit may be
is not stated where the other flags state it. The combination check is not in
Brice's room data either, so the test that opens the lock is somewhere I have
not found.

Building the input model and then not using it reads like wasted work, and it
is worth saying why it is not. The blocker was named from the outside, by
counting handlers that call `stillDown`; the digit range and the solve check
are only visible from inside the handler. Two different unknowns, and only one
of them was the one I had measured.

## 53. Tests, chosen from the bug history

helba asked for unit tests where they were needed. The useful question was not
where coverage was thin -- it was thin everywhere, seven tests in one crate --
but which failures had actually reached a player. The log above is a list of
them, so I used it as the specification.

Four bug classes, all in pure functions, none previously covered:

  - `TRUE` parsed as a symbol rather than 1, so the front door opened and
    could not be walked through.
  - A compound guard read with `as_list()` found neither clause and became a
    vacuously true `And([])`. Every locked thing in the game opened.
  - `trimState` deleted the flag named by its second argument instead of
    removing that item from the list the flag holds; `addState` overwrote
    instead of accumulating. The control panel had no solution.
  - `hit_test` broke ties by smallest area. Director takes the first match,
    so the porch sent the player into a lit house they had never lit.

That is 27 tests across `lingo`, `state` and `world`. Then the decoders, which
is where the two bugs I am least proud of lived: the reversed palette, and the
`snd ` samples read twelve bytes early. Both had survived checks at the time --
the palette because both of my implementations shared the assumption and the
frame I compared was nearly grey, the sound because all nine loops reported a
peak of exactly 32768 and I wrote a paragraph explaining why that was fine
rather than treating it as the alarm it was. A synthetic CLUT whose ends differ
catches the first in one line. A synthetic extended `snd ` catches the second.

Every test here was then checked by reverting the fix it guards and confirming
it goes red. A regression test that has only ever been green is a guess about
what the bug was; four of them proved they bite on the palette-order, guard,
list-op and sample-offset mutations. The `trimState` mutation also took down
`includes_and_lacks_read_pools` in `world`, which is the correct blast radius:
the pool guards read what the list ops write.

Writing them turned up one live bug. `unpack`, the PackBits decoder, clamps the
repeat branch to the declared pixel count and does not clamp the literal
branch:

    assert_eq!(unpack(&[0x05, 1, 2, 3, 4, 5, 6], 3), vec![1, 2, 3]);
    // left: [1, 2, 3, 4, 5, 6]

A `BITD` whose final literal run overruns its declared geometry returns a
buffer longer than width*height. Nothing had rendered visibly wrong, which is
why it survived: the extra pixels sit past the end of the last row and the
blitter indexes by geometry. It is still the same shape of defect as the
sample-offset bug -- trusting a length claim from one place while reading from
another -- and it would have sheared an image the moment anything downstream
sized itself from the buffer instead. Clamped, with the asymmetry noted in a
comment so the next reader sees the two branches agree.

44 tests, up from 7. The point is not the number. It is that each one names a
failure that actually happened, so the file reads as a list of things this
engine is now known not to do.

## 54. Every flag is a list

The lock was the errand; the state model was what it turned up.

`tryToOpenGrate` gave up the combination without a fight -- it builds
`list(getState(#lock_A), getState(#lock_B), getState(#lock_C))` and compares it
against `list(3, 2, 1)`, and `adjustLockSettings` checks the same three wheels
again on the way out. The digit range came from the arithmetic rather than from
any declaration: `(x + 11) mod 10` going up and `(x + 9) mod 10` going down, so
the wheels run 0-9 and wrap. The schema declares each wheel as `[6]` and says
nothing about a range, which is why entry 52 could not find one.

`op08` I checked before using it. Twenty-four uses across the four chapters,
every one with a small constant on the right, and several followed by `+ 1` --
the 1-based-index idiom, `getAt(list, (i mod 5) + 1)`. Integer division would
index off the end of a five-element list; modulo is forced.

Then the wheels did not draw. The sprite's `#castNum` is not a number:

```text
[#castName: "B_GZ_LOCK_A.frame", #castNum: [#lock_A, #lock_A_digits], ...]
```

A state flag and the name of a lookup table. Read as an integer it yields
nothing, and `cast_number > 0` filtered the sprite out. Fifty-eight sprites are
written this way, across three of the four chapters, and every one of them had
been invisible since the beginning: the lock wheels, Margaret's four clocks,
Roxy's entire bar-tuning panel, the AMBERVISION monitor, the nails in the
heart. Not one of them ever appeared in a screenshot I took, and I never
noticed, because a room that draws its backplate looks like a room.

The tables are `STXT` chunks. Each chapter ships two copies, one written
against cast names and one with the names already resolved to numbers, and the
resolved copy is the one worth having. My first recogniser demanded that every
top-level entry be a keyed list of integers, which threw away Roxy's chunk --
it mixes tables with plain frame lists -- and I had to widen it to collect
entry by entry, excluding room records and the schema by name first. Room
records are the awkward case: `#earShot: [#houseHum: 224, ...]` is a keyed list
of integers by any structural test.

Margaret's chunk failed for a different reason. Her clocks key on time of day,
`#t1`, `#t1.15`, `#t1.30`, and my parser stopped a symbol at the period. One
character, and every state-indexed sprite in the chapter lost its art.

Fifty-seven of the fifty-eight then resolved. The last was the bedroom radio,
whose `#tunedIn` I had classified as a pool because eleven rooms test it with
`#includes`. Seeded as a pool it holds a list, and a list indexes nothing.

Rather than guess again I read `updateDisplay` in AMBERHUB, which is where the
mechanism actually lives:

```text
assignedCast = getProp(sprite, #castNum)
if not integerp(assignedCast) then
  triggerVar = getAt(assignedCast, 1)
  frameStack = getaProp(oPuppeteer.frames, getAt(assignedCast, 2))
  if voidp(frameStack) then alert(...) : return
  if triggerVar = #AMBERVISION and getState(#AMBERVISION) <> #on then
    assignedCast = getaProp(frameStack, #off)
  else
    assignedCast = getaProp(frameStack, getState(triggerVar))
```

`table[state[flag]]`, which is what I had implemented, plus one special case
for the monitor being switched off. The original alerts when the lookup fails,
and the radio's guard is `#always`, so the original cannot be reaching that
lookup with a list in hand.

So I read the accessors, and the model is not what I built:

```text
on getState me, stateVar
  return getAt( getProp(me.states, stateVar), 1 )

on setState me, stateVar, suggestion
  valueList = getProp(me.states, stateVar)
  if count(valueList) > 1 then
    oldPos = getPos(valueList, suggestion)
    if oldPos then addAt(valueList, 1, suggestion)
                   deleteAt(valueList, oldPos + 1)
                   return #OK
    else return #badValue
  else
    return value("set" & stateVar & "(" & suggestion & ")")
```

Every flag holds a list. The head is the current value; the tail is the other
settings it may legally take. Writing one moves it to the front -- nothing is
replaced, nothing is lost -- and a value the list does not already hold is
refused outright. What I had modelled as three kinds of flag is one kind seen
from three angles: a scalar is a one-element list, an enumeration is a list
whose head is the current choice, and a pool is a list nothing reads the head
of. `#tunedIn` is all three at once, which is exactly why guessing failed.

`derive_list_flags` is gone. It existed to decide which flags "were lists", a
question with no answer because they all are. The heuristic had been wrong in
both directions and I had already patched it once this session, tightening it
from `#includes` usage to mutation -- which was a better guess and still a
guess. Fifty-eight of fifty-eight resolve now.

Two smaller things fell out of the same reading. `addState`, `trimState` and
`inState` are `append`, `deleteAt` and `getPos <> 0` on that list, which is
what entries 50 and 53 had arrived at from the failure and then from a test;
seeing them in the bytecode is the first time they have been confirmed rather
than inferred. And the `count == 1` branch of `setState` calls
`set<StateVar>(suggestion)` -- the ninety-two single-valued flags are not
values at all, they are declarations that a custom setter exists, which is what
the whole `setBalconyDoorIsOpen` / `setBarMode` / `setGrateIsOpen` family is
for. Forty-six of them I have already ported one at a time without knowing they
were a family.

One deliberate divergence. The original answers `#badValue` and leaves the flag
alone when a write is not in its declared list; I insert at the front instead.
A write this engine fails to recognise would otherwise freeze whatever it
gates, and a room the player cannot leave is a worse failure than a flag with
one extra setting.

The lock itself is now the smallest part of it. `adjustLockSettings` writes the
wheel and the art follows, because the sprite reads the flag; the original only
touches the sprite directly to show the motion-blur frame between two
settings, which this engine does not need. It is also the first user of the
held-mouse repeat built in entry 52 -- a click turns one notch, a hold spins.
Dialling 3-2-1 and clicking the lock opens the grate and puts the player on
`Gaz_TrapdoorCU` with the way to the shrine open.

Two of the three tools I reached for lied to me on the way, both in the same
way: `tools/disasm.py` overwrites `sys.argv` at import, and my first table
survey crashed on movies with no `Lnam` chunk. Neither is interesting except
that both failed loudly, which is the only reason this entry is not wrong.

## 55. The setter family, and a table instead of twenty-five ports

Entry 54 ended with the observation that `setState` on a single-valued flag
dispatches to `set<Flag>(suggestion)`, and that the `setGrateIsOpen` family is
what that mechanism is for. This is that lead followed.

Fifty-three setters exist across the four movies. One was ported. Sorting them
by size made the shape obvious before reading any of them: twenty-four sat at
exactly the same instruction count, which is not a coincidence anyone arranges
by hand.

The template:

```text
on set<X>IsOpen suggestion
  currentState = getState( #X )
  if suggestion = 0 and currentState = 1 then
    cue( #<thing>Close )
    setProp( oStoryteller.states, #X, list(0) )
    updateDisplay( oPuppeteer )
  if suggestion = 1 and currentState = 0 then
    cue( #<thing>Open )
    setProp( oStoryteller.states, #X, list(1) )
    updateDisplay( oPuppeteer )
```

Both arms are guarded on the flag actually changing, so setting a door open
when it is already open does nothing at all -- no sound, no redraw, no write.
That is the property that makes these safe to call from anywhere, and it is the
reason the write has to go through the setter rather than to the flag: the
guard lives in the setter, not at the call site.

`op03` fell out on the way. It appears where a zero belongs in five unrelated
places -- `suggestion = 0` here, `getPos(...) = 0` in `inState`, and three
locals initialised in `adjustLockSettings` -- so it pushes zero.

I did not assume the cue from the handler's name, and it is as well. Twenty-five
handlers match the template and they differ in which sound they play, so the
port is a table of `(chapter, handler, opening cue, closing cue)` read out of
the bytecode. Two entries would have been wrong by any naming rule: Margaret's
boathouse door and mailbox are *cabinets*, and Brice's boathouse door plays the
**grate** -- the authors copied `setGrateIsOpen` and never changed the sound.
Reproducing that is the point. It is what the disc does.

The same flag name means different sounds in different chapters, so a handler
now has to know which chapter it is running in. `seed_chapter` writes `gChapter`
alongside the schema, which is the smallest thing that works and is honest
about being a lookup key rather than game state.

The remaining twenty-eight setters are not the template. They range from four
instructions to a hundred and sixty, and the large ones branch on where the
player is standing: Roxy's kitchen cabinet plays one of four sounds depending
on which door of it is in view. Those stay individual ports.

One bug of my own, caught by the flag's own shape. After wiring the dispatch,
the bathroom door opened and the exit behind it appeared -- but the flag read
`[1, 0]` rather than `[1]`. Two elements is the signature of the fallback path:
my generic setter replaces the list, `setState`'s default write inserts at the
head. So the dispatch had missed and the write had happened anyway. The handler
name is built from the flag and the dispatcher matches lower case, and
`setbathroomDoorIsOpen` is not `setbathroomdoorisopen`. Worth recording that
the thing which gave it away was not the door -- the door worked -- but a list
one element longer than it should have been.

Fifty-four tests. The new ones cover the cue table by chapter, the
does-nothing-when-unchanged guard, and a chapter declining a setter it does not
have, since answering with another chapter's sound is the failure the table
invites.

## 56. Doors that are heard from the next room, and a symbol I misread

The remaining setters, ranked by how often the room data calls them, put three
at the top that are the plain template plus one thing: an ambience loop. The
front door, the kitchen's rear door and the balcony door each write their flag
and cue their sound like the other twenty-five, and then start or stop a loop --
but only if the player is somewhere it would carry to:

```text
if suggestion = 1 and currentState = 0 then
  cue( #frontDoorOpen ) : setProp( #FrontDoorIsOpen, list(1) ) : updateDisplay
  if currentRoom = #DarkDn or currentRoom = #Hall then setLoop( #grounds, #disablePeekAlert )
  if currentRoom = #Porch                          then setLoop( #houseHum, 80 )
```

Open the front door from the hall and the grounds become audible; stand on the
porch and it is the house you start to hear. Shut it and that stops. Fifty-eight
call sites between the three.

I got `currentRoom` wrong first. `#Porch` and `#DarkDn` read like room names, so
I wired the rules to `#currentLocation` -- and then `Porch` resolved to no room
at all, because the rooms are `Porch_A_E`, `PorchFrontDoor`, `PorchDoorCU`.
These are areas, not rooms.

Chasing that had me resolve the handler's property operand against the movie's
name table, which returned `gHotSublist`. That is not a current room by any
reading, and it is worth saying plainly why: my disassembler prints those
operands raw *because* the mapping is not established, and I read one anyway. A
tool that says "op61 55" is telling me it does not know. Treating its operand as
a name-table index was me supplying certainty the tool had explicitly withheld.

The answer was in data I already parse. The location table is keyed by area
before it is keyed by room:

```text
[#office: [#OfficeEntry2: [145, 1, 1089], #OfficeExit: [170, 1091, 2096], ...],
 #Porch:  [#PorchFrontDoor: [...], #PorchDoorCU: [...], ...], ...]
```

`LocationTable` was throwing that key away -- `for (_, rooms) in &areas`. It now
keeps it, `Node` carries a zone, and the door rules compare against that.

Two flags fell out of the same work, both of which should have existed already.
`#currentLocation` is a flag the game reads like any other and nothing was
keeping it current: it held whatever the chapter had been seeded with. Every
room change now goes through one `move_to` that writes it, along with the area
as `gZone`. `tryToOpenGrate` reads `#currentLocation` too -- it only moves the
player to the trapdoor if they are not already standing at it, so that the lock
can be worked from a close-up without the room restarting -- and my port from
entry 54 had ignored that check because the flag was not worth reading yet.

The kitchen's rear door has a third rule the other two lack: the scanner is
heard through it, but only when the unit is mounted on that door and switched
on. That is two extra conditions on one loop, so the table carries guards.

Sixty-one tests. The new ones cover each loop by area, the case where the
player is nowhere near the door (the cue still plays -- it is a setter, not a
room -- but nothing about the ambience changes), the scanner's two guards, and
Margaret declining rules that are Roxy's.

Still to do from this family: the one-of-many openables, where the flag holds
*which* door of a cabinet is open rather than whether it is. Roxy's kitchen
cabinet is the biggest single caller left at sixty-one sites, and it has an
asymmetry I want to keep -- the trash can closes with the cabinet sound but
opens with the drawer one, and it opens even when something else is already
open.

## 57. Crunchy audio: no headroom, and ducking the wrong thing

helba reported voices sounding crunchy, and then described the shape of it
exactly: "there's the house hum then the rest on top of it". That is a mixing
fault, not a decoder fault, and saying so saved me from a long detour into the
IMA ADPCM decoder I had already opened.

Measured rather than guessed. A room declares its bed as `#earShot`, levels out
of 255, and the living room asks for a clock at 47%, a radio at 47%, a fire at
100% and the house hum at 88%. That is 2.82 times full scale, and the sources
are real recordings -- the hum alone peaks at 31488 of 32768. Across the game,
**101 rooms ask for more than full scale**. The mixer summed them and finished
with `clamp(-1.0, 1.0)`, so those rooms hard-clipped continuously, before a
single line of speech was added on top. Squared-off peaks are the crunch, and
speech suffers worst because its peaks are frequent and short.

`#earShot` is a balance, not a set of absolute levels: it says how prominent
each source is from where the player is standing. So the bed is scaled only
when it would not fit, which keeps the balance exactly as written and removes
the clipping. The ceiling sits at 0.7 rather than 1.0 because speech and
effects play over this. Nothing below the ceiling is touched.

The final clamp became a saturator with a knee at 0.7: linear below it, bending
smoothly above and approaching full scale without crossing. Anything that still
overshoots now compresses instead of squaring off.

helba then asked whether the hum should step back when other audio plays, which
it should, and the game says so itself: the scripts call `suspendSounds` before
a set piece and `restoreSounds` after, twenty times. I had wired those to the
**master**, which pulled down the set piece along with the background -- the one
sound the call exists to make audible. That now holds the bed down and leaves
everything else alone, and the same duck applies automatically whenever a
one-shot is playing, ramped over about forty milliseconds so it does not click.

Six tests on the saturator and the duck, including one that the curve is
monotone: a saturator that turns back on itself inverts loud peaks, which
sounds worse than the clipping it replaced.

The second half of the report -- effects missing in some rooms -- is not the
mixer. Four sounds the rooms ask for do not resolve at all: `BRclock`,
`DRclock`, `Kclock`, `LRclock`. They are not missing assets. Margaret's chapter
has an entire clock subsystem behind them -- `moveClock`, `touchClock`,
`radioDial`, `prodVLoops`, `fadeUpRadio`, `backAwayFromRadio`, and the flags
`#clockPuzzleActivated`, `#clockPuzzleFrustration`, `#mostRecentClock`,
`#theseClocks` -- none of which is ported. The ticking is driven from there.
That is a set piece to port, not a bug to fix, and it is the largest unported
one I have found.

## 58. An event log, and a measurement that was nearly wrong

helba asked for a way to see what the engine is doing. The case for it is the
list above: nearly every bug found by playing this thing has been the engine
doing something reasonable for a reason invisible from outside. A guard that
read as vacuously true. A sprite filtered out before it drew. A setter that
missed its dispatch and let the fallback write happen anyway -- caught only
because a flag's list was one element longer than it should have been. Each
time the fix was quick and the finding was slow, and each time the finding
started with adding an `eprintln!` and building again.

So: `AMBER_TRACE=room,script,state,sprite,audio,video`, or `all`, to stderr or
to `AMBER_TRACE_FILE`. Records carry the frame and the room, because what was
on screen when it happened is the first question every time. Naming a topic
that does not exist prints the ones that do, because silently tracing nothing
is the worst possible answer to a typo in a debugging session.

Two details worth keeping. The first record it produced showed
`setfrontdoorisopen` firing three times for one click, which looked like a bug
and is not: the walkthrough lists a room's exits by running each hotspot's
actions against a copy of the state, and `verify` sweeps the whole game the
same way. Those runs call handlers and write flags exactly as a real click
does. They are now marked with a leading `~`, because a log that cannot tell a
speculative run from a real one is worse than no log. The second is that
seeding a chapter writes several hundred flags at once, which buries everything
anyone is reading the log for, so the schema announces a count instead.

Then it caught something on its first real outing. Brice seeds thirty-two
flags; Roxy a hundred. In entry 55 I had put the split at ninety-two
single-valued against fifteen hundred multi-valued, and fifteen hundred is
wrong by an order of magnitude -- that number came from a regex sweeping the
whole movie file and counting every `#name: [...]` it found, in room records
and cast tables alike, not just the schema. The real figures are 92 and 115.

The ninety-two is right, which is the part worth being uncomfortable about. The
number I quoted and the number I checked happened to agree on the half that
mattered, so the conclusion built on it -- that single-valued flags declare a
custom setter -- stands. It stands by luck. Had the error fallen the other way
I would have written an entry off a measurement I never questioned, because it
confirmed something I already believed.

Only the correct figure had reached the log; the bad one existed in a message.
That is a thin margin, and the lesson is the same one as the palette: a number
that agrees with what I expect gets no scrutiny, and those are exactly the ones
that need it.

## 59. The effects queue was quadratic

helba said the audio was still crunchy and pasted the effect log, which is the
whole finding:

```text
  effect: PlayVideo(None)
  effect: WaitForVideo
  effect: PlayVideo(None)
  effect: PlayVideo(None)
  effect: WaitForVideo
  effect: PlaySound { name: "MCALL7", loudness: Some("high") }
  effect: PlaySound { name: "MCALL7", loudness: Some("high") }
```

The same sound twice. The handler that produces it picks a call at random and
pushes one effect, and two runs of it would roll different numbers, so this was
not the handler running twice -- it was one effect list applied more than once.

In `pump`:

```rust
merge(&mut combined, outcome);
self.apply(&combined.clone());
```

`apply` queues `outcome.effects`, and it was being handed `combined`, the
running accumulation, once per action. So a list of n effect-producing actions
queued n(n+1)/2 effects. The log I built in entry 58 reported it directly:

```text
queue 1 effect(s), 1 pending
queue 2 effect(s), 3 pending
queue 3 effect(s), 6 pending
queue 4 effect(s), 10 pending
queue 5 effect(s), 15 pending
```

Triangular numbers, on the path into the bee grounds. After the fix, six
actions queue six effects.

This is worse for audio than the headroom problem in entry 57, and in a way
worth stating precisely: two copies of the *same* recording started at the same
moment are perfectly correlated, so they sum to twice the amplitude. Two
unrelated sounds of the same level sum to about 1.4 times. Entry 57 fixed a bed
that was too loud; this was one waveform on top of itself.

Belt and braces, because the game can legitimately ask for a sound it is
already playing: Director plays a sound on a channel, and asking that channel
for it again restarts it rather than layering a second copy. One-shots now do
that. I nearly broke a programme doing it -- a radio or clock programme's takes
are distinct recordings played in turn, and my first pass named them all
`(programme)`, which would have folded each take into the last and played only
the first. Only named script sounds fold; a programme's takes and a movie's
soundtrack are deliberately anonymous.

The `PlayVideo(None)` in helba's paste turned out to be a second bug hiding in
the first. `pushVideo` with no argument means the movie the room already places
on its video channel, which is right -- but `Effect::PlayVideo` was emitted in
five places and **handled in none**. It fell through the match and was
discarded. Every montage that plays through `pushVideo` showed nothing, and the
`wait #videoStop` after it resolved against whatever movie happened to be
loaded, or immediately if there was none. It is wired up now.

Both of these had been in the engine for as long as the queue has existed. The
event log found them in one run, which is the whole argument for entry 58: the
duplication was visible in the effect list all along, and I had been reading
that list for weeks.

## 60. Four bugs in one log

helba pasted a trace of the bathroom mirror haunt. Every line of it was a
finding.

**`scanLoop -0%`, playing at `gain -0.00`.** A negative gain inverts a
waveform. The room's sound sprite declares `#earShot: -1`, and I had been
reading that as a level. `updateDisplay` says otherwise:

```text
sndVolume = getProp(sprite, #earShot)
if sndVolume < 0 then endLoop( value(getProp(sprite, #castName)) )
else                  setLoop( value(getProp(sprite, #castName)), sndVolume )
```

A negative `#earShot` is an instruction to *stop* that loop, which fifty-six
sprites use to silence something a room should not carry. I had also had the
field name wrong in my head for weeks: sound sprites carry `#earShot`, never
`#volume`, in all two hundred and fifty of them.

**The video drew at the wrong depth.** Three fixed layers -- plates, then the
movie, then the puppet channels -- which is right only until a script claims a
channel the movie should sit above. The game supplies the real numbers and I
had never gone looking for them. A room's `#channel: N` is an offset from
`lastScoreSprite`, which `setUpGame` sets to 12, so its channels 1-10 are
really 13-22. Movies live on 44 and 45: `refreshVidSprites` forces QuickTime to
redraw by flickering the visibility of exactly those two sprites. Puppets take
whatever channel they name, 30, 39 and 44 being the ones the game claims. The
stage is now one list sorted by channel, and the movie is at 44 because that is
where it is, not because a comment said films go on top.

**Every wait inside a handler was ignored.** The mirror message is six effects:
`cursorOff`, `suspendSounds`, `pushVideo`, `wait #videoStop`, `restoreSounds`,
`trimState`. `pump` honours a wait *between* actions, but a native handler
emits its whole sequence as one action, and the render loop drained the queue
in a single pass. So the ambience was suspended and restored in the same
instant, and the film played with nothing sequenced around it. The queue now
stops at the first wait and is pumped every frame rather than only in the frame
a click arrives. That is why the message did not read as a scene.

**And a silent failure of my own.** Entry 58's sprite tracing never took: the
replacement I wrote did not match, `str.replace` returns the string unchanged
when it finds nothing, and I did not check. The old `AMBER_TRACE_SPRITES`
block sat there through two commits while I believed it was gone. Every
scripted edit since has an assertion on it now.

Three of these four had been in the engine since the queue existed. helba's
log showed all of them in one run, which is twice now that the event log has
paid for itself in a single paste.

## 61. Half the video never played

helba reported the bathroom mirror as no video and audio "like my speakers are
dying". Both were true, and both were the same mistake in two places.

The probe told me in one line:

```text
Video codec=rle  92x148 samples=117
Sound codec=raw  rate=22254 ch=1
audio: 326464 samples, peak 32768, 326463 non-silent
```

The player handed every soundtrack to the IMA ADPCM decoder and every frame to
Cinepak, whatever the file said. `MIRROR.MOV` is neither.

`raw ` is unsigned eight-bit, silence at 128. Read as signed that is -32768,
full scale negative, on every sample that is not making a sound. The peak of
*exactly* 32768 with 326463 of 326464 samples non-silent is the signature, and
I have seen that number before: entry 57's `snd ` bug reported the same peak
for the same reason. Twice now, and the second time I did not recognise it.
Decoded properly the track peaks at 15104 with half its samples silent, which
is what speech looks like.

The counts across the disc:

```text
video: cvid 144, rle 133, smc 1
audio: ima4  66, raw  16, twos 1
```

**A hundred and thirty-three of two hundred and seventy-eight movies are Apple
Animation, and not one of them had ever drawn a pixel.** A decoder handed the
wrong format does not report anything; it produces a black rectangle, and a
black rectangle in a dark bathroom mirror looks like a mirror. That is the
whole reason this survived: the failure was indistinguishable from the art.

The RLE decoder took two corrections, both mine.

At eight bits a count is *four* pixels, not one, and the skip byte steps four
at a time. That I read from the reference before writing it.

The second I got wrong and the compiler told me. `-1` ends a *line*, not the
frame -- the outer loop moves on to the next one. I had written `return`, which
made the `row += 1` after the loop unreachable, and rather than read the warning
I put `#[allow(unreachable_code)]` on it. That is silencing a diagnostic that
was correctly describing a bug I had just written. Removed, fixed, and there is
a test that fails on the mutation.

The palette needed a third correction. The colour table is written inline after
the eighty-six byte sample description, and the documented signal for that is a
table id of -1. These files leave the id at zero and write the table anyway.
The size is what tells you: 2142 bytes where the fixed part is 86, and 2142
minus 86 is exactly an eight byte header and 256 eight byte entries. Gating on
the id found no table, every index mapped to black, and the decoder looked
broken when it was working.

Nineteen tests over the two decoders. The one that matters is
`minus_one_ends_the_line_and_not_the_frame`, because that bug decodes the first
changed line and leaves the rest of the picture standing -- on a mostly-still
film, almost invisible unless you are looking for it.

## 62. The door tool

helba asked why the scan unit's video and scripts did nothing. The video was
entry 61 -- it is an Apple Animation movie like a hundred and thirty-two
others. The scripts were simply not ported, and they were the two largest
callers left: `setDoorWithScanUnit` at twenty-nine sites and `setPKScanStatus`
at thirty-six.

`setDoorWithScanUnit` is a list of the eleven knobs the unit may be clipped to
and two cues, and the cues are the interesting part. Both are guarded on
crossing to or from `#None`, so moving the unit straight from one door to
another makes no sound at all. It is one click, not an unclip and a clip, and
a port that played both would be wrong in a way nobody could point at.

`setPKScanStatus` is the unit's state machine, and it does one thing worth
naming. It rewrites its own argument twice:

```text
if suggestion = #Online then
  if currentStatus is one of the #Wait states then
    gScanFinish = 0 : suggestion = #Interrupted
  if currentStatus = #ReadyForPlayback then
    suggestion = #ReadyForPlayback
```

Asking to go online during a countdown interrupts that scan rather than
restarting it, and asking for anything while a result is waiting keeps the
result. Between them the player cannot lose a finished scan by fiddling with
the unit, which is the only thing here that would make the puzzle unfair.

Two of my own bugs fell out of writing it. `setScanTime` -- ported back in the
run of small handlers -- set `#PKscanStatus` to `#Scanning`, which is not one of
the twelve statuses the unit accepts. Nothing checked, because nothing existed
to check it; now `setPKScanStatus` refuses it outright. The original builds the
symbol from its argument, `#Wait5min` and `#goodScan5min`, and I had guessed a
name rather than reading the two literal fragments it concatenates.

The second is worse. `setScanTime` computes a deadline as `the ticks + minutes
* 3600`, and I read `the ticks` from a flag called `gTicks` that nothing ever
wrote. It was zero for the whole session, so every deadline the scan unit set
was already in the past. The render loop now advances it, sixtieths since
startup, as Director does.

A hundred tests. The one I would keep if I could keep only one is
`a_finished_scan_survives_being_fiddled_with`, because that is a rule about
fairness rather than about a format, and those are the ones a port loses
silently.

## 63. Scenery that stopped after one pass

helba: the door scanner's animation played once and stopped, the ceiling fan
did not loop, and was the scanner's button gated on house power.

It is not gated on house power. It is gated on `#scanUnitIsActive`, and the ON
and OFF hotspots share one rectangle -- the same physical indicator at the top
of the unit -- with opposite guards. Taking the unit off the door wants that
flag at 0 too, so the button has to be pressed once before the unit will come
away. Driving it from the walkthrough, `[1, 0]` to `[0, 1]` and back, both arms
fire. The scripts were fine.

What was wrong was that the animation stopped, which made a working unit look
dead. Director keeps a QuickTime sprite running for as long as the frame holds
it, and my player held the last frame instead. So the fan turned once, the
scanner swept once, and the monitors froze.

The distinction that decides it is in the room, not in the movie. A movie over
a scene is scenery -- the fan, the scan unit's dial, a monitor -- and runs for
as long as the player stands there. A room carried *entirely* by its movie is
the opening, a montage, an ending, and those play once and hold. So the loop
follows from whether the room draws anything else, and a movie a script is
waiting on with `wait #videoStop` is taken out of the loop when the wait arms,
or the wait would never clear.

The other half was that the scanner was playing the wrong film, or rather
playing one by luck. Its video sprite names its cast the way a plate does:

```text
#castName: "SC_PATIO.multiframe", #castNum: [#AMBERVISION, #QTsc_patio]
```

Twenty-eight video sprites are written that way and I had been reading the
`#castName` -- which is a placeholder, and the reason for the `.multiframe`
suffix I could never account for. The table resolves the monitor's state to a
cast member, and for a monitor that is off that is a dummy parked off stage
rather than the film. The same room has a second video sprite holding the real
movie at x=788, off the side of a 640 wide stage, which is the same trick from
the other direction.

Entry 54's reading of `updateDisplay` had a special case I noted and did not
implement: a sprite keyed on `#AMBERVISION` shows its `#off` entry for every
state except `#on`. That is in now, for plates and for movies both.

A check that found nothing, recorded because a negative is worth as much: Mac
`snd ` resources carry loop points, and I had been discarding them. If an
ambience declared a sustain inside a longer recording, looping the whole buffer
would replay its lead every lap. None of the twenty-seven sounds in the movies
declares one, so the house hum repeating every 3.2 seconds is the asset being
3.2 seconds long, not a seam I have got wrong.

## 64. Recording a route, and the film flag

helba asked for a way to record a session in the window and replay it in the
walkthrough, so a fault could be handed over rather than described. That is the
slow half of every bug this session -- play until it breaks, describe where, and
I try to reach the same state from the terminal -- and it is now one file:

```text
AMBER_RECORD=/tmp/run.walk amber play extract
AMBER_TRACE=all amber walk extract --replay /tmp/run.walk
```

Two things had to be true for a replay to mean anything. The walkthrough needed
the commands the window generates, so `inv x y` and `skip` are commands now.
And it had to run the effect queue: `trimState` and `setState` reach the flags
through it, and the walkthrough never drained it, so a route replayed in the
terminal ended somewhere the same route in the window did not. It settles the
queue after every step, dropping the waits -- they are pacing, and there is no
clock to pace against -- and applying the state.

Then helba asked the right question about entry 63: how does the game know
which films loop? My answer there was a heuristic -- loop if the room draws
anything else -- and a heuristic was the wrong shape of answer. It was also
actively broken: it deadlocked the breaker switch. `pushVideo` is applied
*after* the wait that follows it is armed, so my "stop looping while something
waits" landed on the previous film, the new one came up looping, and
`wait #videoStop` never cleared. The sequence stopped for good, part way
through, with the lights half thrown.

The flag is on the cast member. Director stores it there rather than on the
sprite, which is why nothing in a room's record says which kind of film it has.
Dumping the type-specific bytes of every digital video member and comparing
them across the disc, four values occur -- 0x22, 0x2a, 0x32, 0x3a -- and bit
0x10 is set on exactly the films that should run: the ceiling fans, the door
scanners, the fireplaces, the bubbling. Clear on the opening, the montages and
the mirror message. Ninety-one loop and a hundred and ninety-four do not.

Compare that to what I had guessed. The heuristic and the flag agree on the fan
and disagree on anything a script plays over a scene, which is most of the
haunts.

## 65. Four hundred and sixty-six unreachable regions

helba could not use the scanner on a door knob. The room has an `itemInUse`
region over the knob at rect(228, 128, 396, 309), and a `#pointer` region that
walks through the door at rect(226, 56, 624, 364) -- which contains it. My
hit test ranks `#pointer` above `#itemInUse`, so the click walked the player
away rather than using what they were carrying.

That is not a one-room accident. Four hundred and sixty-six of the game's eight
hundred `itemInUse` regions have their centre inside a higher-priority region,
so **most of the game's item targets could never fire**. It is the sort of
fault that reads as "the puzzle is obscure" rather than as a bug, which is why
it lasted this long.

An item in hand outranks everything. That is what a cursor holding an object
means, and it is why these regions are drawn over navigation in the first
place. The condition matters as much as the ordering: six hundred and
eighty-nine of them are guarded on what is in hand and gate themselves, but
eighty-one are guarded only on `#always` and would fire with empty hands.

## 66. Four channels, and two ghosts talking over each other

helba's recording made this one quick, which is the point of entry 64. The
route: in through the front door, up to the office, throw the breaker, back
along the hall to Margaret's locked door. Replaying it printed the fault on the
second-to-last line.

```text
> click 576 292
  sound: play MCALL7 (high)
> click 223 304
  sound: play MCALL1 (high)
```

Two ghost calls, one room apart, both at full volume, both several seconds
long. The original does not do that. `ghostCalls` walks the sound channels for
the one the last call used, asks `soundBusy` whether it is still running, and
gives up if it is. Two ghosts never speak at once however often the room asks.
Mine started every call it was handed, so they piled up on each other, and
overlapping speech at full scale is not speech.

The wider version of the same fault: every chapter's schema declares
`#soundChannels` with **exactly four**, loops and effects share them, and
`soundEffect` gives up when none is free rather than finding room. My mixer had
no limit at all. Each voice is quiet enough alone; eight at once is not, and
that is what "the audio sounds fucked" was.

Three smaller things came out of chasing it.

The recorder helba could not get a file out of was reachable only through the
environment. It is `--record <file>` on `play` now, because that is where
anyone would look.

Replaying the route in the walkthrough stopped at the breaker switch, because a
sequence that holds leaves its remaining actions queued and only the window
pumps them. The walkthrough runs them out now, so the lights actually come on
in a replay -- otherwise every route through that switch diverged from the
window at exactly the point it mattered.

And `sfx` was lying. It resolves a sound in the current chapter, so run from a
Roxy start it reported `roaringFire`, `outsideLoop`, `iceHole` and five others
as unresolved. Each is requested only by its own chapter and each resolves
there, so nothing was broken -- but a diagnostic that reports false failures is
worse than none, which is the same lesson as the speculative marker in entry
58. Sound lookup now falls back across chapters and the tool tells the truth.

One check that found nothing, worth recording. `gazeboWind` peaks at exactly
32768, which is the signature that has now caught two real bugs. It has four
samples there out of five hundred and forty-five thousand. It is a hot
recording, not a misread, and `sfx` prints the count now so the two never have
to be guessed between again.

## 67. The telephone

helba: nothing happens when the phone is opened. Three handlers gated it, and
the trace named all three in one line each -- `setPlayerIsExaminingPhone`,
`setGhostlyPhoneCall`, `putDownThePhone`, none ported.

`setGhostlyPhoneCall` is the piece worth having. Lifting the receiver asks it
to go to `#speaking`, and what the player hears depends on where they are in
the chapter:

  - the psionic waves are present and the phone message is still pending:
    Roxy's own message plays, and the branch **rewrites its own argument** to
    `#done`, so the call hangs up by itself. It consumes both phone haunts at
    once and switches the monitor on. This is the call that moves the chapter
    forward; the others are atmosphere.
  - the spooky operator is pending and the buttons have been pressed more than
    six times: the operator speaks, is used up, the count is cleared and the
    line goes dead.
  - the message is still pending but the waves are not: a dead line.
  - nothing left to hear: Roxy's call-done tone.

The six is the puzzle, and `putDownThePhone` is where it is set: pressing the
buttons *at all* and then hanging up forces the count to seven, which is the
one number above the threshold. So the answer is not a combination, it is the
gesture -- dial anything, put the phone down, and the operator answers. The
count is cleared either way, so a failed attempt is free but has to be made
again from the start.

Then it did not work, and the reason is a good one. The buttons increment
through the only computed action in the game:

```text
setProp( the lsStateData of oStoryteller, #phoneButtonsPressed,
         [getState( oStoryteller, #phoneButtonsPressed ) + 1 ] )
```

`eval_arg` resolves a nested `getState` but not a sum, so the flag took the
*text of the expression* as its value. The count never rose above six and the
operator could never answer. One site in the whole game -- I counted before
writing anything, because one site is not a reason to build an expression
evaluator, though it is a reason to read a sum.

Handlers also need the room's own mix now: the phone rings at whatever level
the room says it carries from there, loud in the living room and faint
upstairs. That is `#earShot` again, which the engine had been keeping to
itself; it goes into the state as `gEarShot_<source>` on every move, beside the
room and the area.

Eight tests. The one I would keep is
`the_operator_answers_only_after_the_buttons_and_hanging_up`, because it is the
puzzle rather than the plumbing, and a port that quietly loses a puzzle looks
exactly like a port that works.

## 68. Cabinets with more than one door, and a jump that had stopped working

The unported list, ranked by call sites, put `setShowMontage` at the top with
thirty-one. There is no such handler. `#showMontage` is a single-valued flag
with no custom setter, so the plain write *is* the whole behaviour, and my
trace was calling that "not ported" -- overstating the gap by half. About half
the single-valued flags have a setter and the rest do not, and the engine has
no way to tell which, so the line says "no set<Flag> ran" now and implies
nothing.

The two real ones left were the cabinets, which are not booleans.
`#kitchenCabinetIsOpen` holds `#upperLeft`, `#drawer`, `#trashCan`, `#None`,
and the sound depends on which: seven cupboard doors share a cue, the cutlery
drawer has its own, the bin has its own again.

The bin is the part worth having. It **closes with the cupboard sound and opens
with the drawer one**, and it is the only member that opens while another is
already open -- the third arm of the handler has no guard on the current state
where the other two do. A tidier port would sound wrong in a way nobody could
name, and would let a second cupboard door open over the first.

Then helba could not jump to a chapter any more. `play <dir> MARGARET` resolves
its argument as a *room*, and a chapter is not one, so it warned and started at
the default. Worse, the room path wrote `game.room` directly, which skips
seeding: jumping to a room in another chapter left that chapter's flags
unwritten, so every guard there read against a void. That is the same failure
as entry 54 in a place I had not looked, and the fix is the one that already
exists everywhere else -- seed, then move. A chapter name now goes to that
chapter's own opening, and the walkthrough takes one too, so a route can start
wherever the fault is.

Six tests on the cabinets. `the_bin_opens_over_an_open_cupboard_and_nothing_else_does`
is the one that carries the finding.

## 69. A chapter that opened on nothing

helba jumped to Margaret and got a black screen. The chapter's schema declares
its start as `#bedrm_fadeIn`, and that record is scaffolding the authors left
in: its only sprite is a seventeen by three palette holder, its exits go to a
literal `#destination`, and the film it plays is `40sINTRO.mov`, which is one
of seven digital video members on the disc with no file behind it. Two hundred
and seventy-two have one. Entering there is a black screen with no way out, in
this engine and in the original.

So a declared start has to be checked rather than trusted, and the interesting
part was getting the check right. I tried three.

**Does it draw anything?** No: the palette holder draws, and seventeen by three
pixels is drawing by any measure and a scene by none.

**Does it have a live exit?** No, and this one is worse, because it rejected
Brice's opening as well. That chapter opens on a montage of close-ups gated on
`#showMontage`, and every exit is blocked until the montage has played. A test
that asks what is reachable *now* throws away a perfectly good opening.

**Do its exits name a room that exists?** Yes. Guards ignored, destinations
resolved. Brice's montage names real rooms it cannot reach yet; the template
names `#destination`, which is a word rather than a place. Together with "or it
has a film that plays", that keeps Roxy's video-only opening, keeps Brice's
montage, keeps Edwin, and steps Margaret past the scaffolding to `bedrm_A1` --
the forties bedroom, which is where the chapter is actually set.

Worth naming that the first two attempts were both heuristics about *how much*
was there, and the one that works is structural: not how big the sprite is or
what is reachable this instant, but whether the exits refer to anything. The
same shape of mistake as the movie-loop heuristic in entry 63, caught two
attempts earlier this time.

`verify` prints all four entries now, because the rule that skips a dead start
is also the rule that could skip a live one.

## 70. Five music boxes and one film

helba's `mar.walk` ended at Margaret's dresser with the boxes silent. Two
handlers gated it -- `setOpenBox` and `resetBoxPuzzle` -- and between them they
are the whole puzzle.

Five boxes on a dresser, opened by clicking. `#boxList` keeps the last five
opened and no more: the count is trimmed from the front before each append. So
there is no wrong move to undo and no reason to leave and come back. Play them
in order and the fifth press completes the sequence whatever came before it,
which is a kinder puzzle than it first looks.

The performances are all in one film. `BOXPLAY.MOV` is forty frames with five
keyframes, and each box plays its own stretch of it:

```text
boxTimes = [#snd1: [0, 32], #snd2: [36, 60], #snd3: [68, 92],
            #snd4: [100, 124], #snd5: [#flipper, #hGap]]
```

Those numbers took a measurement. They are not frames -- there are only forty.
The track's timescale is 600 and its duration 1600, which is 2.67 seconds, or
**160 ticks**; the keyframes fall every 32 ticks, and each pair sits four ticks
inside a keyframe boundary at both ends. So the units are ticks and the margins
are the author keeping the seams out of sight. Converting is
`time = ticks * timescale / 60`.

The fifth pair is two symbols where the other four are numbers, so that box has
no stretch at all. It still plays its sound -- the original's `startSound` sits
outside the branch that reads the times -- and my first pass had it silent
because I had put the sound inside the lookup. One box in five making no noise
is exactly the kind of thing that reads as a puzzle being subtle.

Segment playback is new and narrow: two handlers in the game use
`pushQTcarefully`, and `prerollQT` is a no-op here because there is nothing to
spin up. The player takes a tick range, converts it, seeks, and finishes at the
end of the stretch rather than the end of the film.

Seven tests. `only_the_last_five_are_remembered_so_a_wrong_start_costs_nothing`
is the one that carries the design.

## 71. Hearing a room without hearing it

helba said the box puzzle made no sound, and I could not answer it. The window
is the only place the audio path runs, and the only way to see a fault there
was to hear it -- which I cannot do. So I had spent a while reasoning about
channel caps and gain lookups from the shape of the code, which is exactly the
kind of guessing this log keeps recording as a mistake.

`mix` runs the same path against a mixer with no output and prints what it is
holding:

```text
bedrm_boxes: the bed
  BRclock            gain 0.03 looping
  houseHum           gain 0.11 looping

click (440, 190)
  snd1box            gain 1.00 one-shot
```

So the box sounds do reach the mixer, at full gain, over a bed at a tenth of
it. Every theory I had been forming was wrong, and it took one command to say
so.

The tool then found a real bug I had not been looking for. A segment plays part
of a film, and the film's *soundtrack* was still handed over whole and started
from the beginning. `BOXPLAY.MOV` holds all five music box performances, so
opening one box played all five tunes -- wrong, and loud enough to bury the
box's own click. The soundtrack is sliced to the segment now.

Two smaller things the report made obvious. The bed is quieter than the room's
numbers suggest because a level is the room's `#earShot` times the game's own
`soundVolTweaks` trim: the bedroom clock reads 19% in the room and plays at 3%,
which is the data's intent rather than a fault. And a programme -- a radio or a
clock -- is not a voice until it is advanced, so it shows as nothing until the
first take is queued; the window does that every frame and the tool needed one
nudge to be representative.

What I should have built three reports ago. The pattern is the same as the
event log in entry 58 and the recording in entry 64: when the answer is only
visible from inside the running game, build the thing that shows it rather than
reasoning about what it must be.

## 72. The radio ate every channel

helba sent the trace, and it named the fault in three lines:

```text
[ 85] play (unnamed) gain 0.20 1ch 2697728 frames
[146] play (unnamed) gain 0.16 1ch 2697728 frames
[185] no free channel for (unnamed), dropped
```

Two million six hundred thousand samples is **a hundred and twenty-two
seconds**. The bedroom radio was starting a fresh two minute take on every room
change, and none of them ended, so by the third room all four sound channels
were radio and everything else was refused. The music boxes were not silent:
they were dropped for want of a channel, which is the mixer doing exactly what
entry 66 taught it to do.

The cause is that a programme's takes were anonymous. I made them so
deliberately in entry 66 -- a programme's takes are distinct recordings played
in turn, and naming them all alike would have folded each into the last and
played only the first. But anonymous also meant `playing_loops` could not see
them, so `update_ambience` never knew the radio was already on and started it
again in every room; and `set_loops` treats an unnamed voice as a one-shot to
be left alone, so the old takes were never retired.

The right identity is the *programme*, not the take. Keyed by `BRradio`, one
take plays at a time, a room that no longer wants the radio retires it, and a
room that still does leaves it running. Two lines that had been fighting each
other for two entries now agree.

The same shape of bug, found in the same session, in the film's soundtrack:
also off-channel, also anonymous, also restarted by every music box that began
a new segment. Five boxes, five copies of the same film's audio. There is one
film on screen, so there is one of those, and starting another replaces it.

What is worth keeping from this: entry 66 capped the mixer at four channels
because the game does, and that cap was right. It also turned a leak that had
been merely wasteful into a fault that silenced the thing the player had just
clicked. A limit does not create the bug it exposes, but it does decide which
symptom you get, and "no sound from the boxes" is a much worse clue than "the
radio is playing four times".

## 73. The boxes say nothing, and entry 57 was measured wrong

Two questions from helba, and the answers went opposite ways.

**Do the boxes say something when the puzzle first loads?** No. Entering runs
`initBoxPuzzle`, and that handler is

```text
on initBoxPuzzle
     0  return
```

an empty stub the authors left in. `BOXPLAY.MOV` has no sound track at all --
the tunes are the `snd1box` to `snd5box` sounds, played on click. So the room
opens on a clock, a radio and a silent film of five closed boxes, and that is
correct.

**Is the house hum right?** No, and helba spotted it from a trace where I had
not:

```text
[  1] bed [houseHum 21%] total 0.21
[ 85] bed [BRclock 2%, BRradio 20%, houseHum 11%] total 0.33
```

All four bedroom rooms declare `#houseHum: 224`. The hum played at 21% in one
and 11% in the next, because entry 57 scaled the whole bed down when it summed
past a ceiling, and a room with more sources therefore got each one quieter.

Entry 57 was measured wrong. I summed the levels a room asks for -- 101 rooms
above full scale, the worst at 2.82 -- and never applied the game's own
`soundVolTweaks` trim, which is a per-sound multiplier the mixer applies right
after. With it, **28 rooms** sum above full scale and the worst is **1.83**,
which the saturator handles as gentle compression. I had corrected for a
problem three times larger than the real one, using numbers that never reach
the mixer.

The correction was also worse than the fault. Clipping on a peak is momentary;
a hum that changes level whenever a clock comes into earshot is continuous, and
the hum is the one sound in this game that has to be steady as the player walks
through the house. Removed. Each source now plays at the level its room asks
for, and the three things standing between that and a bad mix -- the four
channel cap, the duck, and the saturator -- are all things the game or the
hardware actually does.

Recorded plainly because it is the same mistake as the palette and the `snd `
peak: a number that agreed with what I expected got no second look. Entry 57
even says "measured rather than guessed", and the measurement was of the wrong
quantity.

## 74. Talking to herself, and a film that is not on the disc

Four reports from helba at once, and they had four different answers.

**No sound when the box puzzle loads.** Correct. Entering runs `initBoxPuzzle`
and that handler is one byte of code -- `return` -- in the only place it is
defined; I checked every `Lscr` in every movie rather than trusting the first
match, because a duplicate definition is exactly how this tool has misled me
before. `BOXPLAY.MOV` has no sound track either. The room opens on a clock, a
radio and a silent film of five closed boxes.

**Opening a door breaks the capture.** `setDoorIsOpen` was not ported, and it
is the biggest handler left in the chapter at twenty-eight call sites.

**The sound of talking to myself.** Same handler. Closing a door she has just
opened is how Margaret thinks aloud: `#Iwonder` once the clock puzzle is under
way or the radio is tuned to the dining room, `#wasteOfTime` otherwise, with a
forty tick beat before the second which is taken only when the line has not
been used up. Both sounds were sitting on the disc unplayed.

The handler carries something I could not make sense of and have recorded
rather than tidied. Each door passes a time to `goBack`: fifteen minutes for a
door between rooms, three hours for the one to the outside, and the bedroom
resetting to four o'clock. `goBack` in this chapter takes no arguments at all,
so the original discards every one of them. Those symbols appear nowhere else
on the disc. They read as a clock these doors were once meant to move, left in
when it stopped moving.

**The radio does not change.** A programme's take is keyed by its programme so
that a room change cannot restart it, which entry 72 established. But a keyed
voice was refused if one was already there, and a programme is due *exactly*
when its take ends -- the voice is not retired until the mixer next runs, so
the two overlap and the next take was turned away. The radio stalled on
whichever take had been playing at the moment they met. A keyed *loop* is still
left where it is; a keyed one-shot takes the slot over.

**Margaret's opening film.** It is not on the disc. `bedrm_fadeIn` asks for
`40sINTRO.mov`, that name does not appear anywhere in the 574MB image, and the
extraction is complete -- 280 `.MOV` names in the image against 278 files, the
two extras being fragments my scan caught mid-string. Margaret's own directory
holds thirty-six films and no intro among them. Entry 69 steps past that room
for exactly this reason, and there is nothing further to recover.

## 75. Looking for a film that is not there

helba asked the right question about entry 74: what if Margaret's opening is
not *in* Margaret? A file can live anywhere on a disc, and I had only searched
by name.

Four checks, and they all say the same thing.

The movie index is not chapter-scoped: it walks the whole tree once and keys
case-insensitively, so a film in any folder is already found from any chapter.
That was the version of the question worth taking seriously, and it was already
answered by the code.

No room in any other chapter goes to a Margaret room -- I resolved every
`goTo` destination in `BRICE`, `EDWIN` and `ROXY` against Margaret's own
location table and got nothing. There is a `transitToEdwin` and no equivalent
for her.

Her folder has no opening in it. Sorted by duration, the longest film she owns
is `MARGEND.MOV` at thirty-one seconds -- her *ending* -- and everything else
is five seconds or less, which is the length of a door swinging.

And no film anywhere else is a candidate. The three forties-named films in
Roxy's folder run 3.6, 1.7 and 0.2 seconds: a frame, a plan and a scanner
graphic. The long films on the disc are the game's opening, the credits, the
camera log and Edwin's endings.

So the answer stands, but it is worth saying what changed: I had asserted it
from one search of the image for one name, which is thin, and helba was right
to push. It is now four independent measurements that agree, which is a
different kind of claim. The room that asks for `40sINTRO.mov` is scaffolding
by every other measure too -- a seventeen by three palette holder, exits to a
word rather than a place -- so the most likely reading is that the film was cut
and the record that called for it was left behind.

## 76. The opening was in Roxy's chapter all along

helba said they had watched a playthrough of the Mac build and the chapter
transition is right there, around thirty-eight minutes in. Entry 75 said there
was no such film, four ways. Entry 75 was looking in the wrong chapter.

First, the disc. There is no separate Mac tree to find: the image is a single
ISO9660 volume, and the chapter movies are `RIFX` -- big-endian, Mac-authored.
The Windows side is a launcher over the same Director files. So what I have
*is* what the Mac build reads, and a playthrough of it has to come from these
bytes.

The films are in `ROXY/MOVIES`, and they are named for where they go rather
than where they are:

```text
MARGNTRY.MOV    9.4s     BENTRY.MOV   16.6s     EENTRY.MOV   13.8s
```

One per chapter, all in the frame story, because the frame story is what
crosses. I had searched Margaret's folder and the whole image for the name her
own scaffolding room asks for, and never asked the obvious question helba did:
*what if it is not in Margaret*.

The room is `MargPortal_headOn`, and its examine hotspot is the sequence:

```text
setState( #showMontage, 1 )   -> margntry.mov
pushVideo : wait #videoStop
setState( #showMontage, 2 )   -> 40sFRAME.mov
pushVideo : wait #videoStop
setState( #showMontage, 4 )   -> the loading picture
setState( #showMontage, 0 )
enterNewDomain( oStoryteller, string(#Margaret), 15 )
```

`#showMontage` picks the film by gating four video sprites in one room, which
is the same trick as everything else in this game: the state is the index.

Two bugs stood between that and working. `enterNewDomain` reads its chapter
from the *second* argument -- the first is the storyteller -- and I was reading
the first, so the crossing named `oStoryteller` and matched nothing. And
`Outcome::new_domain` was set, merged, and never acted on: nothing anywhere
called `enter_chapter`. The player could watch the whole sequence and stay
where they were.

Both fixed, and the crossing works: portal, film, film, Margaret's bedroom.

Two more of helba's reports resolved on the way.

**Clicking through an animation.** These sequences open with `cursorOff` and
the original hides the pointer for their duration. A click during one moved the
room while the film was still playing, which left the film running over the
room it had moved to. Input is ignored while the effect queue has work.

**The door.** Not decoding: a door film is 82 frames at 0.12 ms each, and
opening one costs 0.4 ms including its audio. What was wrong was
`newDoorStatic`, and both halves of it are behind a platform test I had
flattened:

```text
if gCPU =  #Mac then setLoop( #loopingStatic, 0 )
if gCPU <> #Mac then suspendSounds
```

The static loop is started at volume **zero** -- a placeholder the Mac build
keeps silent -- and the PC build ducks the bed instead. I was doing both, at
full volume, so every door in the chapter came with a constant hiss.

## 77. Reading the playthrough

helba downloaded a Macintosh longplay and told me where to look. Extracting
frames with `ffmpeg` and comparing them against decoded film frames answered in
minutes what I had been arguing about for two entries.

The transition films are confirmed frame by frame. `margntry.mov` is the portal
light expanding to a white flash, and `40sFRAME.mov` is the white photo frame
settling into place around the chapter. Both match the video exactly, both are
in `ROXY/MOVIES`, and both now play.

What comes *between* them and the first room is the part I do not have. It is a
halftone screen-door dissolve from Margaret's body on the living room floor to
her face -- and the screen-door pattern is the giveaway. That is Director's
pixel dissolve, not a film. `setTransition` is called **106 times** in the game
data, all but two of them `#fadeIn`, and I implement none of it: every one of
those is a cut. That is the "entire thing missing" helba could see and I could
not.

Three other things fell out.

`#greater` was not implemented. It appears twice in the whole game and both
fell through to `Cond::Always`, so both sprites drew unconditionally. One of
them is the telegram in `MLR_FLOOR_CU`, which is the room the chapter opens
in -- so my render put a telegram over the scene it is meant to follow. The
same shape as the `#and` bug from entry 47, found the same way: by looking at
something that should not have been on screen.

Margaret seeds `#showMontage: [1, 2, 3, 4, 5, 0]`. The head is **1**, not 0,
and every other chapter starts that flag at 0. Her rooms are written expecting
it.

And `40sINTRO.mov` really is not anywhere. Beyond the earlier four checks, the
`MooV` chunks that could have carried it embedded are all empty: 43 in
Margaret, 107 in Roxy, 34 in Brice, 108 in Edwin, and **not one has a single
byte of data**. They are markers naming an external file. So that film is
absent from this rip, and the room that asks for it stays a dead end.

What I should take from this entry is not the findings but the method. helba
has been describing what the game does for several entries and I have been
answering from the data; the video is the data, and I could have read it the
first time it was mentioned.

## 78. What I could not find

helba is still landing on the ironing board, and asked the right question:
what if the opening is not a room or a film but a *flow* -- a Lingo handler
never ported. I tested it and it is not, and the ruling-out is worth recording
because I would otherwise try the same things again.

Margaret's movie defines 109 handlers. The ones that could plausibly be a
chapter opening:

  - `startMovie`, Director's own "this movie has loaded" hook, sets the cursor
    and returns. Three instructions.
  - `enterFrame` is a `REPORT2` debug trace.
  - `testmontage` is a two-line debug helper that sets `#showMontage` and
    prints it.
  - `initBoxPuzzle` is one byte, already recorded in entry 74.

So there is no opening flow in her chapter. The sequence that crosses is all on
the Roxy side and is now ported.

I rendered all 154 of Margaret's rooms and matched them against the video frame
by normalised correlation. Nothing matches: the best distance is 1.74 where a
real match is near zero. The reason is visible once you look -- **the lamp in
the video is lit and every one of my renders is dim**. So the room may well be
among them and I am rendering it in the wrong state, which is a different bug
from the one I was hunting.

The declared start is a genuine dead end and I have now checked every way out
of it. `bedrm_fadeIn` draws a seventeen by three palette holder, asks for a
film with no data anywhere on the disc -- not a file, not embedded, the 292
`MooV` chunks across all four movies are empty markers -- and its two exits go
to `#destination`, which is not a room in any chapter. `world.resolve` finds
nothing by that name.

So I cannot derive the landing room from the data, and what the engine does
instead is a guess: the first room of the chapter that draws anything, which is
index order and lands on `bedrm_A1`. That is not a finding, it is a fallback,
and this entry exists so the next attempt does not mistake it for one.

The thing worth chasing next is the lamp. If the video's opening room is one I
already render but unlit, then the fault is a state or a light I am missing,
and matching would find it the moment the render is right.

## 79. Which release this is, and dissolves

helba pushed back: this is an original ISO, so the film should be on it. That
deserved a real answer rather than another byte search, so I walked the
ISO9660 directory records properly instead of trusting the extraction.

**676 files, 278 of them `.MOV`** -- exactly what was extracted, so nothing was
lost on the way out. The volume declares 293910 blocks of 2048 bytes, 601927680
in total, against an image of 602263552: complete, not truncated. Volume id
`AMBER_JB`.

And there is no Macintosh side. I checked for an Apple partition map at block
one, and for an HFS master directory block on every 512 byte boundary in the
image validated against its allocation block size -- nothing. This is a pure
ISO9660 disc: **the PC release**. The longplay helba matched it against is the
Macintosh release, which is a different disc.

That resolves it. The PC disc does not carry `40sINTRO.mov`, and it does not
carry `MEewall.mov` either -- whose siblings `MENTRYL.MOV`, `MENTRYR.MOV` and
`MEMRLOOP.MOV` are all present, so the two absences sit right next to films
that are there. The movies are `RIFX`, Mac byte order, because the game was
authored on a Mac and the same movies shipped on both discs; that is what made
me assume a hybrid.

So the montage helba can see is on their video and not in my data, and no
amount of engine work will produce it. Worth having spent the time to be sure
rather than asserting it a third time.

**Transitions.** `setTransition( oPuppeteer, #fadeIn )` had the same argument
bug as `enterNewDomain` in entry 76 -- the receiver is the first argument, so
reading argument zero named the puppeteer. It was also writing to
`Outcome::transition`, which is the *movement* flavour a `goTo` carries. Those
are two different things: one says how the screen changes, the other which way
the player turned.

Now its own effect, and implemented: the stage as it stood is kept before the
new one is composed, and the two are mixed across about fourteen frames.
`#slowMontage`, used twice, takes forty-five. The game asks for a transition a
hundred and six times -- every door, every close-up, every step of a montage --
and every one of those was a hard cut.

## 80. An audit of the verbs, and the montage

Rather than port the next handler by call count, I listed every verb the room
scripts actually call and checked it against what the engine does with it --
the same shape of audit that found `#greater` in entry 77, which was worth two
sprites drawn unconditionally.

Sixty-seven distinct verbs. One stood out at **fifty-three call sites**:
`fadeToMontage`, whose effect was emitted and applied nowhere. Same class as
`Effect::PlayVideo` in entry 59 and `Outcome::new_domain` in entry 76 -- a
value produced, merged, carried through the queue, and dropped on the floor.
Three times now, and all three were found by looking rather than by playing.

The handler is four lines:

```text
on fadeToMontage whichNumber
  setState( oStoryteller, #showMontage, whichNumber )
  setTransition( oPuppeteer, #fadeIn )
  updateDisplay( oPuppeteer )
```

So every montage step in the game was a state change nobody made, without even
the redraw that would have shown it. It stays an effect rather than a direct
write because the handlers that use it step through several in order -- Edwin's
snow blindness goes 1, 2, 1, 0 -- and the order *is* the montage.

It needed entry 79's transitions to be worth implementing, which is the only
reason it was not done sooner: a montage of hard cuts is not a montage.

On helba's remaining complaint, which I cannot fix: the scene after the Margaret
transition needs `40sINTRO.mov`, and entry 79 established that this disc is the
PC release and does not carry it. The room the engine picks instead is still
the fallback recorded in entry 78, and it is still a guess.

## 81. The end of the game, and a tally that flattered itself

The psionic bar is the last puzzle in Roxy's chapter and, it turns out, the
last puzzle in the game. Three verbs run it: `adjustAlgorithm`,
`setFragmentBias` and `setFragmentAlignment`. All three are now ported.

`adjustAlgorithm` reads like the lock in entry 54 -- three columns, a cast
table each, step with the mouse held -- right up to its tail, where it stops
being a dial and becomes an ending:

```text
if getState( #BT_algorithmLeft )   <> 5 then return
if getState( #BT_algorithmMiddle ) <> 2 then return
if getState( #BT_algorithmRight )  <> 8 then return
cursorOff : wait 60 : soundEffect #happyBeep
pushVideo : wait #videoStop
setState( #BT_fragStatus, #allDone )
setState( #endGame, 1 )
```

Five, two, eight. The schema opens the columns on two, three and five, so no
column starts on its answer.

Two details worth keeping. The columns **refuse at their limits** rather than
wrapping -- one to eight, and a `#algorithmNotAvail` beep at either end --
which is the opposite of the lock's wheels, and I would have got it wrong by
analogy if I had not read it. And `#endGame` is written through the effect
list rather than straight onto state, because the original sets it after `wait
#videoStop`: the ending has to have played before the game is over. The test
asserts on the effects for exactly that reason, which is the second time this
port's timeline-order rule has shown up as a test that would otherwise have
been written wrong.

The other two verbs need no sprite code at all. `#BT_checkBox` is keyed
`[#on, 1, 2, 3, #off]` and `#BT_bias1..3` are keyed by the bias value itself,
so writing the flag and asking for a redraw lands every sprite where the
original put it by hand. That one table answering to both `#on` and a number
is why the same check box art serves two sections keyed on different flags --
a small, deliberate piece of authoring that the state-indexed cast machinery
from entry 54 picks up for free.

`setFragmentAlignment` spins `#BT_psionOrder`, a three-element list, and is the
one place where a flag's whole list is the value rather than a history. It is
also guarded: neither the bias nor the psions move while their section is
switched off, and the check box parks the bias in `#BT_storedBias` so switching
back on restores it.

### The tally that flattered itself

While checking what was left I found `verify` reporting

```text
native handlers:     27 distinct, 55 call sites
unhandled calls:     none
```

with six verbs I knew for a fact had no handler. Neither line was lying,
exactly; both were answering a different question than the one they looked
like they answered. `unhandled calls` counts *parse* failures. And every verb
without a specific arm in `script.rs` becomes an `Effect::Native`, ported or
not -- whether a handler exists is decided later, at apply time, by
`natives::call` returning a bool. So the native tally reads the same either
way.

The fix is a separate question, `natives::is_handled`, asked of `call` itself
so it cannot drift out of step with the arms. That took two goes. The first
probed once on a blank state and reported `setFrontDoorIsOpen` unported -- the
openers and the bleeding doors are keyed on `(chapter, verb)` and decline
outright when the chapter is not theirs, so a blank probe reports all of them
missing. It now asks once per chapter. I only caught it because I wrote the
test asserting a known-ported verb answers yes, which I nearly did not bother
with.

The honest number is **27 verbs, 55 call sites**, none of them ported. Not the
six I had been carrying in my head. The list is now printed, so it stops being
something I have to remember.

Removed a dead `resetBoxPuzzle` arm in the process -- a duplicate, shadowed by
an earlier one. The compiler had been warning about it for two commits. The
live copy happened to be the correct one; the dead copy used `set` where the
list model needs `set_all`, and would have pushed an empty list onto the head
instead of clearing it. That is luck, not judgement, and it is the second time
this project a warning I skimmed past was pointing at something real.

150 tests, clippy clean.

## 82. One bit in the opcode table

I have been deferring six verbs for the better part of this port with the same
excuse each time -- "numeric literals resolve unreliably through the name
table" -- and treating it as a property of the bytecode. It was a one-line bug
in my own disassembler.

Director's opcodes pair up on their low six bits: the same operation appears
once with a one-byte operand and once with two. `0x45`/`0x85` is the symbol
push. `0x41`/`0x81` is the integer push. I had `0x81` sitting in the symbol
set, so every integer over 127 was being looked up in the name table and
printed as whatever symbol happened to live at that index.

Which is how `initRadioDial` came to appear to build a table out of
`#propList`, `#snd3`, `#snd5` and `#startSound`, and how the radio's dial
limit read as

```text
if gRadioDial < #snd5
```

I stared at that and concluded the tooling could not resolve names in this
movie. The right conclusion was that it should not have been resolving names
there at all.

The counts are not subtle, and I could have run them at any point in the last
month:

| opcode | operands | out of range for the name table |
|--------|---------:|--------------------------------:|
| `0x81` |      630 |                             221 |
| `0x85` |     2302 |                               1 |

and the commonest `0x81` values are 1000, 255, 500, 300, 200, 256 and 1024.
Nobody's name table is 3999 entries long.

Before changing anything I diffed all 41 already-ported handlers under both
versions of the tool. **None of them changed** -- they deal in small integers
and real symbols, so the bug never touched them. That was worth five minutes
to establish rather than assume; it is the difference between a tooling fix and
a re-audit of everything built on top of it.

What it unblocks, immediately:

```text
on radioDial upOrDown
  if getState( #tunedIn ) <> #inBetween then
    if not voidp( gStaticWhere ) then gStaticWhere = getState( #tunedIn )
    setState( #tunedIn, #inBetween )
    endLoop #BRclock : endLoop #Kclock : endLoop #DRclock
    endLoop #LRclock : endLoop #roaringFire
    idle
    set the visible of sprite 44 = 1
  if upOrDown = #up then
    if gRadioDial < 240 then gRadioDial = gRadioDial + 4
  else
    if gRadioDial > 3   then gRadioDial = gRadioDial - 4
  set the movieTime of sprite 45 = gRadioDial
  patchPalette
  repeat while mouseDown ...
```

So the dial is a movie scrubbed by hand: sixty-one positions, four ticks apart,
and turning it drops whatever station you were on and stops the room's clock
and fire loops with it.

`initRadioDial` builds the map the dial is read against -- each station's
position, and the warm and cool bands either side of it where the signal is
coming in or going out:

```text
gStaticMarkers = [ #bedroomWarm: [0, 4, 8], #bedroomCool: [12, 16, 20],
                   #diningRmWarm: [48, 52, 56], ... #inBetween: [216, 220, 224] ]
```

with one branch I did not expect: **where the kitchen station sits on the dial
depends on where the dumb waiter is.** If it is down in the kitchen the station
is at 248 with its bands at 120-140; otherwise it is at 256 with its bands up
at 168-188. The kitchen radio is heard through the dumb waiter shaft, and
moving the shaft moves the station. That is the sort of detail that only exists
because somebody cared, and it would have stayed invisible behind `#snd3`.

The lesson is not about Director. I had a symptom I could reproduce on demand,
a cheap measurement that would have localised it, and I chose instead to write
the same caveat into my notes six times. The caveat was load-bearing: it kept
five verbs off the list for weeks.

`op03` is still unidentified. In context it is certainly a push of the integer
zero -- `#bedroomWarm: [<op03>, 4, 8]` against `#bedroomCool: [12, 16, 20]`
admits nothing else -- but "certainly in context" is exactly the reasoning that
got me here, so it stays printed as an unknown until I can count it.

## 83. The radio

With entry 82's fix in place the radio reads straight through, so it is ported:
`initRadioDial`, `radioDial`, `checkRadioStations` and `backAwayFromRadio`.

The dial is a movie. `gRadioDial` is a movieTime on sprite 45, running 0 to 240
in steps of four -- sixty-one positions -- and turning the knob scrubs it by
hand rather than playing it. Each station sits at a fixed position:

```text
gRadioStations = [#bedroom: 36, #diningRm: 56, #kitchen: 88, #livingRm: 196]
```

and you are on a station when the dial is exactly on its number, in its **warm**
band four either side, or its **cool** band eight either side. Past that there
is nothing. The band names are built by concatenation --

```text
gStaticWhere = value( "#" & getOne( gRadioStations, i ) & "Warm" )
```

-- which is why they never appear in the name table, and why `gStaticMarkers`
looked for the longest time like a property list keyed on nothing.

Three things about it I would not have guessed.

**Turning off a station stops the house.** Not just the radio: `#BRclock`,
`#Kclock`, `#DRclock`, `#LRclock` and `#roaringFire` all end the moment the
dial moves off a station. Those loops *are* the station -- the room you are
tuned to, heard through the walls, rather than a broadcast out of the speaker.
Which is the whole conceit of the thing and I had it filed as an ambience bug.

**Walking away leaves it on.** `backAwayFromRadio` stops the other three
stations' loops and leaves yours running, so the house keeps the radio on
behind you.

**The dumb waiter moves the kitchen station.** If the dumb waiter is down in
the kitchen the station sits at frame 248 with its bands at 120-140; anywhere
else and it is at 256 with its bands at 168-188. The kitchen radio is heard up
the dumb waiter shaft, and moving the shaft moves where you find it on the
dial. That is two puzzles quietly wired together, and until entry 82 every one
of those numbers was printing as an unrelated symbol.

`onTheAir` fell out for free. The original asks `getProp( oStoryteller.states,
#tunedIn )` -- the whole list, not its head -- and searches it for the station.
Under the list-valued state model from entry 53 that is exactly `get_all`, so a
station is on the air if it is one of the values `#tunedIn` is allowed to take.
No new concept, and a small piece of evidence that the state model is the right
shape.

Only the dining room gets the full fade-in inside `checkRadioStations`: tuner
at 120 and the station at 90 under the announcement, then 230 and 120 once it
has finished. The other three set their band and stop. I have left it that way
because that is the original's shape, not an omission -- but I am flagging it
here rather than in a comment, because if a station turns out to be silent in
play, this is the first place to look.

Also fixed the literal table, which read each string's length and threw the
bytes away. It now prints them, which is how `"#"`, `"Warm"` and `"Cool"`
turned up -- along with the authors' own debug line, still in the shipped
bytecode:

```text
'>¥> Puzzle solved .. tuning in the Living Room! ¥¥¥ -<<'
```

Unported is now **24 verbs across 44 call sites**, down from 27 and 55.

159 tests, clippy clean.

## 84. The weather vane, and a test I should have written months ago

Edwin's weather vane, ported: `setWeatherVane` and `initWeatherVane`.

It turns `#n` to `#E` to `#S` to `#W` and back, or the other way, and each of
the eight turns plays its own 60-tick segment of a 512-tick movie. The wind
only follows once it is already blowing -- turning the vane in still air moves
the vane and nothing else -- and the squeaks rotate through a list so the same
one is never heard twice running.

I nearly got the segment table wrong by being clever. The first four entries
support an obvious rule -- clockwise turn on the direction's resting frame,
counter-clockwise 64 ticks later -- and the rule is false. What actually
decides it is the *destination*: a turn ending at `#n` or `#E` sits on the
resting frame, one ending at `#S` or `#W` sits 64 later. So the movie is laid
out E, n, S, W with two turns each, and its frame zero is East, which is why
`initWeatherVane` rests East at 0 and North at 128. Eight table entries are
cheaper than a rule that has to be believed, so the table is written out.

### The fourth time

While wiring the vane I found `Effect::PlayVideoSegment` **declared, emitted in
three places, and applied nowhere.** That is the fourth: `Effect::PlayVideo` in
entry 59, `Outcome::new_domain` in entry 76, `Effect::FadeToMontage` in entry
80, and now this. All four looked like working code from the handler's side.
All four were found by reading.

The mechanism is one line:

```rust
_ => {}
```

A catch-all arm cannot fail. Every one of these was a handler doing its job,
pushing a well-formed effect into a queue that quietly swallowed it.

So there is now a test that reads `script.rs` for the `Effect` variants and
every file that acts on effects, and fails if a variant is never mentioned in
any of them. Parsing Rust as text is crude and I do not much like it, but it
fails the moment somebody adds a fifth variant without an arm, which is the
entire job. It found one variant on its first run -- `Native` -- which turned
out to be the one deliberate exception: `natives::call` runs first and only
pushes `Effect::Native` when no handler took the verb, so it is a record that
something is unported rather than an instruction. That is exempted by name,
with the reason, rather than by widening the search until it passes.

Writing this in entry 84 rather than entry 59 is the actual mistake. I had the
diagnosis after the first one and treated it as an incident three more times.

Also cleared the build warnings, all of them this time. One was a redundant
`was_down = down` in a branch whose only job was to decline the click; the
foot of the loop already carries it. Entry 81's dead `resetBoxPuzzle` arm had
been sitting behind that same wall of noise for two commits.

Unported is now **22 verbs across 39 call sites**. 167 tests, clippy clean, no
warnings.

## 85. The whirligig, and a name I made up

Edwin's whirligig, ported: `initWhirligig` and `startWhirligig`.

It is two films stacked on the two film channels -- the spin-up on 45 and the
steady loop on 44 -- with a version of each for every wind direction:

```text
#gigStartMovies: [#n: 966, #s: 967, #e: 965, #W: 968]
#gigLoopMovies:  [#n: 970, #s: 971, #e: 969, #W: 972]
```

`startWhirligig` runs the spin-up, blocks until it stops of its own accord,
throws that sprite a thousand pixels off stage and shows the loop underneath.
The wind is set from the vane *here* rather than when the vane turns, so a vane
turned while the whirligig is still decides which way the wind picks up in.

One trap worth naming: those tables spell three directions in lower case and
the weather vane spells three of them in upper. Lingo compares symbols without
regard to case, so the mismatch never mattered in the original and would have
silently found nothing in mine. There is a test for it.

### `carols`

While wiring the whirligig I called `killSongs`, found it had no handler, went
to write one, and discovered that `disableSongs` had been stopping a loop
called **`carols`** -- a name I invented, that nothing in the game answers to,
at eight call sites. The real thing is

```text
#windSongs: [#threeKings, #silentNight, #godRestYe, #goodKing]
```

Four carols the wind carries, faded out one at a time. So for every one of
those eight sites the carols simply never stopped.

It survived because `verify`'s dangling-reference check tested the names on
`PlaySound` and `StartLoop` and **not on `StopLoop`**. Stopping a loop nothing
answers to is exactly as much a mistake as starting one, and the check now
covers all three. It reported `sound carols 8` on the first run.

That is the third check this week that was answering a narrower question than
it appeared to, after `verify`'s unported tally in entry 81 and the effect
coverage in entry 84. The pattern is the same each time: the check passes, the
passing is read as evidence, and the evidence is worthless because the check
could not have failed.

### Warnings

Entry 84 said I had cleared the build warnings, "all of them this time". That
was wrong -- nineteen remained, all dead code, and I had counted only the
category I had just fixed. They are gone now, properly: two rounds of removal,
because deleting dead code uncovers more of it.

Two of them were worth stopping for.

`decode_raw` in the bitmap decoder is documented as being for "tooling that
walks chunks directly". An unused raw-bitmap path could have meant uncompressed
`BITD`s were never decoded, which would be a real gap. It is not -- the tooling
that used it is Python now. Deleted.

`Node.custom_palette` is parsed from every room and read by nothing, which
would be a much worse bug: rooms rendering in the wrong colours is precisely
what a "something looks broken" screenshot looks like. So I counted it before
touching it. **All 1320 rooms declare `#CustomPalette: ""`.** The field is
empty everywhere in the shipped data; the game never uses a custom palette.
Dead for a good reason, and now deleted with the measurement written down so
nobody has to wonder again.

Build warnings: zero. Clippy: zero errors, twenty-nine style lints left
standing. 173 tests. Unported is **20 verbs across 31 call sites**.

## 86. The security tape

Roxy's camera log, ported: `camControl`, `camLogInit` and `camLogShutdown`.

It is a VCR. Six markers on the tape --

```text
markerList = [44, 2152, 4432, 7898, 12474, 14984]
```

-- with previous, next, play and pause. Stepping between markers plays a
shuttle whose length is the distance travelled over twenty, capped at three
hundred ticks, so a long jump takes visibly longer than a short one without
taking forever. Nice touch, and cheap.

The tape's length is **15000**, and the only place that number appears is the
clamp inside the play button:

```text
if the movieTime of sprite 44 > 15000 then set the movieTime of sprite 44 = 15000
```

There is no `#tapeLength` anywhere. If I had needed the end of the tape and not
read the play branch, I would have had to make a number up -- which, going by
`carols` in entry 85, I evidently would have done.

Sitting down at the monitor calls `disablePeekAlert` and standing up calls
`enablePeekAlert`, so the ghost cannot interrupt while the player works through
the tape. That is a considerate piece of design and it is the sort of thing
that would have been invisible from play: you would simply never have noticed
the interruption that did not happen.

Two things are approximated and both are written down in the code. The pressed
button lights for eight ticks -- kept, because it is a beat the player feels --
but the rewind static is a second film swapped onto channel 45, and the
original finds the button sprite by scanning channels 10 to 48 for whichever
one is showing a `#camButtons` cast. This engine resolves those from state, so
there is no sprite to find.

I also wrote a bug and caught it in the same minute: `camLogShutdown` starts the
room's film *and* stops the tape, which in the original is two movies on two
channels. This engine has one player, so my first draft pushed `PlayVideo`
followed by `StopVideo` and cancelled itself. Worth noting only because "the
original does both" is not a reason to do both when the two things were never
the same object.

180 tests, clippy clean, no warnings. Unported is **17 verbs across 24 call
sites**, from 27 and 55 at the start of the day.

## 87. Three nails

Brice's nail puzzle, ported. Three nails, each `#out`, `#halfway` or `#in`,
and they are linked: pushing one deeper drags the next one round back a step,
and letting one pop out pushes the next one in a step. All three out opens the
heart box.

The handler has a shape worth pointing at:

```text
targetCurrentState = getState( targetNail )
if targetCurrentState = #out     then setState( targetNail, #halfway )
if targetCurrentState = #halfway then setState( targetNail, #in )
if targetCurrentState = #in      then ...
```

Three sequential `if`s that would cascade -- `#out` becoming `#halfway`
becoming `#in` in one press -- except that every one of them reads the *saved
local* rather than re-reading the flag. That is the only thing standing between
this and a nail that jumps two steps at a time. I have written it as a match on
the saved value so the property is structural rather than something a later
edit has to remember, and there is a test that presses a nail from `#out` and
insists it lands on `#halfway`.

Before writing the tests I ran the puzzle through a breadth-first search over
all twenty-seven positions, because if my transition rules were wrong the
puzzle would most likely be *unsolvable* rather than visibly broken -- and
nothing else I have would have told me.

```text
reachable states: 27 of 27
solution from (halfway, halfway, halfway): push 1, 1, 3, 1, 2
```

Every position reaches every other, so the puzzle cannot be locked up, and the
schema's starting position is five pushes from open. That is a much stronger
check than any single assertion I would have thought to write, and the sequence
it found is now the test.

The three win sounds are behind `if gCPU = #PC` and are not played here. This
port takes the Mac arm throughout, and the reason it is right shows up cleanly
in this handler: on the Mac the films carry their own audio, which is exactly
why the PC build has to start the sound separately alongside the film. This
engine decodes film soundtracks, so playing them again would double them.

186 tests. Unported is **16 verbs across 21 call sites**.

## 88. Two dials on one machine

`adjustBarSettings` and `resetHeartBox`, both small, both ported.

`adjustBarSettings` is the psionic bar's other control: three digits -- level,
gain and FM -- one of them selected by `#BarSelection`, each running 0 to 9.
The arithmetic is the same trick as the lock in entry 54:

```text
if upOrDown = #up   then newLevel = (currentLevel + 11) mod 10
if upOrDown = #down then newLevel = (currentLevel + 9)  mod 10
```

Adding eleven and nine rather than adding and subtracting one keeps the value
positive so the modulo behaves.

What is worth stopping on is that these **wrap** while the algorithm columns
on the same machine, ported in entry 81, **refuse at their limits**. One panel,
two dials, deliberately different, and nothing about either one predicts the
other. If I had ported this first and reached the columns second I would have
carried the wrap across without a second thought -- which is exactly what I
nearly did in the other direction. Reading both is the only thing that works.

Nothing moves unless `#BarMode` is `#setON`, which is a third rule the panel
does not advertise.

`resetHeartBox` puts the three nails back to `#halfway` on leaving, so the
puzzle has to be solved in one visit -- unless it already has been, in which
case the open box is left alone. Same shape as `resetBoxPuzzle` in Margaret's
dresser, and the same reason: these puzzles are about a sequence, and letting
the player accumulate progress across visits would make the sequence
irrelevant.

191 tests. Unported is **14 verbs across 18 call sites**.

## 89. The Macintosh release

helba bought the Mac disc and sent it over: `AMBER-Journeys-Beyond_Mac_EN.zip`,
two StuffIt archives holding two bare HFS volumes. Entry 79 established that
the disc I had been working from was the PC release and that the Macintosh
release was a different disc. It is, and it is a **two-disc** release --
`AMBER_A` at 374 MB and `AMBER_B` at 625 MB against the PC's single 602 MB.

Nothing on this machine could read a bare HFS volume: no partition map, no
ISO9660, and 7-Zip declines it. So `tools/hfs.py` reads the master directory
block, walks the catalogue B-tree and pulls a file's data fork out through its
extents, with the extents-overflow file for anything fragmented past three.
That is the whole of it -- HFS is a small format and this needs about a page.

**879 files extracted, every one matching its catalogue size exactly.**

The two discs share 322 paths and exactly ten differ. Three are Finder
bookkeeping. The other seven are Roxy's endgame films -- `endanim`, `electro`,
`psimerge`, `fragdlog`, `warn`, `algodone`, `RoxyHG` -- which are real on disc
A and **468-byte stubs on disc B**. That is the disc-swap: B ships placeholders
so the projector can still find them by name. The merge takes the larger copy.

### What the PC release is missing

This is the part that matters, and it answers three of helba's complaints at
once. Pointing the engine at each release:

```text
PC    movies: 278 on disc, 196 referenced, 5 unresolved
Mac   movies: 283 on disc, 196 referenced, 1 unresolved
```

The five the PC disc references and does not ship:

| file | what it is |
|---|---|
| `40sINTRO.mov` | Margaret's opening |
| `MEewall.mov` | Roxy's east wall |
| `ST-CPU-LED` | the scan unit's indicator |
| `UH-BATHKNOBSCAN-ON1` | the scan unit fitted to the bathroom knob |
| `UH-MARGKNOBSCAN-ON1` | the scan unit fitted to Margaret's knob |

helba asked, weeks apart: why the scanner cannot be used on door knobs, and
whether its button is supposed to light up. Both were the same answer and
neither was a bug in this engine. The films are not on the disc.

The Mac's single gap is `tuner_bg.mov`, which only the PC ships -- one file
each way out of 196.

### `bedrm_fadeIn`

And the question open since entry 78. `40sINTRO.mov` is five seconds of Cinepak
dated February 1996: a black-and-white close-up of a woman's eyes in a soft
1940s vignette. **Not a room.** Which is why entry 78's search -- render all 154
rooms, correlate against the film -- could not have worked. I was matching a
room against a face.

The room that plays it has been in the world data the whole time:

```text
MARGARET   41  bedrm_fadeIn   palette-holder   -> (none)
```

A palette holder and a film on the video channel, every exit going to
`#destination`. It is in `MARG_2.DAT` on **both** releases -- the PC build kept
the room and dropped the film. Its cast entry still points at
`C:\AMBER building\AJBDISC1\MARGARET\movies_M`, which is the two-disc layout
showing through in the single-disc build.

Entry 78 listed what I had ruled out and called the landing room a guess. The
guess was wrong and the method was wrong: I searched for a room with art among
rooms with art, and this room has none. The name says what it is. I could have
read the list.

### Two bytes and a filename

Two things stopped the engine reading the Mac data at all, and both are the
same kind of mistake.

The `.DAT` files are byte-for-byte the same shape -- `MARG_2` has 37 records on
both discs -- but the PC separates records with `0xBC` and the Mac with
`0xC5`. Reading only `0xBC` turned every Macintosh chapter into a single room:
44 rooms across the game instead of 1320.

And filenames. The PC pressing shouts -- `MARGARET.DXR`, `MOVIES_M` -- and the
Mac does not: `MARGARET.dxr`, `movies_M`. HFS is case-insensitive so these were
the same name to the original. On a case-sensitive filesystem they are not, and
looking for the shouted form found nothing, which is why every room came back
unnamed.

Both now handled, and the PC release still parses to its same 1325 rooms.

The Mac is not simply better. It carries 183 sound files to the PC's 325 and 71
of its sound symbols name a file that is not on the disc, because the Mac keeps
most of its audio inside the movie resources. For films the Mac is the complete
release; for sounds the PC is.

## 90. `on exitFrame`

Margaret's chapter opening was never a missing film. It was a missing
*handler*, and I had walked past it.

```text
on exitFrame
  repeat with i = 1 to 48: puppetSprite i, 1
  moveToLocation( oPuppeteer )
  gStaticWhere = getState( #tunedIn )
  if getState( #currentLocation ) = #bedrm_fadeIn then
    cursorOff
    fadeOutTransit
    setaProp( oStoryteller.states, #soundChannels,
      [ 1: [#sndType: #virtualLoop, #sndName: #BRradio, #volume: 0],
        2: [#sndType: #loop,        #sndName: #BRclock, #volume: 0], ... ] )
    restoreSounds
    setState( #showMontage, 1 )
    goTo( #bedrm_margaret, #fadeIn )
    setState( #showMontage, 2 ) : setTransition( #fadeIn ) : updateDisplay : wait 45
    setState( #showMontage, 3 ) : setTransition( #fadeIn ) : updateDisplay : wait 45
    setState( #showMontage, 4 ) : setTransition( #fadeIn ) : updateDisplay : wait 60
    setState( #showMontage, 0 ) : setTransition( #fadeIn ) : updateDisplay
    fadeUpRadio( #None, 1 )
    wait 20
    assertSound #awful
```

The film plays, the stage fades, the bedroom radio and clock are laid in
silent, and the player is put down in `bedrm_margaret` -- whose art is called
`BR-MARG'S BODY` -- while the montage steps 1, 2, 3, 4 and back to 0 over the
top of it. Then the radio comes up and Margaret says `#awful`.

`exitFrame` is a **frame script**: Director runs it as each frame ends. That is
why entry 78, which went looking through the verbs for an opening handler,
found `startMovie` setting a cursor and `enterFrame` doing debug output and
concluded there was nothing. I checked two of the three frame events and
stopped.

helba said, at the time: *"what if it's not a playable room? what if its a
segment of the room where missing like in the lingo code it's self a funciton
we never ported/flow"*. That is precisely what it was, said plainly, and I went
and rendered 154 rooms instead.

Every chapter has an `exitFrame`. Roxy's carries the scan unit's countdown --
its locals are `scanStatus` and `minutesRemaining` -- which is another thing
helba has reported as not working. Edwin's and Brice's are one line.

This engine has no score and no frames, so only the `#bedrm_fadeIn` branch is
ported, run once when the chapter is entered rather than every frame. The rest
of the handler is `moveToLocation` and static bookkeeping this engine's loop
already does. That divergence is written into the comment above the handler
rather than left for someone to discover.

### `Effect::GoToRoom`

One new effect. `Outcome::destination` moves the player *before* any of a
handler's effects run, which is right for clicking an exit and wrong for a
scripted sequence: this one has to play a film, then fade, then move. So the
move became an effect, for the same reason `SetState` did in entry 71 -- if it
has to land between two waits, it belongs in the timeline and not in the
handler's return value.

The effect-coverage test from entry 84 is already carrying its weight: it would
have failed the moment I added the variant without an arm to apply it.

```text
> MARGARET
  sound: loop BRradio at 0
  sound: loop BRclock at 0
  sound: loop BRradio at 255
  sound: play awful

MARGARET / bedrm_margaret   [BR-MARG'S BODY]
```

196 tests. Four of helba's reports closed in one afternoon, and not one of them
was a bug in the engine's own logic: three were films the PC disc does not
carry, and this one was a handler I did not read.

## 91. Favouring the Macintosh release

helba's call, and the right one: the Macintosh release is the more complete
one, and this port already took the Macintosh arm at every `if gCPU = #Mac` in
the Lingo. Saying so out loud makes the two consistent.

The obvious move was to copy the five missing films into the PC tree, which is
what I did first and then undid. It gives one working directory and destroys
the thing that makes either disc worth having: after the copy, `extract` is no
longer a PC release, and the next time I want to know what the PC actually
shipped I have to remember which files I put there.

So instead there is `AMBER_FALLBACK`, a `:`-separated list of further
directories searched after the one on the command line:

```sh
AMBER_FALLBACK=extract amber play mac_game
```

Both file indexes were already first-match-wins -- a chapter's own copy of a
film is not displaced by an identically named one found later -- so this needed
one loop in each and nothing else. Every configuration now resolves what it
should:

```text
extract   alone              196 referenced, 5 unresolved
mac_game  alone              196 referenced, 1 unresolved
mac_game  + extract          196 referenced, 0 unresolved
extract   + mac_game         196 referenced, 0 unresolved
```

Neither tree is modified and neither is lying about itself.

### One rule, twelve places

Switching to the Mac data broke Margaret's opening immediately, and the reason
was mine: the PC location table spells the room `bedrm_fadeIn` and the
Macintosh one spells it `bedrm_fadein`. My guard compared exactly.

Lingo compares symbols without regard to case. I had been writing

```rust
state.get("currentLocation").as_symbol() == Some("bedrm_fadeIn")
```

in **twelve** places, every one of which happened to be right on the PC data
and wrong in principle. Rather than patch twelve comparisons there is now
`Value::is_symbol`, which is the rule in one place, and the twelve sites use
it.

This is the third time this week the same mistake has surfaced -- the whirligig
tables spell three directions in lower case where the vane spells them in
upper (entry 85), the `.DXR` filenames differ in case between pressings (entry
89), and now the room names. Each time I fixed the instance. The rule is that
**nothing in this game's data is case-sensitive and nothing in the port should
be either**, and it took three encounters to write a helper instead of a patch.

Also corrected the README, which had said since the beginning that the disc was
"a hybrid Mac/PC CD built with Toast". Entry 79 disproved that months of notes
later and I never went back to fix the front page. It now says what there
actually is: two releases, one disc and two discs, and what each is missing.

198 tests.

## 92. The scan unit finishes

Roxy's frame script, and the other half of helba's scanner complaint.

`setScanTime` has been parking a deadline in `gScanFinish` since it was ported,
and **nothing ever read it**. A scan started never finished. The deadline was
written, the clock was ticking, and the two were never introduced.

The reader lives inside the peek unit's own `on mouseDown`:

```text
minutesRemaining = (gScanFinish - the ticks) / 3600 + 1
if minutesRemaining > 0
  then currentStatus = getAt( [#Wait1min .. #Wait5min], minutesRemaining )
  else currentStatus = #ReadyForPlayback
```

and Roxy's `exitFrame` rebuilds the deadline on entering the chapter, from
whatever the status says is left -- so a scan does not run while the player is
off in another haunting.

### Where the test was wrong and the code was right

I wrote the countdown, wrote a test for it, and the test failed. My first
instinct was that the handler was off by one. Working the original's arithmetic
through for a five-minute scan set at tick zero:

| tick | `(finish - now) / 3600 + 1` | shows |
|---|---|---|
| 0 | 6, clamped | `Wait5min` |
| 3600 | 5 | `Wait5min` |
| 7200 | 4 | `Wait4min` |
| 18000 | 1 | `Wait1min` |
| 21600 | 0 | `ReadyForPlayback` |

The `+ 1` rounds a part-minute up, so the unit finishes a minute after its
nominal deadline -- and that is *self-consistent*, because the number on the
display is genuinely how many minutes are still to run. I had assumed a plain
countdown, written the test to my assumption, and then nearly changed working
code to match it. The table is now in the test as a comment.

One thing the original never has to handle: evaluated on the very tick a scan
starts, that expression comes out as six and indexes off the end of a
five-element list. It only recomputes when the player looks at the unit, by
which point the clock has moved. This engine recomputes every frame, so the
clamp is written down rather than left to an index that happens to fall off the
end.

### Two frame scripts, one dispatch chain

Adding Roxy's `exitFrame` silently broke Margaret's opening, ported an hour
earlier. The native dispatch is `shared || roxy || edwin || brice || margaret`,
first match wins, and Roxy's arm answered for every chapter.

I caught it because I ran the walk again rather than trusting the build. Both
arms are now guarded on `gChapter`, and there is a test that goes through the
real chain -- my first attempt at that test called Roxy's module-local `call`
and would have passed no matter what the dispatcher did.

That is the third check this month that could not have failed. I am starting to
think the rule is: a test that exercises a *module* proves nothing about a
*chain*, and every bug this class has produced was at a seam between two things
that each worked.

203 tests. Unported is 14 verbs across 18 call sites.

## 93. Counting what I had not been counting

`verify` says 14 unported verbs across 18 call sites, and I have been reading
that as "what is left". It is not. It counts verbs named in a room's action
list, and the last two entries were both handlers that no action list mentions.

So I counted the other kind. `tools/disasm.py --list` now enumerates every
handler in a movie, which it could not do before -- a movie has several frame
scripts sharing a name and asking by name returned whichever came first, which
is how I read Roxy's loader `exitFrame` for ten minutes while looking for its
scan countdown.

Event handlers across the four chapters:

| | ROXY | MARGARET | EDWIN | BRICE |
|---|---:|---:|---:|---:|
| `mouseDown` | 10 | 13 | 2 | 2 |
| `exitFrame` | 4 | 4 | 3 | 3 |
| `enterFrame` | 1 | 1 | 1 | 1 |
| `idle` | 1 | 1 | 1 | 1 |
| `startMovie` | 1 | 1 | 1 | 1 |
| `mouseUp` | 1 | | | |

**Fifty-four handlers this engine has never run.** Some are trivial --
`startMovie` sets a cursor, the `enterFrame`s are debug output. But 27 of them
are `mouseDown` sprite scripts, and the two I have read so far were Margaret's
entire chapter opening and the scan unit's whole interface. The interactive
devices live here: this is where the peek unit's display, the psionic bar's
panel and the radio's tuner are.

The number I have been quoting was never wrong, it was answering a narrower
question than it looked like -- which is the fourth time this month, after
`verify`'s unported tally, the effect coverage, and the `StopLoop` names. I
have stopped treating that as a coincidence. The shape is always the same: a
count that is easy to produce stands in for a count that is hard to produce,
and then gets quoted as though it were the hard one.

Both numbers now, and both said out loud: **14 verbs across 18 call sites, and
54 event handlers of which 2 are ported.**

## 94. Two Directors

helba pointed the window at the Macintosh data and got a black screen. It
reproduced headlessly on the first try -- `shot mac_game officeentry2` wrote a
PNG that was pure black while the same room from the PC disc rendered fine, and
both reported "1 sprites drawn".

The cast listing said what was wrong:

```text
mac_game/ROXY/ROXY.dxr: 2738 cast slots
  Unknown(1572864)   178
  Unknown(1835008)  1222
  Unknown(1966080)   851
  Unknown(262144)     21
  Unknown(65536)      64
```

Every member an unknown type, and the numbers are the giveaway: 1835008 is
0x1C0000, a small number sitting in the high half of a word that should have
held a small number in the low half.

**The Macintosh release is a Director 4 movie and the PC release is Director
5.** Same `RIFX` byte order, same chunk tags, different `CASt` record:

| | Director 5 | Director 4 |
|---|---|---|
| header | kind `u32`, info `u32`, data `u32` | data `u16`, info `u32` |
| then | info block, then type block | type block, then info block |
| kind | its own field | first byte of the type block |
| palette reference | spec + 0x1a | spec + 0x18 |

There is no kind field in Director 4 at all. What I had been reading as the
kind was the data length in the top half of the word and the top of the info
length in the bottom.

They are told apart by which arithmetic accounts for the whole record --
`12 + info + data` against `6 + data + info` -- and across Roxy's 2444 members
exactly one of them fits, every time, with nothing left over. That is a much
better test than sniffing a version number, and it is why the decision is a
pure function with both records from the disc written into its test.

Aligning the same member from both discs is what settled the bitmap header.
Cast 1590, `O_ENTRY2`, 600 by 300:

```text
PC   82 58 0000 0000 012c 0258  430c 0000 0000 0000  0096 012c 0008  ffff  0351
MAC  82 58 0000 0000 012c 0258  fff4 fff4 0138 0264  0096 012c 0008        0342
```

Identical up to and including the depth byte, and Director 4 has no `ffff`
field before the palette -- so everything works unchanged once the two-byte
prefix is skipped, except the palette reference, which sits two bytes earlier.

With that, the Macintosh release reads exactly like the PC one:

```text
before   rooms: 1320   named rooms: 0
after    rooms: 1325   named rooms: 1325
```

Those five rooms and all 1325 names were the embedded copies, which are cast
members, which is why none of them parsed.

### What is still different

The art is genuinely darker on the Macintosh disc, and not by a constant.
Margaret's rooms come out at about 0.6 of the PC's mean luminance, which is
what gamma 1.8 against sRGB's 2.2 does to an image. Roxy's come out at about
0.2, which is not.

The `BITD` chunks differ -- 168241 bytes against 158518, different hashes --
so this is different artwork rather than a decode fault, and rendering the
Macintosh bitmap against the PC's palette produces obvious colour artifacts
while its own palette produces a clean image. Both discs are being read
correctly. They just do not contain the same pictures.

Whether Roxy's chapter is *supposed* to be that dark I cannot yet say. The
obvious reference was helba's Macintosh longplay capture, and I deleted it --
see the next entry.

## 95. What the purge took with it

`git filter-repo` resets the working tree to the rewritten history. I knew it
rewrote commits and told helba so; I did not think about what that does to
files the rewrite removes. It deletes them.

Gone from the working directory:

| file | recoverable |
|---|---|
| `AMBER-Journeys-Beyond_Mac_EN.zip` | yes -- already unpacked to `mac_iso/` and `mac_game/` |
| `AMBER-Journeys-Beyond_Win_EN_CD.zip` | yes -- the ISO it holds was never tracked and is untouched |
| `playthrough/…Longplay - Macintosh….mp4` | **no** -- 932 MB, has to be downloaded again |

The two zips cost nothing: everything in them is already extracted and verified
against its catalogue. The longplay is a real loss. It is helba's Macintosh
capture, it is the reference I have used repeatedly to check what the game is
supposed to do, and it is the exact thing I wanted ten minutes later to settle
whether Roxy's chapter is meant to render as dark as the Macintosh disc draws
it.

Three things I should have done and did not:

1. Listed what the rewrite would remove before running it. I knew the paths --
   I had just printed them, sorted by size -- and I put them in the script
   without saying "and these files will disappear from your disk".
2. Noticed that `playthrough/` was in `.gitignore` *and* tracked. That
   combination is what a file added by mistake looks like, and it was in the
   list I read out.
3. Copied the three files somewhere outside the repository first. I did take a
   backup, and then deleted it after the rewrite succeeded -- having checked
   that the *commits* were intact and not that the *files* were.

The `.gitignore` now covers all of it, so nothing new gets swept in. That does
not help with what is already gone.

## 96. Ink, and a phone that would not go down

helba got stuck holding the phone in Roxy's living room, with two white
rectangles either side of it.

### The white

Every sprite in the game carries an `#ink`, and I had parsed the field into
`Sprite.ink` and never read it. Counting the values across all four chapters:

```text
   2345 #ink: 0
     15 #ink: 36
```

Two values, and the fifteen are all the same kind of thing -- something held up
in front of a room rather than part of it:

```text
DR-WDKLLR-CU                    #playerIsExaminingWeedkiller
ST-DRAWER-OPEN-ARTICLE-ACTOR    #playerIsReadingNoteInStudy
LR-PHONE-CU2                    #playerIsExaminingPhone
```

Each is drawn on a white field, and index zero is the only pure white in every
one of this game's palettes. So ink 36 means "do not paint the background", and
`Bitmap::to_rgba` has taken a transparent index since it was written -- every
caller passed `None`.

I have not modelled Director's ink table, because the data does not pose that
question. It poses one question, fifteen times.

### The phone

The phone was not actually stuck. Every hotspot in that room is guarded on
`[#equals: [#playerIsExaminingPhone, 1]]`, and with the phone raised the
keypad's rectangle sits on top of `putDownThePhone`'s, so clicking the middle
of the screen presses buttons and clicking anywhere else hangs up. That works,
and the walk shows it working.

What did not work was seeing it: the sprite that comes down is the one drawn
with the wrong ink, so the phone was still there afterwards over two white
blocks, and nothing about the screen said the click had done anything.

Which is the more interesting failure. A hotspot that does nothing gets
reported; a hotspot that does exactly the right thing invisibly gets reported
as being broken in some other way entirely, and I would have gone looking at
`putDownThePhone` -- which is fine -- instead of at the compositor.

### `shot ... flag=value`

Half of what a room draws is conditional, and a screenshot of the default state
cannot show the conditional half, which is the half that tends to be wrong. So
`shot` now takes `flag=value` pairs:

```sh
amber shot mac_game LivingRmPhoneCU out.png playerIsExaminingPhone=1
```

That is how I saw the white blocks without a window, and it is the third time
this month the answer was "build the thing that shows it" -- after the event
log in entry 58 and `mix` in entry 71.

206 tests.

## 97. The bar panel, and a third thing I was not counting

helba asked what was missing for the psionic bar's panel to turn on. Nothing,
on their side. Two handlers on mine.

The panel has four modes -- `#runOFF`, `#runON`, `#setOFF`, `#setON` -- and two
buttons. **Power** switches on and off without changing which mode it is in;
**mode** switches between running and setting without changing whether it is
on. And inside `setBarMode`, the reason to care:

```text
if i = #runON then
  if getState( #BarLevel ) = 6 and getState( #BarGain ) = 5
                              and getState( #BarFM )    = 8 then
    setState( #BarOnline, 1 )
```

**Level six, gain five, FM eight**, then set it running with the power on. That
is the whole puzzle and those three numbers appear together nowhere else.

Neither `#power` nor `#mode` is a mode. They are what the buttons *ask for*,
and `setBarMode` turns a request into a state. Without it the flag was being
set to the request itself, so the panel sat in a fifth state that is not one of
its four, and every guard downstream failed. `adjustBarSettings` -- ported in
entry 88, and correct -- tests for `#setON`, which the panel could never reach.

`setBarSelection` is the other half: which of the three digits the up and down
buttons act on, cycling level, gain, FM, and only while the panel is set and
switched on.

### The third count

Neither of my two "what is left" numbers could see these. `verify` counts verbs
a room's action list *names*, and nothing names `setBarMode`. The event-handler
count from entry 93 covers `mouseDown` and the frame scripts. This is a third
kind:

```text
on setState me, stateVar, suggestion
  if count(valueList) > 1 then ... else
    return value("set" & stateVar & "(" & suggestion & ")")
```

A flag whose value list holds exactly one entry is not a value -- it *declares
a setter*. The schema is therefore a list of handlers the game expects, and it
was never being read as one.

Fifty flags declare a setter. Reporting fifty would have been a number that
cries wolf: 29 of them have no such handler and correctly take the direct
write. Telling those apart needs the movie's actual handler list, so the
director crate can now read one -- the `Lnam` name table and the handler tables
in each `Lscr`, which the Python tooling has been able to do all along and the
engine could not.

The honest figure is **16**, and it includes `setdumbWaiter` -- which is what
moves the kitchen radio station along the dial in entry 83 -- `setcarLocation`,
`setVideoTapePosition`, and the page-turning for all three of the books.

So: **14 verbs across 18 call sites, 54 event handlers of which 4 are ported,
and 16 setters.** Three counts, three times I have found the number I was
quoting was answering a smaller question than it looked like. I have stopped
being surprised by this and started expecting a fourth.

211 tests.

*(A small one for the record: I first concluded the win condition was not
firing, because I grepped the trace for `BarOnline` and the trace prints that
key lower-cased. Case again, in my own tooling this time, ten minutes after
fixing twelve instances of it in the port.)*

## 98. `[#equals: [#always, 0]]`

The bar panel had its two setters and still did not work. The readouts were
missing, and the display showed a waveform that should not have been there.

`#always` holds 1. So `[#equals: [#always, 1]]` is the ordinary unconditional
guard -- 6356 of them -- and `[#equals: [#always, 0]]` is **never true**: it is
how the authors switched a sprite off without deleting it. There are four left
in the shipped data.

I had:

```rust
"equals" if key == "always" => Cond::Always,
```

which reads the key and ignores the value, so all four drew. One of them is
`BPANEL`, a 412 by 252 graphic on channel 10 -- above the digits on 7, 8 and 9
-- and it was covering the whole readout. The panel was working the entire
time; I was drawing a lid over it.

With the guard right, the panel does what it should: **SET** shows LEVEL, GAIN
and FREQ MOD with the selector dash beside whichever is being adjusted; **RUN**
with the wrong numbers shows `ERR`; RUN with six, five and eight plays
`BPANEL.MOV`, whose own guard is `[#equals: [#BarOnline, 1]]`.

### A correction to entry 89

The other three disabled sprites are `B_SHED_PEEPL_XCU_mir`, `G_BHPathUp5` --
and `MEewall.mov`.

Entry 89 said the PC release references five films it does not ship, and used
that as the headline finding about the two releases. One of those five is this
sprite. Its room has two video elements:

```text
MEewall-full   #castNum: 6   [#equals: [#always, 1]]
MEewall.mov    #castNum: 6   [#equals: [#always, 0]]
```

Same cast number, one live and one switched off. So `MEewall.mov` is named by
nothing that can appear, and the PC disc not carrying it is not an omission --
it was cut, and the Macintosh disc simply still has the file. I went and
fetched it from the Mac release to fix a hole that was not there.

The honest figure is **four** films the PC release references and does not
ship: Margaret's opening and the three scan-unit films. Those three are behind
live guards and are the ones that actually cost helba something.

`info` now marks the difference rather than leaving me to remember it:

```text
movies: 278 on disc, 196 referenced, 5 unresolved
  missing 40sINTRO.mov
  missing MEewall.mov  (only named by a sprite that never shows)
  missing ST-CPU-LED.multiframe
  ...
```

Four disabled sprites out of 6360 guards is a rounding error, and it cost a
puzzle. The pattern is the same as `#greater` in entry 77 and the vacuous
`#and` before it: a guard I read loosely because the common case was so common,
and the rare case was the one that meant something.

## 99. A film that only started if you walked

helba set the bar to six, five and eight, pressed RUN, and nothing happened.

Everything in the chain was right by then. The setters were ported, the guard
fix from the last entry meant the digits were visible, and `walk` showed the
whole sequence working -- `set baronline = Int(1)`, and the room reporting
`movie: BPANEL.MOV`.

The window showed nothing, because a room's film was only ever loaded when the
player moved:

```rust
// A move changes which movie is on screen, so reload it either way.
if outcome.destination.is_some() || outcome.go_back {
    self.start_room_video();
}
```

So can standing still. Which film a room plays is guarded like anything else --
`BPANEL.MOV` sits behind `[#equals: [#BarOnline, 1]]` -- so solving a puzzle can
make a film eligible where a moment ago there was none, and the player never
finds out. `video()` had been naming the film correctly the whole time; nothing
asked it again.

`Game` now remembers which film is loaded and reloads when the room's answer
changes, but not while a scripted sequence is running: `pushVideo` puts a film
on the same player, and reloading the room's own underneath it would cut the
sequence off part way.

### Why the terminal said yes and the window said no

This is the second time this week that the walk and the window disagreed, and
both times the walk was right and useless. `walk` calls `video()` fresh every
time it prints a room, so it reported the film the room *would* play; the
window plays the film the room *did* load. The tool that was supposed to answer
"does this work" was answering "should this work".

I noticed only because helba told me it still did not work after I had shown
myself it did. There is a version of this where I insist the walk output proves
the puzzle is fixed.

The fix for the tooling is the same as entry 96's: `shot` now applies its forced
flags *before* the room chooses a film, which is what made the difference
visible headlessly -- with the flags set afterwards it opened the film for the
default state and reported the sprites for the forced one, quietly answering
half of each question.

211 tests.

## 100. The shaft and three books

Seven of the sixteen setters, ported.

`setDumbWaiter` is the one worth the most. It goes up from the kitchen and down
from the bedroom and refuses anything else -- asking it to go up while it is
already up is not an error, it simply does nothing. The flag holds three things
in turn: where it is, then the direction it is travelling while the film plays,
then where it has arrived. That middle value is why this cannot be a plain
write; a sprite keyed on `#dumbWaiter` shows the shaft moving during it, and
the arrival is an effect so it lands after the film rather than while the shaft
is still on screen.

And moving it moves the kitchen radio station along the dial, which was entry
83's finding. Two puzzles, one shaft, and until now the shaft did not move.

The three books -- the dream diary, Realms, and the bar manual -- are one shape
three times. Turning past either end closes the book rather than stopping at
it, and opening one always starts at its first page: there is no bookmark,
which is why the diary is meant to be read straight through.

Their page lists are the detail I would have got wrong:

```text
dream diary   [1, 2, 3, 5, 6]
realms        [0, 1, 3, 5, 7, 19, 21, 35, 37, 51, 53]
bar manual    [0, 1, 2, 3, 4, 5]
```

Not ranges. They are the frames each page lives on, so the gaps are spreads
rather than missing pages, and "the page after three" is five. Incrementing the
number -- which is what I would have written if the bar manual had been the one
I read first -- turns to a frame that is not a page at all. The test says so in
those words, because the next person to look at `[0, 1, 2, 3, 4, 5]` will think
the same thing I did.

Nine setters left: the laptop, the car, the peek unit's two status displays,
two doors, the tape position, the waffle tracks, and `setcurrentLocation`,
which is the engine's own business rather than a puzzle's.

217 tests.

## 101. Displays that correct what they are told

Three more setters. Nine left down to six.

`setPKamberStatus` is the interesting one. The peek unit's amber display does
not show what it is asked to show; it shows the most it can honestly claim:

```text
if suggestion = #Online then
  if getState( #psionicWavesPresent ) then suggestion = #WaveActivated
if suggestion = #WaveActivated then
  if getState( #oscillatorInPlace ) = 0 then suggestion = #WaveButIncomplete
```

The two corrections chain, which is the part I would have missed writing this
from a description: ask for `#Online` with the waves present but no oscillator
and it lands on `#WaveButIncomplete`, having been promoted once and demoted
once on the way. The test walks all four corners for that reason.

`setPKbarStatus` has the refusal without the correction, and is worth porting
for the refusal alone: without it a status the unit does not recognise gets
written anyway, and the sprite keyed on it finds no cast and draws nothing. A
display showing a wrong thing is a bug; a display showing *nothing* reads as a
broken engine.

`setvideoTapePosition` is one line -- it writes the flag and stops. Porting a
handler that does exactly what the fallback would have done looks like waste,
and is not: the fallback is what happens when a setter is **missing**, so
leaving it out means the tally cannot tell "this handler does nothing
interesting" from "nobody has looked at this yet". Now it can.

219 tests.

## 102. A fourth case bug, in my own hands

Going after the last setters, `disasm.py` told me `setshedDoorIsOpen` did not
exist in any chapter. It does, in two, spelled `setShedDoorIsOpen`. The tool
compares handler names exactly:

```python
if hn != want:
    continue
```

The schema spells it one way and the score the other, because Lingo does not
care. That is the fourth time this week: the whirligig's direction tables in
entry 85, the `.DXR` filenames in entry 89, twelve state comparisons in entry
91, and now the disassembler. Three of the four were in tooling I wrote after
already having been bitten.

The rule is not "watch out for case". The rule is that **nothing in this game's
data is case-sensitive**, and any comparison I write against it that is, is
wrong -- whether it is in the port, the tools, or a grep I type at the prompt.

### The last of the doors

`setShedDoorIsOpen` is a bleeding door of exactly the shape Roxy's front door
has: the shed's own doorway is the one place the outside is audible through it,
so `#outsideLoop` starts and stops only while standing there. Opened from
anywhere else it makes its noise and leaves the mix alone, because the room you
walk into declares its own.

`setWaffleTracks` is not a flag but a set -- the tracks the car has been down,
accumulated. Asking for one already in the list is neither an error nor a
repeat: the cursor twitches and nothing happens, which is the game saying "you
have already done that" without saying anything.

Four setters left. `setcurrentLocation` is the engine's own business and the
other three -- the laptop, the car, the conservatory door -- each carry enough
of their own machinery to be worth reading properly rather than squeezing in
here.

222 tests.

## 103. The setters, all of them

`unported setters: none`. Sixteen when the count first existed two entries ago.

The last four:

**`setPlayerIsUsingLaptop`** is eight states with side effects on the way in.
Two matter. `#password` *freezes the inventory*, so while the cursor is in the
password field the player cannot pick something up and use it -- the bar goes
cold rather than silently ignoring clicks, which is a real piece of manners.
And `#off` clears `#passwordAttempt`, as does closing the lid: a wrong password
is not remembered, so switching the machine off and on is a genuine reset and
not a way to keep guessing from where you left off.

Porting it turned up `unFreezeInventory` missing entirely -- `freezeInventory`
had been ported and its mirror had not, so the bar could freeze and never thaw.

**`setCarLocation`** has seven places in two groups: three states of the car
and four positions on the hub, and only the last four redraw the hub display.
The split is made by *position in the list* rather than by name, which is why
the order of that list is not arbitrary and why I wrote it out rather than
sorting it.

**`setConservatoryDoorIsOpen`** is a bleeding door that does not balance.
Closing it stops the outside in two rooms; opening it starts the outside in
one. Stand at `#Cons_CenterS` and the outside dies when the door shuts and does
not come back when it opens. I read both branches twice looking for the reading
that makes them agree and there is not one, so it is reproduced as it is, with
a test that says in words that this is faithful rather than fixed. Someone
tidying this later would otherwise "correct" it in five minutes.

**`setCurrentLocation`** is a stub -- `on setCurrentLocation suggestion` and
then `return`. The flag holds a single value, which declares a setter exists,
and the empty handler is how `setState` is told to fall through to the plain
write. Moving the player is `moveToLocation`'s job.

I ported the stub. A handler that does nothing and a handler nobody has opened
look identical from outside, and the entire value of the count in entry 97 is
that it distinguishes them.

228 tests. What is left: **14 verbs across 18 call sites, and 50 event
handlers.**

## 104. `assertSound` is not `soundEffect`

I went to port the last verbs -- `carComments`, `keyholeComments`,
`windowHints` -- and found they are all the same three lines: check whether a
remark is still in `#utterancesRemaining`, and `assertSound` it. Which sent me
to `assertSound`, which I had been treating as a synonym:

```rust
"assertsound" | "soundeffect" | "startsound" | "playsting" => { ... play it ... }
```

It is not a synonym. It is the mechanism by which **a line is said once,
ever**:

```text
on assertSound whichSound
  if not inState( #utterancesRemaining, whichSound ) then return
  if whichSound = #thoseBees and inState( #utterancesRemaining, #youBees ) then return
  sndDelay = getaProp( [#handwriting: 120], whichSound )
  if voidp( sndDelay ) then sndDelay = 60
  wait sndDelay
  <play whichSound>
  trimState( #utterancesRemaining, whichSound )
```

A line not in the list is not said at all, and saying it takes it out.

That is worth more than the three handlers I set out to port, because the same
remark is placed in many rooms. `assertSound #victoryGarden` appears in **seven**
of Margaret's; `#tedsComingHome` and `#dontWannaStay` in five each. They are one
observation the player might happen upon anywhere, not seven of them -- and
until now she made it every single time. Sixty-two call sites across the game,
drawing on lists of seventeen to twenty-five lines per chapter.

helba said, weeks ago, that the sounds of the character talking to herself
seemed off. They were.

Three details worth keeping. The pause before speaking is sixty ticks, with one
exception per chapter, and Edwin's goes the *other* way -- `#windControl` waits
fifteen, because it is a shout rather than a remark. Brice's bees have an
order: he will not say whose bees they are before he has said anything about
bees at all, and Edwin's chapter carries a copy of that test against lines it
does not have, which is a paste rather than a rule. And Roxy has no
`assertSound` handler and none of her rooms call it -- she is the one who does
not talk to herself.

The trim happens immediately rather than after the pause, where the original
puts it. The original blocks on its `wait`, so a second `assertSound` for the
same line later in the same list finds it already gone; deferring the trim in
an engine that queues its waits would let the line speak twice. The cost is
that a guard read during the pause sees the line already spent, which nothing
in the game does. That trade is in the comment.

233 tests.

## 105. What they say, and how often

The three comment handlers, now that `assertSound` means what it means.

`keyholeComments` is two lines and the whole design of the thing:

```text
if inState( #utterancesRemaining, #someTrouble )
  then assertSound #someTrouble
  else assertSound #concernedCitizen
```

The first look through the keyhole, Brice suspects trouble; after that he is
only a concerned citizen. And the *fallback is itself an `assertSound`*, so the
third look is silent. Two remarks and then he stops, without anything counting
anything -- the utterance list is the counter.

`windowHints` gives two remarks in a row the first time: he notices the window
is hers, waits for that line to finish, pauses another second, and asks it to
tell him something. Afterwards he says the pattern is nice.

`carComments` works down a list. Chippy wants to go home; failing that he will
settle for going anywhere; failing that it is a joy ride. Separately he
mentions he cannot see out. The one guard is that Chippy has to actually be in
the car -- he does not narrate a journey he is not on.

The shared rule moved to `natives::assert_sound`, because these handlers call
it and `script.rs` dispatches it, and two copies of something this quiet would
have drifted apart inside a week.

The tests read like the game: `the_keyhole_has_a_first_look_and_a_second`, and
then a third that is silence.

Verbs down from 14 to 11.

236 tests.

## 106. Three ways of choosing what to say

`goodbyeMandy`, `pyramidSpeaks` and `chippySpeaks` -- three handlers that each
pick a line, and pick it differently.

**`goodbyeMandy`** does not pick at all. It is the end of Brice's chapter: six
montage steps, two films, one remark, and then `enterNewDomain` to Roxy. The
middle of it uses `#slowMontage` rather than `#fadeIn`, which is the one
transition in this game with its own speed -- a third of the rate, found in
entry 79 and until now used by nothing I had ported. This is what it is for.

**`pyramidSpeaks`** takes `#helpMe` off the front and everything after that at
random:

```text
helpTest = getAt( remainingMessages, 1 )
if helpTest = #helpMe then
  myAnswer = 6 : deleteAt( remainingMessages, 1 )
else
  msgPosition = random( count( remainingMessages ) )
```

So the pyramid says the same thing first to everyone and a different thing
second, and once the list is empty it has nothing left to say. The first
message is not drawn from the pile; it is the pile's lid.

**`chippySpeaks howLikely`** is the other way round -- *whether* he speaks is
random and *what* he says is not:

```text
if integerp( howLikely ) then highRoll = howLikely clamped to 1..6
                         else highRoll = 6
if random(6) <= highRoll then ...
```

The argument is how likely out of six, and it defaults to six -- certain --
when it is not a number at all. So `chippySpeaks 2` is a one-in-three chance
and a bare `chippySpeaks` always speaks. He then works through `#chippyPleas`
from the front, so the order he asks for things in is fixed even though whether
he asks is not.

That default is the part worth having read rather than guessed. A missing
argument meaning "certain" is the opposite of what I would have assumed, and
the test walks forty seeds to say so.

Verbs down from 11 to 8. 239 tests.

## 107. The car and its tracks

`chooseTrack`, which is Edwin's car, and which works two different ways
depending on where the car already is.

**At a hub** the three directions each lead somewhere and the film is a third
of `waffle.mov`. The main hub gets 0-223, 225-448 and 450-675; the three
lettered hubs share a shorter set at 0-178, 180-358 and 360-540. Six stretches
of one film, which is the same trick the music boxes use in entry 70 and the
weather vane in entry 84. This game does that constantly.

**Already on a track** it is a lookup instead, and the two tables are each
other backwards:

```text
right  [#c: #B, #B: #A, #AL: #AM, #AM: #AR, #BL: #BM, #BM: #BR, #CL: #CM, #CM: #CR]
left   [#B: #c, #A: #B, #AM: #AL, #AR: #AM, #BM: #BL, #BR: #BM, #CM: #CL, #CR: #CM]
```

A direction with no entry for the track you are on does **nothing at all** --
no film, no sound, no refusal. That is how the dead ends are expressed: not
"you cannot go that way" but an absence. Worth a test of its own, because an
engine that reported the refusal would be adding something the game does not
have.

### 898

I wrote the left-hand film stretches as the right-hand ones with 450 and 360
added, because that is what they look like:

```text
right   #B: [0, 448]     #BM, #AM, #CM: [0, 358]
left    #B: [450, 900]   #BM, #AM, #CM: [360, 720]
```

450 + 448 is 898. Everything else in that table shifts exactly and `#B` does
not, so the derivation is wrong for one entry in four and right for the rest --
which is the worst kind of wrong, because it works when you spot-check it.

This is the second time in this port I have written a rule where a table
belongs; the first was the weather vane in entry 84, where I caught it by
reading. This time the test caught it, which is better. Both tables are now
written out.

Verbs down from 8 to 7. 243 tests.

## 108. The boat

`setSail`, and it is four lines that reach two rooms away:

```text
if boatPos = #forward  and windDirection = #E then setState( #boatPosition, #backward )
if boatPos = #backward and windDirection = #W then setState( #boatPosition, #forward )
```

The boat goes where the wind sends it and nowhere else. An east wind pushes it
back, a west wind brings it forward, and any other wind -- or the right wind
against the wrong position -- does nothing at all. So this is the weather vane
from the other end: `setWeatherVane` in entry 84 decides which way the boat
works, and nothing in either handler mentions the other.

Bringing the boat forward while Teddy is waiting is also the only place Teddy
ends up on the anchor.

Three puzzles wired together through `#Wind` -- the vane sets it, the whirligig
reads it to pick which film to spin, and the boat reads it to decide which way
it can move. None of the three knows about the others.

Verbs down to 6.

## 109. The portal, and a report that lied

helba: "massive issues around click portal to transition". So back to
`MargPortal_headOn`, which is the mirror into Margaret's chapter and the room
in the very first screenshot of this whole project.

### What I got wrong first

I ran the walk, saw this --

```text
set showmontage = Int(1)
set showmontage = Int(2)
set showmontage = Int(0)
```

-- three writes back to back with no film between them, and concluded that
`setState` in an action list writes immediately while `pushVideo` and `wait`
are queued, so the whole sequence was mis-ordered. I measured the blast radius
(59 of 5909 action lists write state after a wait), wrote the fix, and it
changed nothing.

It changed nothing because `pump` already blocks at each wait and resumes on a
later frame. The ordering was right. What was wrong was `settle`, the
walkthrough's shortcut: it runs the *whole* script and only then drains the
effects, so every state write is reported before the first film no matter what
order they actually happen in. I reverted the fix.

That is the second time this week the walk and the window have disagreed and
the walk has been the liar -- entry 99 was the same shape. Both times I built
the diagnostic once and then trusted it past the point it was answering the
question. `settle` now interleaves, and reports films, waits and state writes
rather than only sound:

```text
play toMargaret
film stops
film margntry.mov
wait for the film
film stops
film 40sFRAME.mov
wait for the film
...
```

Three films live on that room's video channel, chosen by `#showMontage`, and
the sequence steps 1, film, 2, film, 0. It has been doing that correctly all
along.

### What was actually wrong

`MEmrloop.mov` is **160 by 120**. Its cast member's rect is **320 by 240**.

Director draws a film into the rect its member declares and scales it to fit;
this engine drew every film at the size the decoder handed back. So the loop
behind the portal was a quarter-size patch in the middle of a black screen --
which is precisely what that first screenshot showed, and what helba has been
describing since.

It is one film in this room and only a handful across the game, which is why
nothing else looked obviously wrong. The other two in the same sequence are
stored at exactly their member's size, so the portal's own transition looked
fine while the thing it sits on did not.

The member's rect is four `i16`s at the top of the type-specific block, the
same place a bitmap keeps its own, and it reads identically on both pressings
once entry 94's layout work is in.

Scaling is nearest neighbour, which is what Director did and what this material
wants: doubling a 160 by 120 film with anything smoother invents detail the
original never had.

245 tests.

## 110. Making the screenshot answer the same question as the window

helba sent a picture of the portal with a tall white panel over the left of the
stage, black inside, a red line down one edge. Nothing like it appears in any
headless render of that room, in any montage state.

Because `shot` did not draw the inventory bar. The window composes the stage
and then draws the bar over it; `shot` only did the first half, so for the
whole life of this tool a screenshot has been answering a slightly different
question than the one the player is looking at. It draws the bar now.

That did not produce helba's panel either -- but it did show something else
wrong. An item in hand renders as a small dark shape with yellow and green
edges where there should be a scanner: a palette fault, plainly, and
reproducible from the terminal.

The icon table is read once from whichever chapter yields one, and the comment
above that code says it is the same in every chapter. It is not: only Roxy's
movie carries it. The numbers in it are therefore Roxy's cast numbers, and
`draw_inventory` resolves them against **whichever chapter the player is
currently in** -- so an icon is right in Roxy's rooms and is some other
member entirely everywhere else.

That is not what is wrong in Roxy's own portal, where the render is still off,
so there is a second fault in the same few lines and I have not found it yet.

I have stopped guessing at helba's state. The recorder from entry 64 exists for
exactly this and I have not once asked for a recording.

## 111. A bug I introduced, and one I still cannot see

helba: the panel is "a squished version of the frame". That places it: my own
sprite trace draws `MARG loadpict` at **(94, 64)**, and the left edge of the
panel in the screenshot is at 93. Same plate, same position, roughly a quarter
of the width -- full height, so it is the width alone that is wrong.

Rendering that room headlessly at `showMontage=4` draws it at 452 by 354 and
looks right, on the same Macintosh data. So I cannot reproduce it from state,
and everything below is what I found while failing to.

**One real bug, and it was mine, from two entries ago.** `start_room_video`
remembers the rect a film occupies; `play_movie` with a *named* film did not.
So a film pushed by name was drawn squeezed into whatever shape the previous
film had left behind. That is the same kind of fault as the panel and it is
fixed, but it cannot be the panel: `MARG loadpict` is a bitmap and does not go
through that path at all.

Every film member's rect is plausible -- I checked all 108 in Roxy's movie for
a zero or absurd width, and there are none -- so the scaling introduced in
entry 109 is not mis-firing on bad data either.

What is left is the difference between the window and a screenshot: the
transition buffer, the real clock, and a window the player has resized. I have
been reading a screenshot and inferring state for three rounds now, and I got
one of those rounds wrong badly enough to "fix" working code. That is enough.
The recorder has existed since entry 64 and I have never once asked for a
recording.

## 112. Depth forty

helba: "it's the frame, it's still squished, it happens during the transition".

That was enough. `40sFRAME.mov` is the white frame the portal flashes up on its
way into Margaret's chapter, and it is **not Cinepak** -- it is Apple Animation,
which is why the decoder trace I had just added showed nothing for it.

QuickTime writes a greyscale track's depth as **32 plus its bit count**. So
8-bit grey is 40, not 8. My decoder read the depth straight:

```rust
let unit: usize = match depth { 0..=8 => 4, _ => 1 };
let bytes_per_pixel: usize = match depth { 0..=8 => 1, 16 => 2, 24 => 3, _ => 4 };
```

Forty falls off the end of both. So a unit became one pixel instead of four and
a pixel four bytes instead of one, and each line filled a quarter of its width
with the channels sheared -- a squeezed picture with a red edge down one side
and a blue one down the other. Which is exactly what helba has been
photographing for me.

The file's own comment, written when I ported this codec, says:

> At eight bits a "unit" is four pixels, not one: the counts are in groups of
> four and the skip byte steps four at a time. Reading them as single pixels
> decodes a quarter of each line and smears it across the rest.

I had the failure mode written down, in this file, above the code that has it.
What I did not have was any reason to think a film could say 8 bits in a way
that does not read as 8.

**One film on the disc says 40**, out of 127 Apple Animation films. One. And it
is the one in the transition that helba has been reporting since the very first
screenshot of this project.

### The part that is mine to own

I rendered this exact frame -- `showMontage=2`, the same command, the same
data -- three rounds ago, wrote the PNG, and never opened it. The bug was
sitting in a file on disk while I told helba I could not reproduce it and asked
them to send a recording. Then I asked twice more.

Producing the evidence is not the same as looking at it. I have a headless
renderer specifically so I can see what the player sees, and I used it to
generate an image and then reasoned about what the image probably contained.

## 113. The real cursors

helba asked when I wanted to do the cursors. Now, as it turns out: the whole
mapping is two handlers and one table, and the art has been decodable since the
bitmap work in entry 94.

`setUpGame` in the hub movie builds `YugoCursors`:

```text
browse 6018   left 6012     right 6006    forward 6001
examine 6024  up 6111       down 6112     pointer 6100
back 3003     noCursor 128  nextPage 6110
rotateLeft 6119             rotateRight 6109
WeedKiller 6102  ScanDevice 6103  Oscillator 6108  Headgear 6107
BedroomKey 6106  Crowbar 6105     Videotape 6104
```

and `castCursor` turns an id into a pair of cast members:

```text
image = 2500 + (id - 6000) * 2
mask  = image + 1
```

Two one-bit members: the image says black or white, the mask says which of its
pixels exist. Cropped to sixteen square they are an arrow for forward, a
viewfinder with an X through it for examine, and a diamond for browse -- and
they have been sitting in the movie the whole time behind shapes I drew by
hand.

Three things worth keeping.

**An item in hand replaces the cursor with itself.** The last seven entries in
that table are inventory items, so carrying the scan unit makes the pointer the
scan unit, whatever region it is over. That is a nicer piece of interface than
the arrow-plus-implied-item I had.

**Two ids are not cast members at all.** `#back` is 3003 and `#noCursor` is
128, both below the 6000 the arithmetic assumes; they are system cursors, and
the second is how the game hides the pointer. Running those through
`2500 + (id - 6000) * 2` asks for a cast below 2500, which is somebody else's
picture entirely. The drawn shapes stay as the fallback for exactly those.

**The members are not sixteen square.** They are eighteen or nineteen, and a
pair does not even agree with itself -- forward is a 19 image against an 18
mask. Whatever the extra rows are, most likely the hot spot, they are not the
picture: drawn uncropped every cursor wears a fringe of speckle down two sides.

Not done: the hot spot itself. Director stores one and this centres instead, so
a click lands within a few pixels of where the art points. Worth reading
properly, but not before it is the thing that costs someone a puzzle.

There is also a smaller finding from looking: the Macintosh disc has files
called `ROXY cursors` and `MARGARET cursors` which extract as **zero bytes**.
They are resource-fork-only, and `tools/hfs.py` reads the data fork alone. The
cursors do not need them -- the casts are in the movie -- but any other
resource-fork file on that disc is invisible to me, and I had not noticed.

250 tests.

## 114. Wrong cursor, wrong place

helba, on the new cursors: "i do think we're showing the wrong cursors in wrong
places". Right, and the cause is not in the cursor code at all.

`Verb::ItemInUse` ranks **above** `Verb::Browse` whether or not the player is
carrying anything. Nearly every room opens its hotspot list with

```text
ItemInUse (-8, 60, 645, 376) ["stowInventory( getState( oStoryteller, #itemInUse ) )"]
```

-- a region covering the whole stage. So with empty hands that region won
everywhere the directional regions did not reach, which meant most of most
rooms showed the pointer cursor instead of the browse diamond, and a click on
the scenery stowed nothing instead of walking.

The enum's own doc comment says what should happen:

> Fires only while the player is carrying something, to use it on the scene.

It says it, and the code did not do it. One line: an `#itemInUse` region is out
of the running with empty hands.

Cursors were the thing that made this visible. The behaviour has been wrong
since hit-testing was written; nothing showed it, because a click landing on a
whole-stage region that does nothing is indistinguishable from a click landing
on nothing.

### `cursorOff`, the fifth one

helba also asked for the desktop pointer to be hidden, which it now is -- the
game draws its own. That made a second fault obvious immediately: every set
piece opens with `cursorOff`, and the pointer sat there through all of them.

`Effect::CursorOff` is emitted at a hundred and four call sites and was applied
at none. **The fifth** effect in this engine to be produced and dropped, after
`PlayVideo`, `new_domain`, `FadeToMontage` and `PlayVideoSegment`.

Which is the part worth writing down, because entry 84 added a test to stop
exactly this, and the test passed. It asked whether a variant is *mentioned* in
a file that applies effects. `Effect::CursorOff` was mentioned -- in a list of
effects to emit. So the guard I built against dropping effects on the floor
could be satisfied by dropping an effect on the floor next to a mention of it.

It now requires a **match arm**, and I checked it by deleting the arm I had
just written and watching it fail:

```text
Effect variants never applied: ["CursorOff"]
```

That check took thirty seconds and I did not do it the first time. A test
written to catch a class of bug is worth nothing until it has caught one, and
the cheapest way to find out is to make the bug on purpose.

250 tests.

## 115. Off by one cast

helba, twice: the cursor on the portal is wrong. It was, and by exactly one
cast member.

I had read `castCursor` from a fragment and written down

```text
image = 2500 + (id - 6000) * 2
mask  = image + 1
```

The handler in full says otherwise:

```text
whichCursor = cursorID - 6000
cMask = 2500 + whichCursor * 2
cursor( [cMask - 1, cMask] )
```

The **mask** is at the computed offset and the image is the cast *below* it. So
every cursor was drawing its own mask as its picture, and the next cursor's
picture as its mask.

Which looked fine, and that is the point worth keeping. A mask is a filled
silhouette of the right shape, so an arrow still looked like an arrow and a box
still looked like a box; only the shading was nonsense and the symbol inside
came from the wrong cursor. helba saw an X where the examine cursor has a
question mark -- 2549 rather than 2547 -- and I had checked my work by looking
at the composite, which is precisely the view that hides the error.

Printing the members raw and separately settles it in a second:

```text
2547  box outline with a ?      detail  -> an image
2548  solid filled box          silhouette -> a mask
2549  box outline with an X     detail  -> an image
```

Images at odd casts, masks at even, pairs of `(mask - 1, mask)`. Once you see
one image next to one mask the ordering is not arguable, and I had never looked
at them apart.

250 tests.

## 116. The last drive

`driveTheCar` was one of the last two verbs with call sites and no arm, and it
turned out to be the handler where Edwin's separate puzzles finally meet.

The shape is a track lookup: `#currentTrack` picks a film, the track loop plays
under it, the film plays, the loop stops. Most tracks have one film named after
themselves. `#BM` has two, depending on whether Chippy is riding along. `#CM`
has four, and that is the interesting one:

```
if currentTrack = #CM then
  if chippyLocation = #inCar    then film = #CM_missRamp
  if boatPosition   = #forward  then film = #CM_anchorDown
  if teddyLocation  = #onAnchor then film = #CM_teddyRescue
  else                               film = #CM_emptyAnchor
```

Four sequential `if`s assigning one local, not a chain of `else if`. So the
**last** match wins, not the first, and that distinction is load-bearing rather
than incidental: bringing the boat forward is exactly what puts Teddy on the
anchor, so after a successful rescue both tests are true at once. Read as a
first-match chain the payoff film never plays and you get the anchor coming
down forever. I have made this mistake before in this codebase -- entry 108's
`pushNail` has the same shape -- and it is the sort of thing that produces a
game which is not obviously broken, merely wrong at the one moment that matters.

The chain it closes runs a long way back: the weather vane sets the wind, the
wind is what `setSail` reads to decide whether the boat comes forward, the boat
coming forward drops the anchor with Teddy on it, and `driveTheCar` on the
middle track of C is where you see the result. Three handlers ported across
three different sittings, and none of them looked like part of a puzzle on its
own.

Driving also clears `#waffleTracks`, so the record of where the car has been
starts again with each journey.

The coverage test failed, which is the correct outcome and worth recording.
Entry 81's rule is that `an_unported_verb_says_so_too` names the unported verbs
deliberately, so that porting one breaks the test rather than silently
weakening it. `drivethecar` was on that list. It broke. The list is now down to
`inittelegrampuzzle` and `set`.

Separately: clippy has gone from zero warnings to forty-eight, spread over
twenty-one files and none of them touched today. That is a newer toolchain
turning on new lints, not a regression, but the gate I have been quoting as
"zero warnings" is no longer true and I would rather say so than let it drift.

252 tests. Two verbs left.

## 117. Three thousand eight hundred hard cuts

helba said the transitions between rooms were never hooked up. My first reading
of the code said otherwise: `Effect::SetTransition` is applied, the outgoing
frame is kept, a dissolve is stepped each frame and blended into the presented
buffer. All of that is real and all of it works. It is also reached from 103
places out of 3809.

The other 3706 go through `goTo`, and this is what `goTo` does:

```
on goTo destination, transition
  ... cursorOff ...
  if destination = #destination and transition = #transition then return
  ... previousLocation = currentLocation, currentLocation = destination ...
  killVideo
  moveMovies( oPuppeteer )
  setTransition( oPuppeteer, transition )
  moveToLocation( oPuppeteer )
```

The second argument of a move is handed straight to the same `setTransition`
I already implement, in the statement before the move happens. So the flavour
on a move *is* the transition for that move. Every real move in the game names
one -- `#forward` 984 times, `#turnRight` 940, `#turnLeft` 933, `#lookAt` 539,
`#backOff` 228, `#lookUp` 84, `#lookDown` 82, `#fadeIn` 18 -- and this engine
parsed all of them into `Outcome::transition`, merged that field carefully
across combined outcomes, and never read it. Written in seven places, read in
zero. The sixth effect in this codebase to be produced and dropped.

I want to be accurate about who was right. helba's report was that transitions
between screens were not hooked up, and mine was that they were. Both
statements described the same code; the difference is that I had checked
whether the machinery existed and they had checked whether the game did it.
The second question is the one worth asking, and it is the one my coverage
tests keep failing to ask -- five previous entries in this log say so.

What the flavours mean turns out to matter more than wiring them up at all.
`setTransition` looks the flavour up in a property list on the puppeteer and
stores a `puppetTransition` argument string. `birth` builds that list:

```text
#turnRight   02,1,16,TRUE      #lookUp      03,1,16,TRUE
#turnLeft    01,1,16,TRUE      #lookDown    04,1,16,TRUE
#forward     26,2,0,TRUE       #fadeIn      26,2,0,TRUE
#lookAt      26,2,0,TRUE       #slowMontage 26,3,0,TRUE
#backOff     26,2,0,TRUE       #nextPage    2,2,16,TRUE
                               #prevPage    1,2,16,TRUE
```

`whichTransition, time in quarter-seconds, chunkSize, changeArea`. Director's
codes 1 to 4 are wipe right, wipe left, wipe down, wipe up; 26 is a dissolve.

So **turning is not a dissolve**. It is a quarter-second wipe advancing in
sixteen-pixel chunks, and the direction is the opposite of the turn: turning
left is a wipe travelling right, because the new view enters from the left as
the world swings away. Looking up wipes down, looking down wipes up. Only
moving forward, looking at something, and backing off are dissolves, and those
take twice as long.

That distinction is the whole point of the feature. A crossfade between two
views says the picture changed. A hard edge sweeping across says *you turned*.
Nearly two thousand of the game's moves are turns, and rendering them as
crossfades -- which is what this engine would have done if I had wired the
field up without reading the table -- would have been the more insidious bug,
because it would have looked like it worked.

The old code kept the timings and threw the codes away: 1/14 and 1/45 of a
change per frame, which are one and three quarter-seconds at sixty frames a
second. Right numbers, no idea why. They now come from the table.

Verified by deleting the arming and watching
`the_flavour_on_a_move_is_the_transition_for_that_move` fail, which is the
standard I set in entry 84 after the coverage test that could not fail.

Also noting, because the gate I keep quoting is no longer true: clippy has gone
from zero warnings to forty-eight across twenty-one files, none of them touched
in this entry or the last. A newer toolchain turning on new lints, not a
regression, and a sweep for another sitting.

258 tests.

## 118. Two ports that never got committed

`transitToEdwin` and `forcePalette` were written, built and verified before the
last break and then sat in the working tree while three commits went past them
naming other files. I found them in the diff of a clippy sweep, which is not
where you want to find your own work.

`transitToEdwin` is Roxy's chapter handing over to Edwin's, and the same shape
as `goodbyeMandy` ending Brice's: two montage steps with a film on each, a
fade to the third, then `enterNewDomain`. The monitor is switched off first --
`#AMBERVISION` to `#off` -- because Amber vision is not something Edwin's
chapter has.

It also calls `castCursor #toEdwin`, and that one is worth a note because it
looks like something missing. `castCursor` takes a number; given a symbol it
prints "wow, a cursor label" and returns. It is dead in the original, so
leaving it out loses nothing.

`forcePalette` is ported as an empty arm, deliberately. Director has one
palette for the whole stage, so a room whose art was authored against a
different one has to force it before drawing. This engine resolved that
differently back in entry 94: every plate carries the number of the palette it
was drawn against and is decoded against that. There is no stage palette here
to force. An empty arm with the reasoning next to it is better than leaving it
on the unported list, where it would read as work outstanding rather than work
done another way.

The lesson is about the commit, not the code. My habit of staging named paths
-- which exists because `git add -A` once swept 1.9 GB into this repository --
means anything I forget to name stays behind indefinitely and silently. Naming
paths is still right. Checking `git status` before saying a thread is finished
is the part I skipped.

## 119. Forty-eight warnings that were never mine

A toolchain update turned on lints this codebase had never been measured
against, and the gate I have been quoting at the end of every entry -- clippy
clean -- quietly stopped being true. Forty-eight warnings across twenty-one
files, none of them in anything I had touched recently.

Forty-one were mechanical and `--fix` applied them. The interesting thing is
that most were the same observation twice: `chunks_exact(N)` with a constant N
is now `as_chunks::<N>()`, which returns the complete chunks and the remainder
separately rather than silently dropping the tail. Same behaviour, but the
type now says what the old call only did. That pattern is all over the audio
and bitmap decoders, which is exactly where a silently dropped tail would be
hard to see.

Seven needed a decision:

- The sample-to-chunk loop in the demuxer walked a range writing one value into
  each slot. It is a `fill` over a slice, and reads as the assignment it is.
- `Video::Animation` carried an `Rle` inline, some eight hundred bytes against
  Cinepak's seventy, so every player was sized for the larger. Boxed.
- Two tuple types wide enough that clippy stopped reading them got names --
  `StageElement` and `Cue` -- which is an improvement I should not have needed
  a lint to make.
- `blit` takes eight arguments. `blit_scaled` beside it takes ten and has
  carried an allow since it was written, so `blit` now does too. Threading a
  surface struct through a hot primitive to satisfy a count is not a trade I
  want.

The rest were `unwrap_or`, `is_none_or`, `div_ceil` and `repeat_n` standing in
for hand-written equivalents, all identical in behaviour.

Nothing here changes what the engine does, and I checked rather than assumed:
258 tests, `verify` unchanged at 1374 sprites and no dangling references, and
Margaret's walkthrough replays to the same rooms.

Clippy clean again, and this time I know what the number means.

## 120. Playing it

helba pointed out that the fun part would be reading the hints and actually
playing the game, and they were right, though not for the reason either of us
expected. Four bugs in the first ten minutes of play, and every one of them was
invisible to `verify`.

### The opening never ended

`Gbhs_playIntro` has exactly one hotspot and its action is the string
`"nothing"`. There is no way out of it by clicking on the room, and the engine
sat on `intro.mov` for ever. Anyone starting a new game got the opening film
and then nothing, which is a fairly complete failure to have noticed.

The way out is in `initInventory`, of all places -- a handler that runs at
startup and ends with a special case:

```text
if getState( #currentLocation ) = #Gbhs_playIntro then
  cursorOff
  suspendSounds
  pushVideo
  repeat while the movieRate of sprite 44 <> 0 and not the mouseDown
    updateStage
  end repeat
  killVideo
  goTo #Gbhs_gameEntry, #fadeIn
end if
```

Worth noting what that loop tests. Every other wait in the game is
`wait #videoStop`, and that handler loops on the movie rate alone:

```text
if howLong = #videoStop then
  repeat while the movieRate of sprite 44 > 0
    updateStage
  end repeat
```

No test on the mouse. So cutscenes are not skippable and the opening is, and
the skip is a property of this one film rather than a feature to generalise. I
had a standing temptation to make clicking skip any film; the disassembly says
don't.

`suspendSounds` is left out: the intro room declares no ambience, so there is
nothing to suspend, and suspending with no matching restore afterwards would
only risk silence later.

This also removed a hack. `skip_video` in the walkthrough tool already knew to
jump from the intro to `Gbhs_gameEntry`, because nothing in the engine did --
and it moved the room without draining the queue, so the intro's own `goTo`
stayed pending and fired under the player's next click, sending them back to
the entry they had just left. Two things doing the same job badly. The
walkthrough recording then caught the mirror image: its first step is a jump to
one of Margaret's rooms, and the queued `goTo` dragged it straight back out.
Jumping now abandons the opening; skipping keeps its destination, because that
is what the original does.

### Holding something read as holding nothing

Picking up the PeeK unit is `useInventory( #PeekUnit )`, with a comment in the
room data saying "don't worry; it'll be added automatically when user is
finished" -- it goes into the hand, and the room-sized `#itemInUse` catcher
stows it into the bag on the next click.

The walkthrough tool printed the hand only when the bag was not empty, so the
first thing the player ever holds showed as nothing at all. And `state
iteminuse` printed the schema's list of every item that could ever be held,
headed by `#None`, whatever was actually in hand -- because `itemInUse` is not
in the property store at all; it has its own field, since what is in the hand
is not one of a flag's declared settings. I spent a while convinced the pickup
had failed when it had worked perfectly, on the evidence of two displays that
were both lying.

### `#carrying` was never written

`addInventory` ends with `setState( #playerHas<Item>, #carrying )` and
`deleteInventory` with `setState( #playerHas<Item>, 0 )`. This engine wrote
`[Int(1)]` and `[Int(0)]`, replacing the flag's whole declared list.

Two consequences. `setScanStatus` tests
`getState( #playerHasPeekUnit ) = #carrying`, which could never be true, so the
interrupted-scan message on the PeeK was unreachable. And a one-entry list is
this engine's own signal that a `set<Flag>` handler exists, so picking anything
up quietly changed how writes to that flag were dispatched.

The reason the wrong value was there is that two ported handlers read the flag
with `as_int`, and a symbol would have read as zero. The scripts always ask
`getState( #playerHas<Item> ) = 0`, so the predicate is "not zero" -- which
also means `#usedUp` counts as had, deliberately, and the handlers that need
the difference test for `#usedUp` by name.

### The desk drawer could not be opened

The worst of the four, and the one that blocks the game. `openable` treated
every flag it handles as boolean, reading the suggestion with `as_int`. But
`#officeDrawerIsOpen` holds `#None`, `#top` and `#bottom`, so `as_int` gave
nothing and the handler returned having done nothing at all. The drawer never
opened. The BAR manual is in that drawer, and it carries two of the three
settings the machine in the living room needs.

The real handler is the same two arms as the boolean one with `#None` where
the `0` is:

```text
on setOfficeDrawerIsOpen suggestion
  currentState = getState( #officeDrawerIsOpen )
  if suggestion = #None and currentState <> #None then cue #drawerClose ...
  if suggestion <> #None and currentState = #None then cue #drawerOpen ...
```

One predicate -- shut is `0` or `#None`, open is anything else -- covers both
families. The second arm needing the flag to be shut first is also why the room
scripts set `#None`, wait ten ticks, then set `#top`: you cannot move from one
drawer straight to the other, and the wait is the chest closing.

None of these four could have been found by reading. `verify` reports 1374
sprites decoded, no dangling references, no unported verbs, and every one of
those numbers was true the whole time. The first was a room with no exit, the
second a display, the third a value nobody compared, and the fourth a type. You
find them by opening the drawer.

263 tests.

## 121. First in the list wins

Playing on from the desk drawer, the BAR manual opened and would not turn a
page. Clicking the right-hand half shut the book instead.

The room lists its regions like this:

```text
#nextPage  rect(341, 58, 558, 363)   next page
#nextPage  rect( 71, 54, 287, 358)   previous page
#pointer   rect( -2, 32, 641, 386)   shut the book
```

Director walks a room's regions in order and takes the first one under the
pointer. This engine ranked them by verb instead, with a table that put
`#pointer` above `#nextPage`, so the stage-sized region to close the manual
beat both page halves and there was no way to read past the first page. The
manual holds two of the three settings for the machine in the living room, so
between this and entry 120's drawer the game was unfinishable by the book
twice over.

I could have moved `#nextPage` up the table, but the table is the wrong idea,
so I measured what the data actually does. Across all 1320 rooms, as a mean
position through each room's list, where 0 is first:

```text
itemInUse    0.03      pointer      0.41
nextPage     0.06      down         0.45
rotateLeft   0.07      right        0.45
forward      0.26      left         0.72
examine      0.26      browse       0.94
```

`#itemInUse` is listed first in 702 of the 1320 rooms and `#browse` is last in
1284 of them. That is not a coincidence to be approximated by a priority
table; it is the data being written for first-match resolution.

So: first in the list wins, with `#browse` alone ranked below everything. I
checked what pure list order would cost by looking for every case where a
region is listed before a smaller one inside it that the player needs. There
are five in the whole game, and all five are `#browse` -- it is the catch-all
and blankets the frame, which is why it is normally last and why it needs the
exception in the 36 rooms where it is not.

Two tests had to change, and it is worth being honest about why. Both
constructed a room with the hotspots in an order the game never uses -- a walk
region before the examine target inside it, a pointer before the `#itemInUse`
region inside it -- and asserted that priority rescued them. The measurement
says there is no such room: zero cases of a navigation region listed before an
examine, pointer or item region it contains. The tests were describing my
model rather than the game, and they passed for years because a model tested
against itself always does.

### The music boxes were fine; the tool was not

helba said they were sure the box puzzle was missing sounds, and `mix` agreed:
all five boxes reported `snd1box`. The engine turned out to be right -- each
box emits its own `snd1box` through `snd5box`, and all five decode with signal.

`mix` runs against a silent mixer, which has no stream pulling samples through
it, so nothing ever finishes. A half-second effect held its channel for the
rest of the run, the four channels filled up, and every later sound was
correctly dropped by the channel cap and incorrectly reported as absent. The
mixer now runs forward between clicks, by two seconds, which is about what a
player takes to reach for the next box.

The cap itself is real and stays. Each chapter's schema declares four sound
channels and loops sit on them alongside effects:

```text
#soundChannels: [1: [#sndType: #loop, #sndName: #lakeside, #volume: 255],
                 2: [#sndType: #None, ...], 3: ..., 4: ...]
```

Channel one holding a loop settles it. Margaret's bedroom runs a clock, the
house hum and the radio, so exactly one channel is free and the five boxes
take turns on it. That is the original's behaviour, not a shortage.

### Watching a recording

helba asked whether a `.walk` file can be watched rather than only read. It
can now: `play <dir> --replay <file>` plays the recording into the window a
step at a time and then hands over. The steps go through the same dispatcher
the terminal uses, so the two readings of a recording cannot drift apart.

Two details that took a moment. A step waits for the game to go quiet before
the next one, but not for the picture to stop -- a room's own film loops for as
long as the player stands there, so waiting on that waits for ever. And the
opening is clicked through when a recording is supplied, because a recording is
rarely a recording of the intro; skipping rather than abandoning keeps the
`goTo` the opening ends with, so a recording that starts by walking from the
entry still works.

265 tests.

## 122. Muting to hear it

helba asked for a way to play with the sound muted but logged, so that I can
work on a game I cannot listen to. `play <dir> --mute` does that: the whole
mixer runs -- gains, the four channels, the groups that refuse to talk over
each other -- into nowhere, and the audio log goes on. It found a real bug
within a minute of existing.

Two things had to be right for it to be honest.

A silent mixer has no stream pulling samples through it, so nothing ever
finishes. Left alone it would fill its four channels and report every later
sound as dropped, which is exactly the false picture entry 121 caught `mix`
giving of the music boxes. The render loop now runs the mixer forward by the
frame time when muted.

Worse, the replay was swallowing the sound before the mixer saw it. Both the
terminal and the window drive a recording through the same dispatcher, and that
dispatcher ran the effect queue out itself, naming the sounds it could not
play. In a window that has a mixer and a clock, that is the wrong half of the
job: the queue has to reach the render loop. Who drains is now the caller's
choice. The five music boxes had been playing to nobody.

### Skipping something that has not started

`--replay` is supposed to click through the opening, and it did not work at
all. `skip_video` cuts short a film that is running, and at startup nothing has
drained yet -- the whole opening is still queued -- so clearing the wait only
let the queue play the film from the top and hold on `WaitForVideo` for its
full minute and a half.

The terminal hid this completely, because its `settle` runs the queue out
ignoring waits, so the same call there produced the right answer for the wrong
reason. The window honours waits, and sat there. There is now a separate
`skip_opening` that drops the queue and goes where the opening was going.

It is worth naming the shape: a tool that steps over the thing being tested
will agree with you about anything.

### The chord that never played

With the log working, the box puzzle in Margaret's bedroom said this:

```text
play snd1box      play snd4box  -> no free channel, dropped
play snd2box      play snd5box
play snd3box      play allboxes -> no free channel, dropped
```

helba had said twice that the puzzle was missing sounds. Entry 121 concluded
the engine was fine and the tool was lying, and that was half right: `mix` was
lying, and there was also a real bug underneath it.

The original plays each box like this:

```text
prerollQT
startSound whichBox
if gHorsepower <> #low then wait 30
pushQTcarefully startTime, stopTime, 4
```

I had left out the wait. For the four boxes with a stretch of film it hardly
shows -- the film's own wait covers it. The fifth box has no stretch, because
its times are written as two symbols where the others have numbers, so that
half second is the *only* gap between its sound and the `allboxes` chord that
follows a solved puzzle. Queued in the same breath, sharing a channel, the
chord lost. The game played the five boxes and went quiet exactly where the
payoff belongs.

So the puzzle solved correctly, every sound existed and decoded, each box asked
for the right one, and the moment it was all for made no sound. That is a
difficult bug to see from the code and a trivial one to hear, which is the
whole argument for the log.

267 tests.

## 123. Loud, long, and nobody calling

Three reports from helba, and they turned out to be three different things: one
faithful, one a real fault, and one a feature that was never wired up.

### The chapter was unreachable from the command line

`play <dir> MARGARET` started in Margaret's bedroom and was pulled back out to
the boathouse on the second frame. Entry 120's opening queues a `goTo
#Gbhs_gameEntry` behind its film, and `jump_to` learned to cancel that but
`enter_chapter` did not -- and naming a chapter goes through `enter_chapter`.
Both cancel it now. Harmless later in a game, since there is nothing to cancel
once the opening is done.

### The entry sound is 45 seconds, and that is correct

`toMargaret` runs for 45.8 seconds over a montage that lasts about thirteen, so
it carries on for another half minute inside the bedroom. That looks exactly
like a decoder reading past the end of a cast member, so I added a level
readout to `sfx` -- one line per second -- and looked:

```text
  0s #####      640          25s ######################################## 5127
  3s #############  1750     30s ######################################## 5111
 10s ############   1553     40s ##################  2392
 15s #######################  3013   45s ####   613
```

A clean swell: quiet at both ends, peaking two thirds through. That is a
forty-five second recording, not two seconds of sound and forty-three of
whatever came next in the file. `enterNewDomain` touches no sound either, so
nothing in the original cuts it short. It is meant to run on under the first
half minute of the chapter.

The tool is the point here. A sound that is longer than it should be looks
quite unlike a sound that is long, and until I could see the shape of one I had
no way to tell those apart without listening.

### But it was half again too loud

What was wrong was the level. The game's own table says `#toMargaret: 1.5`, and
this engine multiplied the samples by it -- so its peaks passed full scale and
were flattened by the limiter. Reading `soundEffect` settles what 1.5 means:

```text
on soundEffect whichSound, volumeDesired
  if voidp( volumeDesired ) then volumeDesired = 255
  ... setProp( channelData, #volume, volumeDesired )
```

Director's channel volume is 0 to 255 and the default is already the top, so a
tweak of 1.5 on a full channel is 382, which is 255. It means "as loud as it
goes", not "half again louder". The tweak exists so a sound asked for quietly
still comes up. Gains are clamped at unity now, which is where the original's
volume clamps.

So helba heard a long sound played too hot and clipping its own peaks, which is
a fair description of something that has gone on too long.

### Nobody is calling

The third report was that an expected sound never came, and chasing it found
something larger. `MargPortal_headOn` declares

```text
#earShot: [#houseHum: 0, #phoneVol: 64, #MargVol: 224, #BriceVol: 0, #EdwinVol: 0]
```

`#MargVol: 224` is how loudly Margaret's ghost calls from that spot. The
ghosts telephone the player through the chapter and that is how anyone learns
where to go -- it is the game's whole signposting.

`ghostCalls` is ported, weighting and all. Nothing calls it. No room action
list mentions it, and the render loop does not drive it; in the original it
runs from a frame handler, through `playDomainEntrySound`, which this engine
has no equivalent of. So the ghosts have been silent for the entire project.

That is a seventh variant of the same fault this log keeps recording, and a new
one: not an effect emitted and never applied, but a handler ported and never
reached. `verify` cannot see it, because its whole model is "which handlers do
the room scripts name", and this one is named by the score.

Also noted while looking: `usePeekUnit` is not ported at all. That is the
PeeK unit's own interface -- its icons, its display, its play button -- and it
is reached from the inventory bar rather than from a room, so it too is
invisible to the tally. The residue the hint book tells a new player to listen
to on their first minute is behind it.

268 tests.

## 124. The ghosts start calling

The ghosts telephone the player. It is how anyone learns there is somewhere to
go: a call from Margaret gets louder as you climb the stairs towards her door,
and that is the entire signposting system of a game with no map and no journal.

Fifty-seven room scripts call `ghostCalls` to say who is calling from where and
how loudly. This engine has never once made a sound from it.

Two separate faults, and the second was mine.

### Half the mechanism was missing

`ghostCalls` only *decides*. The playing is `playDomainEntrySound`, which the
original runs from `idle` -- once a frame, not on a click:

```text
on playDomainEntrySound
  if gSoundsSuspended = 1 then return
  batterUp = getState( #ghostsCalling )
  ... if a call is still sounding on its channel then return ...
  soundList = gEntrySoundFiles[ batterUp ]
  newSound  = ( gCurrentEntrySounds[ batterUp ] mod count(soundList) ) + 1
  if batterUp = #nobody then waitaSec( #start )
  else soundEffect( getAt(soundList, newSound), getState(#ghostCallVol) )
  gCurrentEntrySounds[ batterUp ] = newSound
  if count( #ghostsCalling ) > 1 then
    addAt( stateList, 1, getLast(stateList) )
    deleteAt( stateList, count(stateList) )
```

Being a frame concern is the whole character of it. The calls carry on while
the player stands still and thinks, which is what makes a house feel occupied.
Nothing in this engine ran from the frame except the scan unit's clock, so
there was nowhere for it to live and I had not noticed it was missing.

### And my `ghostCalls` was playing a slot machine

Worse, because it looked finished. My port built the weighted candidate list
faithfully and then picked one at random and played a random call file on the
spot. So a room made a single noise and fell silent for ever.

`ghostCalls` stores the list -- `setProp( oStoryteller.states, #ghostsCalling,
suggestedCalls )` -- and the list *is* the rota. The padding is not weighting
to sample from; it is turns. `#nobody` is a one-second pause in the sequence.

The rotation is worth reading twice: it moves the **last** entry to the front
rather than stepping forward. So `#Margaret_warm`, which is `[Margaret,
nobody, nobody]`, gives a call, then both pauses, then the next call -- and
`#allGhosts`, which is three ghosts and three pauses, gives one call, three
seconds of quiet, then two ghosts speaking one after the other. Sampling at
random gives none of that shape.

Each ghost then works through its own recordings in order, and the order is
the one the authors' directory listing gave them:

```text
Mcall1, Mcall10, Mcall2, Mcall3 ... Mcall9
```

The cursor starts at 1 and the step is `(n mod count) + 1`, so the *second*
file is the first one heard. Standing at the top of the stairs, Margaret says
`Mcall10` first. That is not a detail I would have invented.

Checked by standing in the upstairs hall with the sound logged:

```text
[  61] ghost call Mcall10 at medium (3.6s)
[ 396] ghost call Mcall2  at medium (5.0s)
[ 816] ghost call Mcall3  at medium (4.5s)
```

Three hundred and thirty-five frames between the first two: a 3.6 second call
and two one-second pauses, which is exactly the warm rota. It works.

Two smaller corrections fell out of reading the handler. The loudness words
are `[#low: 90, #medium: 180, #high: 255]` out of 255, where this engine had
been using 0.5 and 0.75 and, in `ghostCalls` itself, 160 for medium. And
`ghostCalls #None` takes down a call already sounding rather than only
clearing the rota, so walking away from a ghost stops it mid-sentence.

This is the seventh variant of the fault this log keeps recording, and a new
shape of it: not an effect emitted and never applied, but a handler ported,
tested, and never reached. `verify` counts which handlers the room scripts
name, and every one of the fifty-seven named this one. What it cannot see is
the other half, which the score calls and no room mentions.

275 tests.

## 125. What else the frame was doing

Entry 124 found the ghosts by reading `idle`, the original's per-frame handler.
Having opened it, it was worth reading the whole thing rather than taking the
one piece I went in for. It drives six things, and this engine was doing two of
them.

### The ghosts should not have been calling yet

The first correction is to the entry before this one. `idle` runs
`playDomainEntrySound` inside a guard:

```text
if getState( #AMBERVISION ) = #on then
  playDomainEntrySound
  if lastEvent() > 5 * 60 and ... then ripple
```

The calls only begin once the Amber headgear is on. The hint book says the same
thing in words: "Once the AMBER device is properly calibrated (it happens
automatically) you will start hearing ghost calls which will lead you to the
domain entry tunnels." Ungated -- which is how I shipped it an hour ago -- the
ghosts telephone from the boathouse path at the start of the game, before there
is anywhere to be led and before the player has any idea what the noise is.

`playDomainEntrySound` opens with `if gSoundsSuspended = 1 then return` as
well, so a cutscene is not talked over part way through. That is tracked now
too.

I would rather have got this right the first time, but the shape of the mistake
is worth keeping: I read the handler I was looking for and not the two lines
above it.

### The inventory bar never lit up

`updateInventory` takes `getAt(itemData, 1)` when the puppeteer is `#hot` and
`getAt(itemData, 2)` when it is `#cool`, and `idle` switches between them on
`the mouseV > gInventoryTopY`. So the icons are full colour while the cursor is
over the bar and a glowing outline when it is not. It is the first thing the
hint book teaches about the interface, and it is a nice piece of design: the
bar tells you it is live before you click it.

This engine had the pair standing for something else entirely -- the second
icon marked the item in hand and the first everything else -- so the bar never
changed as the cursor moved, and the outline art turned up on exactly the one
item that should not have been in the bar at all. The item in hand is on the
cursor; `updateInventory` moves its sprite off the stage.

Two icons, two states, and I had guessed at which was which. `plain` and `lit`
are now `hot` and `cool`, which is what the game calls them.

### Still outstanding from that handler

`idle` also installs the menu bar when the cursor reaches the top of the
screen, runs `cursorDance`, and calls `ripple` after five seconds of no input
with the vision on. None of those are ported. They are recorded here so the
next reading of `idle` starts from a list rather than from scratch.

280 tests.

## 126. Nothing left on the list

`verify` says `unported verbs: none` for the first time.

Two left, and neither was what the count made them look like.

The stray `set` was `set the directToStage of cast 1778 to TRUE`, in the room
that plays the opening film. Director's `directToStage` takes a digital video
member out of the stage compositor and lets QuickTime draw it straight to the
screen -- faster, and why a full-screen film does not stutter under Director 5.
Nothing composites films here in that sense; a film is drawn into its member's
rect either way. Ported as the no-op it is, for the same reason as
`forcePalette` in entry 118: an empty arm with the reasoning beside it beats a
number that says work is outstanding when there is none.

`initTelegramPuzzle` lays out the torn telegram, four tiles across and three
down, from `#telegram`:

```text
[#one: 1051, #None: 1063, #three: 1052, #four: 1053, #five: 1054, #six: 1055,
 #seven: 1056, #eight: 1057, #nine: 1058, #ten: 1059, #eleven: 1060,
 #twelve: 1061]
```

The second entry is `#None`, not `#two`. That is the puzzle: the blank is a
real tile with its own art, so this is a sliding puzzle with eleven pieces and
a hole rather than twelve pieces to shuffle. I would have read straight past
it if I had been matching names to numbers instead of reading the list.

The scramble is stored as a permutation rather than as coordinates:

```text
telegramStart = [5, 7, 12, 8, 11, 9, 6, 10, 2, 3, 1, 4]
workingNumber = getPos( telegramStart, i ) - 1
```

Tile `i` is sprite `24 + i`, takes the `i`th cast from the table, and goes to
the slot where `i` appears in that list. Four across, so the slot divides by
four for the row and takes the remainder for the column.

The ink is not carried over. Director's ink 10 is a matte and this engine
mattes a plate from its own transparency rather than being told to per sprite.

The coverage test fired, which is what it is for -- entry 81's rule is that it
names the unported handlers so that porting one breaks it. With nothing left to
name it had to change shape rather than be deleted, so it now names
`usePeekUnit`: the PeeK unit's own interface, a modal screen with its own event
loop, opened from the inventory bar rather than from a room script. No tally
counts it, which is exactly why it is worth naming.

The repository is public now, at `Toyz/amber-journeys-beyond`, MIT. Eight
megabytes of history and ninety-odd files, all of it source and notes; the
purge from entry 76 held, and nothing game-shaped is on the remote. Every
commit is pushed from here on.

281 tests.

## 127. Playing the hint book

Carried on through the walkthrough that shipped on the disc, and it now runs
from a cold start as `hints.walk`: up the hill from the boathouse, in through
the front door, through the dark house to the office, the breaker, the loft,
the PeeK unit, the desk drawer, the BAR manual, the videotape, and the machine
in the living room.

**The BAR works.** helba asked several entries ago why setting it to 6, 5 and 8
did nothing, and the answer is that two things in the way are now fixed rather
than the machine being wrong. The drawer holding the manual could not be opened
at all (entry 120) and the manual could not be turned past its first page
(entry 121), so the settings were unknowable from inside the game. With those
out of the way the panel behaves:

```text
6,5,8 -> baronline = [1, 0]
6,5,7 -> baronline = [0, 1]
5,5,8 -> baronline = [0, 1]
```

Only the right answer brings it online, which is the puzzle.

What is missing is the reward. The hint book says "The PeeK unit will flash in
inventory. Click on the PeeK." `peekAlert` is ported and makes it flash;
`usePeekUnit` is not, so there is nothing behind the flash. The machine works
and its payoff is invisible, which is the same shape as the music box chord in
entry 122.

Two things worth keeping from the tape.

Taking the videotape is two clicks, not one: the first sets
`#playerIsExaminingVideotape` and pops the tape up, the second calls
`addInventory`. The hint book says "click on the tape to place it in
inventory", so even the authors' own walkthrough elides it.

And the room is called `O_NO_TAPE_CU` -- the plate is the desk *without* the
tape, and the tape is a separate sprite gated on `#playerHasVideotape` being 0.
I spent a minute convinced the room was showing me the after state.

### The same recording, two behaviours

`hints.walk` failed every navigation step in the terminal and worked in the
window. The window's `--replay` clicks through the opening; the terminal's did
not, so it was still standing in `Gbhs_playIntro` where none of those exits
exist. Exactly the drift entry 122 set out to prevent, reintroduced by fixing
one front end and not the other. Both skip it now.

### Recordings that can fail

A replay now exits non-zero, which makes these files tests rather than
transcripts. That immediately said something about the two recordings already
in the repository -- one step in `mar.walk` and three in `portal.walk` find
nothing.

They turned out to be helba clicking the scenery: a click at y=25, which is
above the play area entirely, and three at points no room declares a hotspot
for. Nothing is blocked there; there is simply nothing there.

So the exit code distinguishes two things. A step naming a room or an exit that
no longer resolves fails the run: the world changed under the recording, which
is what a recording is worth testing for. A click that lands on nothing is
counted and printed but does not fail, because a recording replays what a
player did and players click the scenery. Making every miss fatal would have
meant either editing helba's recordings to be tidier than the play they record,
or a test that cries wolf.

281 tests, and three recordings that pass.

## 128. The unit everything reports to

`usePeekUnit` is ported, and with it the game has a feedback loop for the first
time.

The PeeK is the hand-held unit Roxy leaves in the loft, and it is the whole of
the game's reporting: the BAR says it is running through it, the scanner says a
residue is ready through it, the cameras play back each haunt through it, and
the Amber device reports its calibration through it. The hint book's standing
advice is "whenever the PeeK flashes, click on it".

`peekAlert` has been ported since entry 63 and makes it flash. Nothing was
behind the flash. Every puzzle solved in this port so far has been reporting
into nothing.

### A wait this engine could not express

The unit is modal: it comes up, shows what it has, and stays until dismissed.
The original says that with a bare `mouseDown` inside the handler, and this
engine had no way to say it -- every wait it knew was on a clock, a film or a
sound. `Effect::WaitForClick` is that, and a click while the queue holds on one
dismisses it instead of reaching the room underneath. Without that last part
the click that closes the unit would also work whatever is behind it.

### What it shows

Reading `#PeekDisplay` clears it, so an alert is consumed by being looked at.
The six camera haunts share one shape:

```text
gPeekPlayList = [#PkFadeIn, #PkKitchenGhost, #PkFadeOut]
setState( #PKbarStatus, #ActivityDetected )
trimState( #cameraFeedbackRemaining, #ghostKnife )
```

The three status readouts do not need a dispatch table at all, because the
pages are named for the machine and its reading run together: `#PKscanStatus`
of `#Wait3min` is the page `#scanWait3min`, `#PKbarStatus` of `#Online` is
`#BarOnline`. That is why `#peekText` has twenty-six entries and why the
handler can be a prefix and a concatenation rather than a list of cases.

### The link that was missing

With the unit working the BAR still reported nothing, because my `setBarMode`
set `#BarOnline` and stopped there. The original ends:

```text
setState( #PeekDisplay, #BARstartup )
unFreezeInventory
peekAlert
```

Two lines, and without them the puzzle was solvable and silent -- the right
numbers brought the machine online and nothing anywhere acknowledged it. The
same shape as the music box chord in entry 122 and the drawer in entry 120: the
mechanism worked and the acknowledgement did not, which is the failure mode
this port keeps producing and the reason playing it matters more than reading
it.

`hints.walk` now runs the whole chain from a cold start: up the hill, the
breaker, the PeeK, the manual, the tape, the machine set to 6, 5 and 8, and
then the unit picked out of the bar to hear it say so.

```text
> state pkbarstatus
  pkbarstatus = [noActivity, Online, Offline]
```

Online, then nothing to report -- which is exactly what the original does, and
the first complete puzzle-to-feedback loop in this port.

Also fixed while testing: the walkthrough's `inv` command did not run the
effect queue out, so taking the PeeK from the bar opened it and then dropped
everything it wanted to do. Every other click drains; that one did not.

285 tests.

## 129. As far as the phone

Carried the walkthrough on. `hints.walk` now runs from the boathouse to the
AMBER device coming online, and every step of it works:

```text
the hill, the front door, the dark house, the breaker
the loft and the PeeK unit
the desk drawer, the BAR manual, the videotape
the BAR at 6, 5 and 8 -> online -> the PeeK says so
the mailbox: the box, the label, the letter, the oscillator underneath
the study: the oscillator into the AMBER device -> online -> the PeeK says so
```

Two puzzles now report their own completion, which is the shape the whole game
is built on.

The mailbox deserves a note. It is not a container but a seven-state machine on
one flag -- `#playerIsExaminingPackage` runs 0, `#closed`, `#open`,
`#readingLetter`, `#oscInside`, `#oscPopup`, `#oscGone` -- and the oscillator
only exists at the end of it. You open the box, read the label, open it again,
read the letter, and the oscillator is revealed underneath the letter, pops up,
and is taken. Seven clicks to get one object, and every one of them a state
this engine had to have right.

### The unit stays in your hand

Porting `usePeekUnit` I had it put itself away at the end. It does not. The
handler sets `#playerHasPeekUnit` to `#inUse` and never sets it back, and
nothing in it stows anything -- the room-sized `#itemInUse` catcher stows it on
the next click, which is exactly what the office table means by the comment
next to it: "Don't worry; it'll be added automatically when user is finished."
Stowing is also what restores the flag to `#carrying`, so writing that at the
end of the handler was inventing a step.

It shows up immediately in play: after reading the PeeK the next click puts it
away rather than doing what you meant, and the walkthrough needs that click
exactly as a player would. My version had the unit tidying itself and the
walkthrough desynchronised from the game by one click.

### What the game opens with

Chasing the scan device turned up the opening state, and it is more deliberate
than I had assumed:

```text
#scanUnitIsActive: [1, 0]
#PKscanStatus:     [#ReadyForPlayback]
#DoorWithScanUnit: [#kitchenOutside]
```

The game begins with Roxy's scanner already attached to the kitchen door,
already finished, with a residue waiting to be played. That is why the device
cannot simply be picked up -- the two hotspots that take it are both guarded on
`#scanUnitIsActive` being 0 -- and it is what the hint book means by "the sound
you heard when you first picked up the PeeK in the loft was the residue from
the front door". The first thing the game has to say to the player is already
queued before they walk in the door.

### Where it stops

The next gate is the telephone. The hint book: "At the appropriate time the
phone will start ringing in the living room. Answer the phone and listen to
Roxy's message. --- The PeeK unit will flash, advising you that the AMBER
headgear has been activated." Only then does the headgear appear, only then
does `#AMBERVISION` reach `#on`, and only then do the ghosts call and the
portals open. Everything after that point is behind one ringing telephone.

285 tests, and three recordings that pass.

## 130. The telephone

Everything after the first hour of this game is behind one ringing telephone,
and the telephone is behind `testForPsionicWaves`, which was not ported.

```text
on testForPsionicWaves
  cameraFeedbackRemaining = count( #cameraFeedbackRemaining )
  oscillatorInPlace       = getState( #oscillatorInPlace )
  tonalResidueRemaining   = count( #tonalResidueRemaining )
  if cameraFeedbackRemaining < 1 and oscillatorInPlace
     and tonalResidueRemaining < 4 then
    setState( #psionicWavesPresent, 1 )
    if inState( #hauntsRemaining, #phoneMessage ) then
      setState( #ghostlyPhoneCall, #ringingNow )
```

Three counts. The house has to have shown everything its cameras caught, the
oscillator has to be in the AMBER device, and at least one of the four door
residues has to have been listened to. Then the phone rings in the living
room; answering it activates the headgear; the headgear turns the Amber vision
on; the vision is what lets the ghosts call; the ghosts are what lead the
player to the portals and the other three chapters.

All three counts were being kept correctly. `cameraFeedbackRemaining` is
trimmed by the PeeK unit as each haunt is watched back, `oscillatorInPlace` is
set by the study, `tonalResidueRemaining` by the scanner. Nothing read them.
The bookkeeping for the whole first act was right and the question was never
asked.

### Asked at exactly the right moment

`testForPsionicWaves` is called from one place: `stowInventory`, and only when
what is being stowed is the PeeK unit.

That is a nicer piece of design than it looks. The PeeK is how all three of
those things are seen -- the haunts play back on it, the scan results arrive on
it, and it is the thing that flashes when the AMBER device comes online. So the
question is asked at the moment the player finishes looking at whatever the
house had to show them and puts the unit away. Not on a timer, not on entering
a room: on the gesture that means "I have seen that".

Tested by walking the progression rather than by asserting the arithmetic: two
haunts left to watch, watch them both through `usePeekUnit` and let it trim the
list, then the oscillator, then a residue, checking at each step that the phone
is still silent and that it rings only when the last of the three lands.

287 tests.

## 131. One comparison, half a game

The Amber vision comes on. The whole chain runs: the telephone rings, Roxy's
message plays, the headgear is activated, raised, put on, and the vision is
live -- which is the gate on the ghost calls, which are the signposts to the
portals, which are the way into the other three chapters.

What was in the way was one line of the condition evaluator.

Approaching the AMBER device runs this, and it is the only way the headgear
moves from activated to ready:

```text
if getState(oStoryteller, #AMBERVISION) = #waitingForPlayer
   then setState( oStoryteller,#AMBERVISION, #readyToGo )
```

`eval_condition` read each side with `parse_value`, which is for Lingo
literals and has no idea what to do with a call. It did not fail -- it
returned something that was neither the flag nor an error, the comparison came
out false, and the body never ran.

That is the worst of the three possible answers. An unreadable condition is
supposed to fail **open**, and there is a comment saying so: "these guards gate
presentation, not progress, so failing open keeps the game moving." The design
was right and one of the two paths into it silently returned a wrong value
instead of no value.

The symptom was a player who could answer the telephone, walk to the device,
and click on it for ever.

Both sides of a comparison now go through `parse_call` first, so `getState` is
read as a call rather than guessed at as a literal.

### Playing the chain

Worth writing down, because none of it is guessable from the code:

```text
testForPsionicWaves  -> #ghostlyPhoneCall = #ringingNow
answer the phone     -> #speaking, Roxy's message plays
                     -> #AMBERVISION = #waitingForPlayer, phoneMessage trimmed
approach the device  -> #readyToGo
click the headgear   -> #popUp
click again          -> #startingUp, useInventory( #Headgear )
stow it and step back-> #on
```

Two things a player would swear were bugs and are not. Hanging up the phone
means clicking the room *around* the handset: the buttons and the handset are
their own hotspots and are listed first, so clicking the middle presses
buttons for ever -- which is exactly what helba reported as not being able to
put the phone down. And the headgear has to be stowed before stepping back,
because the room-sized `#itemInUse` catcher takes the first click.

`headgear.walk` records the whole chain. It sets the three counts directly
rather than replaying the hour of play that earns them -- `hints.walk` covers
earning them -- so what it tests is the second act's opening.

288 tests, and four recordings that pass.

## 132. Into the second chapter

The game runs from its opening film to Margaret's chapter.

```text
the intro, the hill, the front door, the breaker
the PeeK unit, the BAR manual, the videotape
the BAR at 6, 5 and 8, and the unit reporting it
the mailbox and the oscillator, the AMBER device
the telephone, Roxy's message, the headgear, the vision
the 1940s bedroom, the portal, and MARGARET / bedrm_A1
```

Two recordings cover it: `hints.walk` earns the first act, `headgear.walk`
opens the second.

The portal is a nice piece of design and worth recording. It is not a door.
The 1940s bedroom has three walls -- the east wall, the bureau and the north
wall -- and each carries an `#examine` region guarded on

```text
#and: [#equals: [#AMBERVISION, #on],
       #includes: [#ghostsRemaining, #Margaret]]
```

so with the headgear off those walls hold a picture of Hitler, a window and a
pack of cards, and with it on the same three walls each offer a way through.
The room does not change; what you can see in it does. And the guard also
carries `#includes: [#ghostsRemaining, #Margaret]`, so once her chapter is
finished the way through is gone and the wall is a wall again.

`toMargaret` plays at gain 1.00 now rather than being pushed through full
scale, which is entry 123's clamp doing its job on the sound that prompted it.

288 tests, and four recordings that pass.

## 133. The film in the headgear's place

helba said the film played in the wrong position on the AMBER device after the
oscillator went in. It did.

A room's video channel can hold more than one film, each gated on a different
state and each with its own `#coords`. The study holds three:

```text
HGup.mov      #AMBERVISION = #waitingForPlayer    (303, 220)
HGdown.mov    #AMBERVISION = #maybeLater          (303, 220)
oslator1.mov  #oscillatorInPlace = #placingNow    (317, 185)
```

Choosing *which* film to play already tested each sprite's guard. Choosing
where to put it took the first video sprite in the list and used its
coordinates, whatever was actually playing. So the film of the oscillator being
fitted -- the right film, correctly selected -- was drawn where the headgear's
films go, fourteen pixels left and thirty-five down from where it belongs.

Forty rooms declare more than one film and twenty-six of them at differing
coordinates, so this was not one room's problem.

The fix is to place the film by the same test that picked it.

### And a clock that is not broken

helba also mentioned the clock texture. I rendered both: Margaret's bedside
clock is fine, and the living room's is a dim, warm, low-contrast photograph of
a wooden clock behind glass, with the wood grain of the cabinet reflected
across its face. It reads as washed out next to the room around it, which is
what drew the eye.

It is not a decode fault. The plate uses 243 of its palette's 256 entries, and
forcing a neighbouring palette makes it markedly worse -- blue and noisy -- so
the member's own palette reference is the right one. `AMBER_PALETTE` exists for
exactly this question and answered it in one command.

Recording it as checked rather than fixed, since the next person to look at
that room will have the same reaction.

289 tests.

## 134. Where a chapter puts you

Walked into Margaret's chapter and mapped what is reachable from the portal:
thirty-one rooms, all of them her bedroom. Her chapter has a hundred and
fifty-five: forty-two in the kitchen, forty in the dining room, thirty-seven in
the bedroom and thirty-five in the living room.

Two things came out of chasing that.

### The number at the end of enterNewDomain

```text
enterNewDomain( oStoryteller, string(#Margaret), 15 )
enterNewDomain( oStoryteller, string(#ROXY), 12 )
```

The receiver, the chapter, and *where in it to arrive* -- the room's index
within that chapter. Margaret's 15 is `bedrm_C4`; Roxy's 12 is
`HallLivingRmEntry`, the hall by the living room, which is where you come back
to standing rather than out on the boathouse path.

This engine read the first two and dropped the third, so both crossings put the
player in the right chapter and the wrong room. It landed on whatever the
chapter's schema calls its start, which is where a *new game* begins and not
where the story has just put them.

### Four islands and a radio

The rest is not a bug, or not one I can fix by finding a dropped exit. There is
no `goTo` anywhere in Margaret's room scripts that leads from her bedroom to
her kitchen, dining room or living room, in either direction. I checked both
ways round and the four areas are separate islands.

The one thing they share is `bedrm_radio`. It is a single room -- index 22, art
`BR-RADIO-2ANT-ACTOR` -- with no exits of its own, and the kitchen's dumb
waiter, two dining room walls and three living room walls all `goTo` it. Her
bedroom reaches it too.

And her chapter's `exitFrame` carries this:

```text
#bedroom    -> #bedrm_table
#dingingRm  -> #diningRm_W_wwall
#livingRm   -> #livingRm_c2_n
#kitchen    -> #kitchen_dWaiter
```

(The typo is the authors'.) Each area also gets its own sound bed there --
`#BRclock` and a virtual `#BRradio` loop for the bedroom, and so on.

So the radio is the chapter's transport. You stand at a radio, and which room
you step back into depends on what the radio is doing -- which is what
`initRadioDial`, `checkRadioStations` and `radioDial` are for, and why entry 83
found the dial tied to the dumb waiter. A ghost's memory of a house, moved
through by tuning a wireless.

That makes Margaret's chapter the next real piece of work rather than a bug to
fix: the return map is in a frame handler this engine does not run, the same
shape as `idle` in entry 125. Recorded here so it starts from a description
rather than from a survey.

290 tests.

## 135. The wireless is the door

Margaret's chapter is traversable. Her bedroom, kitchen, dining room and living
room are four sets of rooms with no door between them anywhere in the data, and
the radio they all keep is what joins them: tune it to a station, step back, and
you are in that part of her house.

The dial reads as a number, and the stations sit at fixed places on it:

```text
#bedroom 36    #diningRm 56    #kitchen 88    #livingRm 196
```

It moves four at a time, and `checkRadioStations` grades how close you are --
exactly on a station is the station, four away is `#bedroomWarm`, eight away is
`#bedroomCool`, anything else is static. Tuning is a game of hot and cold
played on a wireless.

Three pieces were missing, and they were not the ones I expected. The dial
already worked: `radioDial` and `checkRadioStations` were ported, the bands came
out right, and I could watch `#gStaticWhere` go `bedroomCool`, `bedroomWarm`,
`diningRmWarm`, `diningRm` as I turned it.

What was missing was everything that happens once you have tuned it.

`checkRadioStations` only ever locked a station in for the dining room -- the
one case with an announcement over it -- so every other station tuned perfectly
and never registered. And `backAwayFromRadio` stopped the other three radios and
then did nothing at all: the original ends each of its four branches with a
`goTo` into that part of the house, and my port had the sound and not the
movement.

So the chapter had a working radio wired to nothing.

```text
#bedroom    -> #bedrm_table
#dingingRm  -> #diningRm_W_wwall
#livingRm   -> #livingRm_c2_n
#kitchen    -> #kitchen_dWaiter
```

The typo is the authors'. The same table appears in her chapter's `exitFrame`,
where it also carries each area's sound bed.

### Not every station is broadcasting

`#tunedIn` declares `[#bedroom, #kitchen, #inBetween]`, and
`checkRadioStations` gives static for anything not on that list. So the chapter
opens with two stations and the other two are earned -- the dining room arrives
with an announcement playing over it, which is what `#madeItToDR` records.

That cost me a test, correctly. One that checked the warm and cool bands around
the dining room had never put the dining room on the air, so it had been
asserting the shape of a station that the game does not offer yet.

### An honest gap

In the original the moment a station locks in is in her chapter's `exitFrame`,
which also restores the previous station when the dial is left between two.
This engine has no per-chapter frame handler, so that rule lives in
`checkRadioStations` here instead. The effect is the same and the mechanism is
not, which is worth writing down in case the timing ever turns out to matter.

Forty-four kitchen rooms open up from the bedroom, and the same door leads on
to the rest.

292 tests, and five recordings that pass.

## 136. A station has to be earned

The dining room comes on the air, and the way it does is the knitting needle.

`#tunedIn` opens as `[#bedroom, #kitchen, #inBetween]` and one room script adds
to it:

```text
pushVideo : wait #videoStop
setState( oStoryteller, #knittingNeedle, #usedUp )
addState( #tunedIn, #diningRm )
```

That is in the kitchen, at the bottom of the dumb waiter. So: call the shaft up
from the kitchen, go to the bedroom and put Margaret's knitting needle into it,
send it back down, and take it out below -- and her wireless picks up a third
station. A puzzle whose reward is a new part of the house, delivered as a radio
station coming on the air.

### The shaft moved exactly once

Walking it found a good bug. `setDumbWaiter` writes the flag twice: the
direction while the film runs, and the destination once it has finished. The
first is `set_all`, which replaces; the second was `Effect::SetState`, which
inserts.

The original writes both the same way -- `setProp( oStoryteller.states,
#dumbWaiter, list(suggestion) )`, and `list(v)` replaces. The difference is not
the value, which came out right either way. It is that a flag left holding
exactly one setting is this engine's signal that a `set<Flag>` handler exists,
and inserting grew `#dumbWaiter` to two settings. So the second time anything
asked the shaft to move, `setState` decided there was no setter, wrote the
direction straight into the flag, and stopped: no film, no arrival, and a dumb
waiter stuck between floors for the rest of the game.

It worked once. That is the worst kind of bug to catch by reading, and about
ninety seconds to catch by playing.

`Effect::ReplaceState` is the deferred write that replaces, and the test now
sends the shaft up, down and up again -- because the needle has to ride it
twice and the dining room only opens if it does.

`radio.walk` runs the whole thing: bedroom to kitchen by radio, the needle down
the shaft, and the dial to 56 for the dining room.

293 tests, and five recordings that pass.

## 137. The last station

Margaret's chapter is four areas behind two puzzles, and the second one is the
clock.

```text
if clockTime = #t7 and getState( #clockPuzzleActivated ) = 1 then
  addState( #tunedIn, #livingRm )
```

That is in `moveClock`, and neither it nor `touchClock` is ported. So the
living room is the one part of her house still out of reach, and it is the part
her chapter ends in -- `livingRm_trashcanCU` and `MLR_FLOOR_CU`, the telegram,
the montage, and `enterNewDomain( #ROXY, 12 )`.

I checked the rest of the way is clear rather than assuming it. Putting
`#livingRm` on the air by hand and tuning to 196 steps into `livingRm_c2_n`, and
all thirty-four of her living room rooms open up from there, the trashcan
included. So the only thing between here and the end of her chapter is the
clock puzzle.

Two notes for whoever ports it, from reading `touchClock`:

It tracks `#mostRecentClock` and `#mostRecentTime`, and reacts to touching the
*same* clock showing the *same* time -- so the puzzle is about noticing that
the clocks are not running, and the game has a line for the player who keeps
prodding one. `#clockPuzzleFrustration` counts those prods, and past four she
says something about wasting time.

And all of that is behind `hipToThePuzzle`, which is
`inState( #utterancesRemaining, #Iwonder )`. The remarks only start once she
has said the thing that puts the idea in your head. A game that will not
explain a puzzle to a player who has not yet been told there is one.

### And the tools lie in a way worth remembering

Twice while chasing this I set a flag with the walkthrough's `set` and watched
it vanish. Both times the reason was correct behaviour: entering a chapter
seeds its flags from the schema, so anything set before the jump is overwritten
by the arrival. The `set` command is for steering a chapter you are already in.

293 tests, five recordings.

## 138. Seven o'clock

Margaret's clock puzzle, ported: `moveClock`, `touchClock`, and the thing that
starts it.

The clocks carry the time in their own flag's name. `#clockTime` opens as `#t4`
and reads `#t4.30` for half past, so `moveClock` takes a symbol apart into
numbers, does the arithmetic, and puts a symbol back together:

```text
Hrs = Hrs + ( min / 60 ) : min = min mod 60
Hrs = Hrs mod 12 : if Hrs = 0 then Hrs = 12
if min = 0 then newTime = value( "#t" & Hrs )
else            newTime = value( "#t" & Hrs & "." & min )
```

Four moves and no way back except a reset to four o'clock. From four, three
hours lands exactly on seven, and seven is the answer -- it puts the living
room on the wireless, the fourth and last of her stations.

### It will not tell you there is a puzzle

`touchClock` is the nicest thing I have read in this game. It remembers
`#mostRecentClock` and `#mostRecentTime`, and reacts to being shown the same
clock at the same time -- the whole puzzle being to notice that the clocks are
not running. She says `#timeIsntPassing`; if you keep prodding, and
`#clockPuzzleFrustration` passes four, she says something about wasting time.

And every line of it is behind `hipToThePuzzle`:

```text
hipToThePuzzle = not inState( #utterancesRemaining, #Iwonder )
```

`#utterancesRemaining` is what she has *yet* to say, so this is "has she
already wondered aloud about the clocks". Until she has, touching them says
nothing at all. The game refuses to explain a puzzle to a player who has not
been told there is one, and it counts their prods so it can change its tone
once they have.

### Started by listening

What activates it is not a click. `prodVLoops` -- the virtual-loop sequencer,
which is the same job as this engine's programme ticker from entry 71 --
watches the dining room radio and fires when one of two announcements has
nearly finished:

```text
if sndName = #news   then sndLength = 707
if sndName = #buster then sndLength = 946
if sndElapsedTime > sndLength - 60 and sndElapsedTime < sndLength + 300 then
  setState( #clockPuzzleActivated, 1 )
```

So you have to stand in her dining room and let an announcement play out. The
programme ticker already knows when an item ends, because that is what
schedules the next one, so it fires as the announcement gives way rather than
on a measured deadline.

### What is still in the way

The hands themselves. No room hotspot calls `moveClock`; the clock rooms only
set `#playerIsExaminingClock`, and the hands are a sprite `mouseDown`. There
are twenty-seven of those in this game and this engine runs none of them --
the headgear is another. That is the last architectural gap between here and
the end of Margaret's chapter, and it is now the only one.

296 tests, five recordings.

## 139. A route rather than a list of rooms

helba asked to watch the game played rather than teleported through, so
`margaret.walk` names no rooms at all. Ninety-seven steps, every one a move or
a click, from the opening film to the oscillator in hand.

Building it needed three fixes to the walkthrough tool, and each of them was
the tool being less able to express play than the game is.

`browse`, `rotateLeft` and `rotateRight` were not parsed as verbs. Every route
that needed one had to be written as a jump to a room name instead -- so the
teleporting helba objected to was partly the recorder's fault rather than a
shortcut I had chosen.

Exits now print where to click them. A recording that says `examine` gets the
first `#examine` in the room, and most rooms have four or five, so a route
computed as a list of verbs walks into the wrong close-up. Printing each
region's centre lets a recording name the affordance it means.

And the builder picks between them: a verb when the room has only one hotspot
of that verb, a click otherwise. A region that blankets the frame is the
exception in the other direction -- `#browse` covers everything, so its middle
is always underneath something listed before it, and it can *only* be reached
by verb.

### A room you can get stuck in

Taking the oscillator leaves `PorchMailboxCU2` with three exits, two of which
lead nowhere, and `browse` takes one of those. After it, the room reports no
live exits at all.

The way out is `pointer`, which is on the room's last hotspot and always true.
So the player is not stranded, but the affordance that reads as "step back"
is a dead end and the one that works is the one that reads as "touch it". I
have not chased whether that is the game's shape or my hit test's; it is the
next thing to look at, and it is the sort of thing only walking into finds.

### Where the recording stops

It ends with the oscillator in hand rather than at the portal, and the reason
is honest: the telephone needs all six camera haunts watched back, and those
arrive on their own clock as the house is explored. A route cannot be written
for waiting.

296 tests, six recordings.

## 140. A wait only the replay could end

helba watched `margaret.walk` in the window and it stopped dead at the PeeK
unit. The recording replays fine in the terminal.

`usePeekUnit` ends with `WaitForClick`, because the unit is modal and stays up
until it is dismissed. The window's replay only takes its next step once the
effect queue has gone quiet -- which is right for a film, and a deadlock here:
the queue is waiting for a click, and the only thing that could click is the
step the replay will not take.

The gate now makes an exception for a click wait. Everything else still has to
finish first.

The terminal could not see it. Its `settle` steps over every wait, including
this one, so the same recording ran there and hung in the window -- the third
time the two front ends have disagreed about a recording, after entry 122 and
entry 127. Each time the terminal was the one that could not see the problem,
because stepping over waits is exactly what makes it useful and exactly what
makes it blind.

### And no, muting does not stop the films

helba also asked whether the QuickTime plays under `--mute`. It does. The
player runs on its own clock -- `self.started.elapsed()` against the movie's
timescale -- and muting only swaps the mixer for one with no output. Traced,
muted, the opening film draws on frames 1, 6, 10, 14, 18, 22, which is its own
fifteen a second against the loop's sixty; the portal's `MEmrloop.mov` reports
`loops` and draws on the same cadence.

So what does and does not move is worth stating plainly. Films play: the room
loops, the cutscenes, the segments the music boxes and the radio dial use.
Sprite animation plays wherever a handler drives it, because that is a series
of cast changes with waits between them. What does not move is anything driven
by a frame handler this engine has no equivalent of -- `cursorDance`, the
`ripple` after five idle seconds -- and anything behind a sprite `mouseDown`,
which is still the last architectural gap.

296 tests, six recordings.

## 141. The films that never played

helba said the power switch did not play its film. It did not, and neither did
any other film inside a scripted sequence reached by a verb. Two faults, one on
top of the other.

Throwing the breaker is meant to be:

```text
setState( #eSwitchInUse, TRUE )
soundEffect #breakerSwitch
pushVideo : wait #videoStop            -- the switch thrown, in the dark
setState( #eSwitchInUse, #blackout )
goTo( #OfficeEmergencySwitch )
pushVideo : wait #videoStop            -- the lights coming up, in the lit room
setState( #houseLightsAreOn, TRUE )
```

What the trace showed instead was all three flag writes landing in the same
frame and neither film ever opened.

### A wait satisfied before the thing it waits for starts

`wait #videoStop` becomes a `Wait::Video`, and that asked "is a film playing,
and has it finished". At the moment the wait is set the answer is that no film
is playing -- because the `pushVideo` on the line above is an *effect*, still
sitting in the queue unapplied. So the wait was satisfied on the spot and the
script ran on past its own cutscene.

The log line even said so: "will hold on WaitForVideo once the effects above
are applied". The intent was written down and the test did not check for it. A
script's video wait now also requires the queue to be empty, because the film
it is waiting for is in that queue.

### And the walkthrough's verbs never used the timeline at all

That fix alone changed nothing, which is the more interesting half. Clicking a
hotspot goes through `pump`, which runs a sequence one action at a time and
stops at each wait. Typing a *verb* ran the whole action list in a single call
to `script::run` and applied the result in one go -- every flag written and
every film queued in the same instant, so the room had already changed before
the first film was asked for.

So a recording made of clicks played its cutscenes and the same recording
made of verbs did not, and `margaret.walk` uses `pointer` for the breaker.
Both paths go through `pump` now.

The trace after both fixes, which is what it should always have looked like:

```text
[37] set eswitchinuse = Int(1)
[37] open DKSWITCH.MOV        -- in DarkUp_OfficeEswitch
[58] set eswitchinuse = #blackout
[58] open ESWTCHUP.mov        -- in OfficeEmergencySwitch
[84] set eswitchinuse = Int(0)
```

Three flag writes twenty frames apart with a film between each, instead of
three writes in one frame and no films at all.

296 tests, six recordings.

## 142. Both sides of a wait

helba got stuck at the PeeK table again, and it was the previous entry's fix
that put them there.

Making the walkthrough's verbs go through `pump` meant a sequence can now stop
part way -- which is the point -- but it also meant the *script* can be the
thing holding for a click, where before only the effect queue ever was.
`waiting_for_click` looked at the queue alone, so the click that should have
closed the PeeK unit cleared a wait that was not the one holding, and the unit
stayed up for ever.

Two places, and both now checked. The queue holds when the wait arrives as an
effect; the script holds when `pump` meets it in a sequence and stops there.

The replay gate needed the same widening, and for the same reason: it refuses
to take a step while a script is running, which is right except when what the
script is running is a wait only the next step can end.

Worth naming the pattern, because this is the third time it has bitten. A wait
lives in two places, a click has to satisfy either, and every time I have
taught one of them something new the other has been left behind.

296 tests, six recordings.

## 143. The telegram comes together

Margaret's chapter can be finished. `telegram.walk` tips out her wastepaper
basket, kneels to the torn telegram, slides it back into order in thirty-three
moves, and comes out the other side in Roxy's house.

This needed the thing that has been called the last architectural gap for
several entries: a click on a sprite a script is driving.

### The clickOn

A sprite a script has taken over is not a hotspot. It has no rectangle in the
room data, so the room knows nothing about it -- the telegram's twelve tiles
sit on top of a `#browse` region that would otherwise swallow every one of
them. The only way to know one was clicked is to ask where its art actually
landed, which is the same sum the renderer does: the sprite's location, its
registration point, and the size of its plate.

That is `the clickOn`, and `moveMe` is the first of the game's twenty-seven
sprite scripts to be wired to it. Which sprite runs what is decided here by
which puzzle is on the stage rather than by the member's own script, because
that link is not read yet -- honest, and enough for the tiles.

### Eleven pieces and a hole

```text
chosenSpace = getPos( puzzleState, chosenPiece )
emptySpace  = getPos( puzzleState, theHole )
if abs( chosenSpace - emptySpace ) = 1
   and ( chosenSpace - 1 ) / 4 = ( emptySpace - 1 ) / 4 then #sameRow
if abs( chosenSpace - emptySpace ) = 4 then #sameColumn
```

Being one apart is not enough -- slot four and slot five are one apart and on
different rows -- so the row is compared as well. Being four apart needs no
such check, because in a grid four wide that is always the same column. The
hole is piece 2, which is why `#telegram`'s second entry was `#None` rather
than `#two`: the blank is a tile like any other.

Solved is the numbers in order, which sets `#showMontage` to 1, and two clicks
later `enterNewDomain( #ROXY, 12 )` puts the player back in the hall by her
living room.

### And a bug of my own making, immediately

Wiring the tiles meant they took every click in that room -- including after
the puzzle was solved, when the montage needs the clicks instead. So the
telegram came together and then nothing happened, for ever. The tiles only
take a click while the order is still out of order.

298 tests, seven recordings.

## 144. Two render faults, reported and not yet fixed

helba sent two screenshots. Recording both here rather than leaving them in a
conversation, because neither is fixed.

**The telegram tiles do not line up.** The pieces are not a uniform grid:
`MLR_Telegram_1` is 62 by 60, `_3` is 64 by 57, `_12` is 63 by 63, and
`MLR_Telegram_none` -- the hole -- is one pixel square. `initTelegramPuzzle`
places them on a 65 by 68 lattice, so they cannot all be centred on their slot
and still meet.

Director positions a sprite by its registration point, and these members must
carry ones that make the irregular pieces align. This engine centres a plate
whenever its registration point reads as zero, which is a guess made for room
sprites with `#coords` and is wrong here. The fix is to stop treating a zero
registration point as "no registration point", which means finding out whether
these members genuinely have one.

**A room drawing as several crops of itself.** `HallLivingRmEntry`, arrived at
by a turn, came up as three or four bands of the same scene at different
offsets and scales. Not diagnosed. It arrived on a `turnLeft`, which is a
chunky wipe, so the transition is the first place to look -- although a wipe
takes each pixel from one buffer or the other and cannot rescale anything, so
that may be a coincidence of timing rather than the cause.

Neither is in the way of the chapter being finished; both are in the way of
watching it.

## 145. The telegram was right; what was missing was the sweep

Entry 144 guessed wrong, so this entry starts by undoing it.

The tiles do carry registration points, and they are dead centre. `dims`
prints them beside the origin of each member's rectangle:

```text
MLR_Telegram_1     62x60  reg=(33,35)  origin=(2,5)   ->  (31,30), centre (31,30)
MLR_Telegram_4     63x55  reg=(32,34)  origin=(1,7)   ->  (31,27), centre (31.5,27.5)
MLR_Telegram_8     63x70  reg=(32,35)  origin=(1,0)   ->  (31,35), centre (31.5,35)
```

Director anchors a bitmap at `reg - initialRect.topLeft`, which the decoder
already does, and for every one of these that lands on the middle of the tile.
So centring and honouring the registration point give the same answer here,
and the layout `initTelegramPuzzle` asks for is the layout that was drawn. I
checked it the other way round as well, by correlating each tile against
`MLR_Telegram_full_blur` -- the whole telegram, 265 by 199, which is what the
room puts behind the puzzle. Every tile matches within about two pixels of
where the engine puts it.

Which left the actual fault, and it was not a fault of placement at all: the
tiles were still on the stage. Solving the puzzle shows the whole telegram
behind them, so a sharp piece and its blurred copy sat a few pixels apart and
every line of the message read twice. That is what the screenshot showed.

### Nothing un-puppets anything

`moveMe` ends, on the last move, with

```text
setState( oStoryteller, #showMontage, 1 )
setTransition( oPuppeteer, #fadeIn )
updateDisplay( oPuppeteer )
```

and no `puppetSprite ... 0` anywhere. The pieces come down because
`updateDisplay` takes them down. Its last act, after placing the room's own
sprites, is

```text
repeat with i = <last placed> + 1 to 37
  set the castNum of sprite i = 6
  set the loc     of sprite i = point( 320, 360 ) + gOriginPoint
```

-- blank the channel and park it off to one side. A puppet lives exactly as
long as no other composition happens. That is a general rule I had missed
entirely, not a detail of this puzzle: this engine only dropped puppets on a
room change, and only in the window, so anything a script put on a channel
survived every redraw that was not a move.

So `updateDisplay` now emits `ParkSpareSprites`, which drops every claimed
channel above the room's own and at or below 37. `updateStage` does not --
that one is Director's, and only blits what is already composed. The
distinction matters: several handlers call `updateStage` in the middle of
building a display.

The assembled telegram now reads as one sheet.

### gOriginPoint, while I was in there

`birth` sets it, and on a PC it is `point(0, 0)`:

```text
if gCPU <> #mistakenNotionOfaPC then
  gOriginPoint = point( stageLeft, stageTop )
  gMenuBottomY = originY + 30 : gInventoryTopY = originY + 380
else
  gOriginPoint = point( 0, 0 ) : gMenuBottomY = 30 : gInventoryTopY = 380
```

Every `+ gOriginPoint` in the ported handlers was already being treated as
zero, which turns out to be right for the data this engine reads.

### `shot` inside a walk

`amber shot` renders a room from its own sprite list, which cannot show a
puzzle: a puzzle lives entirely on puppet channels and is not in that list.
So `walk` takes a `shot <file>` step now, and a recording can reach a state
and then ask for the frame. That is how both halves of this entry were
checked.

### The clock puzzle is not in the shipped data

Chasing the other half of the full walk -- what sets a clock to seven, which
is what puts Margaret's living room on the air -- ends somewhere I did not
expect. `moveClock` exists and is complete:

```text
on moveClock command      -- #add_15min, #add_30min, #add_3hr, #reset_4pm
  ... add to Hrs and min, wrap at 12 ...
  setProp( oStoryteller.states, #clockTime, list(newTime) )
  if newTime = #t7 and getState( #clockPuzzleActivated ) = 1 then
    addState( #tunedIn, #livingRm )
    put ">-> Puzzle solved .. tuning in the Living Room!"
```

and `touchClock whichClock` beside it, with the utterances for a player who is
stuck. Neither is called. Not by a handler in any of the five movies, not by
a hotspot action string in any room record, not on the PC disc and not on the
Mac one. The names appear exactly once each in `MARGARET.DXR`, in its own name
table; `#add_15min` and the other three commands appear twice, both in
`moveClock` itself. The dining room's clock close-up has four hotspots and all
four of them either open the case, close it, or leave the room.

`#clockTime` starts at `#t4` and is a one-entry list, so `setState` would
build `setClockTime( #... )` -- and there is no such handler either. The only
writer is the `setProp` inside `moveClock`.

Which makes the living room unreachable. `backAwayFromRadio` transits to
`getState( #tunedIn )`, and `#tunedIn` opens as `[#bedroom, #kitchen,
#inBetween]`. `setState` on a list that long moves an entry to the head and
refuses a value that is not in it, so tuning the dial to 196 cannot put you in
the living room until something has added `#livingRm` to the list. Two things
add to it: the knitting needle adds `#diningRm`, from a hotspot, and
`prodVLoops` adds `#livingRm` when the dining room's programme reaches
`#startPuzzle`:

```text
setState( oStoryteller, #clockPuzzleActivated, 1 )
if getState( #clockTime ) = #t7 then
  addState( #tunedIn, #livingRm )
  put "They got lucky; it's 7 o'clock, so I'm tuning in the Living Rm"
```

"They got lucky" is the branch for a player who had already set a clock. The
branch for the player who has not is `moveClock`, and nothing can reach it.

I am not going to invent the missing interaction. What I can say precisely is
where the chain breaks, and that the rest of it -- the programme reaching
`#startPuzzle`, the activation, the test, the transit -- is all present and
ported. If it turns out the clock hands were meant to be sprite scripts on
their multiframe casts, that is the one place left to look; this engine does
not read a member's own script link, and entry 143 already owed that debt.

Until then Margaret's living room is reached by a recorded `set`, and the walk
says so where it does it.

298 tests, eight recordings.

## 146. The house was never going to haunt anybody

The second act is behind the telephone, the telephone is behind six camera
haunts, and I had written in entry 143 that the haunts "arrive on their own
clock as you explore" -- which is true, and which I had taken as a reason not
to look for the clock. It is in `goTo`, in ROXY's own copy, and it was not
ported at all:

```text
lsMoveCounter = getProp( oStoryteller.states, #moveCount )
setAt( lsMoveCounter, 1, getAt( lsMoveCounter, 1 ) + 1 )
...
if getState( #BarOnline ) and getState( #PeekDisplay ) = #None then
  showTime = getState( #hauntDelay )
  if getAt( lsMoveCounter, 1 ) > showTime and destination <> #LivingRmBarCU2 then
    spawnGhostlyEvent()
    setProp( oStoryteller.states, #hauntDelay, list( max( 0, showTime - 4 ) ) )
```

`#moveCount` opens at 0 and `#hauntDelay` at 60, so the first haunt is sixty
moves into the house, and each one afterwards comes four moves sooner. The bar
has to be running, and nothing new happens while the PeeK is still holding the
last thing it caught -- which is what makes them arrive one at a time instead
of all at once.

`spawnGhostlyEvent` walks `#cameraFeedbackRemaining` and takes the first haunt
the player is not standing in front of:

```text
if i = #ghostKnife then
  forbiddenLocations = [#DiningRmKitchenEntry2, #HallKitchenEntryOpen,
                        #Ghse_D_S, #Ghse_D_W, #Ghse_E_W,
                        #Ghse_P_KitchenDoorCU, #Ghse_P_KitchenEntry]
  if oPuppeteer.zone <> #kitchen and getPos(forbiddenLocations, currentLoc) = 0 then
    setState( oStoryteller, #PeekDisplay, #ghostKnife )
```

and the same shape five more times. Each haunt names the area it happens in,
and most name the doorways it can be seen from as well: the point of a
recording is that it happened where nobody was looking, so the game will not
offer you one you could have watched through a door. The living room's is held
back from the study too, which is where its camera feed is watched.

So the order the six arrive in is not random -- it is decided by where the
player happens to be standing when the counter comes round, which is why the
hint book says "each game of AMBER is different, with haunts occuring in
different orders". Pacing the porch, as the walk does, is out of every room a
camera is watching, so they come in list order.

### And then the key, which was a bug of mine

Watching the bedroom haunt shows where the key to the last upstairs door is.
Taking it and turning it in the lock did nothing: the door stayed shut, and its
guard says why.

```text
[#and: [#equals: [#FortiesBedroomDoorIsOpen, 0],
        #equals: [#playerHasBedroomKey, #usedUp]]]
```

The door opens on the key being *spent*, not on it being gone. And
`deleteInventory` is careful about exactly that:

```text
if whichItem = #ScanDevice or whichItem = #Headgear then
  if whichItem = #ScanDevice then setState( #playerHasScanDevice, 0 )
  if whichItem = #Headgear   then setState( #playerHasHeadgear, #inUse )
else
  setState( value("#playerHas" & whichItem), #usedUp )
```

The scan device is put down and picked up again, so it goes back to zero; the
headgear is worn rather than spent; everything else is spent. This engine wrote
zero for all three. I had even left a comment in `sync_possession` saying
`deleteInventory` ends with `setState( #playerHas<Item>, 0 )`, which is what
happens when you write down what you assumed instead of what you read.

That one line was the whole second half of the game.

### `full.walk`

Which makes a start-to-finish recording possible, and there is now one:

```text
amber play extract --replay full.walk --mute
```

Up the hill from the boathouse, the breaker, the loft, the PeeK, the BAR
manual, the videotape, the BAR itself with its 6, 5 and 8; the package on the
porch and the oscillator inside it; the oscillator into the AMBER device; then
the porch, back and forth, until all six cameras have caught something and each
one has been watched back; the telephone; the headgear; the key out of the
bedside drawer and into the 1940s door; the portal. Then her house: the
wireless up to the kitchen, the dumb waiter, the knitting needle, the dining
room coming on the air, the living room, the wastepaper basket, and the
telegram put back together. It ends on `enterNewDomain( #ROXY, 12 )`, which is
the game putting the player back in the hall outside her living room.

No room is named anywhere in it. Every line is a click or a move.

Two lines are not: `trim tonalResidueRemaining PkPatioScan` and
`set tunedIn livingRm`. Both are marked in the file where they happen, and both
are the same kind of hole -- an interaction the shipped data has no reachable
click for. The tonal residue is read by clicking the scan unit's own display,
which is a `mouseDown` on the cast member `TXT-tonal ready`; the clock is
`moveClock`, which nothing calls. The first of those is a debt I already owed
and can pay: a member's own script link is not read yet, and the telegram tiles
are wired by hand for the same reason. The second I still cannot explain.

### One more thing the sweep may have fixed

`enterNewDomain( #ROXY, 12 )` is `HallLivingRmEntry`, which is the room helba
photographed drawing as several offset bands of itself. Coming back from her
chapter is exactly the arrival that room gets at the end of this walk, and the
montage that carries the player out of it runs on puppet channels. Before the
sweep those channels were still claimed on the other side of the domain change.
A shot taken at the last line of `full.walk` draws the hall clean. I have not
reproduced the original fault, so this is a likely explanation rather than a
confirmed one, and entry 144's second half stays open until helba sees it
again -- or does not.

298 tests, nine recordings.

## 147. Stuck in a cardboard box, and the bar was in the wrong place

helba got stuck inside the opened package on the porch. The room listing
offered two pointers, one of which said it led back out, and typing `pointer`
took the other one for ever.

That is not a fault in the room. `PorchMailboxCU2` has nine hotspots and the
last of them is guarded `#always`:

```text
[#pointer, rect(14, 55, 627, 366),
 ["setState( oStoryteller, #playerIsExaminingPackage, #open )",
  "updateDisplay( oPuppeteer )", "goTo( #PorchMailboxCU, #fadeIn )"],
 [#equals: [#always, 1]]]
```

so there is always a way out, and it is the whole screen. What sits above it in
the list is the letter, at `rect(268, 61, 514, 170)`, and above that a
full-screen region that puts the letter down again. Clicking the letter reads
it; clicking anywhere else leaves. That is a fine piece of design with a mouse
and a cursor, and it is unusable through a terminal that only lets you say the
word "pointer" -- which takes the first live hotspot carrying it, which is the
letter.

Two changes to the walkthrough tool, and both were overdue.

**The live exits are numbered per verb.** A room offering three pointers now
lists `pointer`, `pointer 2`, `pointer 3`, and `pointer 3` takes the third.
A bare verb still means the first, so every recording already written keeps
working.

**The click point printed against a row is now a point that actually resolves
to that row.** It used to be the middle of the region, which is right until
regions overlap -- and they overlap constantly, since first in the list wins.
The middle of a region lying under a wider one belongs to the wider one, so the
listing was printing a point that, clicked, did something else. That is not a
cosmetic problem: it is a lie a recording then acts on, and it sent my own
route-finder from the mailbox straight back into the mailbox. The listing now
probes the middle, the quarter points and the corners, prints the first that
resolves, and says `(covered; use `pointer 3`)` when none does.

Writing that verification meant a second copy of the hit test, and the second
copy got the `#browse` exception the wrong way round -- it ranked browse first
where the real one ranks it last. The listing then swore that the middle of the
mailbox's browse region reached the browse region, the click went to the
pointer above it, and the route walked back into the box it had just left. So
there is one copy now: `hit_index` does the ranking and answers with a place in
the room's list, and `hit_test` is a line on top of it.

### And then the bar

helba's other question was about the inventory bar: why do items show as
outlines when nothing has been clicked, and why does an item sometimes vanish.

Neither is a bug. `updateInventory` draws icon 1 while the cursor is over the
bar and icon 2 while it is anywhere else, and those are "full colour" and "a
glowing outline" -- the hint book describes exactly that, and says to move the
cursor down and watch it happen. The outline is the resting state, not a mark
of anything. And an item that is in hand is skipped entirely, because in the
original it is on the cursor; taking the PeeK out of the bag is what makes it
disappear from the bag.

But going to check that turned up a real fault next to it. The bar's geometry
is written down and I had guessed it:

```text
foundationSprite = 3 : itemV = 410 : itemH = 110
repeat with i = 1 to 7
  ...
  set the loc of sprite itemSprite = point( itemH, itemV ) + gOriginPoint
  itemH = itemH + 70
```

Seven fixed slots, left-aligned from 110, seventy apart, all on the row whose
centre is 410. Every icon is 67 by 67 registered at (33, 33), so the first
slot's corner is (77, 377). This engine centred the whole group and put it
flush with the bottom of the stage at (`(640 - n*67)/2`, 413) -- so the bar sat
thirty-six pixels too low, and every icon moved sideways whenever anything was
picked up or put down. `gInventoryTopY`, which decides when the icons light up,
was `stage_h - ICON` for the same reason; `birth` sets it to 380.

The moving is the part that mattered beyond looks. A recorded click on the bar
means a slot, and a slot that shifts when you pick up a crowbar is a recording
that stops meaning what it meant. All three recordings that click the bar have
been repointed at the real slots, and the driver that writes them computes
`110 + 70 * slot` now rather than measuring a centred row.

`full.walk` is regenerated against the corrected bar and still plays start to
finish.

300 tests, nine recordings.

## 148. The bar is not a queue, and the PeeK never flashed

helba asked whether the PeeK's alert was missing an effect and a sound. Half
right, and the half that is wrong is worth saying first: **there is no sound**.
`peekAlert` is twelve alternations of one sprite's castNum, five ticks apart,
and then it puts back whatever was there. Not a note anywhere in it.

The flash, though, has never once happened in this engine, for three separate
reasons stacked on top of each other.

`gPeekAlertEnabled` is set by `enablePeekAlert` and cleared by
`disablePeekAlert`, one line each, and both were no-ops here -- so the flag was
never anything, `peekAlert` read it as zero and returned immediately. Only the
camcorder log turns the pulse off, so an unset flag now means on.

Then the drawing. The original does this:

```text
oldPeekGraphic = the castNum of sprite 7
repeat with i = 1 to 12
  hold five ticks
  set the castNum of sprite 7 = <the high glow, then the low, alternating>
set the castNum of sprite 7 = oldPeekGraphic
```

Sprite 7 is `foundationSprite + 4`, the bar's fourth slot. This engine drew the
bar from what is carried rather than from score channels, so puppeting channel
7 put a 67-pixel icon in the middle of the room -- and underneath the room's
own plates, since those start at channel 12, so it was invisible as well as
wrong. The alert now asks the bar for a different icon instead, which is the
same idea expressed in the terms this engine has.

### Why sprite 7 is always the PeeK

Because the bar is not a queue, which is the third thing, and the one helba was
looking at when they said it looked confused.

`lsInventory` is a fixed seven-element list with `#None` in the empty places,
and `addInventory` decides which place an item takes:

```text
leftSlotsOpen  = leading #None among slots 1..3
rightSlotsOpen = trailing #None among slots 7..5

if getAt( inventoryList, 4 ) = #None then setAt( inventoryList, 4, whichItem )
else if whichItem = #PeekUnit then      ... take slot 4, pushing the rest aside
else if whichItem = #ScanDevice then    ... favour the left
else if whichItem = #Headgear then      ... favour the right
else                                    ... whichever side has more room,
                                            packed towards the middle
```

Slot 4 is the middle of the bar. The PeeK is the first thing the player picks
up, so it lands there; and if anything else got there first, the PeeK's branch
turns it out. That is why the alert can name sprite 7 and know what it is
holding. The scan device leans left and the headgear right, and everything else
fills inward from whichever side is emptier. `deleteInventory` is the mirror:
the half the item was in closes up towards the middle, and taking the middle
one pulls a neighbour in from the roomier side.

This engine packed items left to right in the order they were picked up. So an
item moved every time anything else was picked up or put down, and the whole
row sat wherever the count happened to put it. Two entries ago I fixed the row
to `110 + 70n` on the row centred at 410 and thought that was the end of it;
it was half of it, because I was still choosing `n` myself.

The bar now models the seven slots, and the walkthrough prints which slot each
item is in -- `carrying: 3:oscillator 4:PeekUnit 5:Videotape` -- because a
recording clicks a slot and the slot is not the order you picked things up in.
The three recordings that click the bar are repointed again, and `full.walk` is
regenerated.

### `--log`

Also helba's, and fair: `AMBER_TRACE=all AMBER_TRACE_FILE=/tmp/run.log` is two
environment variables to remember for a question you ask constantly. Any
command now takes `--log [file]`, defaulting to every topic and `amber.log`,
with `--trace <topics>` to narrow it. Both are stripped from the arguments
before the command reads them, since `walk` would otherwise try to replay the
filename as a step. The environment variables still work and are still what a
script should use.

300 tests, nine recordings.

## 149. A recording is only as honest as the steps you leave out

helba sent the log of `full.walk` playing, and it walks back into the mailbox
it has just left, fumbles the PeeK, and clicks points that hit nothing. Every
step in the file resolves -- the replay's own tally says so -- and it still
looks like the player has lost their mind.

The file was written by a driver that plays the game live and records what it
sends. Between steps it needs to read the room to find out where it is, and
there was no command that only reads: `skip` prints the room, so it used
`skip`. `skip` also cuts short whatever film is playing, and cutting a film
short can let a queued move run.

So the driver would refresh, the refresh would move the player, and the next
recorded step would be one that only makes sense from where the refresh had
put them. Replayed without the refreshes, those steps land somewhere else, and
the router's next move is a recovery from a position the recording never
mentions. Half the porch is that.

`walk` has a `look` command now, which prints the room and does nothing else.
It is two lines, and it is the difference between a recording that describes a
playthrough and one that describes a playthrough plus a ghost.

The other half was the PeeK. Opening it holds for a click, and that click is
spent dismissing the playback rather than putting the unit away -- so one click
leaves it in the hand, and an item in the hand puts an `#itemInUse` region over
the whole stage. Every exit in the room disappears underneath it. The driver
now clicks until the hand is empty before going anywhere.

Regenerated, the route reads: out of the package, out of the mailbox, in
through the front door. Six haunts caught and watched, six `#ActivityDetected`
in the log, and no dead steps at all.

300 tests, nine recordings.

## 150. The terminal and the window finally want the same clicks

Entry 149 blamed `skip`, and that was a real fault, but it was not helba's.
Their log is the window replaying `full.walk`, and lining it up against the
same file replayed in the terminal shows where the two front ends part company.

```text
  window                                terminal
  27  pointer      OfficePeekTable      27  pointer      OfficePeekTable
  28  browse       OfficeLoftWwall      28  click 320 200  OfficePeekTable
```

Taking the PeeK unit opens it, and it holds for a click. The window honours
that hold: the next step is spent dismissing the playback, and the unit stays
in the hand. `settle`, which is how the terminal runs a queue, cleared every
wait it met including that one -- so in the terminal the unit came back to the
bag on its own and the recorded route never needed the click.

From there the two runs are a step out of phase for the rest of the game, and
an item left in the hand is not a cosmetic difference: it puts an `#itemInUse`
region over the whole stage, so every exit in the room vanishes underneath it.
That is the flailing helba was watching -- clicks landing on the catcher, the
route recovering, and the recording describing none of it.

`settle` now stops at a click wait and leaves it standing:

```rust
if matches!(self.pending.first().and_then(wait_for), Some(Wait::Click)) {
    self.pending.remove(0);
    self.effect_wait = Some(Wait::Click);
    report.push("wait for a click".into());
    return report;
}
```

Every other wait is still stepped over, because every other wait is the clock's
and the terminal has no clock. This one is the player's.

Both front ends now want the same clicks, and the three recordings that open
the PeeK have gained the one they were missing: one click to dismiss what it is
showing, one to put it back. `full.walk` is regenerated and is thirteen steps
longer than it was.

This is the fourth time the two front ends have disagreed and the first time
the disagreement has been removed rather than worked around. The rule it leaves
behind: if the window makes the player do something, the terminal has to make
them do it too, or a recording written in one is fiction in the other.

300 tests, nine recordings.

## 151. The PeeK unit, at last

helba asked three questions in a row and they turned out to be one question:
is the PeeK meant to animate when you pick it up, is a haunt meant to play as
a video, and why does opening it say "amber alert" and show nothing.

Yes, yes, and because none of it was ported. `usePeekUnit` opens like this:

```text
peekBody = 38 : peekAntenna = 46 : peekRollUp = 44 : peekText = 40
set the castNum of sprite peekBody    = #PeekDown     -- ink 8, matte
set the loc     of sprite peekBody    = point( 320, 200 )
set the castNum of sprite peekAntenna = #peekAntenna  -- ink 36
set the castNum of sprite peekRollUp  = #PeekUpAnim
set the loc     of sprite peekRollUp  = point( 317, 189 )
repeat while the movieTime of sprite peekRollUp < the duration of cast peekAnim
  updateStage
set the castNum of sprite peekBody    = #PeekUp
camSprite = peekRollUp
set the loc     of sprite camSprite   = point( 317, 132 )
set the castNum of sprite camSprite   = PkVideoNormal[#PkNone]
```

The unit is drawn, its aerial goes up, `PeeKup.mov` plays it sliding into
view, and the channel that animation was on becomes the little screen the
recordings play in. This engine drew a line of text over the room and nothing
else.

### A film on a channel

The reason is worth writing down, because it is a hole in the model rather
than a missing handler. Director makes no distinction between kinds of cast
member: a sprite points at one, and if that one happens to be a digital video,
the sprite plays it. Six of the PeeK's frames are films --

```text
cast 915  PKKNIFE.MOV   128x96     #PkKitchenGhost
cast 930  Kdknob.mov    128x96     #PkKdKnob
cast 929  CrazyLR.mov   128x96     #PkCrazyLR
cast 928  crazydr.mov   128x96     #PkCrazyDR
cast 922  Pkmbrgst.mov  128x96     #PkBedroomGhost
cast 914  Bludbath.mov  128x96     #PkBloodBath
```

-- and so are both fades, the four scan playbacks, and the roll-up itself.
This engine had one film at a time, the room's, and puppet channels drew
bitmaps. Pointing a channel at a film set a cast number the bitmap decoder
could make nothing of, so it drew nothing, silently.

`point_channel` is the fix and it is where all three sprite-cast effects go
now: if the member is a digital video it opens a player for it, otherwise it
sets the number as before. The film is drawn in its own channel's place in the
stack rather than with the room's, at the registration point the script gave
it, and the window advances it and plays its soundtrack alongside the room's.
`WaitForOverlay` is the wait the roll-up's `repeat while` loop is.

So the whole point of the camera system is visible for the first time: six
recordings the game sends you to watch, in the unit's screen, and every one of
them had been a blank rectangle.

### And an ink

Puppet channels had no ink and were drawn without a matte, on the grounds that
"the game only ever puts full plates on one". The PeeK is a counterexample in
two inks at once -- 8 on the body and 36 on the aerial, which are Director's
matte and background-transparent and mean the same thing here. Without them the
unit arrives as a white rectangle with a PeeK inside it, covering the room.

### The screen it left behind

And immediately: the unit came down and its screen stayed, a grey hatched
rectangle hanging in the middle of the room. `PkNone` is `PkBlank.mov`, so
what the channel was holding was a film, and `puppetSprite 44, 0` removed the
channel without stopping it. Releasing a channel now stops whatever it was
playing, and so do a room change and `updateDisplay`'s sweep -- the three
places a channel can be taken away.

253 tests, nine recordings.

## 152. Three things helba could see

**The unit closed before it could be watched.** A recording is a list of what a
player did, and a player watching something takes time over it -- but there was
no way to write that down, so the replay dismissed the PeeK's recording in the
same instant it started. `wait <ticks>` is that beat. It paces the *replay*
rather than the game: the window takes it before sending the next step, and the
queue carries on underneath, so the film keeps playing while the recording
stands still. The terminal has no clock and nothing to watch, so it says `wait
300` and moves on.

Getting that wrong first taught me something. My first attempt queued a
`WaitTicks` into the effect queue, which meant `settle` ran the queue again --
and `settle` cleared the click wait that was standing at the time. So the beat
ate the click, and every step after it was one out of phase. `settle` now
refuses to run at all while the queue is holding for a click, which is what
entry 150 should have said in the first place.

**The bar was a row of clocks in Margaret's house.** The icon table is written
as cast *numbers*:

```text
#PeekUnit: [the number of cast "PeeK color", the number of cast "PeeK glow", ...]
```

and a number means something different in every movie. 951 is `PeeK color` in
ROXY and `MDR-CLOCK-4.45` in MARGARET. The bar was drawn out of whichever
chapter the player was standing in, so stepping through the portal turned the
inventory into three clock faces. The table now remembers which chapter its
numbers are written in, and the bar is drawn from that one wherever the player
is.

**The film on the AMBER device jumped.** Not where it was drawn -- that has
been right since entry 141 -- but *when*. The position was re-derived every
frame from the sprite whose guard currently holds, and the sequence that fits
the oscillator moves on while its own film is still running:

```text
setState( #oscillatorInPlace, #placingNow )   -- the film's guard
pushVideo : wait #videoStop
setState( #oscillatorInPlace, TRUE )          -- and it is gone
```

For the last frames no video sprite's guard held at all, so the film fell back
to the middle of the stage and the 60 by 40 patch that had been sitting neatly
in the device's slot jumped into the middle of the glass. That is the picture
helba sent. A film now keeps the position it was opened at until it is
replaced.

254 tests, nine recordings.

## 153. The unit's three lights

helba asked whether the PeeK is meant to have buttons on it when it is open.
Three of them, and they were missing along with everything else the unit is
made of.

```text
buttonCoords = point( 247, 270 ) + gOriginPoint
set the loc of sprite pkScanIcon  = buttonCoords
set the loc of sprite pkBarIcon   = buttonCoords
set the loc of sprite pkAmberIcon = buttonCoords
```

All three at the same point, which looks like a mistake until you look at the
casts: they are 32 by 29 with registration points at (336, 212) and origins at
342, 389 and 437, so each one's registration point sits a different distance
outside its own rectangle and they land in a row. The engine already handled
that -- it is the same arithmetic as any other sprite -- it simply had never
been asked to draw them.

Each light has three frames and reads its own machine's flag:

```text
#scanIcon:  [6, 979, 982, 985]     -- offline, online, active
#barIcon:   [6, 980, 983, 986]
#amberIcon: [6, 981, 984, 987]
if getState( #BarOnline ) = 1 then getAt( barIcon, 3 ) else getAt( barIcon, 2 )
```

The lists are read by position rather than by symbol, and this engine's cast
tables only stored the ones written as property lists -- so a table like this
was not there to be looked up at all. It stores a plain list of numbers under
its positions now, which is the same table addressed the way its readers
address it.

The readout got its place too. Every branch of `usePeekUnit` points the text
channel at a page of `#peekText`, and nothing had ever claimed the channel or
told it where to go, so the pages landed where a sprite with no location goes
-- the middle of the stage. That is close enough to the middle of the unit
that the mistake did not show, which is the worst kind.

253 tests, nine recordings.

## 154. The game ended at the end of its first chapter

helba asked for the walk to carry on past Margaret's chapter into the next
one. The route is straightforward -- the weedkiller from the dining room
cabinet, the kitchen's rear door, the weeds behind the garage, the gazebo, and
the portal in its roof -- and it did not work, because the gazebo's portal is
only there with the Amber vision on and the vision could not be turned back
on. Nor could anything else be done, as it turned out:

```text
playerHasHeadgear = [0, #carrying, #inUse, #endGame, #usedUp]
```

Zero. Coming home from her chapter, the player was no longer holding the
headgear they had walked in wearing.

`enterNewDomain( oStoryteller, string(#ROXY), 12 )` comes back through
`enter_chapter`, which seeds the chapter -- and a schema is a chapter's
*starting* state. So returning to Roxy's house wrote her opening declarations
over everything that had happened in it. The breaker was unpulled, the BAR was
off, the oscillator was back in its box, and the headgear was gone. Nothing
tested for it because nothing had ever come back from a chapter before: the
walk ended the moment it arrived.

The original is explicit about not doing this -- `enterNewDomain` stashes a
domain's flags and puts them back, and says so in its own log line, "Just
stashed Roxy's state-data into #stateOnIce". Here a chapter is seeded once and
re-entering one leaves its flags alone.

That single line is the difference between a game with one chapter and a game
with four.

### The route to Brice

With it fixed, the walk carries on: the weedkiller out of the right-hand
cabinet under the dining room sideboard, the kitchen's rear door pushed open,
the weeds behind the garage killed, up to the gazebo, and the portal in its
roof. `Ggaz_domainEntry`'s one hotspot is a montage ending in
`enterNewDomain( oStoryteller, string(#Brice), 15 )`, and it lands at
`Iris_BenchE` in the iris garden.

His chapter is a chapter's worth of work of its own -- a conservatory, a shed
with three nails in the door, bees, a heart-shaped box, two mirrors, a trap
door, a grate, a three-wheel combination lock and a control panel -- and it
ends in the basement closet, in a handler called `goodbyeMandy`. That is the
next block.

254 tests, nine recordings.

## 155. Where the second chapter is, and what stands in front of it

The route to Brice's portal is all there and all reachable -- the weedkiller,
the kitchen's rear door, the weeds, the gazebo -- and the last step is not:

```text
up  blocked by And([ Equals { ambervision, #on },
                     Includes { ghostsremaining, #Brice } ])
```

The portal in the gazebo roof is only there with the Amber vision on. Coming
home from Margaret's chapter it is off, and nothing I can find turns it back
on.

The chain that turns it on runs

```text
#waitingForPlayer -> #readyToGo -> #popUp -> (put it on) -> #startingUp -> #on
```

and every step of it is a hotspot in the study guarded on the step before.
The only thing that sets the first is `setGhostlyPhoneCall`, which is the
telephone -- and answering it trims `#phoneMessage` out of `#hauntsRemaining`
in the same breath, so the phone rings once in the whole game.

Margaret's chapter never touches `#AMBERVISION`. What turns it off is Roxy's
own portal, on the way in:

```text
"setState( oStoryteller, #ambervision, #off )", ... ,
"enterNewDomain( oStoryteller, string(#Margaret), 12 )"
```

So by the plain reading of the data, entering one chapter spends the vision
for good and the other two portals can never open -- which cannot be what the
game does, because the hint book says "you may enter these domains in any
order".

The thing I have not ported is what closes it. `enterNewDomain` does not just
swap rooms; it stashes a domain's flags and puts them back:

```text
if count( lsStateData ) then
  ... put the stored state-data back ...
  "Just put stored state-data into lsStateData of oPuppeteer"
else
  setProp( ..., #houseLightsAreOn, [0, 1] )
  quietly( me, #AMBERVISION, #off )
  endLoop #amberHum
```

-- and elsewhere, "Just stashed Roxy's state-data into #stateOnIce". Two
stores, one of them named for Roxy's house specifically. This engine has one
flat state and no stash at all, which is what entry 154's seeding bug was a
symptom of: I patched the symptom by seeding once, and the real shape is
per-domain state that is saved on the way out and restored on the way in.

That is the next thing to build, and it is the difference between a game with
two chapters and a game with four. Until it exists, `full.walk` ends at the
foot of the gazebo with the portal shut.

254 tests, nine recordings.

## 156. The freezer

Entry 155 said what was missing and this is it. `enterNewDomain` swaps the
whole state list, and the house goes in the freezer while the player is away:

```text
if destination = "ROXY" then
  savedRoxy = getProp( states, #StateOnIce )
  states <- savedRoxy
  quietly( me, #lastDomainVisited, lastOuterDomain )
  if lastOuterDomain = "MARGARET" then setProp( states, #currentLocation, [#DarkUp_40sReentry] )
  if lastOuterDomain = "BRICE"    then setProp( states, #currentLocation, [#Ggaz_Reentry] )
  if lastOuterDomain = "EDWIN"    then setProp( states, #currentLocation, [#Gbhs_Reentry1] )
else
  storedState = states
  states <- value( the text of cast 'stateData' )
  addProp( states, #StateOnIce, storedState )
```

A chapter always starts from its own declarations, and the house is put back
exactly as it was left -- and the player is not put back where the call says.
`enterNewDomain( oStoryteller, string(#ROXY), 12 )` names the hall by her
living room; the game ignores that and puts them in the dark upstairs, in a
room called `DarkUp_40sReentry` that exists for no other purpose. Each chapter
has its own, and they are all in the dark half of the house. You come back
wrong and have to walk out.

So `full.walk` now comes home through the dark landing, down the stairs, out
of the front door -- which has to be opened -- and back in through the porch,
before it can get to the dining room for the weedkiller. It reads like the
game and it is a good deal longer than the route it replaced.

Entry 154's "seed a chapter once" is gone; this replaces it and is what it was
approximating.

### And the third thing done by hand

The vision is still off on the way home, because it is off when the house goes
in the freezer -- Roxy's own portal turns it off in the statement before
`enterNewDomain`. Everything in entry 155 stands: the chain that turns it back
on starts at a state only the telephone sets, and the telephone rings once.

`full.walk` sets `#AMBERVISION` by hand, marked where it happens, and joins
`moveClock` and the scan readout on the list of things the shipped data has no
reachable click for. Three now, and all three are one step in an otherwise
complete chain, which is starting to look less like three coincidences and
more like one missing mechanism I have not found yet.

### The router learned about doors

Coming home into the dark house broke the route-finder, which plans over the
static room graph and had no idea the front door was shut. It now notices when
it is going round in a circle and tries the affordances that lead nowhere --
which is what a door is -- once each before giving up. That is how the walk
gets out of the dark house without a single hand-written step.

254 tests, nine recordings, and `full.walk` reaches Brice's chapter.

## 157. Matte is not a colour key

helba photographed the PeeK unit opened over the BAR panel: the top bar and
the bottom bar were there, and the middle of the unit was gone, with the room
showing through where the body should be.

The unit's body is `set the ink of sprite peekBody = 8`, which is Director's
`#matte`. This engine had one idea of what an ink can mean -- "do not paint
the background colour" -- and applied it to everything that was not ink 0.
That is ink 36, `#bgTransparent`, and it is the right reading for the fifteen
sprites that use it: a phone lifted to the ear, a bottle turned over, a
newspaper being read, each drawn on a white field.

Matte is a different thing. It keys out the background *outside the shape*,
from a mask derived from the member's outline; background-coloured pixels
inside the shape stay painted. The PeeK's body is a slab of exactly that
colour, so keying on the colour punched its middle out and left a frame.

`to_rgba_matte` floods from the edges and keeps what the flood cannot reach.
Room plates stay ink 0 and opaque, the fifteen stay keyed, and the inventory's
icons are keyed too -- they are a glowing outline and the room is meant to
show through the middle of it.

### The tools should hide, and they were not hiding

helba, in the same breath: in Margaret's chapter, aren't the tools supposed to
be gone? Yes, and the freezer says so. `states <- value( the text of cast
'stateData' )` *replaces* the state list; it does not write over it. A chapter
starts with its own declarations and nothing else, which means the player
walks in carrying nothing -- Roxy's tools are Roxy's, and they went into the
freezer with her house.

Entry 156 seeded the chapter on top of the old state instead of replacing it,
so the PeeK unit, the videotape and the headgear sat in the inventory bar in a
bedroom in 1943. Now the bag is empty going in and full again coming home.

### Two that were not bugs

The six camera haunts come out in `#cameraFeedbackRemaining`'s list order in
`full.walk` -- KdKnob, crazyDR, ghostKnife, ghostlyKey, crazyLR, bloodBath --
because the walk paces the porch, and the porch is out of every room a camera
is watching, so `spawnGhostlyEvent` always takes the first one still on the
list. Wandering the house instead, each haunt is skipped while you are
standing where it happens, and the order comes out differently. The hint book
says as much: "Each game of AMBER is different, with haunts occuring in
different orders." What makes them feel mechanical is the other half of the
clock -- `#hauntDelay` starts at 60 moves and comes down by four each time, so
they arrive faster and faster.

And the ghost on the stairs is drawn right: `strghost.mov` is 128 by 232 at
(357, 176), and it lands on the staircase with its plume blending into the
landing light. The hard-edged dark shape at the top left of that frame is the
room's own art.

254 tests, nine recordings.

## 158. The background is not always index zero

helba: the PeeK is not the only thing drawing over what is behind it. The
inventory bar was three opaque black boxes sitting over the bottom of the
room, with the glowing outlines inside them.

Two mistakes, one on top of the other.

The first is that I had put the fix in the wrong function. `sprite_at` --
which is the click test over script-driven channels -- and `draw_inventory`
sit near each other and look alike, and the ink went into the one that decides
where a click lands rather than the one that draws. Both want the key, for
different reasons: a click belongs to the art and not to the field around it,
and the bar's icons are outlines the room is meant to show through.

The second is the interesting one. Director keys on a sprite's background
*colour*, not on a fixed palette index, and the game's members do not agree on
which index that is:

```text
cast 976  PeeK.down    244x387   field index 0
cast 951  PeeK color    67x67    field index 255
```

Room plates lay their field in 0 and the inventory's icons lay theirs in 255,
so keying on 0 did nothing at all to an icon. `Bitmap::background` reads it off
the border instead, which is what a field is, and both the key and the matte
mask use it.

This was invisible on everything drawn whole and obvious the moment anything
was not.

### And an ink I invented

While checking, the readout and the three status lights on the PeeK turned out
to have no ink at all: `usePeekUnit` sets a castNum and a location on each and
nothing else. I had given them ink 8 on the assumption that everything on the
unit needed to let the room through. Only the body and the aerial carry an ink
-- 8 and 36 -- because they are the two with an outline.

254 tests, nine recordings.

## 159. Brice's chapter

All of his handlers were already ported -- `verify` has said so for a while --
so the chapter was a matter of finding out what it wants rather than building
anything. What it wants, read out of the handlers rather than played for:

**The trapdoor.** A button by the gazebo steps runs `toggleTrapDoor`.

**The padlock on the grate beneath it.** Three wheels, all starting at six.
`tryToOpenGrate` compares them against `list(3, 2, 1)` and nothing else, and
`adjustLockSettings` sounds the unlock the moment the last one lands, so the
combination is 3-2-1 and it is stated twice. The top row of buttons turns a
wheel up and the bottom row down: three down, four down, five down.

**The weathervane.** The rope in the basement takes three pulls, each guarded
on where the last one left it: `#gazFlag` goes `#off` to `#on` to `#stuck` to
`#flying`, and only the third is any use.

**The control panel on the basement door.** Six buttons, and `panelButton`
wants exactly

```text
repeat over [#A1, #A2, #B2, #B3]  -- all down
  if not inState(#panelGuess, i) then exit
repeat over [#A3, #B1]            -- neither down
  if inState(#panelGuess, j) then exit
```

Pressing a wrong one does not reset anything; it only keeps the check from
passing until it is pressed again.

**And the closet**, which is what all of it walks towards: `testClosetLock`
opens it with the flag flying and the door ajar, and behind it is Mandy.

### The line I had left off

The closet's door is set ajar in the last line of `panelButton`, after the
`goTo` that carries the player back to the door -- and my port stopped at the
`goTo`. So the panel could be solved, the player was moved, and the door was
still shut. `#closetDoorIsOpen` is *read* in three places in the room data and
written in none, which is why nothing had flagged it: the only writer is that
line.

That is the fourth time a handler's tail has been the thing that mattered.

### Two recordings out of one file

`brice.walk` is his chapter on its own, starting at the bench the portal lands
on. `full.walk` plays the game to get there. Both come out of the same
`play(r)`, because a chapter written down twice is a chapter that will disagree
with itself.

`full.walk` is 587 steps now: the opening film, Roxy's house, the six haunts,
the telephone, the headgear, the portal into Margaret's chapter, her wireless
and her telegram, home through the dark house, out to the gazebo, Brice's
chapter, and home again. No step in it fails.

254 tests, ten recordings.

## 160. A film played twice, and a chapter that ended at the beginning

Both of helba's, watching Brice's chapter.

**The weathervane's film ran through twice on every pull.** The rope's hotspot
reads

```text
setState( oStoryteller, #showMontage, 1 )
updateDisplay( oPuppeteer )
setLoop #win_pulleyLoop, 100
pushVideo
wait #videoStop
```

The `setState` and the `updateDisplay` are what choose *which* film is on the
video channel, and the redraw starts it -- so by the time `pushVideo` runs the
film is already playing, and `pushVideo` with nothing named started it again.
It now only starts the room's film when that is not already the film that is
running, which is what `pushVideo` means when the thing it would push is
already pushed.

**And then the chapter ended on the opening titles.** `goodbyeMandy` finishes
with

```text
enterNewDomain( oStoryteller, string(#Roxy), 'Brice_reentry' )
```

-- a room *name* as the third argument where Margaret's chapter passes a
number, and not a name that resolves either: the room is `Ggaz_Reentry`, and
the game does not use the argument at all. It works out where to put the
player from which domain they are leaving, which entry 156 already ported.

But that only ran when there was something in the freezer, and a recording
that starts inside a chapter -- `brice.walk` does, so the chapter can be
watched on its own -- has nothing in it. So the return fell through to the
chapter's declared start, which for Roxy's house is the opening film. helba's
log says it plainly:

```text
[   4015] audio  basement_doorGadgets   play solidDoorOpen
[   4171] audio  Gbhs_playIntro         play (unnamed) 4396096 frames
```

Coming home with an empty freezer now uses the same re-entry room the thawing
path does, so `brice.walk` ends where his chapter ends: in the gazebo.

254 tests, ten recordings.

## 161. Not a bug: the picture the game comes back through

helba sent a screenshot of `Ggaz_Reentry` -- a smeared, colour-shifted mess --
and asked what happened. Nothing did.

The plate is `G_REENTRY_LOADPICT`, and it decodes cleanly: dumping its indices
straight out as greyscale gives a perfectly coherent, if soft-focus, view up
into the gazebo roof. Its palette is real too, a member of its own called
`reentry.WIN`, and 1536 bytes of it, white at 0 and black at 255 like every
other palette in the game. The colours are what the authors chose. It is the
picture the game shows while it is coming back through the portal, and it is
meant to look wrong.

What was wrong was that the recording *stopped* on it. The room has exactly
one live hotspot and it covers the whole frame:

```text
[#down, rect(18, 58, 622, 363),
 ["setState( oStoryteller, #showMontage, 2 )", "updateDisplay( oPuppeteer )",
  "setState( oStoryteller, #showMontage, 0 )", "goTo( #Ggaz_viewE, #forward )"]]
```

-- so a click carries the montage on and puts the player in the gazebo. Both
recordings take that step now and end looking at the gazebo instead of at the
loading plate.

Worth writing down for the discipline as much as the fact: the way to tell a
bad decode from odd art is to throw the palette away and look at the indices.
A wrong palette keeps the structure and loses the colour; a wrong decode loses
both.

254 tests, ten recordings.

## 162. All of Brice's chapter, not just the way out

Entry 159 played his chapter down its spine -- trapdoor, padlock, rope, panel,
closet -- which is the shortest line through it and misses most of what is
there. helba asked for the whole thing.

**The heart box**, buried in the iris bed. Three nails, each `#out`,
`#halfway` or `#in`, and `pushNail` moves two of them at once: the one pressed
goes a step deeper, or pops all the way out from `#in`, and the next one round
goes the other way -- back a step when this one went deeper, forward a step
when it popped. All three out opens the box, and `resetHeartBox` puts them
back to halfway the moment the player walks away, so it has to be done in one
visit.

Solved by walking the eighteen states rather than by pushing nails until
something happened: from all-halfway it is five pushes, left, left, bottom,
left, right.

**The shed**, which is what the chapter is really about. Two planters stand in
it with mirrors on them and there is a peephole in the wall, and the three
together are how he watched her window:

```text
NE = #west, SW = #east   -> B_SH_PEEPR_XCU_MIRROR   (his own mirror)
NE = #west, SW = #north  -> B_SH_PEEPR_XCU_WINDOW   (her window)
NE = #north              -> B_SH_PEEPR_XCU_WOOD     (a plank)
```

One arrangement of three shows the window, and the right-hand hole is the one
that is open. The recording sets both mirrors and looks through it.

Also the conservatory, where he grew what he gave her, and the hives at the
bottom of the garden that he talks to.

`brice.walk` is 211 steps through 102 of his 235 rooms with nothing in it that
fails, and `full.walk` is 718 steps through 238 rooms across all three
domains. Both still come out of the same `play(r)`.

254 tests, ten recordings.

## 163. Edwin's chapter, and the handler that drives it

The third chapter is behind the boathouse door -- the crowbar from the garage
pries it, and the portal wants the Amber vision on and `#Edwin` still in
`#ghostsRemaining`, the same shape as the gazebo's.

Inside it is bigger than the other two. Its schema declares a wind and a
weathervane, a boat with a sail and an anchor at three heights, a car that
runs on a network of thirteen named tracks, a chipmunk called Chippy who
wants pulling out of something, and a teddy that has to end up in the car:

```text
#Wind: [#None, #s, #e, #W, #n]   #weatherVane: [#n]
#boatPosition: [#backward, #forward]   #hookHeight: [#high, #medium, #low]
#carLocation: [#inStorage]
#currentTrack: [#main, #A, #B, #c, #AL, #AM, #AR, #BL, #BM, #BR, #CL, #CM, #CR]
#chippyLocation: [#waiting, #inCar, #home]
#teddyLocation: [#waiting, #onAnchor, #inCar, #home]
```

Nearly all of its handlers are ported. The one that is not is `driveTheCar`,
which is the chapter: two thousand bytes of bytecode that picks a film for the
stretch of track being driven, plays it while running a list of timed markers
against the film's own clock -- most of them a volume envelope for the track
loop, some of them Lingo lines to `do` -- and then puts the car down somewhere
else.

The port chose the film and stopped there. So every drive played its stretch
of track and left the car exactly where it started.

### The half of it that is a table

The end of the handler is not clever at all:

```text
if getPos( [#BL, #BR],          whichTrack ) then goTo #teN_fwd
if getPos( [#CL, #CM_missRamp], whichTrack ) then goTo #teS_fwd
if getPos( [#AR, #CR],          whichTrack ) then goTo #teE_fwd
if getPos( [#AM, #AL],          whichTrack ) then goTo #teW_fwd
```

Four tunnel mouths and a table saying which stretch comes out at which. And
one stretch that does something else: driving the middle of the C track with
the teddy hanging on the anchor is the rescue -- `teddyGetsIn`, the film,
`teddyGetsOut`, and then `goTo #car_domainExit` and
`enterNewDomain( #Roxy, 'Edwin_reentry' )`. That is how the chapter ends, and
it is the only way out of it.

That table is ported now, so the car goes where it is driven and the chapter
has an ending again. What is still missing is the marker machinery in the
middle -- the timed lines that put Chippy in the car and take the teddy off
the anchor as the film runs past them. That is the next piece, and it is the
last large one in the game.

254 tests, ten recordings.

## 164. The heart box, and a deadlock I shipped twice

helba: why does Brice's heart box never finish and move on. It opens -- their
log shows the montage films selected -- and then the game stops.

Entry 158 changed what a `wait #videoStop` asks. It used to require an empty
effect queue, which deadlocked whenever anything was queued behind it; I
changed it to require that no film is still waiting to be *started*. That
fixed the case in front of me and broke a different one, because it is asking
about the wrong films.

A wait armed out of the effect queue has already had everything before it
handed over. The film it is waiting for is running, and everything left in the
queue is *after* the wait -- so looking there finds the films belonging to
later waits. The heart box queues three of them:

```text
FadeToMontage 1, PlayVideo, WaitForVideo, StopVideo,
FadeToMontage 2, PlayVideo, WaitForVideo, StopVideo,
FadeToMontage 3, PlayVideo, WaitForVideo, StopVideo, ...
```

The first wait saw the second and third films sitting in the queue and never
cleared.

A wait the *script* arms is the other case, and the one entry 141 was about:
`pump` stops at it with the rest of that action's effects still queued, and
the `pushVideo` on the line above may be among them. So the two need telling
apart, and `wait_satisfied` is told which it is holding.

### Why neither time was caught

`settle` -- how the terminal runs a queue -- steps over film waits entirely.
So every recording replays without ever asking whether a queue of films
terminates, and both deadlocks reached helba's screen with ten passing
recordings behind them.

There is a test now that queues the heart box's shape and drains it the way
the window does, asserting it reaches the end. Against the old logic it fails
with "the queue stalled with 10 effect(s) left", which is precisely what helba
was looking at.

That is the third time the terminal's blindness to a wait has shipped a
window-only fault, and the second time the answer has been a test rather than
another rule about how to be careful.

255 tests, ten recordings.

## 165. The ending helba could feel was missing

"I swear there is a missing cutscene at the end of Brice, something feels
missing." There was, and all of it: the click on the closet went from the
basement to the gazebo in one frame.

`goodbyeMandy` is a minute of film. The closet swings open, the basement's
loop fades out and the drips come up, three montage steps walk the closet from
a first look to a second to Mandy herself, he says his line, `Bxtlites.mov`
plays over her, the lights go out, `Bexit.mov` plays, and only then does it
ask for Roxy's house back. Its last line is

```text
enterNewDomain( oStoryteller, string(#Roxy), 'Brice_reentry' )
```

and this engine acted on that the moment it saw it. An outcome's `new_domain`
was handled at the top of `apply`, before the outcome's own effects had even
been put in the queue, so the whole ending was queued behind a jump that had
already happened. There is no way to notice this from the state: the chapter
finished, the flags were right, the player was in the right room. Only the
minute of film was gone.

A domain change is now an effect. If anything is queued or waiting when the
flag arrives, the change goes on the end of the queue behind it and takes its
turn; only an `enterNewDomain` with nothing in front of it still jumps
straight away, which is what a transit room's last action is.

### And the beat inside it

Step 3 of that montage declares no plate and no film. The screen goes black,
and

```text
setState( #showMontage, 3 )
set the queuedSound of oPuppeteer = #lightsOut
```

-- the sound of the lights going out is the whole of it. The port had the
black and not the sound, which makes a moment read as a gap. Steps 3 and 4 arm
no transition either; only step 5 does, on the way to the re-entry picture.

### One more thing the freezer took with it

`#currentDomain` is declared by each chapter's schema, so seeding a chapter
once -- entry 156 -- meant it stopped being written. After a chapter handed
the player back, the flag still named the chapter they had left. It is written
on every chapter change now, which is what it is for.

255 tests, ten recordings.

## 166. The power is not reset; the house is

helba: after Margaret's chapter, going inside and out resets the power.

It does not. Tracing every write to `#houseLightsAreOn` across the whole
playthrough gives four, all of them before her chapter:

```text
[0] state DarkUp_OfficeEswitch  set houselightsareon = Int(1)   -- the breaker
[0] state OfficeEmergencySwitch set houselightsareon = Int(1)
[0] state StudyAmberCU          set houselightsareon = Int(0)   -- "to momentarily
[0] state StudyAmberCU          set houselightsareon = Int(1)      suppress AVISION"
```

and it stays at 1 for the rest of the game. Nothing in the return path touches
it: `enterNewDomain` writes `#houseLightsAreOn: [0, 1]` only on the branch for
a domain with nothing stored, which is a chapter being entered for the first
time, never the house being handed back.

What actually happens is that the house you are handed back into is the dark
one. Roxy's house exists twice in the data -- `DarkUp_*` and `DarkDn_*` beside
the lit rooms, which is how the opening works before the breaker is pulled --
and coming home from Margaret's chapter puts the player in `DarkUp_40sReentry`,
art `DK_40s_REENTRY`, a dark copy of the room the portal was in. Every exit
out of the dark half leads to more of the dark half; nothing there is guarded
on the lights at all. The way out of it is the front door, and coming back in
through the porch is what puts the player in the lit house again.

So from the player's chair it reads exactly as helba described -- the power is
off until you go out and come back -- and no flag has changed. The chapter
hands you back into the version of the house you left at the beginning of the
game, and you have to walk out of it.

Recorded rather than fixed, because as far as I can tell it is what the game
does: the re-entry rooms are named only from `enterNewDomain` and Margaret's
is the dark one, while Brice's is the gazebo and Edwin's the boathouse -- each
of them the place the portal was, and hers was upstairs in the dark.

255 tests, ten recordings.

## 167. Into Edwin's chapter

The third portal is the boathouse, and the route to it is all there: the
crowbar hangs on a post on the bottom level of the garage, behind a door that
has to be pushed open; using it on the boathouse doors sets
`#boatHouseIsLocked` to 0, a click swings them, and then the portal wants what
the gazebo's wanted -- the Amber vision on and `#Edwin` still in
`#ghostsRemaining`. Walked end to end it lands at `ice_b_entry`.

Inside, the chapter turns out to be further along than I thought. Nearly every
handler is ported, and what stood out as missing was one small one.

### The passenger

```text
on chippyHopsIn
  wait 30 : startSound #carDoorOpen
  setState( oStoryteller, #chippyLocation, #inCar )
  wait 30
  passengerSprite = 45
  set the castNum of sprite passengerSprite = 1183   -- chpenter.mov, 172x80
  set the loc     of sprite passengerSprite = point( 454, 365 )
  ... run it out ...
  startSound #carDoorClose
```

A film of the chipmunk climbing in through the passenger window, parked at the
passenger side. It is asked for from one place and asked for conditionally:

```text
"goTo( #car_inside, #fadeIn )",
"if getState( oStoryteller, #chippyLocation ) = #waiting then chippyHopsIn"
```

-- so he gets in when *you* do, not when the duck on the wing is squeezed,
which is what calls the car over. With the handler missing he simply never
rode along, and two of the car's films are the ones with him in it.

Ported, and it needed nothing new: a film on a script-driven channel is what
entry 151 built for the PeeK unit.

### What the chapter wants

Reading the handlers rather than playing it: the crank in the boathouse lowers
the hook two turns from `#high` to `#low`, and with it down the hook pulls
Chippy out of the ice. The duck calls the car; getting in brings him along.
`setSail` swings the boat between `#backward` and `#forward` depending on
which way the wind is blowing, and on the swing forward it hangs the teddy on
the anchor:

```text
if getState( #teddyLocation ) = #waiting then
  setState( oStoryteller, #teddyLocation, #onAnchor )
```

which is the state `driveTheCar` looks for when it picks `CM_teddyRescue` --
the film that ends the chapter. So the spine is: free the chipmunk, collect
him, turn the wind, sail the boat, drive the middle of the C track.

255 tests, ten recordings.

## 168. The wind, and a guard I read backwards

There is no `edwin.walk` yet. Working towards one turned up why there could
not have been.

The whirligig on the ice is what starts the wind blowing. Both of its handlers
open with the same line:

```text
  0  push #Wind
  6  call getState
  8  push #None
 10  compare <>
 11  jump -> 15      -- taken when the comparison is FALSE
 14  return
```

The jump is taken when `Wind <> None` is false -- that is, when the air is
still -- so the handler *runs* in still air and returns once the wind is up,
which is what a thing that starts the wind should do. I had ported it as `if
getState( #Wind ) = #None then return`: the exact opposite, and worse than an
ordinary inversion, because it made the whirligig refuse to work until there
was already a wind and nothing else in the chapter makes one. The vane only
steers a wind that is already blowing; `setSail` only reads it.

So the wind could never start, the boat could never sail, the teddy could never
reach the anchor, and the film that ends the chapter could never be chosen.
Edwin's chapter was unstartable, and the reason was one comparison.

Worse, I had written a test that asserted it -- `nothing_happens_in_still_air`
-- and a fixture called `windy` that set the wind blowing before every case. A
test can hold a mistake still as easily as it can catch one. It is now
`nothing_happens_once_the_wind_is_up`, and the fixture is still air, which is
the state the whirligig is actually worked in.

With that turned round the whirligig starts a north wind on the first click,
which is what the vane was pointing at.

### What is left

The vane turns to steer it -- `setWeathervane #clockwise` / `#counter` in
`ice_a_weathervane` -- and a west wind is the one that brings the boat forward
and hangs the teddy on the anchor. Getting there through Edwin's ice field is
where my route-finder gives up: it is a grid of `ice_<row><col><facing>` rooms
and the shortest path in the static graph keeps leading into rooms whose exits
are not live. That is the next thing to sort out, and then the walk is: free
the chipmunk with the hook, start the wind, turn the vane west, sail the boat,
collect the chipmunk with the car, and drive the middle of the C track.

255 tests, ten recordings.

## 169. The tail of the handler, and the order of the queue

Edwin's chapter now runs end to end, and `edwin.walk` records it. Getting
there cost two bugs, and they are the same bug twice: something that happens
last was not happening.

### The wind leaves the world read-only

`startWhirligig` was ported down to its last five lines, and the last of those
is

```text
setState( oStoryteller, #showMontage, 0 )
```

`#showMontage` is not a decoration. It is a small number saying which of the
storyteller's montage images is over the room, and half the chapter's hotspots
are guarded on it being 0 -- the boat's sail, the crank, the duck, the car's
windscreen. So the whirligig raised the wind, left the flag on 1, and from
that moment on the player could walk about the ice and do nothing else. The
chapter was unfinishable, and it looked like a level design problem rather
than a missing line.

That is now the fifth handler whose *tail* was the thing that mattered:
`panelButton`'s door, `goodbyeMandy`'s return, `deleteInventory`'s slot,
`setDoorIsOpen`'s film, and this. The pattern is clear enough to say plainly:
when a handler ends in a state write, that write is usually the point of the
handler, and the film above it is the decoration.

### The write that arrived before the film

With that fixed the chapter still stopped, one room further along. The
weathervane's trellis hotspot reads:

```text
setState( oStoryteller, #showMontage, 4 )
goTo( #ice_a_trellis_wUP, #fadeIn )
fadeToMontage 3
fadeToMontage 2
fadeToMontage 1
setState( oStoryteller, #showMontage, 0 )
goTo( #ice_a_trellis_w, #backOff )
```

Climb down and the montage should end on 0. It ended on 1.

The reason is a seam in this engine that had not been leaned on before.
`setState` writes the flag *as the action list is read*. `fadeToMontage` does
not: it queues an effect, and the effect writes the flag when the queue
drains. So the four writes in that list happened in the order 4, 0, 3, 2, 1 --
the script's own last line beat the three effects it was written to follow.

The fix is not to stop writing at read time. A condition later in the same
list has to see the new value, and plenty of lists depend on that. So the
write still happens immediately, and the outcome now also carries a note of
what it wrote:

```rust
/// Plain state writes this list made as it was read, in order.
///
/// They have already happened; this is a note of what they were, so a
/// caller with a queue still draining can make them happen again in the
/// right place. `Game::pump` is the only caller that does.
pub writes: Vec<(String, Value)>,
```

and `pump`, which is the one place that can see the queue, replays them into
it when there is something outstanding:

```rust
if !self.pending.is_empty() || self.effect_wait.is_some() {
    let repeats = outcome
        .writes
        .drain(..)
        .map(|(key, value)| Effect::SetState { key, value });
    outcome.effects.extend(repeats);
}
```

The second write is idempotent -- `State::set` moves a value to the head of
its list, and doing that twice is doing it once -- so the cost is a duplicate
entry in the walkthrough's report and nothing else. When the queue is empty
the note is dropped and nothing changes, which is why the reports for most
rooms look exactly as they did.

Worth recording about the test I wrote for this first: it passed against the
broken engine. It drained the queue and asserted the final value, and in a
test harness with no room and no decoder the fades never got applied at all,
so 0 was the answer either way. The test that actually catches it asserts the
*queue*, not the outcome:

```rust
assert!(matches!(
    game.pending.as_slice(),
    [
        Effect::FadeToMontage(3),
        Effect::FadeToMontage(2),
        Effect::FadeToMontage(1),
        Effect::SetState { key, value: lingo::Value::Int(0) },
    ] if key == "showMontage"
));
```

I only found that out by breaking the fix on purpose and watching the test go
on passing, which is a habit worth keeping.

### Where a drive ends

`driveTheCar` picks the film for the stretch of track you are on, and the port
stopped there. Everything after the film -- which is where the car *goes* --
was missing:

```text
if whichTrack is one of [#A, #B, #c] then
  carLocation = value( "#hub_" & string(whichTrack) )
  currentTrack = #main : showMontage = 1 : return
carLocation = #standingBy : currentTrack = #main
if whichTrack is one of [#CM_anchorDown, #CM_emptyAnchor,
                         #BM_withChippy, #BM_noChippy] then
  killVideo : showMontage = 3 : return
if whichTrack <> #main then setLoop #underWater
... and then the four tunnel mouths ...
```

Three trunk lines each end at their own hub; four of the spurs are ramps the
car does not make and drop it back where it started on montage 3, which is the
state the windscreen hotspot is guarded on; the rest come out of a tunnel
mouth back on the ice. So every drive played its film and left the car exactly
where it was, and the track network could not be crossed.

The end of the chapter is in the same handler and was guessed rather than
read. `CM_teddyRescue` does not jump domains on the spot. It calls
`teddyGetsIn`, drives out through `#car_domainExit`, plays that room's film,
and only then

```text
startSound #toRoxy
enterNewDomain( oStoryteller, string(#Roxy), 'Edwin_reentry' )
```

-- which is a room *name*, where Margaret's and Brice's both hand back an
index. It lands at `Gbhs_Reentry1`, which is where the freezer from entry 158
already expected him to come back to.

### Chippy's list

One more, small and worth it. `chippySpeaks` plays the plea at the head of the
list and then rotates the list one place -- the back one comes round to the
front. The port played the plea and left the list alone, and the consequence
was that `pullOnChippy`, whose test is "is `#pullMyFinger` lying *second*",
could never fire. Leaning in to look at him is the first turn, so two more
clicks make three, and after three the joke is there to be taken. Once.

### edwin.walk

177 lines, and it plays the whole chapter rather than the spine: the horse and
the salt lick in the boathouse, the finger joke and the grunt that follows it,
the crank, the trellis montage, the whirligig, the sail, the dive, the duck,
a wrong turn down the middle of the B line to see the one stretch of film with
the chipmunk in it, and then the middle of C.

`full.walk` is 940 lines and now runs from the boathouse at the start of the
game to the end of Edwin's chapter, through Margaret's and Brice's on the way.
The route in is the third portal: push the door on the bottom level of the
garage, take the crowbar off its post, pry the boarded boathouse doors, swing
them, and look. It needs `set AMBERVISION on` once more, for the reason entry
155 gives -- that makes four hand-set lines in a 940-line walk, and all four
are marked where they happen.

260 tests, eleven recordings.

## 170. Two paths, one queue

helba, playing the car: "some of events seem to double play randomly a lot in
the car".

Not random, and not the car. Two of the window's paths were acting on effects
that had already been queued.

`Game::pump` runs a hotspot's action list one action at a time, and for each
one it calls `apply`, whose last line is

```rust
self.pending.extend(outcome.effects.iter().cloned());
```

Then it hands back a merged `Outcome` covering everything it ran. That
outcome is a *report*. Every effect in it is in the queue.

The window's main loop drains that queue every frame, in `apply_effects`, and
that is where sound is played and films are started. But two other places also
walked the returned effects and played them themselves:

- the resume block, which is what lets a part-run sequence carry on once the
  film it was waiting for has finished;
- the held-button block, which is how a dial keeps turning.

So each of their cues sounded twice: once the moment `pump` returned, and
again when the queue reached it. It sounded random because the two are not
close together -- the queue holds at waits and the direct call does not, so
the gap between the two copies is however long the wait in front of them
lasts. In most rooms a sequence is short and the two land almost on top of
each other, which reads as one thick sound. The car's drive is a long
sequence with several films in it, and there the copies come seconds apart.

Both blocks now do nothing but set `dirty`. Everything they used to do,
`apply_effect` already did -- including `StopVideo`, which the resume block
had its own duplicate copy of.

The invariant is worth a test rather than a comment, because it is the kind of
thing a future caller will get wrong the same way:

```rust
let outcome = game.pump();
for effect in &outcome.effects {
    assert!(game.pending.contains(effect),
            "{effect:?} was reported but not queued");
}
```

The terminal never had this: `walk` prints what `settle` drains and plays
nothing, so a fault that is only audible could not show up in a recording.
That is the second time the walkthrough's blindness has hidden something --
entry 150 was the first -- and both times the answer was the same, which is to
put the invariant in a test instead of trusting the front end to reveal it.

261 tests, eleven recordings.

## 171. The car's films, and a terminal that can see a deadlock

helba, with a photograph of the windscreen and a log: "looks like a clipping
bug with the video and also got stuck".

Both were the same line of the port.

### A film named after the wrong thing

`driveTheCar` chooses which stretch of track to show and my port pushed the
track's own symbol as the movie:

```rust
out.effects.push(Effect::PlayVideo(Some(film.to_string())));   // "CM_teddyRescue"
```

There is no movie called `CM_teddyRescue`. The names are in `trackData`, and
the disc ships the table twice -- once resolved to cast numbers and once, in
the source copy, written out:

```text
#trackData:[ #main: [#trackMovie: the number of cast "carback.mov", ...],
             #CM_teddyRescue: [#trackMovie: the number of cast "CMUend.mov", ...],
             #BM_withChippy:  [#trackMovie: the number of cast "bmchpout.mov", ...], ... ]
```

Seventeen of them, and only three share the track's name. So every drive
opened nothing at all, and what stayed on the windscreen was whichever film
had last been opened, held at its last frame and drawn at its own size in the
rect the room had set for a different one. That is the picture helba sent.

The table is now in the port beside the tunnel mouths, and a drive names a
real film.

### And where it sits

The other half of the picture. `car_inside` declares its film like this:

```text
#castName: "carBack.mov", #channel: #video, #coords: point(322, 204),
#showIF: [#equals: [#showMontage, 3]]
```

`start_room_video` takes the coords from the sprite whose guard holds, which
is right for choosing *which* film. It is not right for position: in Director
a channel's location is a score property and the `#showIF` decides what is on
the channel, not where the channel is. The moment the car sets off the montage
goes to 0, the guard stops holding, and the coords went with it -- so the
track films drew centred on the stage instead of in the windscreen. `play_movie`
now takes the room's video coords with the guard preferred and without it as a
fallback.

### What `pushQT` actually does

Worth writing down, because I had it wrong in a way that would have bitten
again:

```text
on pushQT startTime, stopTime, ticksPerFrame
  totalTime = stopTime - startTime
  set the movieTime of sprite 44 = startTime
  startTimer
  repeat while the timer <= totalTime
    set the movieTime of sprite 44 = startTime + the timer
    updateStage
  end repeat
  set the movieTime of sprite 44 = stopTime
```

It is a scrub against the clock, not a playback. It runs for exactly
`stopTime - startTime` ticks whatever the film's length, and the `wait
#videoStop` after it clears at once because nothing ever set a movie rate.
The three thirds of a junction film -- 0-223, 225-448, 450-675 at a hub, and
0-178, 180-358, 360-540 on a spur -- are ticks, and they match the films
exactly: `STRT_CBA.MOV` is 225 frames at 20fps, which is 675 ticks, and
`B_BLBMBR.MOV` is 180 frames, which is 540.

### A terminal that can hang

None of this showed in a recording, and that is the third time. `walk`'s
`settle` steps over every wait, so a `wait #videoStop` on a film that never
finishes is invisible there and fatal in the window. Entry 150 was the first,
158 the second, 170 the third.

So `walk` has grown a `--strict` flag. It replays with the window's own gate:
the queue is drained rather than settled, and the next step waits for the game
to go quiet the way the window waits -- unless what is being waited on is a
click, which only the next step can supply. A recording that hangs in the
window now hangs here, and says where:

```text
!! the game never went quiet after `click 322 242`: 0 effect(s) pending,
   wait "a film", script 11 line(s), holding "a film"
```

Two things had to be lent to it, and both are only the clock:

  - a tick wait is brought forward to now, because the terminal cannot spend
    thirty sixtieths of a second;
  - a film is ended by hand, *after* the queue has had its turn -- arming a
    film wait is what takes the loop off a film, so ending it first read every
    looping room film as a deadlock. Margaret's doorway mirror turns for as
    long as the player stands in front of it and is not stuck at all.

Nothing else is stepped over. A click wait stands, a queue that will not drain
stands, and a film that genuinely never ends stands.

Building it found one more bug in the building of it. `drain_ready` hands its
effects back for the caller to act on -- the window's `apply_effect` is what
plays them -- so reporting them without applying them gave a front end that
watched its own queue go past: every state write in a sequence was announced
and none of them happened. The same mistake as entry 170 seen from the other
side, and the fix is that both front ends now share one `describe`.

All eleven recordings go quiet under `--strict`, `full.walk` included.

261 tests, eleven recordings.

## 172. Something to point at the screen with

helba, with a second photograph of the windscreen: "looks like double video
still playing".

I could not tell from the picture, and I could not tell from the terminal
either, which is the point of this entry. `walk` reports films rather than
opening them -- deliberately, since there is nothing to show them on -- so
`stage`, `--strict` and every recording are all blind to what is actually
painted. Three entries in a row have now been diagnosed from a photograph.

So: `Game::stage_report`, which is the compositor's own layer list in the
order it paints, as text.

```text
room sprite ch1 cast 1067 ink 0 at Some((320, 240))
film b.mov 320x240 drawn 320x240 at Some((322, 204))
overlay film on ch45 172x80 drawn Some((172, 80)) at Some((454, 365))
puppet ch44 cast 1031 ink 0 at Some((336, 184))
```

It is on `stage` in the terminal, and on the **S** key in the window, where
it goes to the log. A fault that is only visible can now be asked about at the
moment it looks wrong, rather than reconstructed from pixels afterwards.

Four things could put a second film on the stage, and the report separates
them at a glance: the room's own film, a film a script pushed over it, a film
running on a puppet channel, and a puppet left behind by a sequence that has
finished. In the car all four are in play -- `initWhirligig` puts films on
channels 44 and 45, `chippyHopsIn` puts one on 45, and `driveTheCar` pushes
one over the room's -- and which of them is still alive after a drive is
exactly the question I could not answer.

Worth being honest about the limit while I am here: `--strict` models the
queue and the waits, not the picture. It caught the deadlock in entry 171
because a deadlock is a queue that will not move. It would not have caught a
film drawn twice, and no recording ever will.

261 tests, eleven recordings.

## 173. One sprite, one film

helba, with the picture marked up: the arrow points at a thin blue band at
the top of the windscreen -- "the right video drawing wrong" -- and the block
under it is "a still frame from another video misplaced". Two films on screen,
one over the other.

The `stage` command from entry 172 answered it in one line:

```text
room sprite ch1 cast 1067 ink 0 at Some((320, 240))
film carBack.mov 320x240 drawn 320x240 at Some((322, 204))
overlay film on ch44 320x240 drawn Some((320, 240)) at None
```

A film on channel 44, and the room's film also on channel 44 -- because
`MOVIE_CHANNEL` is 44, and I had written that constant without noticing what
it meant.

It is not a made-up number. Every handler that swaps a film writes `the
castNum of sprite 44`, and `pushQT` scrubs `the movieTime of sprite 44`. The
room's `#video` channel and the channel a script puts a film on are **the same
sprite**. Writing one replaces the other; they cannot both be on screen. This
engine drew both, and the compositor sorts by channel with a stable sort, so
the overlay -- pushed second -- landed on top. The film underneath showed as a
band round the edge of the one covering it, which is exactly what the arrow is
pointing at.

Edwin's car is where all of this meets. `setCarLocation` puts the junction
film on 44 when the car reaches a hub -- three thirds of one film, left,
straight and right, which is what `chooseTrack` scrubs a third of -- and
`driveTheCar` pushes the stretch of track over the room's video channel. Both
were live at once.

Three changes, all the same rule:

  - the compositor does not draw the room's film while a script owns the video
    channel;
  - `play_movie` and `start_room_video` release an overlay on that channel,
    because writing the channel is what replaces it;
  - an overlay on that channel with no position of its own takes the room's
    `#video` coords rather than the middle of the stage. A channel's location
    is a score property and `#showIF` decides which film is on it, not where
    it is -- so the junction film belongs at (322, 204), in the windscreen,
    and not centred on the plate.

The last of those is entry 171's positional fix again, arriving from the other
side: there for the room's own player, here for a film a script put on the
channel. Both now go through one `video_channel_centre`, which prefers the
sprite whose guard holds and falls back to the first video sprite when none
does -- the usual case while a script is playing a film of its own.

The report says which is which now, so the next one of these is a keypress
rather than a photograph:

```text
film carBack.mov 320x240 drawn 320x240 at Some((322, 204))
  -- not drawn: a script has the video channel
overlay film on ch44 320x240 drawn Some((320, 240)) at Some((322, 204))
```

262 tests, eleven recordings.

## 174. A film on a channel is a still

helba: "the first car inside played white ... where he cleans the fog off the
window", and then "got to the end of the first section and it played the first
wipe off fog again".

Both are one mistake, and it is a Director one rather than a Lingo one.

`carback.mov` opens on a completely fogged windscreen and clears over its
hundred and twenty-one frames -- frame 0 is white. So "played white" is a film
showing its first frame and not advancing, and "played it again" is a film
that should have been standing still and was running instead.

`setCarLocation`, in full:

```text
if getPos( validSuggestions, suggestion ) > 3 then
  set the castNum of sprite 44 = getProp( oPuppeteer.<hubClips>, suggestion )
  updateDisplay( oPuppeteer )
```

`#hub_main` is `strt_CBA.mov`, `#hub_A` is `A_ALAMAR.mov`, and so on: the
junction film whose three thirds are left, straight and right. It is put on
the channel and *left there*. Nothing plays it. `chooseTrack` scrubs a third
of it when the player picks a direction, and until then it is a still of the
junction ahead.

This engine's `point_channel` opened the film and started it, looping. So
reaching a hub started the junction film playing over and over, and since
these films all open on the same fogged windscreen it read as the fog wipe
repeating. The one before it -- the drive itself -- was `carback.mov` held on
frame 0, white, because the overlay had taken the channel from it.

The rule, which is Director's: pointing a channel at a digital video member
shows a frame. What makes it move is a handler setting the movie rate, which
is what `pushVideo` and `pushQT` do. So `point_channel` now parks the film on
its first frame, and the three handlers that want motion say so with a new
`PlayOverlay` -- the PeeK unit's roll-up, the chipmunk climbing in through the
passenger window, and the whirligig, which spins.

The stage report says which it is, since that was the whole difficulty:

```text
overlay film on ch44 320x240 drawn Some((320, 240)) at Some((322, 204))
  (a still, not playing)
```

### The audio in the log

helba's log also had `no free channel for trackLoop, dropped`, and `trackLoop`
starting twice sixty ticks apart. Both are right. The game mixes on four
channels and `soundEffect` gives up rather than finding room; with the house
hum, the water, `homeEdwin` and `iCantSee` all running there was nothing free.
And the two starts are two separate drives -- `chooseTrack` raises the loop
and drops it, then `driveTheCar` does the same -- which is what the handlers
do. A loop already running is left alone; these were not running.

263 tests, eleven recordings.

## 175. The finish line

helba: "we escaped edwins with no bugs onward to the finishline?"

There was more of the game left than I thought, and one handler standing
between here and the end of it.

### The handler that closes a chapter

`initInventory` rebuilds the inventory bar, and then, at the bottom, does
something else entirely:

```text
if getState( #currentLocation ) = #DarkUp_40sReentry then
  fadeOutTransit
  trimState( #ghostsRemaining, #Margaret ) : ghostCalls #None
  trimState( #hauntsRemaining, #ghostBrushingHair )
  trimState( #hauntsRemaining, #stairsGhost )
  setLoop #houseHum, 96
  setState( #showMontage, 1 ) : setTransition #slowMontage : updateDisplay
  setState( #PeekDisplay, #psionicFragment ) : peekAlert
```

-- and the same for `#Ggaz_Reentry` and `#Gbhs_Reentry1`. Coming home is not
just arriving in a room. The ghost comes off `#ghostsRemaining`, the haunts
that were *that ghost's* come off the pool so they stop being drawn, and the
PeeK unit reports a psionic fragment, which is the game telling the player one
of the three is done.

None of it was ported. Three chapters played and `#ghostsRemaining` still held
all three, the retired ghosts' haunts were still in the rotation, and the PeeK
never said a word about any of it.

The original hangs it off the inventory refresh, which runs constantly. Here
it runs once, when a chapter's own way home puts the player in its re-entry
room, which is the moment it means.

### The last click

`GarageRoxyXTCU`. Roxy's body in the garage loft, where she has been since the
opening film, and the last hotspot in the game:

```text
[#itemInUse, rect(67, 74, 503, 353),
 ["cursorOff", "deleteInventory( #headgear )",
  "setState( #showXfile, 1 )", ... "setState( #playerHasHeadgear, #usedUp )",
  "setState( #showMontage, 1 )", "goTo(#GarageEscape, #backOff)",
  "suspendSounds", "pushVideo", "wait #videoStop", "killVideo",
  "setState( #showMontage, 0 )", "updateDisplay(oPuppeteer, #fastVideo)",
  "pushVideo", "wait #videoStop",
  "showCreditScreen( oPuppeteer, #endGame )"],
 [#equals: [#itemInUse, #Headgear]]]
```

Put the headgear on her. The X-file shows, the headgear is used up,
`endAnim.mov` plays, then `Endsize`, and then the credits.

`showCreditScreen` was ported as `out.credits = true` and nothing read the
flag. `credits.mov` is in the cast at 2285, 220 by 220, and placed in no room
-- because this is what places it. It plays now.

One small reporting fix fell out of watching the ending: `settle` resolved
"which film would the room play" once at the top and used it for every
`pushVideo` in the queue. The ending steps `#showMontage` between its two, so
they are two different films, and both were reported as `(none)`. Asked per
effect they are `endsize` and the one before it.

### full.walk

970 lines, and it now runs from the opening film to the credits: Roxy's house,
the BAR, the cameras, the telephone, the Amber vision, Margaret's chapter,
Brice's, Edwin's, and then back to the garage with the headgear.

The whole game, start to end, in one recording that replays clean in the
terminal and under `--strict`.

263 tests, eleven recordings.

## 176. The soundtrack of a drive

Three reports from helba playing: the car's first film still doubling, the
beehive in Brice apparently stuck, and Edwin's chapter "buggy in places ...
feels like we're missing steps".

### The double, again

`play_movie` has had a guard since the weathervane (entry 154): a handler that
pushes a film has almost always just asked for a redraw, and the redraw has
already started the room's film, so pushing it again plays it twice. That
guard was only on the unnamed case.

Naming the car's films in entry 171 walked straight into the named one.
`driveTheCar` pushes `carback.mov` and the room plays `carBack.mov` while the
montage is 3 -- the same film, so the redraw starts it and the push restarts
it. The guard now covers both.

### The beehive

Not stuck. `p4b_h3.mov` is five hundred and forty frames at fifteen a second:
`listenToBees` is thirty-six seconds of film with the cursor off, and the
window refuses clicks while a sequence is running, which is what the original
does too. Worth knowing rather than fixing.

### What was missing

`#trackData` gives each stretch of track a film and two lists of cues against
that film's own clock -- one for driving alone, one with the chipmunk aboard:

```text
#B: [#trackMovie: 1197,
     #alone:  [200: 178, 385: 195, ..., 525: #edwinLaugh, ...],
     #chippy: [0: 3, 76: 2, ..., 380: #yell1, ..., 1226: 172, ...]]
```

The key is a movie time in ticks, and the value says what happens there:

  - a symbol is a sound -- `#edwinLaugh`, `#yell1` through `#yell8`,
    `#getTheBear`, `#tooHeavy`, `#gonnaBeSick`;
  - a number above five is the engine's volume, so the track loop swells and
    falls with the gradient the car is on -- the B track has forty of them;
  - a number from one to five is a pose for the passenger's head;
  - a list of strings is Lingo to run, which the table does exactly once, for
    `assertSound #aCleverCar`.

Sixty-four cues on the B track with Chippy aboard. None of them were ported,
so a drive was a film with a flat engine note behind it, which is precisely
"feels like we're missing steps".

They are not queued effects and could not be: a queue is sequential and would
have to wait for the film it is meant to play *over*. So `#trackData` is read
off the chapter's own text chunk -- it shares one with `#sndDurations` and
`#waffleClips` -- into a cue list armed when the film starts, and the frame
loop asks `due_cues` what the film has reached.

The lists are written roughly in order and not exactly: `#A` has 587 twice and
510 after 511. They are sorted, so a cue is not skipped because the one before
it was written later.

One limit worth stating: the terminal cannot fire these. It reports films
rather than opening them, so there is no clock to fire against. It reports the
arming instead -- `cues for B` -- and that a recording cannot check them is
the same blindness entry 172 was about.

264 tests, eleven recordings.

## 177. The clock the terminal did not need

helba, on the limit I had just written down: "how bout you fix that limit it
shouldn't be impossible".

It was not. The reasoning behind the limit was wrong.

I had said the terminal cannot fire a drive's cues because it reports films
rather than opening them, so there is no clock to fire against. But the
terminal's model of a film is already "it happens instantly": `settle` steps
over the wait, `--strict` runs the film out by hand, and the report shows the
film and moves straight on. If a film is over the moment it starts, then
everything keyed to it is due at once -- *in order*.

So `flush_cues` runs the list with no clock at all, and both terminal paths
call it the moment a film is reported. A drive now reads:

```text
cues for B
film b.mov
  0: passenger 3
  76: passenger 2
  163: passenger 3
  182: passenger 4
  200: engine at 178
  302: passenger 3
  380: passenger 2
  380: play yell1
  385: engine at 195
  ...
```

Which is the thing that matters: a recording can now carry a drive's cues, and
therefore check them. `full.walk` fires a hundred and two of them, including
Chippy yelling twice and being sick once.

The window is unchanged and still runs them against the film's real clock;
this is the same list read the way a front end without a device has always
read a film. One `act_on` does the work for both, so the two cannot disagree
about what a cue means.

Worth saying plainly: the limit was not a property of the terminal, it was a
property of how I had thought about it. Three entries in a row have now been
about the walkthrough's blindness, and this one was self-inflicted.

265 tests, eleven recordings.

## 178. An audit, and the scripts that belong to a member

helba: "can we do a full from start to finish making sure we're not missing any
steps, any triggers, anything" -- and then, while I was in the middle of it,
"i think we're missing the door scanner step from roxy ... i swear we're
missing story pieces".

### The audit

`verify` already reports unhandled room-script lines, unported verbs and
unported setters, and all three are empty -- the room scripts are fully
understood. What it could not see is a handler that no room names but another
handler calls, which is how half the game works.

So: every handler defined in every movie, against every name the engine
answers to, with the call sites counted both ways.

```text
237 handlers defined, 322 names answered, 93 not ported
```

Of the ninety-three, sixty-two are called from nowhere at all -- Director
housekeeping, palette and patch tools, debug printers, `xxx`-prefixed
leftovers, and a handful of genuinely dead handlers like `moveClock` from
entry 145. Thirty-one are reachable, and the list is short enough to give in
full:

  - **Housekeeping, no game content:** `report`, `report2`, `patchpalette`,
    `gammafade`, `setcolor`, `setcursorquality`, `closepatchfile`,
    `preloadlocations`, `killlistactors`, `prodvloops`, `waitasec`, `random`.
  - **Ported elsewhere, under another name:** `spawnghostlyevent` is the haunt
    clock and lives on `Game`; `initinventory`'s tail is entry 175's
    `closechapter` and its head is the inventory bar, which this engine builds
    itself; `updateinventory` likewise.
  - **Cursor feedback:** `castcursor` with twenty-four call sites, `cursoron`,
    `cursordance`.
  - **Content still missing:** `playASong`, `chippyGetsOut`, `teddyGetsOut`,
    `gust`, `cyclestatic`, `fadeupradio`, `refreshalignmentpuzzle`,
    `playdomainentrysound`, `fadeouttransit`, `pushqt`, `pushvideocarefully`,
    `chippyspeaksmedium`.

That is the honest answer to "are we missing anything": yes, and here it is.

### The carols

First one taken off the list, because it is atmosphere rather than plumbing
and it was entirely absent:

```text
on playASong
  if getState( #carolsEnabled ) = 0 then return
  if the ticks - lastSong < 12600 then return
  newSong = getAt( lsCarols, 1 ) : startSound newSong
  setState( #windSongs, getLast( lsCarols ) )
```

Four Christmas carols over Edwin's frozen lake, one every three and a half
minutes, the list turning over the way Chippy's pleas do. `killSongs` and
`disableSongs` were ported and this was not, so the ice had four songs on it
and played none of them.

### The scripts that belong to a member

helba's scanner. Director lets a cast member carry its own handlers, and a
click on a sprite showing that member runs them before the room sees it. The
game has twenty-eight, and this engine read none: the dispatch in `click`
knew the telegram's tiles by channel number and nothing else.

The one that matters is on `TXT-tonal ready`, the PeeK unit's readout:

```text
on mouseDown  -- cast 'TXT-tonal ready'
  whichKnob = getState( oStoryteller, #DoorWithScanUnit )
  if whichKnob = #kitchenOutside    then PKscan = #PkPatioScan
  if whichKnob = #bathroomInside    then PKscan = #PkBathroomScan
  if whichKnob = #margaretRmOutside then PKscan = #Pk40sScan
  if whichKnob = #boatHouseOutside  then PKscan = #PkBoathouseScan
  if PKscan <> #None then trimState( #tonalResidueRemaining, PKscan )
  set the visible of sprite 44 = TRUE
```

That is the only click in the shipped data that reads a tonal residue back,
and it is not a hotspot anywhere. `full.walk` has been faking it since entry
145.

Porting it took three things, and two of them were bugs:

  - a table of `(chapter, member name, handler)`, looked up from the cast a
    script-driven channel is showing;
  - **`sprite_at` tested a rectangle, not the art.** It keyed the member --
    the comment even says a click lands on the art and not the field around it
    -- and then only checked bounds, so the PeeK unit's aerial, a tall keyed
    sprite covering the whole body, took every click meant for the readout
    underneath. It now tests the pixel's alpha, which is what Director's `the
    clickOn` means;
  - **the modal dismissal came first.** The unit holds for a click while it is
    up, and `click` spent that click on the hold before any sprite was asked.
    Director runs a sprite's script before the frame sees the click at all, so
    the member script now goes first.

With those, clicking the readout reads the residue. What is still set by hand
in `full.walk` is one step earlier: the alert that puts the readout on that
page unprompted. The click exists now; the thing that makes it worth clicking
does not yet.

268 tests, eleven recordings.
