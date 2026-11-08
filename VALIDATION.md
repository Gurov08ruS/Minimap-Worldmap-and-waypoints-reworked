# Validation

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
