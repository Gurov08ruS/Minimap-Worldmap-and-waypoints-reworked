# Map rendering performance — 2026-11-16

## World-space visual streaming

The normal display pipeline always builds GPU tile pages. It does not select the
legacy screen-raster fallback when a view becomes wide. A hierarchy of derived
16×16 visual tiles is persisted in the same schema-2 `.mwwr` container. These are
rebuildable images, not replacement canonical terrain or fabricated Horizon LOD.
Near views retain original block texels; wide views select reduced levels.
Coverage is stored independently of alpha: unknown samples do not darken known
colours, and detailed known void masks lower-priority LOD.

Existing containers build the hierarchy in background steps on first use.
Viewport sources receive priority over historical backfill. Commits checkpoint
pending work; completed visuals are read directly on subsequent opens. A marker
tied to the committed generation rejects stale images after an older writer
changes canonical terrain. Damaged derived records are rebuilt from source.
See CONTAINER.md for record layouts and compatibility limits.

The camera's current coverage and target zoom are requested together. A direct
wake queue bypasses slow snapshot sampling. Ready coarse tiles cover the view
until finer requested coverage is ready, and existing pages can be retained even
after their source payloads leave RAM. Atlas assembly uses only immutable loaded
images; it performs no file reads. Hover height, biome and portal metadata are
requested separately and loaded asynchronously without changing terrain images.

Visible Horizon LOD acquisition prefers intersecting coarse footprints before
local detail, with at most eight snapshots per 50 ms. Snapshot deduplication
tracks live engine identities independently of RAM coverage. Off-screen payload
eviction therefore cannot repeatedly resubmit the same near sources and starve
far terrain. Selection partially sorts only the bounded selected prefix.

## Budgets and GPU pages

Visual tiles retain stable slots. Source and fallback dependencies determine
which pixels changed. Relief edits invalidate the source and its south/east
dependants. Pages are independent 1024×64 GUI pictures through Horizon's portable
HUD renderer. Unchanged pages retain identity; draws group by page and LOD level.
The renderer owns texture lifetime. Resource invalidation releases client handles.

The visual image cache targets `clamp(Map RAM / 2, 32 MiB, 64 MiB)`. Requested
coverage pins at most 32,768 entries; pins can raise the cache capacity to the
requested count plus 512 entries (hard ceiling 65,536). A presentation admits
at most 32,768 visual tiles, selecting a coarser level when the requested grid
would exceed this limit. This bounds work without changing to a screen image.
One tile has 1024 RGBA bytes; cache accounting also reserves metadata/coverage.
CPU page buffers, renderer picture buffers, GPU copies, display/coverage
snapshots, disk indexes and overlapping old/new generations consume additional
memory. The Map RAM slider is not a whole-process limit.

A streaming step rebuilds at most 256 derived nodes with a cooperative 6 ms
budget, then loads requested visuals up to a total 12 ms budget. Historical
backfill admits 32 canonical keys per step. These limits are checked between
operations: an individual read, shading operation or fsync can take longer.
Busy visual work polls at 1 ms; an idle terrain worker waits up to 100 ms.
No claim is made that arbitrary cold storage or missing game LOD becomes instant.

A derived region-column index selects keys by dimension, layer and resolution.
The index remains proportional to history; both roots are materialized and each
commit still writes a full disk index. Bounded index pages, incremental COW,
compression, deletion and cache-manager controls remain subsequent container work.

## Workers and client callbacks

- `map-sampling` extracts colours and biome data from immutable game snapshots.
- `map-terrain` owns the descriptor, streaming, commits and publication.
- `map-shading` prepares relief from immutable tile/neighbour handles.

The sampling, prepared-result and shading-input queues each hold eight jobs.
No descriptor crosses a worker boundary. Optional shading-worker startup failure
uses inline preparation. Session checks reject stale results; shutdown drains
accepted work. Write failures preserve accepted unsaved records and pause new
sampling. A busy container uses temporary bounded RAM without changing its file.

Ordinary chunk selection uses engine membership deltas, an incremental sweep and
bounded nearby/recent candidates. At most four ordinary chunk snapshots are
submitted per pass, yielding after approximately 800 microseconds once an attempt
has been made. An individual engine snapshot cannot be preempted.

## Profiling and reproducible checks

Set `MWWR_PROFILE=1` in the client process environment. The ordinary client log
receives summaries around the five-second commit interval; the mod creates no
profiling files or network requests. Stages include `client-frame`, `hud-map`,
`lod-snapshot-pick`, `snapshot-collection`, `sampling`, `disk-read`, `commit`,
`visual-stream`, `visual-atlas`, `shade-worker`, `page-upload` and `texture-transfer`.
Each stage retains its latest 128 timing samples per window. Counts and bytes
cover the window; percentiles describe retained samples. Disk counters include
pending hits and absent-key attempts, not just physical reads; upload counters
describe submissions/copies rather than hardware GPU execution.

```sh
python3 tools/profile_maps.py /path/to/client.log
cargo test --release --locked --offline benchmark_persistent_zoom -- --ignored --nocapture
```

The persistent-zoom fixture uses 4096 detailed tiles and four native LOD sources.
It measures first preview, initial background backfill, reopening ready visuals,
prefetching a finer target and assembling a zoomed frame. The local run measured
8.354 / 256.126 / 8.176 / 4.693 / 0.850 ms respectively, using two texture pages.
This is synthetic CPU work with warm OS file cache, excluding GPU/FPS and actual
gameplay. The other ignored atlas/raster benchmarks exercise legacy reference
paths and are not timings of the new normal streaming path.

Remaining performance validation is live-game CPU/GPU capture, cold storage and
large-world flight/teleport/zoom scenarios, including macOS and Windows. Existing
portable source and synthetic checks cannot establish seamless frame pacing on
all hardware. Further copy/index optimizations should follow these measurements.
