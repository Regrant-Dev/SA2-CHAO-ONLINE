#!/usr/bin/env python3
"""Rebuild an analysable sonic2app.exe from the Steam exe and a code dump.

The Steam exe's .text section is encrypted by Steam's wrapper. Build the mod with
`--features dumpexe`, start the game once, then:

    tools/rebuild_exe.py <Steam sonic2app.exe> <mod folder>/sonic2app.text.bin <out.exe>

The output is the Steam exe with .text replaced by the decrypted bytes (the SA2 Mod Loader's
own code patches are included). Never share it: it is the game's code.
"""
import struct
import sys

TEXT_VA = 0x1000


def main() -> None:
    exe, dump, out = sys.argv[1:4]
    data = bytearray(open(exe, "rb").read())
    text = open(dump, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    count = struct.unpack_from("<H", data, pe + 6)[0]
    table = pe + 24 + struct.unpack_from("<H", data, pe + 20)[0]
    for i in range(count):
        name, _vsize, va, raw_size, raw_at = struct.unpack_from("<8sIIII", data, table + 40 * i)
        if va == TEXT_VA:
            if len(text) != raw_size:
                sys.exit(f"dump is {len(text):#x} bytes, .text is {raw_size:#x}")
            data[raw_at : raw_at + raw_size] = text
            open(out, "wb").write(data)
            print(f"wrote {out}")
            return
    sys.exit("no .text section at 0x1000")


if __name__ == "__main__":
    main()
