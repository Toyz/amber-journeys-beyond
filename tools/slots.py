"""Test which opcodes address a handler's arguments or local variables.

Slot references may be indices or byte offsets, and the slot width is unknown,
so every plausible divisor is scored against both tables. A correct pairing
must hold for every use in every handler that has such a table; anything less
is a coincidence. Pass rates for all opcodes are printed so the margin between
a real fit and the background is visible.
"""
import sys, collections
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, frame, rd

MOVIES = ['extract/BRICE/BRICE.DXR', 'extract/MARGARET/MARGARET.DXR',
          'extract/ROXY/ROXY.DXR', 'extract/EDWIN/EDWIN.DXR',
          'extract/AMBERHUB.DXR']
DIVISORS = (1, 2, 4, 6, 8)

ok = collections.defaultdict(collections.Counter)
tot = collections.defaultdict(collections.Counter)

for path in MOVIES:
    d, be, res = load(path)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c:
            continue
        hc = rd(s, be, 0x48, 2); ho = rd(s, be, 0x4a, 4)
        for i in range(hc):
            p = ho + i * 42
            if p + 42 > len(s):
                break
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            argc = rd(s, be, p + 12, 2); varc = rd(s, be, p + 18, 2)
            if coff + clen > len(s):
                continue
            instrs, okf = frame(s[coff:coff + clen])
            if not okf:
                continue
            for (_, op, arg, w) in instrs:
                if arg is None:
                    continue
                for table, count in (('arg', argc), ('var', varc)):
                    if count == 0:
                        continue
                    for dv in DIVISORS:
                        tag = f'{table}/{dv}'
                        tot[op][tag] += 1
                        if arg % dv == 0 and arg // dv < count:
                            ok[op][tag] += 1

rows = []
for op in tot:
    best = max(((ok[op][t] / tot[op][t], t, tot[op][t]) for t in tot[op] if tot[op][t] >= 100),
               default=None)
    if best:
        rows.append((best[0], op, best[1], best[2]))
rows.sort(reverse=True)
print(f"{'op':>5} {'best slot fit':>14} {'rate':>8} {'samples':>9}")
for frac, op, tag, n in rows[:16]:
    print(f" 0x{op:02x} {tag:>14} {100*frac:>7.1f}% {n:>9}")
