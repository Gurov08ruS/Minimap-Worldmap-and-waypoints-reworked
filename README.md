# Minimap, Worldmap and waypoints reworked

A native Horizon mod by [daniliammo](https://github.com/daniliammo).
Version **0.1.0** provides a configurable minimap using Horizon's terrain renderer.
**World map and waypoints are planned and are not included in this release.**

## Features

- Place the minimap in any of the four screen corners.
- Choose square or round shape, and fixed orientation or player rotation.
- Select sizes from 64 to 256 GUI pixels (128 by default).
- Select 0.1, 0.25, 0.5, 0.75, 1, 2 or 4 blocks per pixel.
- Fit the map to the available screen space without changing saved preferences.
- Save settings asynchronously, with pending writes flushed on normal exit.
- Leave an existing mod's occupied minimap slot alone.

The map shows ordinary loaded terrain. Persistent terrain history, distant LOD
coverage, manual height layers, a world map and waypoints are not implemented.
The settings labels are currently in Russian.

## Install and configure

Download the `.hnmod` from [Releases](https://github.com/daniliammo/Minimap-Worldmap-and-waypoints-reworked/releases).
For a local install, place it in the instance's `horizon-native-mods/` folder
and build the native game through a Horizon launcher with native-mod support.
After registry approval it will also be available under **Mods → Horizon**.
In the game, use **Options → Mods** to enable the mod and open its settings.
Disable other minimap providers if this map does not appear.

This is native source code which the launcher compiles into the game and which
runs with the player's permissions. It reads and writes only its local settings
file, `settings.ini`, under the mod data directory supplied by Horizon. The mod
does not make network requests. No game executable or Minecraft assets are shipped.

## Compatibility

- Minecraft Java Edition **26.3**, through Horizon.
- Horizon Nightly **2026-11-07**, commit
  `2ebf00a0281c592f79191e76fce0e7d522c6f425`.
- Horizon Mixins `d9c83d7096b7ea5b83678da2efdfd6161f74997e` (schema 6).

These revisions are pinned in [compatibility.json](compatibility.json) and the
mixin manifest. Other revisions and combinations with other mods are not claimed
compatible. See [VALIDATION.md](VALIDATION.md) for check results and practical
limits. Interactive gameplay and Windows/macOS have not been verified.

## Build and validate

Use Rust **1.95.0**, Python **3.12+**, Git, and separate Horizon and Horizon Mixins
checkouts. Python tooling currently requires Linux. `HORIZON_MIXINS` may point to
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
`target/minimap_worldmap_waypoints_reworked-0.1.0.hnmod`.
Check it with `cargo horizon-mod check <file>` using Horizon's official
`cargo-horizon-mod` tool. The [validation workflow](.github/workflows/validate.yml)
runs the standalone tests, source audit, real client check and tests, and official
package check on the pinned source. It uploads the bundle and check logs.

## License and attribution

[GPL-3.0-only](LICENSE). The MIT-licensed Horizon native mod template and its
Veinminer-derived build tooling are credited in [NOTICE](NOTICE), with their
original permissions preserved in [LICENSE-MIT](LICENSE-MIT).
[Permission for Horizon registry review](NOTICE-REVIEW-PERMISSION.md) accompanies
the distributed sources. This project is not affiliated with Mojang or Microsoft.
