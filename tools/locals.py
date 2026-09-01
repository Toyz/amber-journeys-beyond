"""Parse each handler's argument and variable name lists, then test which
opcodes index them.

The handler record carries a count and an offset for arguments and for local
variables. If those offsets point at arrays of name IDs, resolving them gives
real names, which is a check the parse is right before anything is built on it.
"""
import sys, collections
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

d, be, res = load('extract/BRICE/BRICE.DXR')
names = names_of(d, be, res)
lscr = [i for i, r in enumerate(res) if r[0] == 'Lscr']

shown = 0
for si in lscr:
    s = body(d, res, si)
    if len(s) < 0x5c:
        continue
    hc = rd(s, be, 0x48, 2); ho = rd(s, be, 0x4a, 4)
    for i in range(hc):
        p = ho + i * 42
        if p + 42 > len(s):
            break
        nameID = rd(s, be, p, 2)
        clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
        argc = rd(s, be, p + 12, 2); argo = rd(s, be, p + 14, 4)
        varc = rd(s, be, p + 18, 2); varo = rd(s, be, p + 20, 4)
        if argc == 0 and varc == 0:
            continue
        hn = names[nameID] if nameID < len(names) else f'?{nameID}'

        def read_list(off, count):
            out = []
            for k in range(count):
                q = off + k * 2
                if q + 2 > len(s):
                    out.append('<oob>'); continue
                nid = rd(s, be, q, 2)
                out.append(names[nid] if nid < len(names) else f'#{nid}')
            return out

        print(f"  {hn:<28} args={argc}@{argo} {read_list(argo, argc)}")
        print(f"  {'':<28} vars={varc}@{varo} {read_list(varo, varc)}")
        shown += 1
        if shown >= 8:
            sys.exit(0)
