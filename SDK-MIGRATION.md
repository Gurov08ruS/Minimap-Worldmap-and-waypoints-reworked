# Public SDK migration: feature-preserving route

Status: mod 0.7.4, signed Horizon SDK for
`0a875da91dbb0fdcb7ba8c42c11341aa229af51a` (2026-11-20).
The chosen scope preserves maps, distant LOD, persisted terrain, waypoints,
cache management and HUD placement. It does not remove functionality to obtain
an API-only manifest.

## Implemented boundary

| Concern | Public SDK used in 0.7.4 | Native dependency removed |
| --- | --- | --- |
| Map/waypoint keyboard bindings | `Hooks::input_key`, `mod_key`, `mod_screen_open/close`, `playing` | `map-key` injection |
| Mouse bindings | `consume_click`, registered key mappings | Uses the SDK mapping queue; no new engine access |
| Language selection | `client_session().client_language()` | Private `horizon_client::lang::code()` read |
| Disconnect | `Hooks::client_disconnecting` | `map-disconnect` injection |
| Resource reload | `Hooks::client_resources_reloaded` | `map-resources` injection |
| Transparent waypoint background | `Hooks::menu_background`, `MenuBackgroundEvent::radius` | `waypoints-no-blur` injection |

Hooks retain no hosts or borrowed game objects. Disconnect and reload publish
coalescing atomic notifications; the remaining native frontend consumes them
before its next update/draw and applies existing cleanup. Runtime tests cover
binding priority, modifiers, unbinding and notification coalescing. The waypoint
background flag follows screen-mode transitions, including nested name forms.

The package now has **13 native injections**, down from 17. Public callbacks
compile against the signed API crate. The revision pin remains required because
`native.module` and `native.mixin` still expose engine internals.

## Remaining frontend conversion

Public screen draw/input hooks already exist. `DrawContext` provides text,
fills, sprites, quads and clipping. Public player/session views, settings,
clipboard and command services also exist. These should be used as the frontend
is decoupled from `Game`, `HudBatch` and `Picture`; their presence must not be
reported as a missing hook. Registration already uses the public SDK.

The native frame/screen/render/input adapters currently share one frontend and
its texture/sampling state. Replacing their callbacks alone would leave the
private dependencies in that frontend. They stay enabled until that state can
be expressed through the contracts below. Existing typed setting reads and raw
setting writes also need to replace the current live GUI-row synchronization;
verify immediate updates in an already-open settings screen during that port.

`client_packet` is public and reports packet type IDs, but ordinary decoded
fields do not cross the API. Death detection by packet ID can be moved to a
public callback with public player/session reads. This is frontend conversion
work, not evidence that a new death hook is required. Chunk-coordinate tracking
currently shares the native packet adapter and still needs a supported chunk
change/snapshot contract. Do not introduce numeric protocol IDs or decode
private packet layouts into the supposedly stable SDK boundary.

## Requested upstream contracts

The following are proposals, **not existing SDK methods**. Names are illustrative;
Horizon maintainers should choose the final vocabulary and compatibility policy.
No upstream checkout was changed and no issue or pull request has been sent.

### 1. Immutable client terrain snapshots and revisions

Provide bounded read-only snapshots of chunks already present on the client,
on remote and integrated servers, without loading/requesting/generating terrain.
Include session/world identity, dimension, chunk coordinates, vertical bounds,
blocks, biomes and the lighting/surface data needed for map rendering. Explicitly
distinguish unavailable data from air. Provide membership/content revisions and
resource/palette revisions, including changes within an already loaded chunk.

The mod must be able to hand an owned immutable snapshot to a worker without
retaining a host, locking the live game or copying the entire world each frame.
Define quotas, eviction and lifetime across disconnect/dimension change. A
bounded query can return a cursor or continuation rather than scan every chunk.
Current server-oriented `loaded_chunks` and client point/surface reads do not
supply this bulk immutable worker contract.

Acceptance: edits, chunk replacement, unload/reload and dimension switches
refresh only affected terrain; remote worlds work; no chunk request is emitted;
worker data remains safe after disconnect; unavailable chunks do not erase maps.

### 2. Existing CPU LOD footprints

Expose bounded snapshots of *existing* far-terrain samples, with world/dimension,
world-space origin, extent, stride and a documented sample layout. The current
adapter consumes 16×16 footprints. Preserve surface/ground/water heights and
material/biome/climate/tint information sufficient for current LOD colouring,
water opacity and approximate snow/ice policy. Revisions must change when
content changes; pointer addresses must not be the public identity.

No terrain generation or request should be triggered by enumeration. Supply
change/removal notifications or a bounded revision query, including resource
reload. Ordinary detailed terrain must continue to override far samples.

