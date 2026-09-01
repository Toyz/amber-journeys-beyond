"""Solve unknown stack effects from argument-evaluation windows.

Whole-handler balance failed because handlers branch. Argument evaluation does
not: the values feeding `arglist N` are pushed by a short straight-line run
immediately before it. Each such window gives an exact equation, sum of effects
equals N, over a handful of instructions. Windows containing a jump, a call or
a return are discarded, so every equation used describes genuinely linear code.
"""
import sys, collections
import numpy as np
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, frame, rd

MOVIES = ['extract/BRICE/BRICE.DXR', 'extract/MARGARET/MARGARET.DXR',
          'extract/ROXY/ROXY.DXR', 'extract/EDWIN/EDWIN.DXR',
          'extract/AMBERHUB.DXR']
CALLS = {0x1e, 0x1f, 0x46, 0x56, 0x57, 0x63, 0x66, 0x86, 0x97, 0xa3, 0xa6}
ARGLIST = {0x42, 0x43}
JUMPS = {0x93, 0x95}
RET = 0x01
# Effects already established.
KNOWN = {0x44: 1, 0x84: 1, 0x4b: 1, 0x4c: 1, 0x52: -1,
         0x41: 1, 0x85: 1, 0x45: 1, 0x81: 1, 0x49: 1, 0x89: 1, 0x4f: -1}
BOUNDARY = CALLS | ARGLIST | JUMPS | {RET}

windows = []   # (counts of unknown opcodes, target)
for path in MOVIES:
    d, be, res = load(path)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c: continue
        hc = rd(s, be, 0x48, 2); ho = rd(s, be, 0x4a, 4)
        for i in range(hc):
            p = ho + i * 42
            if p + 42 > len(s): break
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            if coff + clen > len(s): continue
            ins, ok = frame(s[coff:coff + clen])
            if not ok: continue
            seq = [(op, arg) for _, op, arg, _ in ins]
            for k, (op, arg) in enumerate(seq):
                if op not in ARGLIST:
                    continue
                # Walk back to the previous boundary; that run pushes the args.
                j = k - 1
                while j >= 0 and seq[j][0] not in BOUNDARY:
                    j -= 1
                run = seq[j + 1:k]
                if not run:
                    continue
                counts = collections.Counter()
                fixed = 0
                for o2, a2 in run:
                    if o2 in KNOWN:
                        fixed += KNOWN[o2]
                    else:
                        counts[o2] += 1
                windows.append((counts, (arg or 0) - fixed))

print(f"{len(windows)} argument windows")
solo = collections.defaultdict(collections.Counter)
for counts, target in windows:
    if len(counts) == 1:
        (op, n), = counts.items()
        if target % n == 0:
            solo[op][target // n] += 1

print(f"\nopcodes appearing alone in a window (effect -> how many windows agree):")
for op, dist in sorted(solo.items(), key=lambda kv: -sum(kv[1].values())):
    total = sum(dist.values())
    if total < 15:
        continue
    best, n = dist.most_common(1)[0]
    print(f"  0x{op:02x}  effect {best:+d}  in {n}/{total} windows ({100*n//total}%)"
          f"  others={dict(dist.most_common()[1:4])}")
