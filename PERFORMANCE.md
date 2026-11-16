# Map rendering performance — 2026-11-16

## Current development implementation

Both maps reuse immutable GPU terrain images while the camera moves. Detailed
chunks retain one texel per block; Horizon LOD retains its original 16×16
samples. Nearest sampling, detailed known-void priority over LOD, relief across
chunk edges, marker coordinates and the rotated/circular minimap are preserved.
No averaged overview levels or distant smoothing are introduced.

### Incremental visual tiles and GPU pages

An atlas keeps stable slots for surviving detailed/LOD tiles. Preparation compares
source versions, including the north/west neighbours used for relief. A changed
chunk invalidates itself and its south/east dependants. Disk record versions
survive commits within the process, so RAM eviction does not invalidate a ready
image or require re-reading its pixels. These versions are derived metadata and
do not change schema 2 on disk. Removed slots are cleared before reuse.

Views fitting the shared 1024×1024 mod image continue to use changed-band
uploads. Larger views use separate 1024×64 GUI picture textures through
Horizon's existing portable HUD renderer. They do not overwrite another atlas
slot or assume a platform graphics API. Unchanged pages retain their Arc and
picture identity; an edit replaces only affected pages. Rendering groups detail
by page and LOD by level/page, avoiding a texture switch for every chunk. The
padded/predicted page set stays in the batch, so the renderer retains its GPU
textures for short reverse drags. Resource invalidation clears client handles;
the renderer owns and releases the actual textures.

Visible source admission is bounded: `clamp(Map RAM bytes / 4096, 4096, 32768)`
tiles, or 32768 for the unlimited synthetic fixture. The default 64 MiB admits
up to 16384 visual source tiles. Crossing 4096 tiles alone no longer forces CPU
rasterization, nor does decoded-payload paging alone. Admission includes disk
keys and can stream payloads through the bounded read cache.

A final image costs 1024 bytes per source tile plus row padding. Large images
also retain CPU page buffers, the renderer's picture buffers and GPU textures;
old/new generations can briefly coexist. Shading replies are bounded by the
admitted visual tile count. These allocations, display/coverage snapshots and
index metadata are additional to the terrain RAM slider. The slider is not a
whole-process limit. Extremely wide views beyond the visual admission budget
still use the bounded raster fallback; this work does not claim unlimited
full-resolution GPU residency.

### Region lookup, lookahead and client work

A derived region-column index selects container keys by dimension, layer,
resolution and intersecting regions. Moving to new coverage no longer walks the
entire disk key history. It is rebuilt from checked metadata on open and updated
for pending writes. The bounded admitted set is prioritized around the camera.
The index itself remains metadata proportional to history size.

Lookahead includes the eased camera's target and target zoom, with movement and
zoom expansion clamped to preserve the current view's working set. Existing
padding also keeps terrain behind the camera. Requests from another session,
dimension, layer or view type cannot expand the current request.

The client follows Horizon's chunk membership revision/deltas. It rebuilds the
key set only on an unavailable delta/reset, rather than collecting and sorting
all loaded chunks every 50 ms. Each sampling pass considers up to 64 recent
arrivals, 81 nearby positions and 64 positions from an incremental sweep. Only
these bounded candidates are sorted. At most four snapshots are submitted;
the snapshot loop yields after approximately 800 microseconds once it has made
an attempt. A single engine snapshot cannot be preempted, so this is a
cooperative work budget, not a hard callback deadline.

### Worker pipeline

- `map-sampling` extracts colours/biomes from accepted immutable game snapshots.
- `map-terrain` owns the container descriptor, residency, commits and publication.
- `map-shading` prepares dirty tile relief from immutable tile/neighbour handles.

The sampling and prepared-result queues each hold at most eight jobs. The
shading input queue also holds eight. Disk reads can overlap preparation of
previous dirty tiles; no descriptor crosses a worker boundary. Failure to start
the optional shading worker uses inline preparation. Sampling failure pauses
sampling. Session checks reject stale results, orderly shutdown drains accepted
work, and the existing write-error/unsaved-data policy remains in effect.

