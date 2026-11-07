# Validation — first minimap implementation

Date: 2026-11-07. Linux, Rust 1.95.0. Horizon and horizon-mixins revisions are
pinned in `compatibility.json`; the runtime and five native adapters are the
actual sources shipped in the `.hnmod`.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass |
| `cargo test --workspace --locked --offline` | 7 runtime tests pass |
| `python3 tools/test_template.py` | Pass: actual bundle compiled and executed on the small client fixture |
| `python3 tools/test_stage.py --horizon /tmp/minimap-horizon-validation` | Pass: full pinned export, five HEAD hooks and runtime module audited; repeatable source bytes/mtimes; original checkout unchanged |
| `python3 tools/horizon.py check --horizon /tmp/minimap-horizon-validation --target-dir /tmp/minimap-horizon-cargo` | Pass: real `horizon-client` binary type-checks without warnings |
| `python3 tools/horizon.py test --horizon /tmp/minimap-horizon-validation --target-dir /tmp/minimap-horizon-cargo` | Pass: all 7 runtime tests run inside the real client test binary |
| `python3 tools/package.py` twice | Pass: identical archive SHA-256; CRC and packaged licenses checked |

Native builds used `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0` to limit memory and
artifact size. The final native check took about 10 seconds in Cargo, reusing
cached dependencies; the native test build/run took about 128 seconds. These
are observed check times, not gameplay performance measurements.

The full-source audit used a separate detached worktree at the pinned commit.
The existing `/home/danil/horizon` checkout contained unrelated work and was
changing concurrently; its initial global-status comparison was unsuitable for
a stable audit. No unrelated source changes were reverted. The isolated
worktree audit passed, including unchanged target sources and Git status.

## What the checks exercise

- Settings parser: invalid sizes, NaN zoom and malformed boolean/shape values
  retain valid defaults; valid preferences round-trip.
- An edit made while preferences load preserves unrelated saved fields.
- A bounded notification queue persists the newest state after a burst of edits.
- Normal shutdown drains pending saves; a failed last revision is retried once
  at shutdown if a transient error has cleared. Failed saves remain visible and
  do not overwrite an unrelated file.
- Viewport fitting retains the requested size and suppresses the map when
  Horizon's minimum size cannot fit.
- Actual native hooks register one mod, populate seven live controls, ignore
  foreign mod IDs, apply settings, honour global/per-map disabling, skip server
  games, preserve another mod's occupied minimap slot, and flush on exit.
- Original hook/exit bodies continue to execute. The AST audit removes only
  the exact planned helpers and support module, then compares original ASTs;
  Rust sources outside the declared target files remain byte-identical.
- Wrong Horizon revisions and each individually missing hook are rejected;
  failed transformations leave the prior stage unchanged.

## Artifact

`target/minimap_worldmap_waypoints_reworked-0.1.0.hnmod`

SHA-256: `f04fab80eda9ac1f5046157ed540d3a744bdfe7e45fc2bcc0ed825d67ab7772e`

The archive contains the five adapters, shared runtime, mixin manifest, native
metadata, GPL v3 text, preserved upstream MIT notices, and NOTICE.

## Practical limits

The current minimap uses Horizon's existing renderer and its automatic
Nether/covered-player height selection. It draws ordinary loaded terrain, not
this project's future LOD/shared persistent world cache. Rectangle shape,
manual height, stored height layers, world map and waypoints remain planned.

No interactive game window was launched. GUI appearance, controller navigation
and live gameplay still need visual verification with prepared Minecraft
resources and the matching Horizon version. No existing saves, resources,
launcher installation or published registry entry were changed.
