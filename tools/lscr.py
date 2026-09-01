import struct, sys
def rd(d,be,p,n): return struct.unpack_from(('>' if be else '<')+{1:'B',2:'H',4:'I'}[n],d,p)[0]
def fcc(d,be,p):
    v=d[p:p+4]; return (v if be else v[::-1]).decode('latin1')

path=sys.argv[1]
d=open(path,'rb').read(); be=d[:4]==b'RIFX'
mo=rd(d,be,0x18,4); used=rd(d,be,mo+16,4)
res=[]; p=mo+32
for i in range(used):
    res.append((fcc(d,be,p),rd(d,be,p+4,4),rd(d,be,p+8,4))); p+=20
def body(i):
    t,sz,off=res[i]; return d[off+8:off+8+sz]

lnam=[i for i,r in enumerate(res) if r[0]=='Lnam']
lscr=[i for i,r in enumerate(res) if r[0]=='Lscr']
print("Lnam chunks:",len(lnam)," Lscr chunks:",len(lscr))

# Lnam
n=body(lnam[0])
print("\nLnam first 32 bytes:", n[:32].hex())
hdrlen=rd(n,be,16,2); count=rd(n,be,18,2)
print(f"  headerLen={hdrlen} count={count}")
names=[]; p=hdrlen
for i in range(count):
    if p>=len(n): break
    l=n[p]; names.append(n[p+1:p+1+l].decode('latin1')); p+=1+l
print(f"  parsed {len(names)} names; first 24: {names[:24]}")

# Lscr
s=body(lscr[0])
print(f"\nLscr[0] size={len(s)}")
for o in range(0,min(len(s),0x70),16):
    row=s[o:o+16]
    print('  %04x  %-47s  %s'%(o,' '.join('%02x'%b for b in row),''.join(chr(b) if 32<=b<127 else '.' for b in row)))

print("\n=== header field hypothesis ===")
f = {
 'propertiesCount': rd(s,be,0x3c,2), 'propertiesOffset': rd(s,be,0x3e,4),
 'globalsCount': rd(s,be,0x42,2),    'globalsOffset': rd(s,be,0x44,4),
 'handlersCount': rd(s,be,0x48,2),   'handlersOffset': rd(s,be,0x4a,4),
 'literalsCount': rd(s,be,0x4e,2),   'literalsOffset': rd(s,be,0x50,4),
 'literalsDataCount': rd(s,be,0x54,4), 'literalsDataOffset': rd(s,be,0x58,4),
}
for k,v in f.items(): print(f"  {k:<20} {v}")
print(f"  literalsDataOffset+Count = {f['literalsDataOffset']+f['literalsDataCount']} (size {len(s)})")

print("\n=== handler records at offset %d ===" % f['handlersOffset'])
ho=f['handlersOffset']
for i in range(f['handlersCount']):
    p=ho+i*42
    if p+42>len(s): break
    nameID=rd(s,be,p,2); vecPos=rd(s,be,p+2,2)
    clen=rd(s,be,p+4,4); coff=rd(s,be,p+8,4)
    argc=rd(s,be,p+12,2); argoff=rd(s,be,p+14,4)
    varc=rd(s,be,p+18,2); varoff=rd(s,be,p+20,4)
    nm = names[nameID] if nameID<len(names) else '?%d'%nameID
    print(f"  [{i}] name={nm!r:<26} args={argc} vars={varc} code@{coff} len={clen}")
    if i==0 and coff<len(s):
        print("      bytecode:", s[coff:coff+min(clen,40)].hex())
