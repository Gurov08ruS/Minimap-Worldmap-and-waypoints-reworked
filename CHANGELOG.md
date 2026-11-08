# Changelog

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
