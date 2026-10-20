# Menu texture fallback

## The native PRS fallback

`44C620` is the native PRS texture fallback, already wrapped by the Mod Loader. After normal
PAK, PVMX and texture-folder resolution fails, the original body reads a compressed file:

- `44C699 CALL 48F980`: input ECX, stack destination `1DEFE20`, unbounded PRS decode.
  At callee entry, destination is `[ESP+4]`, filename buffer starts at `ESP+20h`.
- `44C6B3 CALL 42FC30`: EAX archive `1DEFE20`, EBX NJS_TEXLIST. At callee entry the
  filename buffer starts at `ESP+14h`. The caller frees the compressed data before this call.
- The caller ignores the parser's result and returns success. Fixed-list load `44C350`
  reaches it at `44C3B6`; dynamically allocated texture-list load `44C510` at `44C56F`.

`42FC30` walks chunk lengths without bounds checks. It byte-swaps GVM/PVM header flags
and texture count in place, then calls `42FBC0` (header/texture metadata) and `430A20`
(texture creation), writing each returned texture into list entries at `+8` with stride
12. It finishes after the count in the header. Invalid input can walk arbitrary memory: the
main menu has crashed at `42FCE1` reading the next chunk length from an invalid address while
loading `stageMap.PRS`, with a zero archive header and texture count. `42FBC0`/`430A20` take
their inputs from the supplied header/image pointers; the archive doesn't need to be in the
global scratch buffer.

## Recovery (`src/menu_textures.rs`)

- At start the mod preloads base `stageMap.prs` and `stageMapBG.prs` from the player's own
  installation. Reads and PRS decode are bounded to 1 MiB. Validation checks the complete GVM
  chunk walk, header records/names, counts (16/18), dimensions, supported base formats and
  exact image payload sizes (retail decoded sizes 721776/492528). No game assets are
  distributed or overwritten.
- Only the two internal fallback CALLs are replaced, after checking both original targets. The
  Mod Loader entry hook and normal replacement loading run unchanged. If either base archive is
  absent or incompatible, no hooks are installed; if the second patch fails, the first is
  rolled back.
- Only exact, case-insensitive `stageMap.PRS` / `stageMapBG.PRS` filenames select recovery. For
  them the unbounded decoder is skipped and the original parser gets the validated, owned data.
  Other archives go to the original codec/parser with their original arguments.
- Each archive has separate pristine and working copies, kept for the process lifetime. Before
  every parse the working copy is restored in place, including the header's original byte
  order, so native retained pointers stay valid. Both shims align the Rust callback stack to 16
  bytes and restore the caller's stack and nonvolatile registers.
- When recovery is used, the menu graphics fall back to the original PRS versions; successful
  higher-resolution texture loading keeps its normal path.
