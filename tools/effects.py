"""Solve for the remaining opcodes' stack effects, with the known ones anchored.

Each handler must leave the stack balanced, giving 543 equations. Earlier this
was underdetermined; now that pushes, slot access and argument lists are known,
their effects can be fixed and the system solved for what is left. The answer is
then checked by simulating every handler: depth must never go negative and must
end at zero. That simulation is the real test, because a wrong effect for a
common opcode drives the depth negative almost immediately.
"""
import sys, collections
import numpy as np
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

MOVIES = ['extract/BRICE/BRICE.DXR', 'extract/MARGARET/MARGARET.DXR',
          'extract/ROXY/ROXY.DXR', 'extract/EDWIN/EDWIN.DXR',
          'extract/AMBERHUB.DXR']

CALLS = {0x1e, 0x1f, 0x46, 0x56, 0x57, 0x63, 0x66, 0x86, 0x97, 0xa3, 0xa6}
ARGLIST = {0x42, 0x43}
# Effects established by the slot and literal work.
ANCHOR = {0x44: 1, 0x84: 1, 0x4b: 1, 0x4c: 1, 0x52: -1}

handlers = []
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
            if coff + clen > len(s):
                continue
            instrs, ok = frame(s[coff:coff + clen])
            if ok:
                handlers.append([(op, arg) for _, op, arg, _ in instrs])

# An argument list of N pops N and pushes one; a call pops the list and pushes
# a result, so it is neutral. Both are treated as known.
def known_effect(op, arg):
    if op in ANCHOR:
        return ANCHOR[op]
    if op in ARGLIST:
        return 1 - (arg or 0)
    if op in CALLS:
        return 0
    return None

unknown = sorted({op for h in handlers for op, _ in h if known_effect(op, 0) is None})
index = {op: i for i, op in enumerate(unknown)}
print(f"{len(handlers)} handlers, {len(unknown)} opcodes with unknown effect")

A = np.zeros((len(handlers), len(unknown)))
b = np.zeros(len(handlers))
for r, h in enumerate(handlers):
    for op, arg in h:
        e = known_effect(op, arg)
        if e is None:
            A[r, index[op]] += 1
        else:
            b[r] -= e

sol, *_ = np.linalg.lstsq(A, b, rcond=None)
rounded = {op: int(round(sol[i])) for op, i in index.items()}
resid = np.abs(A @ np.array([rounded[op] for op in unknown]) - b)
print(f"handlers balancing with rounded effects: {int((resid < 0.5).sum())}/{len(handlers)}")
print("\nsolved effects (most common first):")
freq = collections.Counter(op for h in handlers for op, _ in h if op in index)
for op, n in freq.most_common(16):
    print(f"  0x{op:02x}  effect {rounded[op]:+d}   {n} uses")
