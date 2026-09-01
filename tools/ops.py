"""Classify opcodes by what their operands index into.

Rather than assume an opcode table, this looks at where each opcode's operands
land. An opcode whose operands are always valid name indices is name-based; one
whose operands always land on an instruction boundary is a jump; one whose
operands are always small is probably an immediate.
"""
import struct, sys, collections
sys.path.insert(0, 'tools')
from dis import load, body, names_of, frame, rd

paths = sys.argv[1:]
use = collections.Counter()
operands = collections.defaultdict(list)
handler_names = set()

for path in paths:
    d, be, res = load(path)
    names = names_of(d, be, res)
    handler_names.update(names)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c:
            continue
        hcount = rd(s, be, 0x48, 2); hoff = rd(s, be, 0x4a, 4)
        litc = rd(s, be, 0x4e, 2)
        for i in range(hcount):
            p = hoff + i * 42
            if p + 42 > len(s):
                break
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            argc = rd(s, be, p + 12, 2); varc = rd(s, be, p + 18, 2)
            if coff + clen > len(s):
                continue
            instrs, ok = frame(s[coff:coff + clen])
            if not ok:
                continue
            starts = {o for o, _, _, _ in instrs}
            for (o, op, arg, w) in instrs:
                use[op] += 1
                if arg is not None:
                    operands[op].append((arg, len(names), litc, argc, varc,
                                         (o + arg) in starts or (o + arg) == clen,
                                         (o - arg) in starts))

print(f"{len(handler_names)} distinct names, {sum(use.values())} instructions\n")
print(f"{'op':>5} {'count':>7}  {'max arg':>8}  classification")
for op, n in use.most_common(40):
    ops = operands.get(op, [])
    if not ops:
        print(f" 0x{op:02x} {n:>7}  {'-':>8}  no operand")
        continue
    args = [a for a, *_ in ops]
    in_names = sum(1 for a, nn, *_ in ops if a < nn)
    in_lits  = sum(1 for a, _, lc, *_ in ops if a < lc)
    fwd      = sum(1 for *_, f, b in ops if f)
    back     = sum(1 for *_, f, b in ops if b)
    tags = []
    if fwd == len(ops):  tags.append("jump-fwd")
    if back == len(ops): tags.append("jump-back")
    if in_names == len(ops): tags.append("name-index")
    if in_lits == len(ops):  tags.append("literal-index")
    if max(args) < 16: tags.append("small-immediate")
    print(f" 0x{op:02x} {n:>7}  {max(args):>8}  {', '.join(tags) or 'unclassified'}")
