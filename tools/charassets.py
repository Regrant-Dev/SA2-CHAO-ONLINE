# For each character loader: texture, model, motion files and AnimInfo table.
import struct, sys, re
d = open(sys.argv[1], 'rb').read()
o = struct.unpack_from('<I', d, 0x3c)[0]; n = struct.unpack_from('<H', d, o+6)[0]; opt = struct.unpack_from('<H', d, o+20)[0]
secs = [struct.unpack_from('<4I', d, o+24+opt+i*40+8) for i in range(n)]
def off(a):
    for vs, va, rs, ra in secs:
        if va <= a-0x400000 < va+rs: return ra + a-0x400000-va
def cstr(a):
    f = off(a); return d[f:d.index(b'\0', f)].decode('latin1') if f else '?'
tva, trs, tra = secs[0][1]+0x400000, secs[0][2], secs[0][3]
code = d[tra:tra+trs]
def calls(target):
    for m in re.finditer(rb'\xe8(....)', code, re.S):
        a = tva + m.start()
        if a + 5 + struct.unpack('<i', m.group(1))[0] == target: yield a
for mdl, mdl_va in [("SONICMDL",0x900968),("TERIOSMDL",0x9009d0),("AMYMDL",0x9009ac),("METALSONICMDL",0x900a1c),
                    ("KNUCKMDL",0x900b1c),("ROUGEMDL",0x900ba8),("TICALMDL",0x900c34),("CHAOS0MDL",0x900c60)]:
    i = code.find(b'\xb8' + struct.pack('<I', mdl_va))
    lo, hi = max(0, i-0x300), i+0x200
    win = code[lo:hi]
    tex = [cstr(struct.unpack_from('<I', win, j+1)[0]) for j in range(len(win)-10)
           if win[j]==0xb9 and win[j+5]==0xe8 and tva+lo+j+5+5+struct.unpack_from('<i',win,j+6)[0]==0x44c510]
    mtn = [cstr(struct.unpack_from('<I', win, j+1)[0]) for j in range(len(win)-10)
           if win[j]==0xb8 and win[j+5]==0xe8 and tva+lo+j+5+5+struct.unpack_from('<i',win,j+6)[0]==0x459740]
    anim = [hex(struct.unpack_from('<I', win, j+6)[0]) for j in range(len(win)-10)
            if win[j]==0xc7 and (win[j+1]&0xc7)==0x80|(win[j+1]&7) and win[j+2:j+6]==b'\x98\x01\x00\x00']
    chars = [win[j+2] for j in range(len(win)-3) if win[j]==0xb0 and win[j+2:j+5]==b'\x88\x46\x01'] # mov al,N ; mov [esi+1],al
    print(f"{mdl:14} tex={tex} mtn={mtn} anim={anim}")
