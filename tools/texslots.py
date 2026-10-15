# Find character loaders: MOV ECX,tex; CALL 0x44C510; MOV [reg+disp],EAX; MOV EAX,mdl; CALL 0x459590
import struct, sys, re
d = open(sys.argv[1], 'rb').read()
o = struct.unpack_from('<I', d, 0x3c)[0]
n = struct.unpack_from('<H', d, o + 6)[0]; opt = struct.unpack_from('<H', d, o + 20)[0]
secs = []
for i in range(n):
    s = o + 24 + opt + i * 40
    vs, va, rs, ra = struct.unpack_from('<4I', d, s + 8)
    secs.append((va + 0x400000, rs, ra))
def va2off(a):
    for va, rs, ra in secs:
        if va <= a < va + rs: return ra + a - va
def cstr(a):
    off = va2off(a); return d[off:d.index(b'\0', off)].decode('latin1') if off else '?'
text_va, text_rs, text_ra = secs[0]
code = d[text_ra:text_ra + text_rs]
regs = {0x86: 'esi', 0x87: 'edi', 0x83: 'ebx', 0x85: 'ebp'}
for m in re.finditer(rb'\xb9(....)\xe8(....)\x89([\x83\x85\x86\x87])(....)\xb8(....)\xe8', code, re.S):
    at = text_va + m.start()
    call = at + 5 + 5 + struct.unpack('<i', m.group(2))[0]
    if call != 0x44C510: continue
    load = at + 5 + 5 + 6 + 5 + 5 + struct.unpack('<i', code[m.end():m.end()+4])[0]
    disp = struct.unpack('<I', m.group(4))[0]
    print(f'{at:#x} tex={cstr(struct.unpack("<I", m.group(1))[0]):14} slot=[{regs[m.group(3)[0]]}+{disp:#x}] mdl={cstr(struct.unpack("<I", m.group(5))[0]):18} load={load:#x}')
