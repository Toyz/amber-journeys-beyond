import struct, sys, os

def rd(d,be,p,n):
    f = {1:'B',2:'H',4:'I'}[n]
    return struct.unpack_from(('>' if be else '<')+f, d, p)[0]

def fcc(d,be,p):
    v=d[p:p+4]
    return (v if be else v[::-1]).decode('latin1')

class Movie:
    def __init__(self, path):
        d = self.d = open(path,'rb').read()
        self.be = be = d[:4]==b'RIFX'
        mmap_off = rd(d,be,0x14+8,4) if False else rd(d,be,0x1c,4)
        # imap: at 0x0c tag,0x10 size,0x14 count,0x18 mmapoff
        mmap_off = rd(d,be,0x18,4)
        assert fcc(d,be,mmap_off)=='mmap', fcc(d,be,mmap_off)
        used = rd(d,be,mmap_off+8+8,4)
        self.res=[]
        p = mmap_off+8+24
        for i in range(used):
            self.res.append((fcc(d,be,p), rd(d,be,p+4,4), rd(d,be,p+8,4)))
            p += 20
    def data(self,i):
        t,sz,off = self.res[i]
        return self.d[off+8:off+8+sz]
    def find(self,tag):
        return [i for i,(t,_,_) in enumerate(self.res) if t==tag]

m = Movie(sys.argv[1])
be = m.be
print("VWCF:", m.data(m.find('VWCF')[0])[:40].hex())
cfg = m.data(m.find('VWCF')[0])
ver = rd(cfg,be,0x24,2); fileVer = rd(cfg,be,0x02,2)
print(f"  stage rect: {[rd(cfg,be,x,2) for x in (4,6,8,10)]}  fileVersion={fileVer} ver={ver}")

# KEY* : maps owner resource -> child resource by fourcc
k = m.data(m.find('KEY*')[0])
hlen=rd(k,be,0,2); elen=rd(k,be,2,2); mx=rd(k,be,4,4); used=rd(k,be,8,4)
print(f"KEY* entries={used}")
keymap={}
for i in range(used):
    p=hlen+i*elen
    child=rd(k,be,p,4); owner=rd(k,be,p+4,4); tag=fcc(k,be,p+8)
    keymap.setdefault((owner,tag),[]).append(child)

# CAS* : array of resource ids indexed by castNum-minCast
c = m.data(m.find('CAS*')[0])
casn=[rd(c,be,i*4,4) for i in range(len(c)//4)]
print(f"CAS* {len(casn)} slots, first 12: {casn[:12]}")

TYPES={1:'bitmap',2:'filmloop',3:'text',4:'palette',5:'picture',6:'sound',7:'button',
       8:'shape',9:'movie',10:'digitalvideo',11:'script',12:'richtext',13:'ole',14:'transition'}
shown=0
for idx,resid in enumerate(casn):
    if resid==0: continue
    cd = m.data(resid)
    ctype = rd(cd,be,0,4)
    if ctype!=1: continue
    infoLen = rd(cd,be,4,4); dataLen = rd(cd,be,8,4)
    spec = cd[12+infoLen:12+infoLen+dataLen]
    # bitmap cast data: pitch(2) rect(8) ... regY regX ... bitdepth
    pitch = rd(spec,be,0,2)&0x7fff
    t,l,b,r = [struct.unpack_from('>h' if be else '<h',spec,x)[0] for x in (2,4,6,8)]
    bd = spec[0x16] if len(spec)>0x16 else 0
    clut = struct.unpack_from('>h' if be else '<h',spec,0x17)[0] if len(spec)>0x18 else 0
    kids = keymap.get((resid,'BITD'),[])
    print(f"  cast#{idx+1:<5} res={resid:<5} {r-l}x{b-t} pitch={pitch} depth={bd} clut={clut} BITD={kids}")
    shown+=1
    if shown>=8: break
