# Validation

## 0.3.0 development — 2026-11-08

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
| `python3 tools/package.py` | Pass: 62,929 bytes, SHA-256 `573bc00c9a9396ca3cfbcde0d6624deee65a6e651c1ea79c61504d03144ab54d` |

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
