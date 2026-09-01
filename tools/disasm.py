"""Disassemble a handler using the opcodes established so far.

Anything still unidentified prints as its raw opcode, so the gaps are visible
rather than guessed at. If the identified opcodes are right, the result should
read as sensible code; if they are wrong, it will not.
"""
import sys, struct
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

CALLS = {0x1e, 0x1f, 0x46, 0x56, 0x57, 0x63, 0x66, 0x86, 0x97, 0xa3, 0xa6}
ARGLIST = {0x42, 0x43}
LITERAL = {0x44, 0x84}
JUMP = {0x93, 0x95}

def literals_of(s, be):
    litc = rd(s, be, 0x4e, 2); lito = rd(s, be, 0x50, 4); dato = rd(s, be, 0x58, 4)
    out = []
    for i in range(litc):
        p = lito + i * 8
        if p + 8 > len(s):
            break
        kind = rd(s, be, p, 4); off = rd(s, be, p + 4, 4)
        q = dato + off
        if kind == 1 and q + 4 <= len(s):
            n = rd(s, be, q, 4)
            out.append(repr(s[q + 4:q + 4 + n].decode('latin1')))
        elif q + 4 <= len(s):
            out.append(str(struct.unpack('>i' if be else '<i', s[q:q + 4])[0]))
        else:
            out.append('?')
    return out

def disasm(path, want):
    d, be, res = load(path)
    names = names_of(d, be, res)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c:
            continue
        hc = rd(s, be, 0x48, 2); ho = rd(s, be, 0x4a, 4)
        lits = literals_of(s, be)
        for i in range(hc):
            p = ho + i * 42
            if p + 42 > len(s):
                break
            nid = rd(s, be, p, 2)
            hn = names[nid] if nid < len(names) else f'?{nid}'
            if hn != want:
                continue
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            argc = rd(s, be, p + 12, 2); argo = rd(s, be, p + 14, 4)
            varc = rd(s, be, p + 18, 2); varo = rd(s, be, p + 20, 4)
            slot = lambda off, k: (names[rd(s, be, off + k * 2, 2)]
                                   if rd(s, be, off + k * 2, 2) < len(names) else f'slot{k}')
            args = [slot(argo, k) for k in range(argc)]
            vars_ = [slot(varo, k) for k in range(varc)]
            print(f"on {hn} {', '.join(args)}")
            print(f"  -- locals: {', '.join(vars_) or 'none'}")
            print(f"  -- literals: {lits}")
            instrs, _ = frame(s[coff:coff + clen])
            for (o, op, arg, w) in instrs:
                if op in ARGLIST:
                    txt = f"arglist {arg}"
                elif op in CALLS:
                    txt = f"call {names[arg]}" if arg is not None and arg < len(names) else f"call #{arg}"
                elif op in LITERAL:
                    k = arg // 8
                    txt = f"push literal {lits[k] if k < len(lits) else '?'}"
                elif op == 0x4b:
                    k = arg // 8
                    txt = f"push arg {args[k] if k < len(args) else k}"
                elif op == 0x4c:
                    k = arg // 8
                    txt = f"push local {vars_[k] if k < len(vars_) else k}"
                elif op == 0x52:
                    k = arg // 8
                    txt = f"set local {vars_[k] if k < len(vars_) else k}"
                elif op in JUMP:
                    txt = f"jump -> {o + arg}"
                elif op == 0x41:
                    txt = f"push int {arg}"
                elif arg is not None:
                    txt = f"op{op:02x} {arg}"
                else:
                    txt = f"op{op:02x}"
                print(f"  {o:>4}  {txt}")
            return
    print(f"no handler named {want}")

disasm(sys.argv[1] if len(sys.argv) > 1 else 'extract/BRICE/BRICE.DXR',
       sys.argv[2] if len(sys.argv) > 2 else 'setGrateIsOpen')
