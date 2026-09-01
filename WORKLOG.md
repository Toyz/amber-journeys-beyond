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
