import struct,sys
def rd(d,be,p,n): return struct.unpack_from(('>' if be else '<')+{2:'H',4:'I'}[n],d,p)[0]
def fcc(d,be,p):
    v=d[p:p+4]; return (v if be else v[::-1]).decode('latin1')
path,needle=sys.argv[1],sys.argv[2].encode()
d=open(path,'rb').read(); be=d[:4]==b'RIFX'
mo=rd(d,be,0x18,4); used=rd(d,be,mo+16,4)
res=[]; p=mo+32
for i in range(used):
    res.append((fcc(d,be,p),rd(d,be,p+4,4),rd(d,be,p+8,4))); p+=20
# find all occurrences of needle
hits=[]; start=0
while True:
    i=d.find(needle,start)
    if i<0: break
    hits.append(i); start=i+1
print(f"{len(hits)} occurrences of {needle!r}")
for h in hits:
    owner=None
    for idx,(t,sz,off) in enumerate(res):
        if off <= h < off+8+sz: owner=(idx,t,sz,off); break
    print(f"  @0x{h:x} -> chunk {owner[1]!r} idx={owner[0]} size={owner[2]} off=0x{owner[3]:x}  ctx={d[h-40:h+40]!r}")
