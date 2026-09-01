"""Identify what each push opcode indexes, by per-handler bounds.

Every handler declares how many arguments, local variables and literals it
has. An opcode that reads locals can never carry an operand at or beyond that
handler's variable count; one that reads literals can never exceed its literal
count. Those are hard per-handler bounds, so a wrong attribution shows up as a
violation rather than as a low score.

Operands may be indices or byte offsets into a table of entries, so both
readings are scored and the better one reported.
"""
import sys, collections
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

MOVIES = ['extract/BRICE/BRICE.DXR', 'extract/MARGARET/MARGARET.DXR',
          'extract/ROXY/ROXY.DXR', 'extract/EDWIN/EDWIN.DXR',
          'extract/AMBERHUB.DXR']

# For each opcode: how often its operand fits each candidate table.
fits = collections.defaultdict(lambda: collections.Counter())
uses = collections.Counter()

for path in MOVIES:
    d, be, res = load(path)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c:
            continue
        hc = rd(s, be, 0x48, 2); ho = rd(s, be, 0x4a, 4)
        litc = rd(s, be, 0x4e, 2)
        propc = rd(s, be, 0x3c, 2)
        globc = rd(s, be, 0x42, 2)
        for i in range(hc):
            p = ho + i * 42
            if p + 42 > len(s):
                break
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            argc = rd(s, be, p + 12, 2); varc = rd(s, be, p + 18, 2)
            if coff + clen > len(s):
                continue
            instrs, ok = frame(s[coff:coff + clen])
            if not ok:
                continue
            for (_, op, arg, w) in instrs:
                if arg is None:
                    continue
                uses[op] += 1
                c = fits[op]
                # Score each candidate only over the handlers where that table
                # exists. Counting empty-table handlers in the denominator but
                # never in the numerator makes every candidate look wrong.
                def score(tag, count, ok):
                    if count:
                        c[tag + '/n'] += 1
                        if ok:
                            c[tag] += 1
                score('arg-index', argc, arg < argc)
                score('arg-offset2', argc, arg % 2 == 0 and arg // 2 < argc)
                score('var-index', varc, arg < varc)
                score('var-offset2', varc, arg % 2 == 0 and arg // 2 < varc)
                score('literal-index', litc, arg < litc)
                # Literal records are 8 bytes each, so a reference may be a
                # byte offset into that table rather than an index.
                score('literal-offset8', litc, arg % 8 == 0 and arg // 8 < litc)
                score('prop-index', propc, arg < propc)
                score('global-index', globc, arg < globc)

print(f"{'op':>5} {'uses':>7}   best-fitting table (share of uses that fit)")
for op, n in uses.most_common(26):
    c = fits[op]
    ranked = []
    for tag in ('arg-index', 'arg-offset2', 'var-index', 'var-offset2',
                'literal-index', 'literal-offset8', 'prop-index', 'global-index'):
        denom = c.get(tag + '/n', 0)
        if denom >= 20:
            ranked.append((c.get(tag, 0) / denom, tag, denom))
    ranked.sort(reverse=True)
    strong = [f"{t} {100*f:.0f}% of {d}" for f, t, d in ranked if f > 0.995]
    near = [f"{t} {100*f:.0f}%" for f, t, d in ranked[:2] if 0.7 < f <= 0.995]
    print(f" 0x{op:02x} {n:>7}   {', '.join(strong) or '-':<34} {', '.join(near)}")
