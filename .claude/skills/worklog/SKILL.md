---
name: worklog
description: Keep an append-only WORKLOG.md while doing substantial multi-session work, then write a retrospective from it at the end. Use when starting, resuming, or finishing significant work in this repo - reverse engineering, format decoding, engine implementation, or any task where findings accumulate and mistakes are instructive. Also use when asked for a retro, a write-up, or "what did we learn".
---

# Work log and retrospective

Long technical work generates knowledge faster than it generates code, and
most of that knowledge evaporates. A wrong guess about a byte offset costs
an hour, gets fixed in one line, and leaves no trace of why the offset was
surprising. The log exists to stop that.

## The two artifacts

**`WORKLOG.md`** is written during the work, append-only, at the repo root.
**The retrospective** is written at the end, from the log.

They are different documents with different jobs. The log is a record. The
retro is an argument about what the record means. Do not try to write the
retro incrementally, and do not rewrite log entries to make the retro
easier - a log edited to look coherent has destroyed the evidence the retro
needs.

## Writing the log

Append an entry when something is settled: a format decoded, a bug found, a
decision made, a claim disproved. Not every tool call, not every commit.

Each entry says what was being attempted, what turned out to be true, and
what it cost. The cost line is the part people skip and the part that is
worth most later.

Write entries as they happen, in the tense they happened in. An entry that
says "I had guessed the layout twice and been wrong twice, so I hexdumped
it" is worth more than one that says "the layout is X", because the second
one hides the method.

### Record mistakes at full strength

This is the rule that makes the log worth keeping. When something was
wrong, the entry says it was wrong, says what made it survive, and says
what caught it. Do not soften it in the writing and do not fix the entry
later when the fix lands - append the correction as a new entry instead.

Be especially alert to **verification that looked strong and was not.** In
this project I checked a bitmap decoder against a second implementation and
got 307,200 of 307,200 pixels identical, and reported that as strong
evidence. Both implementations were mine and both shared the same wrong
assumption about palette ordering, so the agreement proved consistency and
not correctness. The test image was also the one frame in the game least
able to reveal the bug. That entry is the most useful one in the log, and
it only exists because it was written while it still stung.

When a claim in the log turns out to be wrong, also check whether it was
copied into documentation. A wrong finding in a README is worse than a
wrong line of code, because it actively teaches the error.

### Also record

- Numbers that disagreed with each other, and which one was right. Most
  bugs here were caught by a count not matching a count from a different
  method, never by reading code.
- Decisions not taken, and why. "A full bytecode VM is not on the critical
  path because the logic is in plain text" is the kind of thing that looks
  obvious in hindsight and was not obvious at the time.
- What the data says about itself. Formats that declare their own schema,
  start state, or name tables should be read rather than hard-coded, and
  the log should note where that table lives.

## Writing the retrospective

Write it in first person, as the engineer who did the work. helba leads
this work; refer to helba by that name and credit direction, review, and
the reports that caught real bugs. Do not write the retro as if helba wrote
it, and do not use helba's real name anywhere.

Structure that works:

1. **What the thing was** - brief, for someone who was not here.
2. **The decision the project turned on** - usually one finding that made a
   hard problem tractable, or a wrong turn that cost the most.
3. **What went wrong, honestly** - the section that justifies the document.
   Include bugs that survived verification, and say what the verification
   was actually worth.
4. **What held up** - techniques that repaid their cost. Be specific about
   the cost so the reader can judge whether it transfers.
5. **What is still open** - with an honest read on difficulty, separating
   "mechanical, just work" from "genuinely unsolved".

Prose, not bullet soup. Concrete numbers over adjectives. No emojis, no
self-congratulation, no tool branding.

The test for a finished retro: someone picking the project up cold should
learn more from it than from reading the diff, and should be warned off at
least one thing that looked like a good idea.

## Practical notes

- The log is append-only. Corrections are new entries.
- Keep it at the repo root as `WORKLOG.md` so it is found without hunting.
- If the repo ships no game content or other excluded data, make sure the
  log does not quote it either.
