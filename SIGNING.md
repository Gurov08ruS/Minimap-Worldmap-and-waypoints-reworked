# Release verification

The mod continues from daniliammo as Gurov08ruS. The original Git history is
preserved. New commits and tags use the same GPG key with the current account's
verified committer email. A package signature does not imply review or listing
by the Horizon registry.

## Trust anchors

Original GPG fingerprint:

`9FDE42EE71F7AB70D1767C13215D4C2574068FCC`

Minisign public key (the base64 line used by Horizon's registry `key` field):

`RWQwVo88UXiKlIF0MG0YSn9MFlxcdlzoQUx7yQiXVZEciCpDzgPFW4RJ`

`signing/minimap.pub.asc` is a detached GPG signature of `signing/minimap.pub`,
made with the original key. `signing/daniliammo-gpg-public.asc` contains its public
key. Compare the fingerprint against an independently trusted historical copy;
importing a key by itself does not establish its owner's identity.

## Verify a release

Download the `.hnmod`, its `.hnmod.minisig`, and the three public key files from
the release. From the folder containing them:

```sh
gpg --import daniliammo-gpg-public.asc
gpg --fingerprint 9FDE42EE71F7AB70D1767C13215D4C2574068FCC
gpg --verify minimap.pub.asc minimap.pub
minisign -Vm minimap_worldmap_waypoints_reworked-0.5.1.hnmod -p minimap.pub
```

The `.hnmod.asc` is an additional detached GPG signature of the same archive.
It can be verified manually; Horizon checks minisign, not OpenPGP:

```sh
gpg --verify minimap_worldmap_waypoints_reworked-0.5.1.hnmod.asc minimap_worldmap_waypoints_reworked-0.5.1.hnmod
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --check SHA256SUMS
```

The signed Git tag can be checked in a clone with `git verify-tag v0.5.1`.
Horizon verifies package signatures against the key pinned in its registry.
Publishing these files does not change that registry entry or its owners.
Secret keys are never included in the repository, package or release.
