"""Probe Director bytecode framing.

The claim to test: opcodes below 0x40 take no operand, 0x40-0x7f take one
byte, and 0x80 and above take two. If that is right, decoding a handler must
land exactly on its declared length, and every jump must land on an
instruction boundary. Both are structural checks that a wrong table fails.
"""
import struct, sys

def rd(d,be,p,n): return struct.unpack_from(('>' if be else '<')+{1:'B',2:'H',4:'I'}[n],d,p)[0]
def fcc(d,be,p):
    v=d[p:p+4]; return (v if be else v[::-1]).decode('latin1')

def load(path):
    d=open(path,'rb').read(); be=d[:4]==b'RIFX'
    mo=rd(d,be,0x18,4); used=rd(d,be,mo+16,4)
    res=[]; p=mo+32
    for i in range(used):
        res.append((fcc(d,be,p),rd(d,be,p+4,4),rd(d,be,p+8,4))); p+=20
    return d,be,res

def body(d,res,i):
    t,sz,off=res[i]; return d[off+8:off+8+sz]

def names_of(d,be,res):
    # A movie can carry several Lnam chunks, some of them empty stubs left in
    # the resource map; the real table is the largest.
    cands=[k for k,r in enumerate(res) if r[0]=='Lnam']
    cands.sort(key=lambda k: res[k][1], reverse=True)
    n=body(d,res,cands[0])
    if len(n) < 20: return []
    hdr=rd(n,be,16,2); cnt=rd(n,be,18,2)
    out=[]; p=hdr
    for _ in range(cnt):
        if p>=len(n): break
        l=n[p]; out.append(n[p+1:p+1+l].decode('latin1')); p+=1+l
    return out

def frame(code):
    """Split bytecode into (offset, opcode, operand) using the width rule."""
    out=[]; p=0
    while p < len(code):
        op=code[p]
        if op < 0x40: width=0
        elif op < 0x80: width=1
        else: width=2
        if p+1+width > len(code):
            return out, False   # ran off the end: framing wrong
        arg=None
        if width==1: arg=code[p+1]
        elif width==2: arg=struct.unpack_from('>H',code,p+1)[0]
        out.append((p,op,arg,width))
        p += 1+width
    return out, p == len(code)

path=sys.argv[1]
d,be,res=load(path)
names=names_of(d,be,res)
lscr=[i for i,r in enumerate(res) if r[0]=='Lscr']

total=exact=0
jump_ok=jump_bad=0
for si in lscr:
    s=body(d,res,si)
    if len(s) < 0x5c: continue
    hcount=rd(s,be,0x48,2); hoff=rd(s,be,0x4a,4)
    for i in range(hcount):
        p=hoff+i*42
        if p+42>len(s): break
        nameID=rd(s,be,p,2); clen=rd(s,be,p+4,4); coff=rd(s,be,p+8,4)
        if coff+clen > len(s): continue
        code=s[coff:coff+clen]
        instrs, ok = frame(code)
        total+=1; exact += ok
        # jump targets must land on instruction boundaries
        starts={o for o,_,_,_ in instrs}
        for (o,op,arg,w) in instrs:
            if op in (0x53,0x54,0x55,0x93,0x94,0x95) and arg is not None:
                tgt = o+arg if op in (0x53,0x55,0x93,0x95) else o-arg
                if tgt in starts or tgt==len(code): jump_ok+=1
                else: jump_bad+=1
print(f"{path.split('/')[-1]}: {total} handlers, {exact} frame exactly ({100*exact//max(total,1)}%)")
print(f"  jump targets: {jump_ok} aligned, {jump_bad} misaligned")
