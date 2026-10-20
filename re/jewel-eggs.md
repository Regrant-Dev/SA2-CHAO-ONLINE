# Built-in jewel eggs in the native Black Market

Addresses are for the Steam `sonic2app.exe` (decrypted), base 0x400000. Exant64/CWE's
`alg_kinder_bl.cpp` and egg color mapping were used as a factual cross-check only; the
feature is an original Rust implementation with no copied code or assets.

## Native path

- BlackMarketExec **0x58B5E0** calls the stock generator **0x589A00** at **0x58B8EC**.
  The original generator has no arguments and the caller ignores EAX. The mod replaces this
  one CALL, runs the original, then appends at most one eligible egg.
- Emblems: unsigned byte **0x1DEC60E**, read by the original generator.
- Inventory: count dword **0x1DBED90**, packed `{category:u8,id:u8}` entries at
  **0x1DBEDC0**, capacity 32. Category 1 is eggs. The purchased bag is a different
  structure at 0x1DBEDA0; this feature never writes the bag directly.
- Egg category descriptor **0x8A8730**: pointer **0x8A7B68**, count **69**. Attributes
  are 16 bytes: purchase/sale u32, emblem requirement/name/description/unknown i16.
  Native IDs 54..65 have zero prices, requirement 999 and generic names.
- Purchase delivery **0x548F40** checks garden capacity and obtains a native Chao record,
  initializes genes through **0x5506B0**, sets gene byte 1 to the egg ID, then calls egg
  creation **0x57B9C0**. That copies the 0xA4-byte genome into the record at +0x438.
- Gene application **0x551DA0** maps egg IDs 54..68 to texture IDs 1..15. It writes the
  body's jewel texture at +0xDA, egg color at +0xDC, and both jewel gene alleles at
  genome +0x98/+0x99. The mod uses IDs 54..65 only: Gold through Pearl. Native purchase,
  payment, capacity checks, hatching, gene inheritance and saves remain in charge.

## Messages and ownership

The base game's twelve `gd_PC/{Message,MessageK}/MsgAlItem_{e,j,f,g,s,i}.prs` files
contain 99 strings. Each has big-endian offsets terminated by FFFFFFFF. Native shop loading
uses **0x57A5D0**, walks the terminator rather than assuming 99, and stores separate dialog
and item tables in its work. Extending the item table is therefore possible.

On startup the mod resolves each source through Mod Loader's GetReplaceablePath, decode
bounded PRS, append names 99..110 and descriptions 111..122, then generate replacements in
the mod's `cache/jewel-eggs/` directory. All original message IDs and payload bytes are
preserved. The files are registered with ReplaceFile; **the native loader allocates and
frees all live message buffers**, with no Rust allocation passed into that lifetime.
Only new English strings and code are shipped; no game messages or textures are bundled.

## Activation and limits

Jewel eggs are always on and are initialized before the shop loads. Old `JewelEggs=Off`
configuration entries are ignored and dropped on the next settings save.
Each unlock tier from 120 to 180 emblems has a price from 10000 to 40000; these thresholds
and prices are the mod's own, not original-game rules. At restock the mod picks one
unlocked jewel egg not already in stock with a private RNG. It does not advance the game's
RNG, overwrite normal offers, or exceed 32 slots; a full stock gets no extra egg.

Before changing anything, the mod verifies the native catalog pointer/count, the stock
function's first eight bytes, the original restock CALL, and every modified attribute byte,
and refuses to activate on a conflict rather than overwrite another shop mod. Message tables
with a different string count, invalid offsets or truncated/oversized PRS are rejected.
Activation never edits existing Chao or eggs. Each restock also checks that the descriptor
still matches.
