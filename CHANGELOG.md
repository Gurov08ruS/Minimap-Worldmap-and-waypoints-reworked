# Changelog

## 0.5.0 — 2026-11-10

- Preserve per-column biome names with detailed terrain in the session cache.
  Unloading a game chunk or browsing another dimension no longer removes its
  known biome name/icon. Missing resamples preserve known names; new known
  samples update them. Data remains in RAM until disconnect, not on disk.
- Give each of the 67 vanilla biomes its own original 16×16 pictogram, including
  sulfur caves and the void. Redraw all dimension pictograms at 16×16 and fit
  their tabs and waypoint rows to the larger size.

- Fix chunk stripes during zoom by publishing geometry only with its exact
  uploaded texture. Equal image dimensions no longer conceal changed atlas
  layouts; failed uploads retain the previous complete frame.
- Remove zoom-dependent texture levels and the distant-smoothing setting.
  Detailed chunks and LOD keep their original texels at every map scale.
- Added a new mod icon built from the in-game map with a thin, single-colour
  square frame, plus world-map, Nether, End, waypoint and waypoint-creation
  screenshots under `assets/screenshots`. Registry-ready copies and an entry
  fragment are in `registry/`; Horizon's mod browser reads images from its
  registry, not from this README.
- Put the cursor and converted Nether coordinates on one line in the world-map
  footer, with the biome line below.
- Let waypoints be edited with a colour palette; markers, map labels and list
  initials use the chosen colour. List rows now pair the dimension icon with
  its text and show the biome beside the waypoint name. Older scoped waypoint
  records without a colour keep the default gold and migrate to `waypoint_v3`.
- Draw grass blocks with their biome's grass colour on detailed and LOD map
  tiles, so plains, forests, swamps and other biomes no longer share one green.
- Draw every leaf block from its real texture colour multiplied by the game's
  own tint source: oak, jungle, acacia, dark oak and mangrove follow the biome
  foliage colour, birch and spruce keep their distinct fixed shades, and cherry,
  pale oak and azalea keep their untinted texture colours. Resource-pack
  textures are read too, and the cached palette refreshes when sprites reload.
- Blend grass, foliage and water tints over a small radius instead of switching
  them in hard biome bands, both on detailed tiles and on LOD tiles.
- Tint water and lily pads with the biome's water and grass colours. Vines use
  the biome foliage colour, fixing the bright grass-green vines in swamps.
- Hovering a water or lava column shows its surface Y first and the bottom Y in
  parentheses in the world-map footer and the point menu.
- Temporarily removed terrain/LOD RAM eviction limits until the persistent
  container is available. Camera changes and incoming LOD levels retain old data.
- Preserve original terrain texels at all map zoom levels.
  Cull GPU presentation independently of retained terrain,
  with a worker raster fallback instead of an oversized/failed atlas upload.
- Retain approximate LOD surface heights and biome names for cursor/menu info.

- Read existing Horizon CPU LOD into a RAM footprint cache. Detailed
  chunks always take precedence; surface LOD never impersonates an underground
  slice or a visited chunk.
- Added one context-menu action for chunk and region information, with yellow
  LOD/mixed-coverage warnings, real source precision and explicit missing data.
- Let the world map fill the viewport with contrasting floating controls.
  Removed the title/player-coordinate header; Nether coordinates now describe
  the point under the mouse.

- Added German localization, including regional German variants.
- Removed mouse-drag and scroll navigation hints from the map interface.
- Added localized biome tooltips and explicit unknown-data icons.
- Added Show on map for selected waypoints, preserving zoom and centering on the
  point in its dimension; adapted waypoint rows and controls to small viewports.
- Made the waypoint manager translucent and disabled its background blur.
- Added independent Overworld/Nether coordinate toggles below the minimap,
  replacing the standalone dimension label, with correct negative rounding.
- Skip world geometry preparation and rendering behind the full-screen map,
  keeping game/network updates, texture uploads and interface rendering active.

## 0.4.1 — 2026-11-09

- Added complete English, French and Spanish interface translations while keeping
  Russian. The interface follows the game language, including regional French
  and Spanish variants. Canadian, British, Australian and other English variants
  share the English translation; unsupported languages fall back to English.
- Replaced the square player marker with an outlined directional arrow on both
  maps. The arrow follows the player's yaw and accounts for minimap rotation.
- Translated the README fully into English and documented language selection.
- Added regressions for regional language switching, preserved settings and
  player headings on fixed and rotating maps.

## 0.4.0 — 2026-11-08

