# Minimap, Worldmap and waypoints reworked

A native Horizon mod by [daniliammo](https://github.com/daniliammo).
Version **0.3.1** adds a world map, a rebindable map key and independent dimension
selection for the minimap and world map. Waypoints remain planned.

## Install

```bash
./install-horizon.sh
```

The Bash installer packages the current sources, atomically installs the `.hnmod`
in `~/.local/share/horizon/horizon-native-mods/`, and enables this mod in
`horizon-mods.txt`. It preserves other mods and key assignments. It does not
compile or launch the game by default. Use `--horizon-dir /path/to/game` for
another game folder. It runs on Linux and macOS (Bash 3.2 or newer); on Windows,
build `python3 tools/package.py` and import the resulting `.hnmod` in the
launcher, or run the script in Git Bash.

Optional native build through the launcher:

```bash
./install-horizon.sh --build --instance NAME
```

Use the instance's name or ID from `horizon-launcher --json instance list`.
The installer resolves and validates that instance's game directory and builds
its installed native mod set. `--launcher /path/to/horizon-launcher` selects a
launcher executable. A source `.hnmod` needs one game build before it can run.
The script does not update a published release or registry entry.

## Controls

- **N** opens the waypoint manager; **M** opens the world map. **Escape** closes
  the open screen (a name prompt cancels first).
- Rebind **Открыть большую карту** and **Вейпоинты** under **Options → Mods →
  this mod → Settings**. The bindings use Horizon's key assignment and
  persistence, including mouse keys and unbinding; they do not poll a hard-coded
  key.
- Conflicting native assignments are highlighted with the same brackets/style as
  Horizon's other duplicate bindings. Hover shows the conflicting actions,
  including built-in controls and other enabled native mods. Unbound and disabled
  native actions do not create warnings. Built-in binding tooltips also include
  native conflicts.
- Drag the world map with the left mouse button; scroll to zoom at the cursor.
  Panning, zooming, the minimap's follow and the player marker ease between
  frames instead of snapping to mouse steps or server ticks.
- The world map outlines the block under the pointer with a thin black border
  and a translucent white fill, and shows the pointed-at coordinates and the
  localised biome name.
- Right-click the world map for a menu at the pointed-at block: **Скопировать
  координаты**, **Поставить вейпоинт** (the name prompt opens next) and, when the
  client knows the player may run `/tp`, **Телепортироваться туда**. The teleport
  entry is hidden without game-master permission (vanilla level 2 or higher).
  Teleporting to another selected dimension uses `/execute in … run tp`.
- The menu also shows **Удалить вейпоинт** when the point is a waypoint. The
  waypoint manager (**N**) lists, renames and deletes waypoints; creation from
  there uses the player's position.
- Waypoints are saved with the preferences (newest 64), drawn as first-letter
  markers on the HUD through walls, on both maps, and their full name appears
  when hovering over the marker on the world map.
- Nether portals, End portals and End gateways are marked on the map; clicking
  one opens its destination. Nether destinations prefer the nearest known portal
  within the vanilla search radius, otherwise the ×8/÷8 conversion; End portals
  show the End platform and gateways the outer islands.
- World borders are drawn on both the Overworld and Nether maps; the Nether
  border shows the same physical area (÷8).
- The world map's **Игрок / Верхний мир / Незер / Энд** tabs select its dimension.
- **Измерение миникарты** in the mod's settings selects the minimap's dimension
  independently. **Игрок** follows the player's current dimension on either map.
- The player marker is drawn in every selected dimension. Nether and Overworld
  positions convert by ×8/÷8, the End and Overworld share one scale, and the
  marker is dimmed while the player stands in another dimension.
- Selecting a dimension never teleports the player. An uncached area displays
  **Нет данных**, without substituting the current dimension's terrain.

## Current map implementation

Both views use one background sampler and a shared, bounded **RAM** repository,
keyed by connection/session, dimension, chunk and sampled height layer. Terrain
from previously visited dimensions remains browsable within the same connection,
subject to the 4,096 tile/layer memory limit. Visible demand is protected against
background eviction; the recovery working set is bounded at extreme zoom-out.
Disconnect/relogin retires the old session. No permanent world cache is written.

The sampler uses ordinary client-loaded chunks and map colours; underground/Nether
views sample below the player's head. Translucent columns are composited instead
of stopping at their top block: water (including bubble columns) and the
glass-family blocks (glass, stained glass and panes, ice, copper grates,
honey/slime blocks, tinted glass) keep their map tint as a semi-transparent
overlay over the first opaque block. Water thickens with depth, so shallow water
keeps most of the seabed colour while deep water darkens it in steps; the relief
shading also follows the seabed under the water. Clear glass and panes use a
neutral tint, while stained panes use their dye colour. Scaffolding and cobwebs
are translucent too.

Grass-like ground cover is left out so it does not carpet every pixel: short and
tall grass, ferns, bushes, dead bushes and dry grass are skipped and the terrain
below them is shown. Flowers, crops, saplings, mushrooms, vines and the other
plants use curated colours close to their petals, fruit and stems instead of the
single generic plant green. Render-invisible blocks such as barriers, light
blocks and moving pistons leave no map pixel. Nether portals, End portals and End
gateways are recorded as map features and get their own markers.

Unknown dimensions are not fetched from the server. Distant LOD, persistent
history, manual Y selection, rectangle shape and world-border overlays remain
future stages in [PLAN.md](PLAN.md).

The displayed camera is eased: drag and zoom targets are approached over a few
frames, a zoom keeps the world point under the cursor fixed, and the minimap and
its marker interpolate the player's 20 Hz positions. The large map is rendered
at the full physical size of its visible rectangle (GUI size times the window's
GUI scale, up to the 1024-pixel shared atlas slot), not at GUI size or a fixed
low resolution, so a GUI scale of 2-4 no longer stretches a small texture over
the screen. The minimap is rendered at up to four times its GUI
size before scaling down, so a block moves in quarter-pixel steps instead of
jumping by whole blocks. The terrain renderer avoids per-pixel trigonometry and
repeated chunk lookups to keep those updates cheap. Relief shading reads the
neighbouring column's height from its own chunk tile, with a stronger
north-west light so slopes and terrain steps stand out; chunk borders have no
seams. The world map's pointer readout uses the frame's own mapping, so the
highlighted block and coordinates match the drawn image, including during
animation. Unknown terrain is left transparent instead of drawing a
screen-aligned checkerboard that turned into moving stripes.

Waypoints are stored in the same `settings.ini` and are currently shared by all
worlds of the instance; they are not yet split per save or server. Portal
destinations are computed on the client: a Nether portal prefers the nearest
already-sampled portal in the target dimension (128 blocks), otherwise the
vanilla ×8/÷8 coordinates are shown; an End portal shows the End platform
(100, 49, 0) and an End gateway is labelled as the outer islands without a
precise point. The world border is the border the client currently knows; the
Nether map draws the same physical area (÷8).

Minimap options: four corners, square/round, 64–256 GUI-pixel sizes, 0.1–4 blocks
per map pixel, and player rotation. Size follows the client's GUI scale and is
limited to the available viewport. Another provider's active native minimap slot
is respected. The world map and minimap are not drawn simultaneously.

Preferences and waypoints load/save asynchronously in `settings.ini` under the
mod data folder provided by Horizon. Normal exit flushes accepted changes. The
only filesystem writes performed by the running mod are its settings file and
its short-lived atomic-replacement file. Map history currently lives in memory,
while waypoints are part of the settings file (bounded to the newest 64). No
network requests are made by the mod.

The two views use Horizon's shared native RGBA atlas slot. Mods that independently
write that same global slot may require integration to coexist; this release does
not add a new engine texture allocator.

## Compatibility

- Minecraft Java Edition **26.3**, through Horizon.
- Horizon Nightly **2026-11-07**, commit
  `2ebf00a0281c592f79191e76fce0e7d522c6f425`.
- Horizon Mixins `d9c83d7096b7ea5b83678da2efdfd6161f74997e` (schema 6).

These revisions are pinned in [compatibility.json](compatibility.json) and the
mixin manifest. Other revisions and combinations with other mods are not claimed
compatible. See [VALIDATION.md](VALIDATION.md) for check results and practical
limits. The native mod and its injected runtime use only portable Rust standard
library APIs, with no Linux-specific calls, and are intended to build and run on
Linux, macOS and Windows. The running game has only been exercised on Linux in
these checks, so macOS/Windows remain unverified.

The Bash installer targets Linux and macOS; on Windows, install the `.hnmod` with
the Horizon launcher. The Python validation tooling takes a platform lock with
`flock` on Unix and `msvcrt.locking` on Windows.

## Build and validate

Use Rust **1.95.0**, Python **3.12+**, Git, and separate Horizon and Horizon Mixins
checkouts. The runtime and Python tooling use portable APIs (the tooling locks
with `flock` on Unix and `msvcrt` on Windows). `HORIZON_MIXINS` may point to
a separate checkout at the exact Mixins revision above. The Horizon checkout must
contain the pinned commit; staging exports only committed sources into ignored
`target/` trees and never changes the checkout or player resources.

```sh
cargo fetch --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
python3 tools/test_template.py
python3 tools/test_stage.py --horizon ../horizon
python3 tools/horizon.py stage --horizon ../horizon
cargo fetch --locked --manifest-path target/mixins/modded/Cargo.toml
python3 tools/horizon.py check --horizon ../horizon
python3 tools/horizon.py test --horizon ../horizon
python3 tools/package.py
```

Fetch the pinned Mixins checkout's dependencies with `cargo fetch --locked`
before the offline checks. The output is
`target/minimap_worldmap_waypoints_reworked-0.3.1.hnmod`.
Check it with `cargo horizon-mod check <file>` using Horizon's official
`cargo-horizon-mod` tool. The [validation workflow](.github/workflows/validate.yml)
runs the standalone tests, source audit, real client check and tests, and official
package check on the pinned source. It uploads the bundle and check logs.

A native mod is injected into `horizon-client`, so after any mod edit the
launcher recompiles that one large crate; Cargo cannot split a single crate
across processes, and Rust's parallel frontend is nightly-only while Horizon
pins stable Rust. The `fast` profile still uses 16 codegen units, so codegen is
parallel, but the frontend and link are not. For iterative builds select the
launcher's **Dev** native profile (`horizon-launcher settings native-profile dev`
or `horizon-launcher native-build --profile dev`), which the launcher documents
as about six times faster to rebuild at some gameplay cost; keep Performance for
playing. `python3 tools/horizon.py check` type-checks the real client without
codegen or linking and is the quickest edit loop.

## License and attribution

[GPL-3.0-only](LICENSE). The MIT-licensed Horizon native mod template and its
Veinminer-derived build tooling are credited in [NOTICE](NOTICE), with their
original permissions preserved in [LICENSE-MIT](LICENSE-MIT).
[Permission for Horizon registry review](NOTICE-REVIEW-PERMISSION.md) accompanies
the distributed sources. This project is not affiliated with Mojang or Microsoft.
