#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
set -Eeuo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
horizon_dir="${XDG_DATA_HOME:-$HOME/.local/share}/horizon"
horizon_explicit=false
build_game=false
instance_name=""
launcher_path=""
temporary=""

usage() {
    cat <<'HELP'
Usage: ./install-horizon.sh [options]

Packages the current mod, installs it into horizon-native-mods and enables
only this mod in horizon-mods.txt. Does not compile or launch Horizon by default.

  --horizon-dir DIR  Game directory (default: ~/.local/share/horizon)
  --build            Also compile the selected instance; requires --instance
  --instance NAME    Horizon launcher instance to build
  --launcher PATH    Path to horizon-launcher (otherwise detected)
  -h, --help        Show this help

Examples:
  ./install-horizon.sh
  ./install-horizon.sh --horizon-dir /path/to/game
  ./install-horizon.sh --build --instance Main
HELP
}
fail() { printf 'Ошибка: %s\n' "$*" >&2; exit 1; }
cleanup() { if [[ -n "$temporary" && -f "$temporary" ]]; then rm -f -- "$temporary"; fi; }
trap cleanup EXIT

while (($#)); do
    case "$1" in
        --horizon-dir|--instance|--launcher)
            (($# >= 2)) || fail "Для $1 нужно значение"
            case "$1" in
                --horizon-dir) horizon_dir="$2"; horizon_explicit=true ;;
                --instance) instance_name="$2" ;;
                --launcher) launcher_path="$2" ;;
            esac
            shift 2 ;;
        --build) build_game=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) fail "Неизвестный аргумент: $1" ;;
    esac
done
command -v python3 >/dev/null || fail 'Не найден python3 (нужна версия 3.11+)'
[[ -n "$horizon_dir" ]] || fail 'Не задана папка игры'
if $build_game; then
    [[ -n "$instance_name" ]] || fail '--build требует --instance NAME: сборка должна относиться к выбранному экземпляру лаунчера'
    if [[ -z "$launcher_path" ]]; then
        launcher_path="$(command -v horizon-launcher || true)"
        if [[ -z "$launcher_path" ]]; then
            for candidate in "$HOME/Загрузки/horizon-launcher-linux-x86_64/horizon-launcher" "$HOME/Downloads/horizon-launcher-linux-x86_64/horizon-launcher"; do
                if [[ -x "$candidate" ]]; then launcher_path="$candidate"; break; fi
            done
        fi
    fi
    [[ -x "$launcher_path" ]] || fail 'Не найден horizon-launcher; укажи --launcher PATH'
    instances_json="$("$launcher_path" --json instance list)"
    instance_home="$(python3 -c 'import json,pathlib,sys; rows=json.load(sys.stdin); matches=[r for r in rows if r.get("id")==sys.argv[1] or r.get("name")==sys.argv[1]]; len(matches)==1 or sys.exit("Instance not found or ambiguous"); matches[0].get("kind")!="java" or sys.exit("This instance does not run Horizon"); print(pathlib.Path(matches[0]["home"]).resolve())' "$instance_name" <<<"$instances_json")"
    if $horizon_explicit; then
        supplied_home="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve())' "$horizon_dir")"
        [[ "$supplied_home" == "$instance_home" ]] || fail '--horizon-dir не совпадает с папкой --instance'
    fi
    horizon_dir="$instance_home"
fi

metadata="$(python3 - "$project_dir" <<'PY'
import pathlib, sys, tomllib
root = pathlib.Path(sys.argv[1])
manifests = list((root / 'mixins').glob('*/horizon-mixin.toml'))
if len(manifests) != 1:
    raise SystemExit('Expected exactly one mod manifest')
mod = tomllib.loads(manifests[0].read_text())['mod']
print(mod['id'])
print(mod['version'])
PY
)"
# macOS ships Bash 3.2 without `mapfile`: read the two lines portably.
mod_id="$(printf '%s\n' "$metadata" | sed -n '1p')"
mod_version="$(printf '%s\n' "$metadata" | sed -n '2p')"
[[ -n "$mod_id" && -n "$mod_version" ]] || fail 'Не удалось прочитать id и версию мода'
[[ "$mod_id" =~ ^[a-z0-9_]+$ && "$mod_version" =~ ^[0-9A-Za-z.+-]+$ ]] || fail 'Некорректные id или версия мода'

printf 'Сборка архива %s %s…\n' "$mod_id" "$mod_version"
python3 "$project_dir/tools/package.py"
artifact="$project_dir/target/$mod_id-$mod_version.hnmod"
[[ -f "$artifact" ]] || fail "Архив не создан: $artifact"

destination="$horizon_dir/horizon-native-mods"
mkdir -p -- "$destination"
installed="$destination/$mod_id.hnmod"
[[ ! -d "$installed" ]] || fail "Вместо файла мода обнаружен каталог: $installed"
temporary="$(mktemp "$destination/.$mod_id.install.XXXXXX")"
install -m 0644 -- "$artifact" "$temporary"
cmp -s -- "$artifact" "$temporary" || fail 'Проверка скопированного архива не прошла'
python3 - "$temporary" "$mod_id" <<'PY'
import json, sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    if archive.testzip() is not None:
        raise SystemExit('Archive CRC verification failed')
    if json.loads(archive.read('horizon-native.json'))['id'] != sys.argv[2]:
        raise SystemExit('Unexpected mod ID in archive')
PY
mv -f -- "$temporary" "$installed"
temporary=""

python3 - "$horizon_dir" "$mod_id" <<'PY'
import os, pathlib, stat, sys, tempfile
path = pathlib.Path(sys.argv[1]) / 'horizon-mods.txt'
key = 'mod.' + sys.argv[2]
text = path.read_text() if path.exists() else ''
lines = [line for line in text.splitlines() if line.partition(':')[0].strip() != key]
updated = '\n'.join(lines + [key + ':true']) + '\n'
if updated != text:
    mode = stat.S_IMODE(path.stat().st_mode) if path.exists() else 0o644
    fd, name = tempfile.mkstemp(prefix='.horizon-mods-install-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(updated)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(name, mode)
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)
PY
printf '\nУстановлено: %s\nМод включён в horizon-mods.txt.\n' "$installed"
if $build_game; then
    printf 'Нативная сборка экземпляра %s…\n' "$instance_name"
    "$launcher_path" native-build --instance "$instance_name" --trust-native-code
else
    printf 'Для запуска нужна сборка игры с модом в лаунчере Horizon.\n'
fi
