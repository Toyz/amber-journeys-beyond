#!/usr/bin/env python3
"""Read files out of an HFS (not HFS+) volume image.

The Macintosh release of Amber ships as two bare HFS volumes -- no partition
map, no ISO9660 -- and nothing on this machine could read them. This is the
smallest thing that can: parse the master directory block, walk the catalogue
B-tree, and pull a file's data fork out through its extents.

Usage:
    hfs.py <image> list
    hfs.py <image> cat <path> <outfile>
    hfs.py <image> extract <outdir> [prefix]
"""
import struct
import sys
import os

def u8(d, p):  return d[p]
def u16(d, p): return struct.unpack_from(">H", d, p)[0]
def u32(d, p): return struct.unpack_from(">I", d, p)[0]

class HFS:
    def __init__(self, path):
        self.f = open(path, "rb")
        mdb = self.read_at(1024, 512)
        if u16(mdb, 0) != 0x4244:
            raise SystemExit("not an HFS volume (no BD signature at 1024)")
        self.alloc_size = u32(mdb, 20)
        self.alloc_start = u16(mdb, 28) * 512
        self.name = mdb[37:37 + mdb[36]].decode("mac-roman")
        # The catalogue and the extents-overflow file are themselves files,
        # described by three extents each right here in the MDB.
        self.xt_extents = self.extents(mdb, 134)
        self.ct_extents = self.extents(mdb, 150)
        self.xt_size = u32(mdb, 130)
        self.ct_size = u32(mdb, 146)
        self.overflow = None            # filled in lazily, and only if needed
        self.catalog = self.read_forks(self.ct_extents, self.ct_size)

    def read_at(self, off, n):
        self.f.seek(off)
        return self.f.read(n)

    def extents(self, d, p):
        return [(u16(d, p + i * 4), u16(d, p + i * 4 + 2)) for i in range(3)]

    def block(self, n, count=1):
        return self.read_at(self.alloc_start + n * self.alloc_size,
                            count * self.alloc_size)

    def read_forks(self, extents, size):
        out = bytearray()
        for start, count in extents:
            if count == 0:
                continue
            out += self.block(start, count)
            if len(out) >= size:
                break
        return bytes(out[:size]) if size else bytes(out)

    # -- B-tree ----------------------------------------------------------
    def nodes(self, tree):
        size = u16(tree, 32)            # nodeSize, in the header record
        return size, len(tree) // size

    def leaves(self, tree):
        """Every leaf node, followed from the header's first-leaf pointer."""
        node_size = u16(tree, 32)
        node = u32(tree, 24)            # firstLeafNode
        while node:
            off = node * node_size
            d = tree[off:off + node_size]
            if len(d) < 14:
                break
            yield d
            node = u32(d, 0)            # fLink

    def records(self, node):
        count = u16(node, 10)
        size = len(node)
        for i in range(count):
            start = u16(node, size - 2 * (i + 1))
            end = u16(node, size - 2 * (i + 2))
            yield node[start:end]

    # -- catalogue -------------------------------------------------------
    def walk(self):
        """Yield (parent_id, name, kind, record) for every catalogue entry."""
        for node in self.leaves(self.catalog):
            if node[8] != 0xFF:         # kind: leaf
                continue
            for rec in self.records(node):
                if not rec:
                    continue
                key_len = rec[0]
                parent = u32(rec, 2)
                name_len = rec[6]
                name = rec[7:7 + name_len].decode("mac-roman", "replace")
                data = rec[1 + key_len + (1 - (key_len & 1)):]
                if not data:
                    continue
                yield parent, name, data[0], data

    def tree(self):
        """Full paths for every file, and their catalogue records."""
        dirs, files = {2: ""}, []
        pending = []
        for parent, name, kind, rec in self.walk():
            if kind == 1:
                dirs[u32(rec, 6)] = name
                pending.append((parent, u32(rec, 6)))
            elif kind == 2:
                files.append((parent, name, rec))
        # Resolve each directory's parent chain into a path.
        up = {child: parent for parent, child in pending}
        def path_of(cnid):
            parts = []
            while cnid in dirs and cnid != 2:
                parts.append(dirs[cnid])
                cnid = up.get(cnid, 2)
            return "/".join(reversed(parts))
        return [(f"{path_of(p)}/{n}".lstrip("/"), rec) for p, n, rec in files]

    # -- data forks ------------------------------------------------------
    def overflow_extents(self, cnid, want_from):
        """Extents past the first three, from the extents-overflow file."""
        if self.overflow is None:
            self.overflow = self.read_forks(self.xt_extents, self.xt_size)
        found = []
        for node in self.leaves(self.overflow):
            if node[8] != 0xFF:
                continue
            for rec in self.records(node):
                if len(rec) < 12 or rec[1] != 0:      # data fork only
                    continue
                if u32(rec, 2) != cnid:
                    continue
                if u16(rec, 6) != want_from:
                    continue
                found = self.extents(rec, 8)
                return found
        return found

    def data_fork(self, rec):
        cnid = u32(rec, 20)
        size = u32(rec, 26)
        extents = self.extents(rec, 74)
        out = bytearray()
        blocks = 0
        while len(out) < size:
            for start, count in extents:
                if count == 0:
                    continue
                out += self.block(start, count)
                blocks += count
            if len(out) >= size:
                break
            more = self.overflow_extents(cnid, blocks)
            if not more or all(c == 0 for _, c in more):
                break
            extents = more
        return bytes(out[:size])

def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    vol = HFS(sys.argv[1])
    cmd = sys.argv[2]
    entries = vol.tree()
    if cmd == "list":
        print(f"volume {vol.name}: {len(entries)} files, "
              f"{vol.alloc_size}-byte blocks")
        for path, rec in sorted(entries):
            print(f"{u32(rec, 26):>10}  {path}")
    elif cmd == "cat":
        want = sys.argv[3].lower()
        for path, rec in entries:
            if path.lower() == want or path.lower().endswith("/" + want):
                open(sys.argv[4], "wb").write(vol.data_fork(rec))
                print(f"wrote {sys.argv[4]} ({u32(rec, 26)} bytes) from {path}")
                return
        raise SystemExit(f"no such file: {sys.argv[3]}")
    elif cmd == "extract":
        outdir = sys.argv[3]
        prefix = sys.argv[4].lower() if len(sys.argv) > 4 else ""
        for path, rec in sorted(entries):
            if prefix and not path.lower().startswith(prefix):
                continue
            dest = os.path.join(outdir, path)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            open(dest, "wb").write(vol.data_fork(rec))
        print(f"extracted to {outdir}")

if __name__ == "__main__":
    main()
