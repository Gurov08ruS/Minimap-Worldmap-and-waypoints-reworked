# Validation

## 0.5.0 — release validation, 2026-11-10

Validated on Linux with Rust 1.95.0 and the unchanged Horizon/Mixins pins in
`compatibility.json`. Historical checks below describe pre-release snapshots;
their older package sizes and hashes do not identify the release artifact.

- Formatting and clippy with warnings denied: pass.
- Locked offline workspace tests: pass (55 runtime tests, one ignored manual
  benchmark, one source-audit test).
- Regressions cover retained detailed biome names after chunk unload, negative
  chunk coordinates, dimension/layer isolation, missing and updated resamples,
  immutable coverage snapshots, and water-surface versus cave biome sampling.
- World, actual-render/LOD and chunk-render borders are drawn in world-border
  style; the chunk border follows full chunk edges and its slack. Labels appear
  only while hovering a border. Right-click outlines the hovered chunk and
  region, and the information card reports the LOD quality level and stride.
- 3D-world waypoints are drawn in the final GUI overlay with their coloured
  marker, name and distance, through walls and only in their dimension.
- Template integration, deterministic AST staging and full source audit: pass;
  the original Horizon checkout remains unchanged.
- Actual pinned Horizon client check: pass without warnings.
- Runtime tests inside the pinned client: pass (55 tests); injected client
  adapter tests pass (9).
- Reproducible source-only package and official package check: pass.
  Artifact: `minimap_worldmap_waypoints_reworked-0.5.0.hnmod`, 103,330 bytes.
  SHA-256: `41969c9bdfc128b6e995784060d68c01ceda2987e9476f08f7a52dc36e6cfce0`.
- Local launcher also compiled the installed package on the user's existing
  `fdc87ae` base in the performance profile. This is a compilation check,
  not an additional supported-revision or gameplay claim.

Biome and terrain history remains in RAM for the current connection. Disconnect
clears it. Previously unknown biome data requires another visit to a loaded
chunk. No live-game visual review, FPS profiling, macOS/Windows run or registry
publication was performed for this release.

## Pre-release snapshot — zoom stripes and fixed source detail, 2026-11-10

- Removed zoom-dependent atlas levels and the distant-smoothing setting;
  legacy preferences load without restoring that behavior.
- Texture publication now uses immutable snapshot identity and commits the
  matching geometry only after the upload succeeds. Resource invalidation
  discards the displayed frame and forces its next upload.
- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (53 runtime tests, one ignored manual benchmark; one source-audit test).
- New regressions cover same-size atlases with different slot layouts, failed
  uploads, repeated zoom, atlas/raster transitions, resource invalidation, and
  unchanged detailed/LOD texels through zoom and atlas repacking.
- Template integration: pass. Pinned Horizon client check: pass without warnings.
- All 53 runtime tests pass inside the pinned client; nine client UI tests pass.
  Deterministic AST staging and full source audit pass; the original Horizon
  checkout remains unchanged.
- Official package check: pass. Local source bundle: 98,052 bytes; SHA-256
  `6c2d8a04f929338902ce7efe2331c2bf792b180440d00ab33ae4a0f8eb58af26`.

No live-game visual review, FPS profiling or macOS/Windows run was performed.
The existing raster fallback remains limited to a 1024-pixel image side when
visible terrain exceeds the shared GPU slot; it samples original terrain without
zoom-dependent averaging. These checks were recorded before the 0.5.0 release.

## Pre-release snapshot — biome blending, water and fluid hover, 2026-11-10

- Hovering a detailed water or lava column reports its surface Y first and the
  lowest liquid Y in parentheses, in the world-map footer, the point menu title
  and the information card.
- Water follows the biome water colour; lily pads use the biome grass colour;
  vines use the biome foliage colour, removing the bright grass-green vines in
  swamps.
- Grass, foliage and water tints are averaged over a small radius on detailed
  tiles; LOD tiles blend the neighbouring 16×16 samples. Biome borders therefore
  fade rather than switching in hard bands.
- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (52 runtime tests, one ignored manual benchmark; one source-audit test).
- Template integration, deterministic AST staging and full source audit: pass.
  The original Horizon checkout is unchanged.
- Actual pinned Horizon client check: pass without warnings. Runtime tests pass
  in the pinned client (52); injected client adapter tests pass (9).
- No live-game visual review, FPS profiling or macOS/Windows run was performed.

## Pre-release snapshot — real leaf colours, 2026-11-09

