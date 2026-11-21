# Validation

## 0.7.9 — Horizon `8f83e5c` and Simplified Chinese

Windows, signed Horizon SDK `8f83e5c`, Rust 1.99.1 and Vulkan. The launcher
built and selected `native-8f83e5c-fd6027b71af4`. The packaged source, all
17 planned injections and the original SDK source hashes passed the audit.
Formatting, Clippy and the package template passed. The standalone and native
SDK suites each passed 133 runtime tests with three manual benchmarks ignored.

The main instance reached the title screen and opened the Chinese mod settings.
In a separate single-player world on the same build, the minimap followed the
player, the world map zoomed and panned, and the Chinese waypoint `森林营地`
displayed on the map and reloaded after restarting the client. The cache
manager listed compatible caches while the current world held its write lease.

Archive `target/minimap_worldmap_waypoints_reworked-0.7.9.hmod` (409,444 bytes),
SHA-256 `ce97dc2b9a910d44a803993673abd1d413aad214658587fd8634492c5ab91cb8`.

## 0.7.9 — progressive map loading and colour icons

Linux, signed SDK `0a875da`, Rust 1.99.1 and launcher 0.2.88.
Native build `native-0a875da-22680d1991ba` passed all 17 planned injections.
Formatting, Clippy with warnings denied, template validation and the
stage audit passed. Both standalone and SDK suites passed 131 runtime
tests. Three performance cases remain ignored by default.

The new regression verifies that a ready detail tile is emitted while a
neighbour is still pending. The manually run 4096-tile synthetic benchmark
with a warm OS file cache measured the first overview at 86 ms in 0.7.8
and 44 ms in 0.7.9. Reopening the visual cache took 180 ms and 91 ms,
respectively. These are worker timings from one run per version. They
do not measure GUI responsiveness or FPS. The live game was not run.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.9.hmod`,
403,175 bytes, SHA-256 `f6d296779917597edb47c43acff69ac5f3a2f2520ed057865f527e10ac5d99e9`.

## 0.7.8 — cache review fixes

Linux, signed SDK `0a875da`, Rust 1.99.1 and launcher 0.2.88.
Native build `native-0a875da-5f999b94e49c` and all 17 planned injections passed
the compiled payload audit. Formatting, Clippy with warnings denied,
package templates and stage audit passed. Standalone and SDK suites each
passed 130 runtime tests, with three performance cases ignored.

- A competing writer and preview are blocked before acquiring a cache file
  while deletion owns the namespace. A later writer creates and commits to
  the visible replacement. Old confirmation tokens reject both file and
  directory replacements.
- A live writer retries after a shared preview lease and persists samples
  gathered during the wait. Disconnect after a write error releases the
  writer's lease while keeping the original error.
- A Nether preview fixture uses a persisted `Below` layer with a saved
  portal and biome, with no surface data or LOD.
- The isolated launcher refused to start Play without a Microsoft account
  that owns Minecraft Java Edition. Live mouse and gamepad smoke for opening
  from title Options, confirming deletion and returning with Back remains
  pending. Windows/macOS execution was not verified.

The namespace lock coordinates clients running 0.7.8 or later with this
protocol. Older builds do not use that lock.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.8.hmod`,
401,690 bytes, SHA-256 `85fc0bffb34ce341d9fcf12a64d35edc84c31bfa4ec79e442b7f76a7935db86e`.

## 0.7.7 — cache map actions and waypoint teleport

Linux, signed SDK `0a875da`, Rust 1.99.1 and launcher 0.2.88.
Native build `native-0a875da-823448f0d142` and all 13 planned injections passed
the compiled payload audit. Formatting, Clippy with warnings denied, package
template validation and stage audit passed. Standalone and SDK test suites
each passed 127 runtime tests with three performance cases ignored.

Tests cover coordinate input, biome and portal data available from a read-only
preview, correct dimension filtering and preservation of cache bytes.
Live GUI and execution of teleport commands in a world remain unverified.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.7.hmod`,
394,693 bytes, SHA-256 `90b0d5555424d5f23516b44d7d23531a9f094e92828929b3287428437fa723fa`.

## 0.7.6 — shared world map renderer

Linux, signed SDK `0a875da`, Rust 1.99.1 and launcher 0.2.88.
Native build `native-0a875da-30c86744680c` passed the plan and payload audit for all
13 injections. The original Horizon checkout stayed unchanged. Formatting,
Clippy with warnings denied, workspace and SDK tests, template checks and
stage audit passed. There were 126 passing runtime tests and three ignored
performance cases in each runtime suite.

The preview regression covers saved terrain rendered with paged textures,
texture page reuse during a pan, byte-for-byte read-only viewing, and legacy
last-position records. The new zoom limit and off-screen arrow projection
are covered separately. Live GUI and frame-rate measurements were not run.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.6.hmod`,
389,028 bytes, SHA-256 `0ab23a9653028bfdd6c92276db5e0c7e84e99b1763c16c8a3be9cdc985111a8b`.

