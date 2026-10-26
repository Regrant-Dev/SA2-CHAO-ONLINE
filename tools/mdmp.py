import struct, sys
d = open(sys.argv[1], 'rb').read()
assert d[:4] == b'MDMP'
n, rva = struct.unpack_from('<II', d, 8)
streams = {}
for i in range(n):
    t, size, r = struct.unpack_from('<III', d, rva + i * 12)
    streams[t] = (size, r)
def s16(r):
    ln = struct.unpack_from('<I', d, r)[0]
    return d[r + 4:r + 4 + ln].decode('utf-16le')
mods = []
if 4 in streams:
    _, r = streams[4]
    cnt = struct.unpack_from('<I', d, r)[0]
    for i in range(cnt):
        m = r + 4 + i * 108
        base, size = struct.unpack_from('<QI', d, m)
        name = s16(struct.unpack_from('<I', d, m + 20)[0])
        mods.append((base, size, name.split('\\')[-1]))
def where(a):
    for b, s, nm in mods:
        if b <= a < b + s:
            return f'{nm}+{a - b:#x}'
    return '?'
_, r = streams[6]
tid = struct.unpack_from('<I', d, r)[0]
code, flags, _, addr = struct.unpack_from('<IIQQ', d, r + 8)
nparams = struct.unpack_from('<I', d, r + 32)[0]
params = struct.unpack_from('<2Q', d, r + 40)
csize, crva = struct.unpack_from('<II', d, r + 8 + 152)
c = crva
regs = dict(zip(['edi','esi','ebx','edx','ecx','eax','ebp','eip'], struct.unpack_from('<8I', d, c + 0x9C)))
regs['esp'] = struct.unpack_from('<I', d, c + 0xC4)[0]
print(f'exception {code:#x} at {addr:#x} ({where(addr)}) params {[hex(p) for p in params[:nparams]]}')
print(' '.join(f'{k}={v:#x}' for k, v in regs.items()))
for k in ['eax','ebx','ecx','edx','esi','edi']:
    w = where(regs[k])
    if w != '?': print(f'  {k} -> {w}')
# memory ranges (stream 5 MemoryList, 9 Memory64List)
mem = []
if 5 in streams:
    _, r = streams[5]
    cnt = struct.unpack_from('<I', d, r)[0]
    for i in range(cnt):
        start, size, mr = struct.unpack_from('<QII', d, r + 4 + i * 16)
        mem.append((start, size, mr))
def readmem(a, ln):
    for s, z, mr in mem:
        if s <= a and a + ln <= s + z:
            return d[mr + a - s: mr + a - s + ln]
print('modules:')
for b, s, nm in mods:
    if nm.lower() in ('sonic2app.exe', 'sa2-chao-online.dll', 'sa2modloader.dll', 'data_dll.dll'):
        print(f'  {b:#010x} {s:#x} {nm}')
st = readmem(regs['esp'], 0x400)
if st:
    print('stack candidates (return addresses into code):')
    for i in range(0, len(st), 4):
        v = struct.unpack_from('<I', st, i)[0]
        w = where(v)
        if w != '?' and ('sonic2app' in w.lower() or 'chao-online' in w.lower() or 'modloader' in w.lower()):
            print(f'  esp+{i:#05x}: {v:#x} {w}')
