# Modern native SDK migration — 0.7.0 development

The mod now builds as a flat `.hmod`: `horizon-mod.toml`, `Cargo.toml`, `src/`,
its own icon and language assets, plus licenses/notices. No legacy
`horizon-native.json`, mod-id wrapper directory, game binaries, exported game
sources, SDK or Minecraft resources are packaged.

The exact target is the signed SDK
[`0b14f8acfd97fa2c750e42795249dcc981178ca4`](https://github.com/horizon-rust/horizon/releases/tag/sdk-0b14f8acfd97fa2c750e42795249dcc981178ca4),
Nightly 2026-11-18, Rust 1.99.1, package format 6 and Mixins
`e59da9094f0c2fdda6c627fe9eff22dec6a906f0` with the SDK's authenticated patch.
The initial claim that no usable SDK was published was incorrect: checks of the
older Nightly and another main commit did not establish availability for this
published build. The matching SDK has since been fetched and authenticated by
the current official package tool and launcher.

## Implementation

- The SDK crate implements `Hooks` and registers settings and key mappings.
  Map/waypoint controls and the RAM slider use the SDK UI; the old renderer widget
  patches are retired. SDK subscriptions keep drawing/input entry points active
  for the function-copy adapters.
- The shared runtime lives in the mod's own crate, rather than an injected copy,
  so callbacks, persistence and terrain workers use one controller.
- Four game support modules provide the map adapter, GUI pictures, read-only LOD
  selection and SDK compatibility helpers. Fifteen HEAD injections are applied
  to SDK-indexed functions through the launcher's Mixins plan/apply route.
- The SDK's staged native metadata widens reachable renderer fields. An actual
  compiled probe confirmed that ready immutable LOD snapshots remain accessible;
  `shape_only` in the wasm-facing catalog was not evidence of a native blocker.
  The mod retains existing far samples and bounded immutable chunk snapshots.
- Text uses the SDK's explicit ARGB semantics; sprite tint and GUI scale follow
  the new signatures. Map view requests use the same GUI scale as the real HUD.
- Existing preferences supply SDK defaults, including regional UI translations.
  Settings synchronize with map controls and retain edits made during async
  loading. The data directory and canonical terrain container remain unchanged.
  Unidentified local memory connections remain transient instead of sharing one
  persistent `server:local` cache across unrelated worlds.

## Validation boundary

See [VALIDATION.md](VALIDATION.md) for the current results. Normal native builds,
package checks, AST audit, shared tests in both crates and live disposable-world
checks use the installed Nightly. This is a local development build, not a new
published release or a claim about future Nightlys or untested operating systems.

The optional upstream `native-build --catalog` discovery route was attempted and
refused a published source path (`E_INVALID_PATH`, `../../creatures/biome_facts.rs`).
The normal signed-SDK function-copy build works and is what the current tools
validate; the failed discovery attempt is not reported as a passing check.
