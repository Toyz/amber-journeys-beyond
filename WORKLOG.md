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