## 0.7.5 — offline cache manager

Linux, signed SDK `0a875da`, Rust 1.99.1 and launcher 0.2.88.
The isolated native build `native-0a875da-1cf8a3401a94` compiled all 13 audited
injections against the pinned export. The original Horizon checkout stayed
unchanged. Formatting, workspace Clippy with warnings denied and 125 runtime
tests pass. Three performance cases remain ignored.

The offline regression verifies last-position persistence, saved terrain
rendering, byte-for-byte preservation during preview and deletion safety.
The test exercises a second cache, a locked preview, the active-world guard,
and path traversal rejection. Live GUI, macOS and Windows execution remain
unverified.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.5.hmod`,
386,101 bytes, SHA-256 `6cf3e0b6cec8d947ef1c0abaa72c77221bbd16e6bfd088ceb994a2b73218c1b4`.

## 0.7.4 — partial public SDK migration

Linux / Rust 1.99.1 / launcher 0.2.88, signed SDK and committed source export
`0a875da91dbb0fdcb7ba8c42c11341aa229af51a`, Mixins schema 6.

- `cargo fmt --all -- --check`, Clippy with warnings denied, workspace tests
  and whitespace checks pass. Native entrypoint/adapter formatting was also applied.
- **124 runtime cases pass** standalone and inside the API SDK; three deliberate
  performance cases remain ignored. New regressions cover binding matching and
  cross-thread disconnect/reload notification coalescing.
- `tools/test_template.py`, `tools/horizon.py check/test --horizon ../horizon`
  and `tools/test_stage.py --horizon ../horizon` pass. Package validation checks
  that the four retired injections and their helpers stay out of the archive.
- Real native compilation: `native-0a875da-11b0ad1eebe0`, initial build 36 seconds.
  All **13 remaining injections** match the plan; payloads match the archive and
  only planned AST changes occur. The original Horizon checkout is unchanged.
- Public SDK callbacks now handle bindings, language, disconnect, resource reload
  and waypoint background blur. No retained host/engine references cross the
  notification boundary. Existing native frontend functionality remains included.
- No live GUI, macOS/Windows execution or remote CI run was performed. Compilation
  and runtime unit tests do not establish visual/input correctness in a live game.
- This is a partial migration with exact-build compatibility. Outstanding frontend
  conversions and missing upstream contracts are documented in SDK-MIGRATION.md;
  compatibility with a later Nightly is not claimed.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.4.hmod`, 381,069 bytes,
SHA-256 `a5ad187922213424c4629c1d14715dac8f59009b5567e5095fe27bdcd71add5c`.

## 0.7.3 — Nightly 2026-11-20 SDK port

Linux / Rust 1.99.1 / launcher 0.2.88. Published Nightly and signed SDK:
`0a875da91dbb0fdcb7ba8c42c11341aa229af51a`, Minecraft 26.3, package format 6,
Mixins `e59da90` / schema 6. SDK and game indexes were verified through the
launcher's signed local-mirror route; only committed pinned sources were exported.

- Formatting, Clippy with warnings denied and workspace tests pass.
- All **122 runtime cases** pass both standalone and inside the new API SDK;
  three intentionally ignored performance cases remain ignored.
- `tools/test_template.py`, `tools/horizon.py check/test` and `tools/test_stage.py`
  pass. All **17 injections** match the plan; compiled payloads match the archive,
  only planned AST changes occur and original Horizon checkout contents/status
  remain unchanged.
- Native result: `native-0a875da-83c7c74d495b`; initial native compilation took 53 seconds.
- Cache navigation, all five languages, HUD layers and effect-icon fixes from
  0.7.2 remain included. No live GUI, macOS or Windows run was performed for this
  port; the updated GitHub workflow has not been executed remotely.
- The SDK still requires an exact game build for `[native.*]` adapters. This
  validates this Nightly only, not automatic compatibility with future builds.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.3.hmod`, 380,862 bytes,
SHA-256 `2bbdb4f403486e2c1a02f54927a6068487072e0dc66f39ed9fe75d1ecbd2e69d`.


## 0.7.2 HUD layers, effects and cache scrollbar — 2026-11-20

Linux, Rust 1.99.1, signed Nightly SDK `5ef7734`, Mixins schema 6.

- Workspace formatting, clippy with warnings denied, tests and diff whitespace checks pass.
  Runtime: 122 passed, three deliberately ignored; source-audit test also passes.
- The same 122 runtime cases pass inside the native SDK crate. The scrollbar
  regression covers preserving different grab points, both ends of the track,
  resizing/clamped offsets and empty/non-overflowing lists.
- `tools/test_template.py`, `tools/horizon.py check/test` and `tools/test_stage.py`
  pass. All 17 injections match the plan; compiled source payloads match the
  archive; only planned AST changes occur; pinned SDK source hashes are unchanged.
- Native build: `native-5ef7734-09eea5451d84`, built in the disposable target
  instance. Player installations and resources were not modified.
- World waypoint drawing now enters `draw_extras` before corner overlays.
  Top-right map placement uses an effect-independent corner origin; the same
  map/footer height offsets effect icons and reserves room before other panels.
- Live game rendering, macOS/Windows execution and GitHub CI were not verified
  for these changes.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.2.hmod`, 380,863 bytes,
