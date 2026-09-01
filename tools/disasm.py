"""Disassemble a handler using the opcodes established so far.

Anything still unidentified prints as its raw opcode, so the gaps are visible
rather than guessed at. If the identified opcodes are right, the result should
read as sensible code; if they are wrong, it will not.
"""
import sys, struct
_argv = list(sys.argv)          # the shim below clobbers argv
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

CALLS = {0x1e, 0x1f, 0x46, 0x56, 0x57, 0x63, 0x66, 0x86, 0x97, 0xa3, 0xa6}
# Globals are named program-wide rather than per script, so these index the
# movie's name table directly. Confirmed against the naming convention: their
# operands land on o- and g-prefixed names 93% of the time against a 5%
# background rate.
GLOBAL_GET = {0x49, 0x89}
GLOBAL_SET = {0x4f}
# Symbol and property names, from the movie's name table.
#
# 0x81 used to be in here, which was wrong and quietly poisoned every handler
# that mentions a number over 127. Opcodes pair as (1-byte operand, 2-byte
# operand) on the low six bits: 0x45/0x85 is the symbol push, and 0x41/0x81 is
# the integer push. Reading 0x81 as a name turned `if gRadioDial < 240` into
# `if gRadioDial < #snd5`.
#
# The counts settle it. Of 630 0x81 operands across the four chapters, 221 are
# larger than the name table has entries, and the commonest values are 1000,
# 255, 500, 300, 200, 256 and 1024. Of 2302 0x85 operands, exactly one is out
# of range.
NAMED = {0x85, 0x45}
# Comparisons feed a conditional jump 73-99% of the time; 0x12 is almost never
# preceded by a push, taking two comparison results instead, which is what a
# logical combinator looks like. Arithmetic is preceded by pushes and followed
# by a store. The exact operator each one is has not been pinned down, so they
# print by role rather than by a guessed symbol.
# Established individually, each by a context where the meaning is forced:
#   0x0c  a clamp's floor      if x < 1 then x = 1
#   0x0d  a counted loop bound body runs while i <= count
#   0x0e  the same gCPU test appears with both 0x0f and 0x0e, and a platform
#         symbol admits only = and <>
#   0x0f  the door setter's mirrored branches
#   0x10  a clamp's ceiling     if x > 6 then x = 6
#   0x11  fits the ordering below; only 15 uses and not independently forced
COMPARE = {0x0c: '<', 0x0d: '<=', 0x0e: '<>', 0x0f: '=', 0x10: '>', 0x11: '>='}
LOGICAL = {0x12: 'and/or'}
ARITH = {0x04: 'arith-a', 0x05: 'add', 0x06: 'arith-c', 0x0a: 'arith-d'}
LOOPBACK = {0x54}
# A sprite write is `push channel; push value; push property; 0x5d 6`.
# The channel operand carries exactly the channels puppetSprite claims, and
# the properties are named by what is written to them: 4 is fed by cast
# lookups, 33 only ever by point().
SPRITE_PROP = {4: 'castNum', 33: 'loc', 13: 'ink', 15: 'flag15',
               16: 'flag16', 25: 'visible', 14: 'prop14'}
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
            # The length is followed by the bytes, and Director stores a
            # trailing NUL inside the count. Printing only the length was
            # enough to see that a handler builds a symbol out of pieces, and
            # not nearly enough to see which symbol.
            n = rd(s, be, q, 4)
            text = s[q + 4:q + 4 + n].split(b'\0')[0].decode('latin1')
            out.append(repr(text))
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
                elif op in GLOBAL_GET:
                    txt = f"push global {names[arg]}" if arg < len(names) else f"push global #{arg}"
                elif op in GLOBAL_SET:
                    txt = f"set global {names[arg]}" if arg < len(names) else f"set global #{arg}"
                elif op in NAMED:
                    txt = f"push #{names[arg]}" if arg < len(names) else f"push #{arg}"
                elif op in COMPARE:
                    txt = f"compare {COMPARE[op]}"
                elif op in LOGICAL:
                    txt = f"logical ({LOGICAL[op]})"
                elif op in ARITH:
                    txt = f"arith ({ARITH[op]})"
                elif op == 0x5d and arg == 6:
                    txt = "set sprite property"
                elif op in LOOPBACK:
                    txt = f"loop back -> {o - arg}" if arg is not None else "loop back"
                elif op == 0x01:
                    txt = "return"
                elif op in (0x41, 0x81):
                    txt = f"push int {arg}"
                elif arg is not None:
                    txt = f"op{op:02x} {arg}"
                else:
                    txt = f"op{op:02x}"
                print(f"  {o:>4}  {txt}")
            return
    print(f"no handler named {want}")

disasm(_argv[1] if len(_argv) > 1 else 'extract/BRICE/BRICE.DXR',
       _argv[2] if len(_argv) > 2 else 'setGrateIsOpen')
