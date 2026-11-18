#!/usr/bin/env python3
"""Assemble the flat, source-only .hmod project for Horizon's signed SDK route."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / 'mixins/minimap_worldmap_waypoints_reworked'
TESTS = {
    'minimap_settings_tests.rs': 'settings_tests.rs',
    'minimap_container_tests.rs': 'container_tests.rs',
    'minimap_maps_tests.rs': 'maps_tests.rs',
    'minimap_map_regression_tests.rs': 'map_regression_tests.rs',
}


def inputs():
    manifest = tomllib.loads((ROOT / 'horizon-mod.toml').read_text())
    cargo = tomllib.loads((ROOT / 'native/Cargo.toml.in').read_text())
    core = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    compat = json.loads((ROOT / 'compatibility.json').read_text())
    if manifest['version'] != cargo['package']['version'] or manifest['version'] != core['package']['version']:
        raise ValueError('mod, native Cargo and standalone Cargo versions must agree')
    native = manifest['native']
    if (native['horizon'], native['mixins']) != (compat['horizon_revision'], compat['horizon_mixins_revision']):
        raise ValueError('manifest and SDK compatibility pins differ')
    files = {'horizon-mod.toml': ROOT / 'horizon-mod.toml', 'Cargo.toml': ROOT / 'native/Cargo.toml.in',
             'src/lib.rs': ROOT / 'native/lib.rs', 'src/runtime.rs': BUNDLE / 'runtime.rs',
             'icon.png': ROOT / 'assets/icon.png'}
    for row in native.get('module', []) + native.get('mixin', []):
        path = row.get('source', row.get('helper'))
        parts = Path(path).parts
        if len(parts) != 2 or parts[0] != 'src' or parts[1] in ('.', '..'):
            raise ValueError(f'expected a flat src/ payload, got {path}')
        source = ROOT / 'native/compat.rs' if path == 'src/compat.rs' else BUNDLE / parts[1]
        if path in files and files[path] != source:
            raise ValueError(f'conflicting payload: {path}')
        files[path] = source
    files.update({f'src/{name}': BUNDLE / source for name, source in TESTS.items()})
    for name in ('LICENSE', 'LICENSE-MIT', 'NOTICE', 'NOTICE-REVIEW-PERMISSION.md'):
        files[name] = ROOT / name
    for path in sorted((ROOT / 'native/assets').rglob('*')):
        if path.is_file():
            files[path.relative_to(ROOT / 'native').as_posix()] = path
    # The SDK resolves GUI translation keys through Minecraft's exact locale;
    # the map's own runtime also accepts these regional language variants.
    for parent, aliases in {
        'fr_fr': ('fr_ca',),
        'es_es': ('es_ar', 'es_cl', 'es_ec', 'es_mx', 'es_uy', 'es_ve'),
        'de_de': ('de_at', 'de_ch'),
    }.items():
        for alias in aliases:
            files[f"assets/{manifest['id']}/lang/{alias}.json"] = ROOT / 'native/assets' / manifest['id'] / 'lang' / f'{parent}.json'
    for name, path in files.items():
        if path.is_symlink() or not path.is_file():
            raise ValueError(f'payload must be a regular source file: {name}')
    return manifest, files


def prepare(out):
    """Update our generated project, preserving only its downloaded .horizon SDK."""
    out = Path(out).resolve()
    if not out.is_relative_to(ROOT / 'target'):
        raise ValueError('generated projects must be inside this repository target/')
    manifest, files = inputs()
    out.mkdir(parents=True, exist_ok=True)
    previous = out / '.mwwr-generated.json'
    if previous.exists():
        for name in json.loads(previous.read_text()):
            if name not in files:
                path = out / name
                if path.is_file() and not path.is_symlink():
                    path.unlink()
    for name, source in files.items():
        destination = out / name
        if destination.is_symlink():
            raise ValueError(f'generated destination is a link: {destination}')
        destination.parent.mkdir(parents=True, exist_ok=True)
        data = source.read_bytes()
        if not destination.exists() or destination.read_bytes() != data:
            destination.write_bytes(data)
    previous.write_text(json.dumps(sorted(files)) + '\n')
    return out


def package():
    manifest, files = inputs()
    out = ROOT / 'target' / f"{manifest['id']}-{manifest['version']}.hmod"
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, source in sorted(files.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, source.read_bytes())
    return {'file': str(out), 'size': out.stat().st_size,
            'sha256': hashlib.sha256(out.read_bytes()).hexdigest()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', type=Path, help='also assemble a launcher build project inside target/')
    args = parser.parse_args()
    if args.project:
        prepare(args.project)
    print(json.dumps(package(), indent=2))