Index lookup, image assembly, coverage publication and raster fallback still
run on the terrain worker. This is a bounded pipeline, not a claim that all
background work can progress during an arbitrarily slow fsync.

## Profiling

Set `MWWR_PROFILE=1` in the **client process** environment before launch. On
Linux/macOS use `MWWR_PROFILE=1 <client command>`; in PowerShell set
`$env:MWWR_PROFILE = '1'` before launching the client. No profiling files or
network requests are created by the mod. The terrain worker writes summaries to
the ordinary client log around each five-second commit interval.

Stages include `client-frame`, `hud-map`, `chunk-collection`,
`snapshot-collection`, `sampling`, `disk-read`, `commit`, `residency`, `atlas`,
`shade-worker`, `raster`, `upload`, `page-upload`, `texture-transfer` and
`shaded-tiles`. Each stage retains the latest 128 timing samples per reporting
window. Counts, item totals and byte totals cover the window; percentiles and
`max_us` describe those retained samples. Byte counters describe adapter
submissions/copies, not hardware GPU execution or renderer-led reuploads.
Profiling is off by default; no timers or sample locking run while disabled.
`disk-read` times store read attempts, including pending-record hits and absent
keys; it is not a count of physical storage reads or OS page-cache misses.

```sh
python3 tools/profile_maps.py /path/to/client.log
python3 tools/profile_maps.py /path/to/client.log --json
```

The tool reports the worst window p95/p99 rather than averaging percentiles.
For an actual game profile, capture camera-only movement, terrain edits and
commit separately, then repeat with a cold cache, slow storage, low RAM and
more than 4096 visible tiles. Record CPU/GPU frames and visual behaviour in the
game as well; these counters do not measure total FPS or GPU execution.

## Reproducible CPU checks

```sh
cargo test --release --locked --offline benchmark_incremental_atlas_pages -- --ignored --nocapture
cargo test --release --locked --offline benchmark_worldmap_cpu_work -- --ignored --nocapture
```

The incremental fixture holds 8192 detailed tiles. It checks a cold image,
unchanged reuse, then one chunk edit: zero shaded tiles on unchanged reuse,
three shaded tiles for the edit, and at most three replacement pages. Timings
are diagnostic CPU preparation observations, not an FPS prediction. See
VALIDATION.md for the final local run.

The historical 2026-11-14 fixture used 256/576/4096 atlas tiles at spans
32/256/2048. GPU quad preparation took 0.001/0.004/0.041 ms, and CPU raster took
39.711/42.584/23.384 ms on that host. These older timings exclude GPU execution,
vertex submission, uploads, sampling and disk I/O, and are not directly
comparable to the new 8192-tile fixture.

## Remaining work

- Capture live-game CPU/GPU profiles and visuals, including Linux/macOS/Windows.
- Persistent overview levels and stronger spatial prioritization for views beyond
  the visual budget; predictive loading cannot guarantee instant detail from an
  arbitrarily large cold history. Overview policy requires explicit treatment of
  coverage, known void and source quality; no fabricated LOD is stored here.
- Bounded disk-index pages and incremental COW index updates. Schema 2 still
  materializes both roots and writes a full index on commit. These storage
  changes belong to the subsequent container work, along with compression,
  deletion/maintenance and cache-manager features.
- Further reduction of immutable image copying and coverage/index preparation,
  guided by the new profiling counters rather than an assumed FPS bottleneck.

An 8.1 MiB user cache inspected on 2026-11-12 contained 1606 records, below the
old shared-slot limit. The old admission check reserved 128 KiB for the *next*
tile, not for every resident tile; no measured FPS gain is attributed to its
replacement with actual decoded-size accounting. None of the synthetic or
source checks establishes the cause of lag in that particular live game.
