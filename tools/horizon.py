#!/usr/bin/env python3
"""Build and audit the flat native project against the exact signed Horizon SDK.

Only ignored target/ projects, launcher state and a disposable instance are
written. An installed Nightly can be reused through hard links. Original
Horizon/Mixins checkouts and player worlds are never build destinations.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import package

ROOT = package.ROOT
COMPAT = json.loads((ROOT / 'compatibility.json').read_text())
PIN = COMPAT['horizon_revision']
REPORT = ROOT / 'target/sdk-build.json'


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def executable(explicit, variable, candidates, command):
    choices = [explicit or os.environ.get(variable)] + candidates + [shutil.which(command)]
    for path in choices:
        if path:
            for candidate in [Path(path), Path(str(path) + '.exe')]:
                if candidate.is_file():
                    return candidate.resolve()
    raise ValueError(f'{command} not found; pass --{variable.lower().replace("horizon_", "").replace("_", "-")} PATH')


def launcher(explicit=None):
    return executable(explicit, 'HORIZON_LAUNCHER', [ROOT / 'target/modern-launcher/squashfs-root/usr/bin/horizon-launcher',
        ROOT / 'target/ci/launcher/squashfs-root/usr/bin/horizon-launcher'], 'horizon-launcher')


def mod_tool(explicit=None):
    return executable(explicit, 'HORIZON_MOD_TOOL', [ROOT / 'target/modern-tools/release/cargo-horizon-mod',
        ROOT / 'target/ci/tools/release/cargo-horizon-mod'], 'cargo-horizon-mod')


def installed_base():
    if sys.platform == 'darwin':
        data = Path.home() / 'Library/Application Support'
    elif os.name == 'nt':
        data = Path(os.environ.get('APPDATA', Path.home()))
    else:
        data = Path(os.environ.get('XDG_DATA_HOME', Path.home() / '.local/share'))
    return data / 'horizon-launcher/versions' / COMPAT['nightly_version']


def owned(path):
    path = Path(path).absolute()
    if not path.is_relative_to(ROOT / 'target') or path.is_symlink():
        raise ValueError(f'build state must be an owned directory inside target/: {path}')
    return path


def sdk_environment(args):
    home = owned(args.launcher_home or (ROOT / 'target/modern-launcher-data' if (ROOT / 'target/modern-launcher-data').exists() else ROOT / 'target/sdk-launcher'))
    game = owned(args.game_home or (ROOT / 'target/modern-game-home' if (ROOT / 'target/modern-game-home').exists() else ROOT / 'target/sdk-game'))
    env = dict(os.environ, HORIZON_LAUNCHER_HOME=str(home), HORIZON_HOME=str(game), RUSTUP_TOOLCHAIN=COMPAT['sdk_rust'])
    return home, game, env


def run(command, env=None, log=None):
    if log:
        with Path(log).open('w') as output:
            result = subprocess.run([str(x) for x in command], cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
        if result.returncode:
            print(Path(log).read_text()[-6000:], file=sys.stderr)
            raise ValueError(f'command failed ({result.returncode}); see {log}')
    else:
        subprocess.run([str(x) for x in command], cwd=ROOT, env=env, check=True)


def checkout_snapshot(path):
    if path is None:
        return None
    path = Path(path).resolve()
    status = subprocess.check_output(['git', '-C', str(path), 'status', '--porcelain'])
    return status, {name: digest(path / name) if (path / name).is_file() else None for name in COMPAT['source_sha256']}


def copy_link(source, target):
    try:
        os.link(source, target)
    except OSError:
        shutil.copy2(source, target)


def reuse_base(base, home):
    destination = home / 'versions' / COMPAT['nightly_version']
    if destination.is_dir():
        return
    if not base.is_dir():
        raise ValueError('install the pinned Nightly in the launcher first, or pass --installed-version-dir PATH')
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(base, destination, copy_function=copy_link, symlinks=True)


def build(args):
    started = time.monotonic()
    original = checkout_snapshot(args.horizon)
    home, game, env = sdk_environment(args)
    exe = launcher(args.launcher)
    reuse_base(Path(args.installed_version_dir or installed_base()), home)
    project = package.prepare(ROOT / 'target/native-project')
    artifact = package.package()
    # Exercise the installable archive, rather than building a different source folder.
    instance = game / 'instances/sdk-migration'
    if not instance.exists():
        run([exe, 'instance', 'create', 'SDK migration'], env, ROOT / 'target/sdk-instance.log')
    destination = instance / 'horizon-native-mods'
    destination.mkdir(parents=True, exist_ok=True)
    manifest, _ = package.inputs()
    shutil.copyfile(artifact['file'], destination / f"{manifest['id']}.hmod")
    settings = instance / 'horizon-mods.txt'
    key = f"mod.{manifest['id']}"
    text = settings.read_text() if settings.exists() else ''
    lines = [line for line in text.splitlines() if line.partition(':')[0].strip() != key]
    updated = '\n'.join(lines + [key + ':true']) + '\n'
    if text != updated:
        settings.write_text(updated)
    log = ROOT / 'target/sdk-native-build.log'
    run([exe, 'native-build', '--instance', 'sdk-migration', '--version', COMPAT['nightly_version'], '--trust-native-code'], env, log)
    output = log.read_text()
    versions = re.findall(r'(native-[0-9a-f]+-[0-9a-f]+) built', output)
    if not versions:
        raise ValueError('launcher did not report a built version')
    candidates = list((home / 'native/prebuilt-build').glob(f'rustc-{PIN[:12]}-*/patch-workspace'))
    if not candidates:
        raise ValueError('no compiled SDK function-replacement workspace found')
    patch = max(candidates, key=lambda p: max(file.stat().st_mtime_ns for file in p.parent.glob(f"*{manifest['id']}.fingerprint"))).parent
    source = home / 'native/source' / PIN
    for name, expected in COMPAT['source_sha256'].items():
        if digest(source / name) != expected:
            raise ValueError(f'pinned SDK source changed: {name}')
    if checkout_snapshot(args.horizon) != original:
        raise ValueError('original checkout changed during SDK validation')
    record = dict(horizon_revision=PIN, rust=COMPAT['sdk_rust'], artifact=artifact,
                  project=str(project), launcher_home=str(home), game_home=str(game),
                  instance=str(instance), version=versions[-1], patch=str(patch),
                  source=str(source), seconds=time.monotonic() - started,
                  original_checkout_unchanged=True, native_build=True)
    REPORT.write_text(json.dumps(record, indent=2) + '\n')
    return record


def audit(record):
    if record['artifact']['sha256'] != package.package()['sha256']:
        raise ValueError('sources changed after native build; run tools/horizon.py check')
    patch = Path(record['patch'])
    home = Path(record['launcher_home'])
    tools = list((home / 'native/tools').glob(f'{COMPAT["horizon_mixins_revision"][:12]}/patched-*/bin/horizon-mixins*'))
    tool = next((p for p in tools if p.is_file() and p.name in ('horizon-mixins', 'horizon-mixins.exe')), None)
    if tool is None:
        raise ValueError('pinned patched Mixins CLI not found in the SDK build cache')
    plan = ROOT / 'target/sdk-audit-plan.json'
    run([tool, 'plan', '--workspace', patch / 'patch-workspace', '--mods', patch / 'patch-mods', '--out', plan], log=ROOT / 'target/sdk-plan.log')
    data = json.loads(plan.read_text())
    manifest, files = package.inputs()
    actual = data.get('injections', [])
    if data['schema'] != COMPAT['mixin_schema'] or {row['injection_id'] for row in actual} != {row['id'] for row in manifest['native']['mixin']}:
        raise ValueError('SDK Mixins plan does not match the declared injection set')
    staged = patch / 'staged' / manifest['id']
    for name, source in files.items():
        if name.endswith('.rs'):
            expected = source.read_bytes()
            if name == 'src/lib.rs':
                # The SDK supplies its native-library entry, outside the package.
                expected += b'\n#[allow(unsafe_code)]\n#[path = "__horizon_entry.rs"] mod __horizon_entry;\n'
            if (staged / name).read_bytes() != expected:
                raise ValueError(f'compiled mod payload differs: {name}')
    run(['cargo', 'run', '--locked', '--offline', '--quiet', '-p', 'native-mod-source-audit', '--',
         patch / 'patch-workspace', patch / 'patch-applied', plan], log=ROOT / 'target/sdk-ast-audit.log')
    for name, expected in COMPAT['source_sha256'].items():
        if digest(Path(record['source']) / name) != expected:
            raise ValueError(f'original pinned SDK export changed: {name}')
    return dict(status='ok', schema=data['schema'], injections=len(actual), source_payloads_match=True, only_planned_asts_changed=True)


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument('action', choices=['stage', 'check', 'test'])
    result.add_argument('--horizon', type=Path, help='optional checkout to audit read-only; never fetched or edited')
    result.add_argument('--launcher', type=Path)
    result.add_argument('--launcher-home', type=Path)
    result.add_argument('--game-home', type=Path)
    result.add_argument('--installed-version-dir', type=Path)
    return result


def main():
    args = parser().parse_args()
    record = build(args)
    record['audit'] = audit(record)
    if args.action == 'test':
        run(['cargo', 'test', '--workspace', '--locked', '--offline'], log=ROOT / 'target/sdk-runtime-tests.log')
        # The same runtime and test macros also compile inside the native SDK crate.
        sdk = Path(record['project']) / '.horizon/sdk'
        if not sdk.exists():
            template_sdk = ROOT / 'target/modern-port/.horizon/sdk'
            if template_sdk.is_dir():
                shutil.copytree(template_sdk, sdk, copy_function=copy_link)
            else:
                run([mod_tool(), 'new', str(ROOT / 'target/sdk-bootstrap'), '--template', 'native', '--horizon', PIN], log=ROOT / 'target/sdk-bootstrap.log')
                shutil.copytree(ROOT / 'target/sdk-bootstrap/.horizon/sdk', sdk, copy_function=copy_link)
        run(['cargo', 'test', '--manifest-path', str(Path(record['project']) / 'Cargo.toml'), '--lib'],
            dict(os.environ, CARGO_TARGET_DIR=str(ROOT / 'target/sdk-api-tests')), ROOT / 'target/sdk-api-tests.log')
        record['sdk_crate_tests'] = True
    print(json.dumps(record, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
