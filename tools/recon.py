import struct, sys, os, collections

class R:
    def __init__(self, data, be):
        self.d = data; self.p = 0; self.be = be
    def u32(self):
        v = struct.unpack_from('>I' if self.be else '<I', self.d, self.p)[0]; self.p += 4; return v
    def u16(self):
        v = struct.unpack_from('>H' if self.be else '<H', self.d, self.p)[0]; self.p += 2; return v
    def i16(self):
        v = struct.unpack_from('>h' if self.be else '<h', self.d, self.p)[0]; self.p += 2; return v
    def fourcc(self):
        v = self.d[self.p:self.p+4]; self.p += 4
        return v.decode('latin1') if self.be else v[::-1].decode('latin1')

def parse(path):
    data = open(path,'rb').read()
    magic = data[:4]
    be = magic == b'RIFX'
    if magic not in (b'RIFX', b'XFIR'):
        return None
    r = R(data, be); r.fourcc()          # RIFX
    total = r.u32(); codec = r.fourcc()  # MV93
    # imap
    tag = r.fourcc(); sz = r.u32()
    imap_end = r.p + sz
    r.u32()                              # mmap count
    mmap_off = r.u32()
    print(f"{os.path.basename(path)}: endian={'BE' if be else 'LE'} codec={codec} size={len(data)} mmapOff=0x{mmap_off:x}")
    # mmap
    r.p = mmap_off
    assert r.fourcc() == 'mmap'
    r.u32()                              # mmap chunk size
    r.u16()                              # header len (24)
    r.u16()                              # entry len (20)
    r.u32()                              # chunk count max
    used = r.u32()
    r.u32(); r.u32()                     # junk head / free head (2x i32)... layout varies
    entries = []
    # re-seek: mmap header is 24 bytes total after size field
    r.p = mmap_off + 8 + 24
    for i in range(used):
        t = r.fourcc(); csz = r.u32(); coff = r.u32(); flags = r.u16(); unk = r.i16(); link = r.u32()
        entries.append((i, t, csz, coff, flags, link))
    return data, be, entries

for path in sys.argv[1:]:
    res = parse(path)
    if not res:
        print(f"{path}: not RIFX"); continue
    data, be, entries = res
    hist = collections.Counter(t for _,t,_,_,_,_ in entries)
    tot = collections.Counter()
    for _,t,csz,_,_,_ in entries: tot[t] += csz
    print("  chunk type   count      bytes")
    for t,c in hist.most_common():
        print(f"  {t:<10} {c:>6} {tot[t]:>12,}")
    print()
