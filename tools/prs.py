"""SEGA PRS decompression (the game's .PRS files)."""


def decompress(src: bytes) -> bytes:
    out = bytearray()
    i = 0
    ctrl = 0
    bits = 0

    def bit():
        nonlocal i, ctrl, bits
        if bits == 0:
            ctrl = src[i]
            i += 1
            bits = 8
        b = ctrl & 1
        ctrl >>= 1
        bits -= 1
        return b

    while True:
        if bit():
            out.append(src[i])
            i += 1
            continue
        if bit():
            word = src[i] | (src[i + 1] << 8)
            i += 2
            if word == 0:
                break
            offset = (word >> 3) - 0x2000
            size = word & 7
            if size == 0:
                size = src[i] + 1
                i += 1
            else:
                size += 2
        else:
            size = ((bit() << 1) | bit()) + 2
            offset = src[i] - 0x100
            i += 1
        start = len(out) + offset
        for k in range(size):
            out.append(out[start + k])
    return bytes(out)
