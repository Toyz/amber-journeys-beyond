"""Match the handlers the engine still lacks against the compiled ones.

The engine records every call it cannot perform as Effect::Native. This lists
those names against the handlers actually present in the movies, with their
size, so the remaining work can be ordered by effort rather than guessed at.
"""
import sys, subprocess, re, collections
sys.argv = ['x', 'extract/BRICE/BRICE.DXR']
sys.path.insert(0, 'tools')
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    from lingodis import load, body, names_of, frame, rd

MOVIES = ['extract/ROXY/ROXY.DXR', 'extract/MARGARET/MARGARET.DXR',
          'extract/EDWIN/EDWIN.DXR', 'extract/BRICE/BRICE.DXR',
          'extract/AMBERHUB.DXR']

compiled = {}   # lowercase name -> (instructions, args, movie)
for path in MOVIES:
    d, be, res = load(path)
    names = names_of(d, be, res)
    for si in [i for i, r in enumerate(res) if r[0] == 'Lscr']:
        s = body(d, res, si)
        if len(s) < 0x5c: continue
        hc = rd(s, be, 0x48, 2); ho = rd(s, be, 0x4a, 4)
        for i in range(hc):
            p = ho + i * 42
            if p + 42 > len(s): break
            nid = rd(s, be, p, 2)
            clen = rd(s, be, p + 4, 4); coff = rd(s, be, p + 8, 4)
            argc = rd(s, be, p + 12, 2)
            if coff + clen > len(s): continue
            ins, ok = frame(s[coff:coff + clen])
            if not ok: continue
            nm = names[nid] if nid < len(names) else None
            if nm:
                key = nm.lower()
                if key not in compiled or len(ins) > compiled[key][0]:
                    compiled[key] = (len(ins), argc, path.split('/')[-1], nm)

# The names the engine still cannot perform, taken from its own report.
import os
env = dict(os.environ, AMBER_LIST_NATIVE='1')
out = subprocess.run(['./target/release/amber', 'verify', 'extract'],
                     capture_output=True, text=True, env=env).stdout
uses = {}
for m in re.finditer(r'^  native (\S+) (\d+)$', out, re.M):
    uses[m.group(1).lower()] = int(m.group(2))
wanted = sorted(uses)

found = [(compiled[w][0], compiled[w][3], compiled[w][1], compiled[w][2], uses[w])
         for w in wanted if w in compiled]
missing = [w for w in wanted if w not in compiled]

print(f"\nengine reports {len(wanted)} distinct native handlers")
print(f"  {len(found)} have compiled bodies, {len(missing)} do not\n")
found.sort()
print(f"{'instrs':>7} {'args':>5} {'sites':>6}  handler                     movie")
for n, nm, argc, mv, u in found:
    print(f"{n:>7} {argc:>5} {u:>6}  {nm:<27} {mv}")
if missing:
    print(f"\nno compiled body found for: {', '.join(missing[:12])}")