- Leaf states get their map colour from the game's own block sprites: the mean
  texture colour is multiplied by the state's `TintSource`. Oak, jungle, acacia,
  dark oak and mangrove therefore follow the biome foliage colour; birch and
  spruce use the mesher's fixed shades (distinct from oak); cherry, pale oak and
  azalea keep their untinted texture colours. Resource-pack textures are read
  and the palette refreshes with `native_block_sprites_version`.
- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (52 runtime tests, one ignored manual benchmark; one source-audit test).
- Template integration, deterministic AST staging and full source audit: pass.
  The original Horizon checkout is unchanged.
- Actual pinned Horizon client check: pass without warnings. Runtime tests pass
  in the pinned client (52); injected client adapter tests pass (7, including
  texture-tint multiplication and colour-swatch layout).
- No live-game visual review, FPS profiling or macOS/Windows run was performed.

## Pre-release snapshot — waypoint colours and compact map footer, 2026-11-09

- World-map cursor and Nether coordinates share one footer line; the biome
  keeps its own line.
- Waypoints gained a persisted `0xRRGGBB` colour chosen from an eight-colour
  palette in the create/edit form. `waypoint_v2` records load with the default
  gold and migrate to `waypoint_v3`; map and HUD markers, list initials and
  hover labels use the selected colour.
- Waypoint rows show the dimension icon beside its text and the biome beside
  the name. The Edit action renames a waypoint and changes its colour.
- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (52 runtime tests, one ignored manual benchmark; one source-audit test).
- Template integration, deterministic AST staging and full source audit: pass.
  The original Horizon checkout is unchanged.
- Actual pinned Horizon client check: pass without warnings; the injected
  runtime tests pass inside the pinned client (52 tests).
- No live-game visual review, FPS profiling or macOS/Windows run was performed.

## Pre-release snapshot — biome-tinted grass, 2026-11-09

- Grass blocks on detailed and LOD map tiles now use the biome's grass tint,
  with the block's ordinary map colour as the fallback for unknown biomes.
- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (51 runtime tests, one ignored manual benchmark; one source-audit test).
- Template integration, deterministic AST staging and full source audit: pass.
  The original Horizon checkout is unchanged.
- Actual pinned Horizon client check: pass without warnings; the injected
  runtime tests pass inside the pinned client (51 tests).
- No live-game visual review, FPS profiling or macOS/Windows run was performed.

## Pre-release snapshot — stable LOD retention and zoom quality, 2026-11-09

- Formatting and clippy with warnings denied: pass.
- Locked offline runtime tests: 50 passed; one manual benchmark ignored.
  Source-audit regression: one passed.
- Pinned Horizon client check: pass without warnings.
- All 50 runtime tests also pass inside the pinned Horizon client.
- Template integration and deterministic AST staging/source audit: pass;
  the original Horizon checkout remains unchanged.
- Five injected client UI tests: pass, including holding the displayed map
  camera inside the uploaded atlas while the next coverage area is prepared.
- Official package check and byte-reproducible packaging: pass. Local source
  bundle: 92,062 bytes; SHA-256
  `32ef5c88434f6a230d4cb29f8dcc6446b095b4ffc7cace75579a98fdcca8f4d4`.

Regressions cover retention beyond the former terrain budgets, camera/source
changes, finest available LOD height and biome metadata, unchanged original LOD
texels at every mip level, and raster fallback beyond GPU atlas capacity.
Terrain RAM is temporarily uncapped until the persistent cache container exists;
bounded work queues and waypoint limits remain. The earlier bounded-eviction
behavior described below has been superseded.

No live-game visual review, FPS profiling or macOS/Windows run was performed.
These checks were recorded before the 0.5.0 release.

## Pre-release snapshot — RAM LOD and full-viewport map, 2026-11-09

- Formatting and clippy with warnings denied: pass.
- Locked offline workspace tests: pass (47 runtime tests, one source-audit test;
  one manual benchmark ignored).
- Template integration, deterministic AST staging and full source audit: pass.
  The original Horizon checkout is unchanged. Public module visibility is
  accepted only when explicitly declared by the Mixins plan; undeclared public
  visibility remains rejected.
- Actual pinned Horizon client check and runtime tests: pass (47 tests).
- Four client adapter tests: pass, including full-viewport map geometry and
  overlay hit testing.
- Official package check and byte-reproducible packaging: pass. Local source
  bundle: 88,933 bytes; SHA-256
  `263635b6c336a159d0997ac6439e55cab4f0fc0c2bfb6490b9a2f81a921204fe`.

New regressions cover real LOD footprint/stride preservation, negative chunk and
region boundaries, unknown portions of partial footprints, detailed known void
masking LOD in both renderers, finer LOD priority, filling unknown cells from new
CPU snapshots, bounded eviction and late jobs crossing world/session resets.
The CPU adapter reads only Horizon's existing surface columns. It does not fetch
or generate extra terrain, interpret surface spans as caves, or record visits.
No live-game visual review, FPS profiling or macOS/Windows run was performed.
These changes are local and have not been pushed or published.

