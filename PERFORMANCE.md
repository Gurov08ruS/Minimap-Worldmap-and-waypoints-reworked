# World-map rendering (2026-11-08)

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
uses the same displayed view. Within an LOD level, camera movement needs no
terrain raster or texture upload; crossing an LOD threshold uploads the cached
level once. The minimap still uses its previous CPU path.

The default-on setting **Большая карта — сглаживание вдали** selects averaged
2×2, 4×4, 8×8 or 16×16 block levels according to physical screen pixel density.
Turning it off selects the original block colours at every distance. Alpha-weighted
averaging prevents transparent pixels tinting known terrain. Levels are cached,
not regenerated during zoom. GPU sampling is nearest within the selected level;
this is distant-detail smoothing, not bilinear magnification or trilinear blending.

## Reproduce the CPU comparison

```sh
cargo test --release --locked --offline benchmark_worldmap_cpu_work -- --ignored --nocapture
```

The synthetic fixture holds 4,096 solid tiles and moves a 1024×1024 view for
60 frames at each span. A release run of the final geometry path measured:

| Visible span, blocks | Old CPU raster, ms/frame | Quad clipping/UV calculation, ms/frame |
| --- | ---: | ---: |
| 32 | 32.932 | 0.025 |
| 256 | 34.347 | 0.026 |
| 2048 | 20.015 | 0.050 |

Preparing the base atlas took 37 ms (excluding averaged-level preparation) on that run. Timings depend on the host.
These numbers exclude GPU execution, HUD vertex emission, texture transfer,
chunk sampling and the rest of the game. They are not an FPS prediction.

## Remaining work, in priority order

1. Stable atlas slots and dirty-region uploads. Currently repository changes
   rebuild all selected-layer tiles/levels and upload the selected level. The
   existing API exposes a full image upload, so partial patches need an injected
   adapter through Horizon Mixins. Invalidate the changed tile and its south/east
   neighbours because relief depends on north/west heights.
2. A dedicated map texture with mipmaps and trilinear filtering, plus edge padding
   or texture arrays to prevent unrelated tiles bleeding together. Current LOD
   transitions are discrete; 16×16 averaging is the coarsest level, so extreme
   zoom-out would benefit from regional LOD as well.
3. Profile chunk snapshot collection/sorting on the client and the sampler.
   Camera motion no longer waits for sampling, but preparation can still consume
   client/worker time. Use budgets and dirty notifications rather than repeatedly
   sorting all loaded chunks where the engine offers those notifications.
4. Spatial selection of visible atlas entries and GPU rendering for the rotated,
   circular minimap. The large map currently culls at most 4,096 entries per frame.

The existing shared 1024×1024 slot holds all 4,096 full-resolution tiles. Cached
CPU levels consume at most about 5.33 MiB of pixel data per atlas generation;
old/new generations can coexist briefly during publication. Only one level is
uploaded at once. Other mods using this same global slot still need coordination.
No native game window or GPU frame capture has been exercised by these checks;
visual behavior and FPS need an in-game run, including macOS/Windows.
