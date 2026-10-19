# Optional jewel eggs in the native Black Market

2026-10-18/19: Steam decrypted `sonic2app.exe`, base 0x400000. Code slices were staged
into the REA VM and inspected with its Ghidra provider. Reference: Exant64/CWE's
`alg_kinder_bl.cpp` and egg color mapping, used for factual cross-checks, not copied code
or assets. This feature is an original Rust implementation.

## Native path

- BlackMarketExec **0x58B5E0** calls the stock generator **0x589A00** at **0x58B8EC**.
  The original generator has no arguments and the caller ignores EAX. We replace this
  one CALL, run the original, then append at most one eligible egg.
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
  genome +0x98/+0x99. We use IDs 54..65 only: Gold through Pearl. Native purchase,
  payment, capacity checks, hatching, gene inheritance and saves remain in charge.

## Messages and ownership

The base game's twelve `gd_PC/{Message,MessageK}/MsgAlItem_{e,j,f,g,s,i}.prs` files
contain 99 strings. Each has big-endian offsets terminated by FFFFFFFF. Native shop loading
uses **0x57A5D0**, walks the terminator rather than assuming 99, and stores separate dialog
and item tables in its work. Extending the item table is therefore possible.

On opt-in startup we resolve each source through Mod Loader's GetReplaceablePath, decode
bounded PRS, append names 99..110 and descriptions 111..122, then generate replacements in
the mod's `cache/jewel-eggs/` directory. All original message IDs and payload bytes are
preserved. The files are registered with ReplaceFile; **the native loader allocates and
frees all live message buffers**, with no Rust allocation passed into that lifetime.
Only new English strings and code are shipped; no game messages or textures are bundled.

## Activation and limits

Off by default (`JewelEggs=Off/False`). F9 persists the option; restart applies it before
the shop loads. Each unlock tier from 120 to 180 emblems has a price from 10000 to 40000.
These are our balance choices, not original-game unlock rules. At restock, choose one
unlocked, absent jewel egg with a private RNG; do not advance the game's RNG, overwrite
normal offers, or exceed 32 slots. Full stock gets no extra egg.

Before changing anything, verify the native catalog pointer/count, the stock function's
first eight bytes, the original restock CALL, and every modified attribute byte. Refuse
activation on a conflict rather than overwrite another shop mod. Reject message tables
with a different string count, invalid offsets or truncated/oversized PRS. Disabling never
edits existing Chao or eggs. Runtime restock also verifies the descriptor still matches.

Tests cover exact thresholds, duplicate/capacity handling, attribute/message indices,
message preservation, invalid inputs and PRS control boundaries/overlapping copies.
The Rust decoder/encoder was also run on all twelve real item-message files; the existing
independent Python PRS decoder produced identical payloads from every generated stream,
and every original table had the required 99 messages. Game files were only read.
Native activation, rendered shop previews, purchase/delivery, hatching and save/reload
still require an in-game retest; headless tests do not establish those outcomes.