SHA-256 `aa22b5484e3d767bb5c51417e2acf194c31ecef6cd629fc0472f1818e6ba249f`.

## 0.7.2 cache-list and localization fix — 2026-11-20

Linux, Rust 1.99.1, launcher 0.2.86, signed SDK `5ef7734`, Mixins schema 6.

- Workspace formatting, Clippy with warnings denied, workspace tests and
  `python3 tools/test_template.py` pass.
- **121 runtime cases pass** in both the standalone library and the exact SDK
  mod crate; three intentionally ignored performance cases remain ignored.
- Regression coverage includes fractional smooth-wheel accumulation, boundary
  handling, non-finite input, paging and catalog resize; all 15 mixed local,
  server and legacy fixtures are reachable with three visible rows. Discovery
  leaves every fixture file byte-for-byte unchanged.
- Diagnostic translations cover all five supported languages, nested cache
  errors and OS error suffixes without changing the stored diagnostic identity.
- `tools/horizon.py check`, `tools/test_stage.py` and `tools/horizon.py test` pass:
  all 15 injections match the plan, payloads match the installed archive, only
  planned AST changes occur and the original Horizon checkout remains unchanged.
- Native build: `native-5ef7734-84eb520536a9` (initial compilation: 87 seconds).
- `examples/sdk_fixture.rs` prepares 14 cache-list entries only inside the
  disposable target instance. Live GUI validation was not completed: managed
  launch requires Launcher Play, and this isolated launcher home has no signed-in
  Microsoft account. Player accounts and resources were left unchanged.
