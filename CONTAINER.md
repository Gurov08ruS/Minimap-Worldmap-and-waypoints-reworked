# Map container, version 2

The running mod uses one `cache/<identity-hash>.mwwr` file per save or public
server address. All dimensions and sampled layers share it. The identity uses
the existing `save:` / `server:` namespaces. Two deterministic 64-bit hashes
make a portable filename; the exact UTF-8 identity is checked inside each root,
so collisions fail rather than merging worlds. Maximum identity length: 2048
bytes. No address aliases, automatic epochs or wipe detection are inferred.

The final initial format is schema 2; schema 1 was an unpublished development
prototype with byte-sized reservations, and is refused without rewriting it.
Previous releases only had RAM terrain, so
there is no previous disk cache to migrate. New schema versions are refused
without rewriting their contents. Settings and waypoints still use the existing
`settings.ini` format, independently of the terrain container.

## Layout

All integers are little endian; checksums are CRC32C (Castagnoli). Ranges are
half open. The first 8192 bytes are reserved for two 4096-byte superblocks.
Each superblock has this layout:

| Offset | Field |
| --- | --- |
| 0 | 8-byte magic `MWWRC002` |
| 8 | u32 schema, currently 2 |
| 12 | reserved zero bytes |
| 16 | u64 generation, starting at 1 |
| 24 | u64 first index page offset, or 0 for an empty index |
| 32 | u64 record count |
| 40 | u32 UTF-8 identity length |
| 48 | exact identity bytes, at most 2048 |
| 4092 | u32 checksum of bytes 0..4092 |

An immutable 4096-byte index page contains magic `MWWRIDX1`, generation (u64 at
8), next-page offset (u64 at 16), entry count (u32 at 24), and up to 100 sorted
40-byte entries starting at byte 32. Its checksum is at 4092. Each entry stores
six i32 key fields, record offset (u64), stored length (u32), and checksum (u32).
The chain must have globally increasing keys, the declared count, valid bounded
extents, and no overlapping pages or records. Invalid roots are discarded;
opening chooses the latest valid index/root. It does not read all tile payloads.
A damaged payload is isolated when requested; unrelated records remain readable.

Terrain keys are `[dimension, kind, layer_y, x, z, level]`. Dimension codes are
explicitly 1 = Overworld, 2 = Nether, 3 = End. Kind 0 is a detailed surface;
kind 1 is a detailed `Below(layer_y)` sample; kind 2 is LOD. Detail X/Z are chunk
coordinates, LOD X/Z and level retain their original footprint. Surface and LOD
use layer_y = 0; detail uses level = 0. Existing Horizon custom dimensions are
still mapped by the adapter's existing dimension policy; this format does not
claim support for identifying arbitrary custom dimensions.

Tile records contain `TIL1`, a u32 per-record codec (0 = raw), 1024 RGBA bytes,
256 i32 heights, 256 validated boolean wall flags, a bounded biome-name palette
and 256 u16 palette indices (0 = unavailable), then portal samples. The palette
contains up to 256 UTF-8 names, each with a u16 length of 1..256 bytes. Up to 768
portals each store three i32 coordinates and a validated one-byte kind. No
runtime block-state IDs, generated terrain, invented visit times or unsupported
vertical data are written. The generic reader bounds any record to 1 MiB before
allocation; tile decoding applies the tighter structural limits above. Zstd is
not implemented yet; future codecs require an explicit compatible decoder.

## Commit and allocation

A single background worker owns the descriptor and performs all file I/O.
Every record reservation and index page starts at a 4096-byte boundary and
reserves a whole number of 4096-byte pages. Record checksums cover their stored
bytes; the padding remains reserved while either root references the record.
This avoids reusing bytes in a page still occupied by recoverable data. File
growth includes the reserved padding before the first sync.

New records and a new index are written into gaps that neither recoverable root
references, or appended if no gap fits. New allocations are reserved for that
transaction. Data and index are synced before writing the inactive superblock;
the new superblock is then synced before publication. Both roots remain
protected during allocation. No file reader survives a worker operation: the
render/game threads receive independent immutable RAM snapshots, so there are
no file reader pins to retire in this implementation.

Free extents are reconstructed from both roots rather than trusting an
unvalidated free list. Obsolete records become reusable only after neither root
references them. There is no truncation, physical deletion, compaction, external
journal, lock file, second container or temporary copy. A write/sync error
poisons the writer until reopen; accepted pending bytes remain in RAM and new
sampling pauses. It never retries allocation using uncertain post-error state.

Batch commits are scheduled every five seconds, and forced on world/dimension
changes and orderly exit. Disk throughput, sampling and fsync can take longer;
this is not a hard real-time durability deadline. Process interruption and short
writes are tested; physical power-loss durability on every filesystem is not
claimed. Creation interrupted before any nonzero header can be initialized
again. A damaged first header with no recoverable root fails closed.

## Ownership and RAM

Creation opens without truncate and initializes only after a successful
nonblocking exclusive standard-library file lock. Unknown world identities
use a bounded temporary RAM map with a warning and never create an unscoped
container. Two creators converge on the
same file. Existing files are never unlinked or renamed by the mod. Unknown
locking errors fail closed; a busy file uses only a bounded temporary RAM map
and reports its unsaved state. After all synchronous I/O ends, the owning lease
explicitly unlocks and closes; it never clones or lends the descriptor. Tests
cover processes, same-process opens, another world, owner death and reacquisition.
External programs that delete/replace open files do not follow this protocol;
manual deletion must happen with the game closed. No unlink/create guarantee or
network-filesystem qualification is claimed.

The RAM slider defaults to 64 MiB and ranges from 16 to 512 MiB. Conservative
payload accounting budgets the working set, LRU disk reads and pending bytes.
Off-screen data can be evicted before commit because its encoded bytes remain
in the pending transaction. Dirty bytes are never discarded after a failed
write. Display/coverage snapshots, GPU textures, bounded input snapshots and
index metadata are separate; this is not a whole-process memory cap. During
normal publication old/new display generations may coexist. A physical
dimension switch flushes before clearing old payloads and display references.
Selecting a previously visited dimension loads the requested area again.

The index currently materializes key/reference metadata for both recoverable
roots and writes a new index on commit. It does not materialize historical tile
payloads, but metadata and commit work still grow with file size. A bounded
index-page cache and incremental COW index updates remain future optimizations.
Large views use a bounded read LRU and rasterize the requested pixels on the
worker instead of retaining all visible terrain. This can increase worker I/O
and latency on slow disks; FPS and huge-world behaviour require game profiling.
