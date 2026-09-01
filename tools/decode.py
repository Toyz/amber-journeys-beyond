import struct, sys, zlib

def rd(d,be,p,n):
    return struct.unpack_from(('>' if be else '<')+{1:'B',2:'H',4:'I'}[n], d, p)[0]
def i16(d,be,p): return struct.unpack_from(('>' if be else '<')+'h', d, p)[0]
def fcc(d,be,p):
    v=d[p:p+4]; return (v if be else v[::-1]).decode('latin1')

class Movie:
    def __init__(self, path):
        d=self.d=open(path,'rb').read(); be=self.be=d[:4]==b'RIFX'
        mo=rd(d,be,0x18,4); used=rd(d,be,mo+16,4)
        self.res=[]; p=mo+32
        for i in range(used):
            self.res.append((fcc(d,be,p), rd(d,be,p+4,4), rd(d,be,p+8,4))); p+=20
        self.keymap={}
        k=self.data(self.find('KEY*')[0]); be=self.be
        hlen=rd(k,be,0,2); elen=rd(k,be,2,2); used=rd(k,be,8,4)
        for i in range(used):
            p=hlen+i*elen
            self.keymap.setdefault((rd(k,be,p+4,4), fcc(k,be,p+8)),[]).append(rd(k,be,p,4))
        c=self.data(self.find('CAS*')[0])
        self.cas=[rd(c,be,i*4,4) for i in range(len(c)//4)]
    def data(self,i):
        t,sz,off=self.res[i]; return self.d[off+8:off+8+sz]
    def find(self,tag): return [i for i,(t,_,_) in enumerate(self.res) if t==tag]

def unrle(src, want):
    out=bytearray(); p=0; n=len(src)
    while p<n and len(out)<want:
        b=src[p]; p+=1
        if b<0x80:
            cnt=b+1; out+=src[p:p+cnt]; p+=cnt
        else:
            cnt=0x101-b
            if p>=n: break
            out+=bytes([src[p]])*cnt; p+=1
    return bytes(out)

def png(path,w,h,idx,pal):
    raw=b''.join(b'\x00'+bytes(idx[y*w:(y+1)*w]) for y in range(h))
    def chunk(t,d):
        return struct.pack('>I',len(d))+t+d+struct.pack('>I',zlib.crc32(t+d)&0xffffffff)
    hdr=struct.pack('>IIBBBBB',w,h,8,3,0,0,0)
    open(path,'wb').write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',hdr)+chunk(b'PLTE',pal)+chunk(b'IDAT',zlib.compress(raw,6))+chunk(b'IEND',b''))

m=Movie(sys.argv[1]); be=m.be
# grab palette: first CLUT
clutres=m.find('CLUT')
cl=m.data(clutres[0])
pal=bytearray(768)
n=len(cl)//6
for i in range(n):
    r=rd(cl,be,i*6,2)>>8; g=rd(cl,be,i*6+2,2)>>8; b=rd(cl,be,i*6+4,2)>>8
    j=(n-1-i)   # Director stores reversed
    pal[j*3:j*3+3]=bytes([r,g,b])

want_cast=int(sys.argv[2])
resid=m.cas[want_cast-1]
cd=m.data(resid); infoLen=rd(cd,be,4,4); dataLen=rd(cd,be,8,4)
spec=cd[12+infoLen:12+infoLen+dataLen]
pitch=rd(spec,be,0,2)&0x7fff
t,l,b,r=[i16(spec,be,x) for x in (2,4,6,8)]
w,h=r-l,b-t
bitd=m.data(m.keymap[(resid,'BITD')][0])
print(f"cast#{want_cast} res={resid} {w}x{h} pitch={pitch} bitdlen={len(bitd)} expect={pitch*h}")
if len(bitd)>=pitch*h: px=bitd[:pitch*h]
else: px=unrle(bitd, pitch*h)
print(f"  decoded {len(px)} bytes")
rows=bytearray()
for y in range(h): rows+=px[y*pitch:y*pitch+w]
png(sys.argv[3],w,h,rows,bytes(pal))
print("  wrote",sys.argv[3])