## 0.5.0 — local validation, 2026-11-09

Local Linux, Rust 1.95.0, unchanged pinned Horizon and Mixins revisions.

- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (40 tests; one manual benchmark ignored).
- Template integration: pass, including German regional variants and the
  coordinate settings alongside the existing language and persistence checks.
- Deterministic staging and source audit: pass; original checkout unchanged.
- Actual Horizon client check and runtime tests: pass (40 tests).
- Four client adapter tests: pass, covering map header layout, waypoint layout,
  centering a selected waypoint across dimension transitions, preserved zoom,
  and the screens eligible for rendering without the world.
- Official package check and byte-reproducible packaging: pass.
  Source bundle: 79,909 bytes; SHA-256
  `f152cbb7f45fb09697d18009ad5d3c96428b9fbf144ba79c0f0ab5ae35ba8006`.

The new pictograms are original code-drawn assets. Waypoint biome samples live
only in the current connection's RAM and are collected from loaded chunks in
the player's actual dimension. Missing/custom biome icons use a question mark.
No live-game visual review, FPS measurement or macOS/Windows run was performed.
Registry review was not repeated for this pre-release snapshot.

## 0.4.1 — 2026-11-09

Local Linux, Rust 1.95.0, unchanged pinned Horizon and Mixins revisions.

- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (37 tests; one manual benchmark ignored).
- Template integration: pass, including English regional variants, Russian,
  French and Spanish regional variants, and settings preserved across switches.
- Deterministic staging and source audit: pass; original checkout unchanged.
- Real Horizon client check and runtime tests: pass (37 tests).
- Official `cargo horizon-mod check`: pass. Repeated packaging is byte-identical.
  Source bundle: 72,483 bytes; SHA-256
  `93b92a321377227840d5adbb21a21b968b0d4d124e8f64de3bffeccf9eada6f8`.
- Player heading regression covers cardinal directions, rotating minimaps and
  a viewport with unequal horizontal and vertical scales.

No live-game review or macOS/Windows run was performed for this release.
Registry review was not repeated for this release.

## 0.4.0 — 2026-11-08

- Standalone formatting, clippy (`-D warnings`) and locked offline tests: pass,
  36 tests plus one ignored manual benchmark.
- Official `cargo horizon-mod check`, registry review and registry file check:
  pass for the 68,514-byte source bundle, SHA-256
  `5d6423a3adfe6bb7b8634d922c5569202aeb1484fff4e9e155eedaf63d53bde6`.
- Fixture, deterministic staging, real Horizon check/test: pass (36 runtime tests, one manual benchmark ignored; source audit unchanged).

New regressions cover matching coordinates in two saves and a server, independent
rename/delete, reconnect/reload, namespace and escaped identity round-trips,
per-world limits without cross-world eviction, and idempotent deletion of legacy
unscoped waypoints while retaining preferences and scoped data. Legacy deletion
is the author's requested migration policy, not a guess at world ownership.
The release also includes the GPU map work and tests described below.
No live-game review or macOS/Windows run was performed for this release.

## GPU world-map development — 2026-11-08

Local Linux, Rust 1.95.0, same pinned Horizon/Mixins revisions.

- `cargo fmt --all --check`, clippy with warnings denied, and workspace tests:
  pass (32 tests; one manual benchmark ignored by default).
- `tools/test_template.py`: pass, including the new smoothing switch and saved
  preference.
- `tools/test_stage.py`: pass, deterministic AST staging and unchanged checkout.
- Real Horizon `check` and `test`: pass (32 runtime tests, one manual benchmark
  ignored; final test stage unchanged).
- Release CPU comparison: pass; see [PERFORMANCE.md](PERFORMANCE.md). This is not
  a GPU timing or game FPS measurement.
- Source package: 66,824 bytes, SHA-256
  `1a344896aa1245c4f7f16056de6908657a0260c6c6e1a915a5a351d70a500738`.

Added regressions compare atlas sampling against the old raster at fractional
positions, multiple zooms, negative coordinates, missing chunks, transparent
pixels, walls and neighbour-based relief. They also exercise atlas reuse across
camera movement, rebuilding after terrain changes, alpha-aware LOD without
cross-slot bleeding, LOD selection, and the smoothing default/toggle/reset.
No game window or GPU frame capture was run; live appearance/FPS and macOS/Windows
remain unverified. The original checkout and player resources were not edited.

## Earlier 0.3.1 development — 2026-11-08

