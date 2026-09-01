"""Resolve candidate opcode operands to names and judge coherence.

The weak test is 'the operand is a valid index'. The strong test is 'the names
it resolves to are the ones this game actually calls'. The action vocabulary is
already known from the room scripts, so an opcode that really is a call will
resolve overwhelmingly to that vocabulary, and a wrong guess will resolve to
noise.
"""
import sys, collections
sys.path.insert(0, 'tools')
from lingodis import load, body, names_of, frame, rd

# Verbs the room scripts fire, established earlier from the .DAT files.
KNOWN = {n.lower() for n in [
    'goTo','goBack','setState','getState','inState','trimState','addState',
    'addInventory','deleteInventory','useInventory','stowInventory',
    'updateDisplay','updateStage','setTransition','showCreditScreen',
    'pushVideo','killVideo','cursorOff','cursorOn','fadeToMontage','wait',
    'soundEffect','startSound','assertSound','setLoop','endLoop',
    'suspendSounds','restoreSounds','enterNewDomain','idle','nothing',
]}

paths = sys.argv[1:]
hits = collections.defaultdict(collections.Counter)

for path in paths:
    d, be, res = load(path)
    names = names_of(d, be, res)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c:
            continue
        hcount = rd(s, be, 0x48, 2); hoff = rd(s, be, 0x4a, 4)
        for i in range(hcount):
            p = hoff + i * 42
            if p + 42 > len(s):
                break
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            if coff + clen > len(s):
                continue
            instrs, ok = frame(s[coff:coff + clen])
            if not ok:
                continue
            for (o, op, arg, w) in instrs:
                if arg is not None and arg < len(names):
                    hits[op][names[arg]] += 1

print(f"{'op':>5} {'resolved':>9} {'known%':>7}  most common names")
# An opcode whose operands only ever take a handful of small values cannot be
# tested this way: the first few entries of the name table are themselves the
# commonest verbs, so any small operand "resolves" to one and the test scores
# high for the wrong reason. Only opcodes whose operands range widely carry
# real signal here.
rows = []
for op, counter in hits.items():
    total = sum(counter.values())
    known = sum(c for n, c in counter.items() if n.lower() in KNOWN)
    spread = len(counter)
    rows.append((spread, total, known / total if total else 0, op, counter))
rows.sort(reverse=True)
print()
for spread, total, frac, op, counter in rows[:10]:
    if spread < 20:
        continue
    top = ', '.join(n for n, _ in counter.most_common(7))
    print(f" 0x{op:02x} {total:>7} uses, {spread:>4} distinct names, {100*frac:>3.0f}% known")
    print(f"        {top}")
