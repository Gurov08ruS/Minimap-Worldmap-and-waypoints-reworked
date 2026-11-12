# World-map rendering (updated 2026-11-10)

The old path rasterized a screen-sized image on the terrain worker whenever the
camera changed. At 1024×1024, that meant up to 1,048,576 coordinate, colour and
relief calculations, then a 4 MiB RGBA upload (including a copy in
`native_mod_image`) per update. Sampling chunk data and rasterizing shared one
worker, adding latency to both zooming and dragging. Optimized compilation does
not remove that work. This is a code-path diagnosis, not a captured game profile.

The large map now uses Horizon's existing portable GPU HUD pipeline: one clipped
textured quad per visible cached chunk, in the HUD batch. Terrain colours and
relief are baked at one texel per block. Pan/zoom updates geometry immediately on
the client, independently of the sampling worker. Pointer/marker/overlay mapping
uses the same displayed view. Camera movement inside uploaded coverage needs no
terrain raster or texture upload. The camera advances while wider coverage is
prepared. The minimap still uses its previous CPU path.

Zoom-dependent averaged texture levels and their setting have been removed.
Detailed chunks always use one texel per block; LOD uses its original samples.
GPU sampling is nearest. Publication compares immutable texture snapshots and
uploads the matching pixels before advancing geometry. A failed upload leaves
the previous complete frame intact. Atlas invalidation clears the displayed
frame so its texture is uploaded again before drawing.

## Reproduce the CPU comparison

```sh
cargo test --release --locked --offline benchmark_worldmap_cpu_work -- --ignored --nocapture
```

The synthetic fixture holds 4,096 solid tiles and moves a 1024×1024 view for
60 frames at each span. The historical 2026-11-08 geometry-path run measured:

| Visible span, blocks | Old CPU raster, ms/frame | Quad clipping/UV calculation, ms/frame |
| --- | ---: | ---: |
| 32 | 32.932 | 0.025 |
| 256 | 34.347 | 0.026 |
| 2048 | 20.015 | 0.050 |

Preparing the base atlas took 37 ms (excluding the then-existing averaged-level
preparation) on that run. These timings have not been rerun for the current
LOD/coverage implementation and depend on the host.
These numbers exclude GPU execution, HUD vertex emission, texture transfer,
chunk sampling and the rest of the game. They are not an FPS prediction.

## Remaining work, in priority order

1. Stable atlas slots and dirty-region uploads. Currently repository changes
   rebuild the selected layer's visible atlas and upload its replacement. The
   existing API exposes a full image upload, so partial patches need an injected
   adapter through Horizon Mixins. Invalidate the changed tile and its south/east
   neighbours because relief depends on north/west heights.
2. A dedicated paged map texture to retain GPU rendering when visible terrain
   exceeds the shared slot, preserving original source texels at every zoom.
   Currently those views use worker raster fallback, sampling the same retained
   terrain into an image with each side at most 1024 pixels. This can cost more
   CPU and is limited by that output resolution.
3. Profile chunk snapshot collection/sorting on the client and the sampler.
   Camera motion no longer waits for sampling, but preparation can still consume
   client/worker time. Use budgets and dirty notifications rather than repeatedly
   sorting all loaded chunks where the engine offers those notifications.
4. Spatial indexing of retained terrain and GPU rendering for the rotated,
   circular minimap. Atlas entries already use a padded visible region; selecting
   them still scans the retained layer.

The shared 1024×1024 slot holds up to 4,096 full-resolution tiles (detailed and
LOD rows are packed separately). One atlas generation consumes at most 4 MiB
of pixels; old/new generations can coexist briefly during publication. Terrain
payloads and pending writes now use the configurable RAM budget; historical
tiles remain on disk. Disk-index metadata and current presentation snapshots are
accounted separately. The current index is loaded as compact key/reference metadata
for both recoverable roots and rewritten on commit: very large histories still
need a bounded index-page cache and incremental index updates. Other mods
using this same global slot still need coordination.
No native game window or GPU frame capture has been exercised by these checks;
visual behavior and FPS need an in-game run, including macOS/Windows.

## Persistent-cache work (2026-11-12)

An 8.1 MiB user cache inspected on 2026-11-12 has 1,606 terrain records
(1,172 detailed and 434 LOD), below the atlas tile limit. Admission now checks
actual decoded tile size instead of reserving 128 KiB for the next tile. The old
check did not charge 128 KiB for every resident tile; the earlier explanation
and inferred 255-tile limit were incorrect. No measured FPS gain is attributed
to this admission change.

Camera motion now advances beyond uploaded atlas coverage without waiting for
worker preparation. In raster fallback, the existing texture is clipped and
reprojected into the current camera on the GPU, sharing its original frame
without copying pixels or uploading again. Newly exposed areas remain empty
until the replacement arrives. Regression tests cover pan/zoom clipping,
texture reuse and camera movement beyond prepared coverage. In-game FPS is
not measured by these tests.

Implemented optimizations:

- A bounded working set and disk-read LRU replace unlimited terrain retention.
  Visible-area candidate lists are bounded even for extremely wide views.
- Pending records replace earlier updates for the same key before commit.
  A five-second batch or pending-byte pressure triggers persistence.
- Per-tile biome palettes store each distinct name once and share decoded names
  through Arc; immutable display snapshots also share tile payloads.
- A physical dimension change flushes and drops the former payloads, coverage
  and worker atlas; only an explicitly requested view is loaded again.
- Read-LRU byte accounting is maintained incrementally rather than rescanning
  all cached payloads for every disk miss.

Next useful measurements and optimizations are a bounded index-page cache,
incremental index updates instead of a full index rewrite, and separating disk
I/O from sampling/raster preparation so a slow disk cannot delay the terrain
worker's other work. These are not claimed implemented. The render callback
does not perform disk I/O in the current version. The existing source audit and
unit tests do not substitute for profiling a live game on a large real cache.
