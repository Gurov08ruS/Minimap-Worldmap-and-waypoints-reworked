#!/usr/bin/env python3
"""Validate the modern package boundary and a reversible installer fixture."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib
import zipfile
import horizon
import package


def main():
    first = package.package()
    before = Path(first['file']).read_bytes()
    assert package.package()['sha256'] == hashlib.sha256(before).hexdigest()
    manifest, sources = package.inputs()
    # These integrations have moved to public hooks. Keep the released
    # boundary from silently regaining their engine patches/helper payloads.
    retired = {'map-key', 'map-disconnect', 'map-resources', 'waypoints-no-blur'}
    assert retired.isdisjoint(row['id'] for row in manifest['native']['mixin'])
    assert {'src/key.rs', 'src/disconnect.rs', 'src/atlas.rs',
            'src/menu_background.rs'}.isdisjoint(sources)
    with zipfile.ZipFile(first['file']) as archive:
        assert archive.testzip() is None
        assert set(archive.namelist()) == set(sources)
        assert 'horizon-native.json' not in archive.namelist()
        assert not any(n.endswith('.hnmod') or '/.horizon/' in n for n in archive.namelist())
        for name, source in sources.items():
            assert archive.read(name) == source.read_bytes(), name
        cargo = tomllib.loads(archive.read('Cargo.toml').decode())
        assert cargo['dependencies']['horizon-mod-api']['path'] == '.horizon/sdk/api/crates/horizon-mod-api'
        for language in ['en_us', 'ru_ru', 'fr_fr', 'es_es', 'de_de']:
            strings = json.loads(archive.read(f"assets/{manifest['id']}/lang/{language}.json"))
            assert strings and all(key and value for key, value in strings.items())
    subprocess.run([str(horizon.mod_tool()), 'check', first['file'], '--json'], check=True)
    # Settings/control labels are generated from the same state cycles as the SDK registrar.
    result = subprocess.check_output(['cargo', 'run', '--locked', '--offline', '--quiet', '--example', 'export_settings'], cwd=package.ROOT, text=True)
    labels = {}
    for line in result.splitlines():
        language, key, value = line.split('\t')
        labels.setdefault(language, {})[key] = value
    for language, expected in labels.items():
        file = package.ROOT / 'native/assets' / manifest['id'] / 'lang' / (language + '.json')
        assert json.loads(file.read_text()) == expected, f'regenerate SDK labels: {language}'
    with tempfile.TemporaryDirectory(prefix='sdk-installer-', dir=package.ROOT / 'target') as scratch:
        home = Path(scratch)
        (home / 'horizon-mods.txt').write_text('mod.other:true\nkey.other.open:key.keyboard.p\n')
        destination = home / 'horizon-native-mods'
        destination.mkdir()
        retired = destination / f"{manifest['id']}.hnmod"
        with zipfile.ZipFile(retired, 'w') as archive:
            archive.writestr('horizon-native.json', json.dumps({'id': manifest['id']}))
        original = retired.read_bytes()
        subprocess.run(['bash', str(package.ROOT / 'install-horizon.sh'), '--horizon-dir', str(home)], cwd=package.ROOT, check=True, capture_output=True)
        installed = destination / f"{manifest['id']}.hmod"
        assert installed.read_bytes() == before
        assert retired.with_suffix('.hnmod.legacy').read_bytes() == original and not retired.exists()
        settings = (home / 'horizon-mods.txt').read_text()
        assert 'mod.other:true\n' in settings and 'key.other.open:key.keyboard.p\n' in settings
        assert settings.count(f"mod.{manifest['id']}:true\n") == 1
    print(json.dumps({'status': 'ok', 'flat_hmod': True, 'deterministic': True,
                      'payloads_match': True, 'sdk_labels': len(labels), 'installer': True}, indent=2))


if __name__ == '__main__':
    main()
