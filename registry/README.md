# Horizon registry assets

Horizon's client does not read the README or `assets/` from a mod repository.
The mod browser loads an icon and screenshots only from the registry index:
`icons/<id>.png` and `screenshots/<id>/*` in
[horizon-rust/horizon-mods](https://github.com/horizon-rust/horizon-mods),
listed by `mods/<id>.toml`.

Copy this folder's contents into that registry checkout, then merge
`entry-fragment.toml` into the existing
`mods/minimap_worldmap_waypoints_reworked.toml` entry. The icon is a 384×384
PNG under the 256 KiB limit; each screenshot is 1920×1080 and under 1 MiB.

After the registry build and the client's index refresh, the icon and these six
screenshots appear on the mod's page in Horizon.