Local Linux checks on Rust 1.95.0 against the pinned revisions below:

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --locked --offline` | Pass: 29 tests |
| `python3 tools/test_template.py` | Pass |
| `python3 tools/test_stage.py --horizon ../horizon` | Pass: plan-only AST changes, deterministic stage |
| `python3 tools/horizon.py check --horizon ../horizon` | Pass |
| `python3 tools/horizon.py test --horizon ../horizon` | Pass: 29 runtime tests |
| `python3 tools/package.py` | Pass: 63,055 bytes, SHA-256 `94e1028027bc2eda334528dab6422ab67c9bcd9ebf43a6eae434c1b7e0724197` |

No game window was launched here, so interactive smoothness, the hover menu,
biome readout, world-border and portal overlays, waypoint naming/manager screen,
HUD letters and cross-dimension marker placement are type-checked and
unit-tested but not verified live. The runtime tests cover translucent water and
glass compositing, depth-dependent water tint, hidden ground cover, neighbouring
chunk relief shading, transparent unknown terrain, portal recording, the eased
camera, settings/waypoint naming persistence, waypoint removal and the existing
settings failure paths.

## Release 0.1.0

Date: 2026-11-08. Linux, Rust 1.95.0, Python 3.12 in GitHub Actions.
Horizon: `2ebf00a0281c592f79191e76fce0e7d522c6f425` (Nightly 2026-11-07).
Horizon Mixins: `d9c83d7096b7ea5b83678da2efdfd6161f74997e` (schema 6).

[Full validation run](https://github.com/daniliammo/Minimap-Worldmap-and-waypoints-reworked/actions/runs/42462033254)
validates implementation and tooling commit `f15951383cbaa2275b5da203218769a3bb1b2356`.
Later release documentation changes do not alter the packaged sources.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --locked --offline` | Pass: 7 runtime tests |
| `python3 tools/test_template.py` | Pass: actual hooks and runtime compiled and executed on a client fixture |
| `python3 tools/test_stage.py --horizon <pinned-checkout>` | Pass: full source audit, repeatable bytes/mtimes, original checkout unchanged |
| `python3 tools/horizon.py check --horizon <pinned-checkout>` | Pass: actual client executable and gameplay library type-check |
| `python3 tools/horizon.py test --horizon <pinned-checkout>` | Pass: all 7 runtime tests execute in the actual gameplay library |
| `python3 tools/package.py` twice | Pass: byte-identical source archives |
| Official `cargo horizon-mod check <bundle>` | Pass: native metadata and source bundle accepted |
| Local registry entry review/check and isolated index build | Pass: entry, version, hash and original icon accepted |

The current Horizon executable is a thin entry point. Native runtime tests must
run in `horizon-client --lib`; the tooling selects this target for the split
client and rejects a successful Cargo exit that ran zero passing tests.

## What is exercised

The seven runtime tests cover invalid settings, preferences round-trip, edits
made during asynchronous loading, viewport fitting, bounded notifications with
latest-state persistence, failed saves preserving prior files, and shutdown
flushing/retry. The fixture also exercises registration, seven live controls,
foreign mod IDs, disabling, server-window exclusion, another provider's occupied
minimap slot, and the original hook/exit bodies.

The source audit verifies the exact planned HEAD helpers and support module,
removes those additions, then compares normalized parsed source tokens. Optional
punctuation and layout are normalized with the transformer's formatter. Rust
files outside the planned target files remain byte-identical. Wrong revisions
and individually missing hooks are rejected before replacing the prior stage.
All game-source transformations use Horizon Mixins plan/apply.

## Artifact

`target/minimap_worldmap_waypoints_reworked-0.1.0.hnmod` — 23,422 bytes.

SHA-256: `ce92608a045f2deb146282ddd429f38ad88578f2bf1402485bbcfe3722f5e334`

The release archive is byte-identical to the successful CI artifact.

The bundle contains the five adapters, shared runtime, mixin manifest, native
metadata, GPL v3 text, upstream MIT notices, NOTICE and explicit permission for
Horizon registry reviewers to read and inspect the source. It is unsigned.

## Practical limits

Native checks use `--no-default-features`, two build jobs, no incremental
compilation and no debug information. No game window was launched; interactive
appearance, controls and live gameplay remain unverified. Windows/macOS and
combinations with other mods have not been validated. Upstream registry checks
with popular mods require maintainer staging and review.

The minimap uses Horizon's existing loaded-terrain renderer and automatic height
selection. World map, waypoints, persistent terrain history, LOD coverage,
rectangle shape and manual height layers remain planned. Existing saves,
prepared Minecraft resources and launcher installations were not changed.
