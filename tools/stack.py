"""Solve for each opcode's stack effect from the constraint that handlers balance.

Every handler must leave the stack as it found it. That gives one equation per
handler: the sum of its instructions' stack effects is zero. With 543 handlers
and far fewer distinct opcodes the system is heavily over-determined, so a wrong
effect for any common opcode cannot satisfy it. This is the same shape of test
as the jump-containment one: a hard constraint with hundreds of chances to fail,
rather than a plausibility score.
"""
import sys, collections
import numpy as np

sys.argv = ['dis', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

MOVIES = ['extract/BRICE/BRICE.DXR', 'extract/MARGARET/MARGARET.DXR',
          'extract/ROXY/ROXY.DXR', 'extract/EDWIN/EDWIN.DXR',
          'extract/AMBERHUB.DXR']

handlers = []          # each: (name, argc, varc, [(op, arg)...])
for path in MOVIES:
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
            nameID = rd(s, be, p, 2)
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            argc = rd(s, be, p + 12, 2); varc = rd(s, be, p + 18, 2)
            if coff + clen > len(s):
                continue
            instrs, ok = frame(s[coff:coff + clen])
            if not ok:
                continue
            nm = names[nameID] if nameID < len(names) else f'?{nameID}'
            handlers.append((nm, argc, varc, [(op, arg) for _, op, arg, _ in instrs]))

print(f"{len(handlers)} handlers collected")

# Opcodes whose pop count is their operand rather than a constant: a call taking
# N arguments. Model those separately so the solve stays linear.
ops = sorted({op for _, _, _, ins in handlers for op, _ in ins})
index = {op: i for i, op in enumerate(ops)}
print(f"{len(ops)} distinct opcodes")

# Opcodes whose operand is small everywhere are candidates for taking their
# pop count from the operand: a call of N arguments pops N and pushes one.
maxarg = collections.defaultdict(int)
hasarg = set()
for _, _, _, ins in handlers:
    for op, arg in ins:
        if arg is not None:
            hasarg.add(op)
            maxarg[op] = max(maxarg[op], arg)
variadic = sorted(op for op in hasarg if maxarg[op] <= 16)
print(f"variadic candidates (operand <= 16): {[hex(o) for o in variadic]}")

vindex = {op: len(ops) + i for i, op in enumerate(variadic)}
cols = len(ops) + len(variadic)

A = np.zeros((len(handlers), cols))
for r, (_, _, _, ins) in enumerate(handlers):
    for op, arg in ins:
        A[r, index[op]] += 1
        if op in vindex and arg is not None:
            A[r, vindex[op]] += arg

def report(M, label):
    sv = np.linalg.svd(M, compute_uv=False)
    tol = max(M.shape) * sv[0] * np.finfo(float).eps
    rank = int((sv >= tol).sum())
    print(f"{label}: rank {rank} of {M.shape[1]}, null space {M.shape[1] - rank}")
    return rank

report(A, "with variadic terms")

# Inspect the null space: a consistent model should contain a solution whose
# entries are small integers, since stack effects are counts.
u, sv, vt = np.linalg.svd(A)
tol = max(A.shape) * sv[0] * np.finfo(float).eps
null = vt[(sv < tol).sum() and -( (sv < tol).sum() + (A.shape[1]-len(sv)) ) or -1:]
print(f"null vectors: {null.shape[0]}")
for k, vec in enumerate(null[:4]):
    scaled = vec / (np.abs(vec[np.abs(vec) > 1e-9]).min())
    entries = {hex(ops[i]) if i < len(ops) else hex(variadic[i-len(ops)])+"*arg": round(scaled[i], 2)
               for i in range(cols) if abs(scaled[i]) > 0.01}
    print(f"  null[{k}] nonzero: {entries}")