Acceptance: wide maps retain distant coverage, mixed old/new footprints update
incrementally, and reloading resources refreshes colours without deleting saved
canonical terrain. Existing client surface heights are not a replacement for
far CPU LOD footprints.

### 3. Independent dynamic GUI texture pages

Provide mod-owned handles for RGBA texture creation, full/subrectangle updates,
UV drawing with clipping and retirement. Specify dimensions, byte/page quotas,
reload/device-loss recovery and handle lifetime. Support the existing paged
map atlas, zooming/panning and round minimap masking without engine types.
Texture updates should occur only when changed.

The current public `mod_image` is one shared HUD slot **for all mods**; a second
mod overwrites it. It cannot preserve independent map pages and coexistence.
Static sprites and untextured quads do not replace dynamic map textures.

Acceptance: two map/image mods cannot overwrite each other's images; wide views
page within a fixed memory budget; resource reload restores pages; circle clips
and UV edges remain correct at fractional GUI scales.

### 4. GUI-only presentation while the full map is open

Expose a documented way for an owned mod screen to suppress preparation/drawing
of the 3D world while continuing GUI rendering, ticks, networking, background
sampling and audio. Define screenshots, screen closure and reload behaviour.
The existing public `render_frame_pre` listener does not replace the preparation
and presentation path intercepted by `map-render-only`.

Acceptance: opening the map avoids the expensive world render without pausing
terrain ingestion; closing it restores normal rendering on the next frame.

### 5. HUD ordering and reserved corner areas

Provide a public layer/area contract that places 3D waypoint overlays below the
minimap and reserves corner rectangles including coordinate footers. Define
cooperation with effect icons, other corner panels and debug overlays. Preserve
the fixed top-right minimap position when effects appear/disappear; place effects
below it and remaining panels below both. This currently needs two injections
into extras/potion layout in addition to native map drawing.

Acceptance: every corner, changing effect count, F3 and GUI scaling produce no
unintended overlap or minimap jumps; multiple mods can reserve areas predictably.

### 6. Resource-derived surface palette

Public `block_sprites` and `block_tint_color` already supply useful model/tint
information. Complete the snapshot contract with a resource-revisioned surface
palette (or safe texture samples/mean colours and material flags) sufficient for
resource-pack textures, transparent leaves, water and biome blending on workers.
Avoid per-pixel host calls or a second private renderer dependency.

Acceptance: resource packs and biome colours match current maps, palette rebuilds
occur once per revision, and no GPU/engine object crosses the SDK boundary.

### 7. Owned screens in the main menu

Provide a supported entry point for adding a main-menu action and opening a mod
screen before a world is loaded. The same screen should get drawing, input,
focus, close/back navigation and the active locale. The public
`mod_screen_open` is currently limited to gameplay and the public language
service requires a logged-in world. Version 0.7.5 uses the existing native
screen adapter to draw its main-menu cache browser.

Acceptance: a mod can browse local data before connecting, close with Escape
or Back, and coexist with other mods' menu actions without overlapping controls.

## Completion criteria and compatibility

Port the remaining frontend to public SDK values/handles, remove every native
module/mixin and private Horizon dependency, and validate packaging/auditing
against signed SDKs. Only then should this mod claim API-only compatibility.
The launcher's API-use compatibility checks, rather than a fabricated revision
range, must decide whether a later game supports the package. Native code may
still be compiled by the launcher for a new build; avoiding author updates is
not the same as eliminating native compilation.

Until then 0.7.4 remains pinned to `0a875da`. Linux compilation and automated
checks do not establish live GUI behaviour or macOS/Windows execution.

## Evidence

Contracts were checked in the pinned signed SDK export and committed sources:

- [SDK compatibility and native fit](https://github.com/horizon-rust/horizon/blob/0a875da91dbb0fdcb7ba8c42c11341aa229af51a/docs/modding/src/reference/sdk/04-fit.md)
- [Client rendering and shared image slot](https://github.com/horizon-rust/horizon/blob/0a875da91dbb0fdcb7ba8c42c11341aa229af51a/crates/horizon-mod-vocab/src/host/client_render.rs)
- [Client surface views](https://github.com/horizon-rust/horizon/blob/0a875da91dbb0fdcb7ba8c42c11341aa229af51a/crates/horizon-mod-vocab/src/host/client_view.rs)
- [Level/chunk services](https://github.com/horizon-rust/horizon/blob/0a875da91dbb0fdcb7ba8c42c11341aa229af51a/crates/horizon-mod-vocab/src/host/levels.rs)
- [Public packet contract](https://github.com/horizon-rust/horizon/blob/0a875da91dbb0fdcb7ba8c42c11341aa229af51a/crates/horizon-mod-vocab/src/hooks/client_packet.rs)