- Waypoint storage, display, create, rename and delete now include the stable
  save/server identity as well as dimension and coordinates. Matching positions
  in two worlds cannot overwrite or delete each other; reconnects restore the
  same world's list. Unknown world identities cannot access stored points.
- Legacy unscoped waypoints are deleted on first load (author-requested migration).
  Preferences and already-scoped points survive; no world is guessed.
- Keep up to 64 points per world/server and 4,096 total. Reaching the total limit
  refuses new entries instead of evicting another world's records.
- The large map now renders chunk tiles through Horizon's GPU HUD pipeline.
  Dragging and zooming reuse terrain textures instead of rasterizing a full
  screen-sized image on the CPU. Camera motion no longer waits on the sampler.
- Added default-on distant smoothing with cached, alpha-weighted detail levels,
  and a persistent setting to return to sharp sampling at every scale.
- Updated CI's official bundle check to the current artifact filename.

## 0.3.1 — 2026-11-08

- Fixed the large map still looking pixelated at GUI scale 4: it now renders at
  the visible rectangle's physical resolution (GUI size × GUI scale, up to the
  1024-pixel shared atlas slot) instead of GUI size. The previous 512-pixel cap
  was not the only cause; the texture was also only as large as the GUI
  rectangle before the GPU scaled it up.
- Slightly stronger landscape relief (north-west light) so terrain steps are
  easier to see.

## 0.3.0 — 2026-11-08

- World map opened by a rebindable key, with independent dimension tabs and
  selection for the minimap and the world map.
- Shared bounded RAM terrain repository with asynchronous sampling, drag-to-pan
  and cursor-anchored zoom.
- Eased panning, zooming, minimap follow and player marker instead of tick- or
  event-stepped jumps.
- Pointer hover on the world map: thin black outline, translucent white fill,
  pointed-at coordinates, the localised biome name, and waypoint names.
- The player marker is shown in every dimension, with ×8/÷8 Nether conversion,
  one Overworld/End scale and a dimmed style outside the player's dimension.
- Translucent water and glass-family blocks, with water tint thickening by depth
  instead of looking one block deep everywhere; seabed relief stays visible.
- Grass-like ground cover is skipped; flowers, crops and other plants use their
  real petal/crop colours, stained glass panes use their dye colour, and
  scaffolding/cobweb render semi-transparent. Render-invisible blocks (barriers,
  light blocks, moving pistons) no longer draw false pixels.
- The large map renders at the visible rectangle's physical resolution (GUI
  size × GUI scale, up to the 1024-pixel shared atlas slot), not at GUI size or
  a fixed 512-pixel texture, so GUI scale 2-4 and zooming out no longer stretch
  a low-resolution image.
- Relief shading is stronger and reads neighbouring chunks; it follows the
  seabed under water; unknown terrain is transparent, removing per-chunk seams
  and moving stripes.
- World borders are drawn on the Overworld and Nether maps (the Nether uses the
  same physical area, ÷8).
- Nether portals, End portals and End gateways are marked on the map and open a
  menu with their destination; Nether destinations prefer the nearest known
  portal within the search radius and fall back to coordinate conversion.
- Waypoints require a name, are saved with the preferences (newest 64), can be
  renamed or deleted from a separate manager screen (default key **N**) or from
  the map's right-click menu, and are drawn as first-letter markers on the HUD
  through walls, on both maps, with full names on map hover.
- The runtime uses only portable standard-library APIs; the Python tooling locks
  with `flock` on Unix and `msvcrt` on Windows.
- Dimension selection neither teleports the player nor substitutes another
  dimension's terrain for missing data.

## 0.2.0 — unreleased

Development milestone: world map, dimensions, shared RAM terrain cache, eased
camera, water/glass transparency, hover coordinates and cross-dimension markers.

## 0.1.0 — 2026-11-08

Initial native Horizon minimap release.

- Four screen corners and square/round shape.
- Sizes of 64, 96, 128, 160, 192, 224 and 256 GUI pixels; default 128.
- Zoom choices of 0.1, 0.25, 0.5, 0.75, 1, 2 and 4 blocks per pixel.
- Optional player rotation and viewport fitting.
- Asynchronous local settings persistence, drained on normal shutdown.
- Pinned to Horizon Nightly 2026-11-07 (`2ebf00a`) and Horizon Mixins `d9c83d7`.

World map, waypoints, persistent terrain history, LOD coverage and manual height
layers are planned. Settings labels are currently in Russian. Interactive gameplay
and Windows/macOS have not been verified.