- macOS/Windows execution and GitHub CI remain unverified for this fix.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.2.hmod`, 378,645 bytes,
SHA-256 `ca93da3af9f45c8bb9702720444a6b6d39244dd0b2ba6d690dbb771ddeafdd12`.


## 0.7.1 SDK port — 2026-11-20

Target: published Nightly `2026-11-19 · 5ef7734fdd854add5efd6817178be3c988d8d050`,
Minecraft 26.3, Rust 1.99.1, package format 6, Mixins `e59da90` / schema 6.

- Standalone workspace: fmt, clippy with warnings denied, and tests pass
  (118 runtime cases, three intentionally ignored cases, plus the audit tests).
- The same 118 runtime cases pass against the new build's fetched API SDK.
- `python3 tools/test_template.py`: deterministic source archive, exact payloads,
  five language catalogs and isolated install/legacy-backup fixture pass.
- Full installed `.hmod` native build passes with launcher 0.2.86, using the
  signed SDK's local-mirror route (all three SDK archive SHA-256 values verified).
  Result: `native-5ef7734-8434ff4c9a53`; initial compilation took 93 seconds.
- `python3 tools/horizon.py check`, `python3 tools/test_stage.py` and
  `python3 tools/horizon.py test` pass against the pinned committed source export.
  All 15 Mixins injections match the plan; source payloads match the archive and
  only planned AST changes occur. The original Horizon checkout is unchanged.
- Clean-project `cargo horizon-mod hooks minimap` fetches and verifies the new
  API SDK from the official signed archives through the local mirror route.
- No live game, macOS or Windows execution has been performed for this port.

The API-test tooling now checks both the SDK index commit and its completed-build
stamp before reusing generated SDK sources, instead of retaining the previous
pin's API. SDK changes are fetched by the official package tool.

Archive: `target/minimap_worldmap_waypoints_reworked-0.7.1.hmod`, 371,178 bytes,
SHA-256 `6db009a832bdd13d790f6fd5345ddab631c85dee4d40669d4956e6bdfd2d2130`.
CI was updated for launcher 0.2.87 but has not been executed on GitHub for this port.


## 0.7.0 SDK migration — 2026-11-18

Local Linux / Rust 1.99.1, signed SDK `0b14f8a`, Mixins `e59da90`, launcher
0.2.78. Historical 0.6.1 artifacts remain unchanged.

- Standalone workspace fmt, clippy with warnings denied and tests pass: **118**
  runtime cases, one source-audit case; three manual benchmarks ignored.
- The same **118** runtime cases pass inside the SDK mod crate against its fetched
  API source, with three benchmarks ignored. Regression coverage includes SDK
  choice persistence, explicit defaults during async loading, labels, cache
  compatibility and colour revisions.
- Official `.hmod` check passes. Packaging is deterministic and every payload
  matches its source. The installer fixture preserves unrelated mods/bindings
  and retains the retired archive outside the launcher's mod filter.
- The launcher builds the actual installed archive against the signed prebuilt
  engine. All **15** declared injections compile. The Mixins plan/apply audit
  restores the original generated function mirrors, and pinned source hashes
  and original checkout snapshots remain unchanged. The SDK's generated native
  entry appended to the mod library is checked separately.
- A real disposable SDK-test world was opened using the user's installed Nightly
  and prepared resources. The live map/LOD view and Russian overlays were captured;
  GUI entry subscriptions were corrected after the initial invisible-overlay run.
  The user also confirmed that the migrated mod works in this live instance.
  These are functional checks, not a controlled performance benchmark.
- Five base SDK language catalogs and their regional aliases ship with the mod.
  The original shared data directory is retained; testing writes only its owned
  instance/cache state, not existing player worlds.
- Rust 1.95 was removed after validation on 1.99.1. Toolchain pins and CI now use
  1.99.1. The updated GitHub workflows have not been executed remotely yet.

Release archive: `minimap_worldmap_waypoints_reworked-0.7.0.hmod`, **371,179 bytes**,
SHA-256 `e467da230b56a6a3e04ff284d822a215c2882ca0955f5a62283190e37a4501e0`.
Both author signatures verify, and the pinned official package checker reports
`signed: true`. Final template, native build/AST staging and SDK runtime tests
were repeated on these exact archive bytes before publication.

The old 18 renderer/UI and nine binding-widget fixtures are archived under
`tests/legacy_adapter/`; they are not modern SDK passes. macOS/Windows execution,
combinations with other mods and exhaustive visual biome/FPS checks remain
unverified. The optional upstream catalog-discovery command failed its path
normalizer; the validated normal build route does not use it. See
[FORMAT_MIGRATION.md](FORMAT_MIGRATION.md).

## Pre-migration development on the legacy pin — 2026-11-18

Development sources on top of `bb99f45`; the published 0.6.1 artifact below is
unchanged. Validation uses Linux/Rust 1.95.0 and the same pinned Horizon
`9f42064` / Mixins `d9c83d7` sources.

- Workspace fmt, clippy with warnings denied and locked offline tests pass:
  115 runtime tests plus one source-audit test, with three benchmarks ignored.
  Changed injected sources pass direct rustfmt checks.
- Final native client check and 115 injected runtime tests pass; all 18 client
  adapter/UI tests and nine binding UI tests pass. At that stage, module payloads
  matched the then-current sources.
- Added regressions verify bounded read-only catalog discovery, recovering the
  second checked header, asynchronous name resolution, refusing busy/missing/
  uninitialized/mismatched files, preservation of future formats and canonical
  terrain, durable upgrade offers, and image rebuild resumption after reopen.
- Colour regressions cover cold natural surfaces/canopies, preserving supplied
  ice/snow/artificial colours, shared water opacity, water without a known floor,
  one-time palette upgrades, partial fallback coverage and later cell updates.
- Native UI fixtures check small viewport layout, bounded icon input, biome
  climate at low/high elevation and fixed-world-radius mangrove water blending.
  The biome test uses generated fixture data, not a live-world visual capture.
- Waypoint-label regressions cover Cyrillic initials, similar/different colours,
  world/dimension separation, skull exclusion, colour changes, renames, removal,
  preference persistence/merging/reset and the 3D popup's expanded-marker gap.
- Template integration exercises the new settings-button dispatch. Deterministic
  AST staging audits the original checkout and changes only planned ASTs.

That earlier run had no live-game screenshot/GPU/FPS run, macOS/Windows execution
or newer-Horizon compatibility validation. Snow/ice inferred from incomplete LOD
is approximate. Existing historical colours can only be refreshed where new
Horizon samples are available; canonical detailed terrain remains authoritative.
Compression, deletion and canonical-format conversion are not implemented.
These earlier results apply only to the pinned legacy native integration.
The modern SDK validation is recorded above.

## 0.6.1 — distant LOD and visual streaming, 2026-11-17

Local Linux validation with Rust 1.95.0, pinned Horizon `9f42064` and Mixins
`d9c83d7`:

- Workspace fmt, clippy with warnings denied and locked offline tests pass:
  103 runtime tests and one source-audit test; three manual benchmarks ignored.
  Direct rustfmt checks pass for the changed injected client/LOD/test sources.
- Final native client check, template integration and deterministic AST staging
  pass. The original pinned Horizon checkout remains unchanged; only planned
  AST changes are present in the ignored export.
- Official registry review against the previous PR head reports no errors and
  one new version. Its package/signature check passes. A focused index build
  validates all four version signatures and all seven gallery images; this is
  not a full upstream registry build or a newest-Nightly compatibility claim.
- Runtime tests inside the pinned native client pass (103, three benchmarks
  ignored). All 15 separate client adapter tests and nine binding UI tests pass.
- Regressions cover snapshot dedup after off-screen RAM eviction, visible coarse
  LOD priority, coverage-aware reduction and known void, negative coordinates,
  persisted-image reopen without canonical payload loading, interrupted dirty
  checkpoints, older-writer generation mismatch, damaged derived-image repair,
  target-zoom prefetch, metadata probes without atlas invalidation, atlas
  assembly without disk reads and ready-page coverage after payload eviction.
- Packaging twice produces identical bytes. Every archived Rust/TOML source
  matches the working tree. Official package check reports `ok: true`,
  `signed: true`; minisign and detached GPG verification pass.
- Archive: `minimap_worldmap_waypoints_reworked-0.6.1.hnmod`, 168,781 bytes;
  SHA-256 `bc28471909a02514f81b035871bf6c4b774c361ae26330e57494b3aa6c9d4ed5`.
  Signatures use the existing minisign key and original GPG key.
- The supplied long-distance screenshot is 2560×1440, 884,773 bytes, copied
  unchanged into the registry assets and referenced in README and the gallery.
  It is an illustration, not evidence of measured frame pacing in this release.

The release-mode persistent-zoom fixture (4096 canonical tiles, four LOD sources)
measured 8.354 ms first preview, 256.126 ms initial backfill, 8.176 ms reopen,
4.693 ms target prefetch and 0.850 ms zoom-frame assembly, using two pages.
Reproduce with
`cargo test --release --locked --offline benchmark_persistent_zoom -- --ignored --nocapture`.
These synthetic CPU timings use warm OS file cache and exclude GPU execution,
real cold-disk latency and FPS. No live-game performance capture or macOS/Windows
execution was performed. Existing cache migration/backfill is asynchronous;
unavailable game terrain and slow disk operations can still delay new detail.

Older sections below describe their own source snapshots and limits at the time;
they are not claims about the current streaming path or release archive.

## Incremental map pages and worker pipeline — 2026-11-16

Development validation on Linux, pinned Horizon `9f42064` and Mixins `d9c83d7`:

- Workspace fmt, clippy with warnings denied and locked offline tests: pass
  (95 runtime tests, one source-audit test; two manual CPU benchmarks ignored).
  Injected client/page support sources also pass direct rustfmt checks.
- Template integration and deterministic AST staging/source audit: pass. The
  original sparse Horizon checkout remains unchanged; new HUD source hashes
  are checked against the pinned export, without materializing checkout files.
- Final native client check and runtime tests: pass (95 passed, two manual
  benchmarks ignored). Separate native filters: all 13 client adapter
  tests and all nine binding UI tests pass, including ordered picture runs and
  release of frame references.
- Coverage includes stable slot positions, dirty relief neighbours, GPU pages
  beyond 4096 tiles, unchanged page identity, disk versions across commit and
  RAM eviction, unreadable detail retaining valid LOD, bounded predictive
  coverage, stale sessions and camera publication during blocked sampling.
- Official package check: pass for the unsigned development
  `target/minimap_worldmap_waypoints_reworked-0.6.0.hnmod`, 157,773 bytes; SHA-256
  `cc515ab1edc1e4110aa879435b10271a4c5b7af22d88d51255bfef1dc0864f5f`.
  The archive includes the new public GUI page support module and matches the
  Rust/TOML sources at that validation. The manifest still says 0.6.0; this is not a signed
  release or a registry update.

The optimized 8192-tile fixture measured 45.250 ms for initial preparation,
10.230 ms for unchanged reuse and 12.454 ms for one edit (three shaded tiles,
one replaced page). Concurrent background checks were active. These are CPU
diagnostics, not controlled GPU/FPS measurements. Reproduce with
`cargo test --release --locked --offline benchmark_incremental_atlas_pages -- --ignored --nocapture`.
The profiling log summarizer was exercised with real fixture output and checked
for additive counts/bytes and worst-window percentiles, without averaging p95.

Native checks use `CARGO_INCREMENTAL=0`. No live-game GPU capture, real cold-disk
latency profile or macOS/Windows run was performed. Visual admission is bounded;
oversized views retain raster fallback. Full disk-index metadata/rewrite costs,
persistent overview and container management remain subsequent work, as listed
in PERFORMANCE.md. Schema 2 bytes and player resources are unchanged.

## Test source separation — 2026-11-16

Validated on Linux against pinned Horizon `9f42064` and Mixins `d9c83d7`:

- `cargo fmt --all --check`, workspace clippy with warnings denied, and locked
  offline workspace tests: pass (88 runtime tests, one source-audit test,
  one manual benchmark ignored).
- Template integration and full deterministic AST staging: pass. Repeated
  staging preserves bytes and mtimes; the original Horizon checkout is unchanged.
- Native client check and runtime tests: pass (88 passed, one manual benchmark
  ignored). Separate native test filters exercise all 12 client rendering tests
  and all nine binding UI tests: pass.
- Official package check: pass for the unsigned development package
  `target/minimap_worldmap_waypoints_reworked-0.6.0.hnmod` (147,219 bytes; SHA-256
  `ed932e48f02b69c30d3e5bf53b21bb7378c52ea0c98c95affefe35bd79447864`).
  All archived Rust and TOML files match the sources at that validation,
  including the six test support modules. This is not a new signed release.

Native checks used `CARGO_INCREMENTAL=0`. No live-game or macOS/Windows
validation was performed for this refactor.

## Unreleased rendering performance — 2026-11-14

Development changes against pinned Horizon `9f42064` and Mixins `d9c83d7`:

- Formatting, clippy with warnings denied and locked offline workspace tests:
  pass (88 runtime tests, one source-audit test, one manual benchmark ignored).
- Template integration and deterministic AST staging: pass; the original
  checkout is unchanged. Native client check: pass. Final runtime tests
  inside the pinned client: 88 passed, one benchmark ignored.
- Regression coverage includes rotated minimap UVs/clipping, texture reuse while
  rotating/following, reconstruction from partial uploads, ordered LRU eviction,
  disk LOD cursor reuse, paging under limited RAM and more than 4,096 entries.
- Official package check: pass for the separate, unsigned development artifact
  `target/minimap_worldmap_waypoints_reworked-0.6.0-performance-dev.hnmod`.
  Size 141,310 bytes; SHA-256
  `6410547ccdfbcdd31f33b1eb3108a310e6bf962aa05ce546dd880b08cf3e7f50`.
  The archived runtime and adapter match the sources at that validation. It
  retains the 0.6.0 manifest version and is not the signed release documented below.

Full native test compilation initially exhausted local disk space; the final
successful run disabled incremental compilation. No live-game GPU/FPS or
macOS/Windows validation was performed. Persistent overview levels, GPU paging
beyond the shared slot and removal of the oversized-view CPU fallback are still
outstanding; this is not completion of the large-history design.

## 0.6.0 — signed release validation, 2026-11-14

Validated on Linux with Rust 1.95.0 against the unchanged Horizon Nightly
2026-11-13 (`9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`) and Horizon Mixins
`d9c83d7096b7ea5b83678da2efdfd6161f74997e` (schema 6).

- Formatting and clippy with warnings denied: pass.
- Locked offline workspace tests: 83 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- Template integration, full source audit and deterministic AST staging: pass;
  the original Horizon checkout and its existing changes are preserved.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 83 passed, one benchmark ignored.
- Packaging twice produces identical bytes and SHA-256.
- Official `cargo horizon-mod check` with the pinned minisign public key and
  `.minisig`: `ok: true`, `signed: true`.
- GPG signatures of the package, SHA256SUMS and minisign public key: verified
  using the original key `9FDE42EE71F7AB70D1767C13215D4C2574068FCC`.
- Artifact: `minimap_worldmap_waypoints_reworked-0.6.0.hnmod`, 137,165 bytes.
  SHA-256: `7892716e30548682f85ea907d41c784d7722bbc65d3ddf53b62574aa49e95a7d`.

No new live-game visual review, FPS profiling or macOS/Windows run was performed.
Publishing the release does not change the Horizon registry entry or its owners.

## 0.6.0 pre-release — 3D-waypoint labels on hover, 2026-11-14

Validated on Linux with Rust 1.95.0 against the unchanged Horizon Nightly
2026-11-13 (`9f42064`) and Horizon Mixins `d9c83d7` pins.

- A 3D waypoint's label is drawn only while the crosshair (the screen centre in
  the 3D view) is over its marker, when **3D labels only on hover** is on
  (default). The toggle restores always-on labels.
- `cargo fmt --all --check` and clippy with warnings denied: pass; injected
  adapter/helper files are rustfmt-clean.
- Locked offline workspace tests: 82 runtime tests and one source-audit test
  passed; one manual benchmark ignored. The new `labels_on_hover` setting has a
  round-trip/toggle/reset regression.
- `python3 tools/test_template.py` and `python3 tools/test_stage.py`: pass, with
  the settings list now 19 rows.
- Actual pinned Horizon client check: pass; runtime tests inside the pinned
  client: 82 passed, one benchmark ignored.

No live-game launch was performed; the hover check is type-checked and unit
tested, not reviewed interactively.

## 0.6.0 pre-release — 3D-waypoint camera fix, 2026-11-13

Validated on Linux with Rust 1.95.0 against the unchanged Horizon Nightly
2026-11-13 (`9f42064`) and Horizon Mixins `d9c83d7` pins.

- The 3D markers were never drawn because `native_camera` returns `None` for an
  injection-only mod (it is computed only when a trait mod is registered in
  `mod_api::mods`). `world_camera` reads the same camera the renderer uses:
  `view.position`, `view.y_rot`/`x_rot`, the bob and spin matrices, and
  `fov_degrees` (so the marker follows the FOV/zoom setting). It is rebuilt from
  the client's own view state, so it renders through walls without a trait mod.
- `cargo fmt --all --check` and clippy with warnings denied: pass; the injected
  adapter/helper files are also rustfmt-clean.
- Locked offline workspace tests: 81 runtime tests and one source-audit test
  passed; one manual benchmark ignored. New client-side regressions project a
  waypoint onto the screen, check that a narrower FOV moves it further from the
  centre, and check that the projection lands at the same relative position at
  1920×1080 and 3840×2160.
- `python3 tools/test_template.py` and `python3 tools/test_stage.py`: pass.
- Actual pinned Horizon client check: pass; runtime tests inside the pinned
  client: 81 passed, one benchmark ignored.

No live-game launch was performed; the projection is verified by unit tests, not
an interactive session.

## 0.6.0 pre-release — map direction letters, 2026-11-13

Validated on Linux with Rust 1.95.0 against the unchanged Horizon Nightly
2026-11-13 (`9f42064`) and Horizon Mixins `d9c83d7` pins.

- The eight direction labels are drawn at the inner edge of both maps as opaque
  HUD text over a dark backing, rotated by the map's angle so the north marker
  tracks true north. The large map keeps them clear of the dimension tabs and
  the coordinate footer.
- `cargo fmt --all --check` and clippy with warnings denied: pass.
- Locked offline workspace tests: 81 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- New regressions cover the `directions`/`direction_style` settings round-trip,
  toggle and reset, and the widened edit mask (`touched` is now `u32`).
- `python3 tools/test_template.py`: pass, with the settings list now 18 rows.
- `python3 tools/test_stage.py --horizon ../horizon`: pass; original checkout
  unchanged, only planned ASTs differ.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 81 passed, one benchmark ignored.

No live-game launch or visual review of the compass letters (including their
rotation on a rotating minimap) was performed; placement is exercised by the
type-checked renderer, not an interactive session.

## 0.6.0 pre-release — 3D-world waypoint markers and per-point visibility, 2026-11-13

Validated on Linux with Rust 1.95.0 against the unchanged Horizon Nightly
2026-11-13 (`9f42064`) and Horizon Mixins `d9c83d7` pins.

- 3D-world markers are drawn in the HUD overlay pass, so they render through
  walls on the GPU (flat-colour quads plus the cached font atlas) without a
  per-frame image upload. The label is composed only when the selected style
  asks for it.
- `cargo fmt --all --check` and clippy with warnings denied: pass.
- Locked offline workspace tests: 80 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- New regressions cover the `world_waypoints`/`world_waypoint_style` settings
  round-trip, toggle and reset; the `waypoint_v5` `show_3d` field round-trip;
  legacy `v2`–`v4` records defaulting to visible; and per-point 3D toggling
  persisting across a controller reopen.
- `python3 tools/test_template.py`: pass, with the settings list now 16 rows.
- `python3 tools/test_stage.py --horizon ../horizon`: pass; original checkout
  unchanged, only planned ASTs differ.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 80 passed, one benchmark ignored.

No live-game launch, visual review of the 3D markers or macOS/Windows run was
performed. The marker style and per-point toggles are exercised by unit tests
and the injected-client build, not by an interactive session.

## 0.6.0 pre-release — Horizon Nightly 2026-11-13 compatibility, 2026-11-13

Validated on Linux with Rust 1.95.0 against Horizon Nightly 2026-11-13
(`9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0`) and the unchanged Horizon Mixins
`d9c83d7096b7ea5b83678da2efdfd6161f74997e` (schema 6).

- `compatibility.json` and the bundle manifest now pin `9f42064`; the
  `source_sha256` entries were regenerated for `game.rs`, `native_hooks.rs`,
  `options_ui.rs`, `lod.rs` and `ui.rs`.
- Injection targets follow Horizon's module split: the application `exiting`
  hook is in `game::window_handler`, `forget_atlas_slots` is in
  `game::render_settings`, and `native_rows` is in `options_ui_pages`. The
  focused fixture in `tools/test_template.py` mirrors that layout.
- `cargo fmt --all --check` and clippy with warnings denied: pass.
- Locked offline workspace tests: 77 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- `python3 tools/test_template.py`: pass, including actual hooks and runtime on
  the client fixture, deterministic staging and missing-target rejection.
- `python3 tools/test_stage.py --horizon ../horizon`: pass; the original
  checkout is unchanged and only planned ASTs differ.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 77 passed, one benchmark ignored.
- `python3 tools/package.py` twice: byte-identical, 130,510 bytes, SHA-256
  `15b866a8925aabf41bbcdfafb3e7809d36318486030b12ac2c881aa41f259565`.
- Official `cargo horizon-mod check`: accepted, reporting
  `native-1/9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0/d9c83d7...` for 26.3.
  The package is unsigned and is a local development artifact.

No live-game launch, FPS profiling, macOS/Windows run or new gameplay check was
performed for this compatibility update. The launcher was not driven here; the
supported claim is the pinned-source build and the checks above.

## 0.6.0 pre-release — persistent map container, 2026-11-12

Validated on Linux with Rust 1.95.0. Horizon and Mixins revisions are unchanged;
`compatibility.json` additionally checks the pinned renderer `ui.rs` because the
RAM slider uses a new declarative HEAD hook there.

- `cargo fmt --all --check`, clippy with warnings denied, and `git diff --check`:
  pass. Injected adapter/helper files were also formatted with Rustfmt.
- Locked offline workspace tests: 75 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- New regressions cover disk reopen, multiple index pages, record checksums,
  unknown schema and mismatched identity rejection, safe reuse of old extents,
  short writes in data/index/root phases, retained pending bytes after failure,
  and sampling pause. Unknown world identities are verified to use RAM without
  creating an unscoped cache directory. Process tests cover simultaneous first creation, busy,
  independent worlds, abrupt owner death and reacquisition.
- Service tests flush on dimension change, verify that the old tile's Weak
  reference expires, reopen a fresh service from the disk file and recover the
  same terrain. Separate cases preserve layer Y, LOD, biomes and portals, and
  page a wide view without exceeding the synthetic payload budget.
- Template integration and deterministic AST staging/source audit: pass;
  the original Horizon checkout and its existing changes were preserved.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 75 passed, one benchmark ignored.
- The actual renderer's cache-slider mouse/drag/release test: one passed.
- Source package passes official `cargo horizon-mod check`; it is an unsigned
  local development artifact, not a published release.
  Final archive: 130006 bytes; SHA-256
  `dd209a47b1faf722fa47989005faa6410dd8a7e98fbb0f33f17f773f9448ace6`.
  All 29 bundled source/manifest files match the working tree.

The RAM setting budgets terrain payloads, the read LRU and pending record bytes;
index metadata, input snapshots and current display/GPU snapshots are extra.
The index currently materializes both roots' key/reference metadata and rewrites
its pages at commit. Very large cache metadata, slow-disk latency and live FPS
have not been profiled. Raw codec 0 is implemented; Zstd, transactional manager
operations, compaction, full vertical pages and physical unlink are not claimed.
No live game window, power-loss hardware test, macOS/Windows run or network-FS
qualification was performed. See `CONTAINER.md` for the format and limits.

## 0.6.0 pre-release — death waypoints and persistent biome samples, 2026-11-12

Validated on Linux with Rust 1.95.0 and the unchanged compatibility pins.

- Formatting and clippy with warnings denied: pass.
- Locked offline workspace tests: 59 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- New regressions cover death-waypoint settings and reset, numbered black skull
  points, rejection of duplicates, three-dimensional reach, and biome samples
  surviving a controller reopen. Older `v2`/`v3` records load as letter points
  without a biome.
- Template integration, full source audit and deterministic AST staging: pass;
  the original Horizon checkout and its existing changes are preserved.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 59 passed, one benchmark ignored;
  injected client adapter tests: 10 passed.
- Live death, respawn and proximity pickup were not exercised in-game. Visual
  review of the skull marker, FPS profiling and macOS/Windows runs were not
  performed.

## 0.5.1 — signed release validation, 2026-11-11

Validated on Linux with Rust 1.95.0 and the unchanged compatibility pins.

- Formatting and clippy with warnings denied: pass.
- Locked offline workspace tests: 55 runtime tests and one source-audit test
  passed; one manual benchmark ignored.
- Template integration, full source audit and deterministic AST staging: pass;
  the original Horizon checkout and its existing changes are preserved.
- Actual pinned Horizon client check: pass.
- Runtime tests inside the pinned client: 55 passed, one benchmark ignored.
- Packaging twice produces identical bytes and SHA-256.
- Official `cargo horizon-mod check` with the pinned minisign public key and
  `.minisig`: `ok: true`, `signed: true`.
- GPG signatures of the package, SHA256SUMS and minisign public key: verified
  using the original key `9FDE42EE71F7AB70D1767C13215D4C2574068FCC`.
- Artifact: `minimap_worldmap_waypoints_reworked-0.5.1.hnmod`, 106,415 bytes.
  SHA-256: `12420822b03b9584cc970eb3ae7e4924eb9b07b1c3f5df4c9a48614072913ad7`.

No new live-game visual review, FPS profiling or macOS/Windows run was performed.
Publishing the release does not change the Horizon registry entry or its owners.


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
