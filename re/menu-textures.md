# Menu texture fallback recovery

Observed on build `4e30ddb`, main menu idle/focus return, 2026-10-19: native GVM parser
`42FC30` crashed at `42FCE1` reading the next chunk length from an invalid address.
The active native filename was `stageMap.PRS`. EBP (recognized archive header) and the
texture-list count were both zero. This is evidence of invalid archive input, not proof
of corrupt files or a particular device-reset/Mod Loader bug. Both original disk PRS
files decoded correctly. Private evidence remains under ignored `re/bin/`.

## Retail evidence, REA VM / Ghidra

`44C620` is the native PRS fallback, already wrapped by Mod Loader. After normal PAK,
PVMX and texture-folder resolution fails, the original body reads a compressed file:

- `44C699 CALL 48F980`: input ECX, stack destination `1DEFE20`, unbounded PRS decode.
  At callee entry, destination is `[ESP+4]`, filename buffer starts at `ESP+20h`.
- `44C6B3 CALL 42FC30`: EAX archive `1DEFE20`, EBX NJS_TEXLIST. At callee entry the
  filename buffer starts at `ESP+14h`. Caller frees compressed data before this call.
- The caller ignores the parser's result and returns success. Fixed-list load `44C350`
  reaches it at `44C3B6`; dynamically allocated texture-list load `44C510` at `44C56F`.

`42FC30` walks chunk lengths without bounds checks. It byte-swaps GVM/PVM header flags
and texture count in place, then calls `42FBC0` (header/texture metadata) and `430A20`
(texture creation), writing each returned texture into list entries at `+8` with stride
12. It finishes after the count in the header. Corrupt input can walk arbitrary memory.
Separate REA checks of `42FBC0`/`430A20` confirm their inputs come from the supplied
header/image pointers; they do not require the archive to reside in global scratch.

## Recovery

`src/menu_textures.rs` preloads only base `stageMap.prs` and `stageMapBG.prs` from the
player's own installation. Reads and PRS decode are bounded to 1 MiB. Validation checks
the complete GVM chunk walk, header records/names, counts (16/18), dimensions, supported
base formats and exact image payload sizes. Retail decoded sizes are 721776/492528.
No game assets are distributed or overwritten.

Only the two internal fallback CALLs are replaced, after checking both original targets.
The Mod Loader entry hook and successful normal replacement loading run unchanged.
Only exact, case-insensitive `stageMap.PRS` / `stageMapBG.PRS` filenames select recovery.
For them the unbounded decoder is skipped and the original parser receives validated,
owned data. Other archives delegate to the original codec/parser with their original
arguments. If either base archive is absent/incompatible, no recovery hooks are installed.
If the second code patch fails, the first is rolled back during Init.

Each archive has separate pristine and working allocations, retained for process lifetime.
Before every parse the entire working archive is restored in place, including the header's
original byte order. Native retained pointers remain valid. Both naked shims align Rust
callback stacks to 16 bytes and restore the original caller stack and nonvolatile registers.
Logs distinguish installation from actual use of each fallback.

## Validation and limits

Host tests cover malformed/truncated chunks, missing/extra images, invalid image lengths,
unterminated names, exact filename matching, repeated header mutation and stable storage.
With `SA2_DIR` set, they also validate both original installed archives.

The Windows x86 test runs the actual Rust shims with controlled native callees at all four
4-byte stack alignments. It checks stack balance, saved registers, both filename offsets,
original source/destination/list arguments for unrelated archives, skipping unsafe menu
decompression, correct owned-buffer input, restored header byte order on repeated loads,
and stable addresses. CI runs this test on native 32-bit Windows as well as host tests.

This guards the exact invalid-input texture path shown in the dump. It does not establish
why Mod Loader's normal PAK load failed, why native PRS input was invalid, or that every
possible menu crash is resolved. A real long-idle/focus-return retest is still required.
If recovery is exercised, menu graphics fall back to the original PRS versions; normal
successful higher-resolution texture loading keeps its existing path.
