#!/usr/bin/env python3
"""Compile the real bundle on a tiny source fixture; no Horizon/assets required."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import horizon


def run(command, **kwargs):
    result = subprocess.run(command, capture_output=True, text=True, **kwargs)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result


def snapshot(root):
    return {str(p.relative_to(root)): (horizon.digest(p.read_bytes()), p.stat().st_mtime_ns)
            for p in root.rglob('*') if p.is_file()}


def audit(original, stage, plan):
    run(['cargo', 'run', '--quiet', '--locked', '--offline', '-p', 'native-mod-source-audit',
         '--', str(original), str(stage), str(plan)], cwd=horizon.ROOT)


TARGETS = {
    "horizon_client::game::native_hooks::Game<'_>::native_mods",
    "horizon_client::game::native_hooks::Game<'_>::native_settings",
    "horizon_client::game::native_hooks::Game<'_>::native_settings_click",
    "horizon_client::game::native_hooks::Game<'_>::native_minimap",
    "horizon_client::game::<Game<'_> as ApplicationHandler>::exiting",
}


def assert_plan(plan):
    assert len(plan['modules']) == 1 and len(plan['injections']) == len(TARGETS)
    assert {p['target'] for p in plan['injections']} == TARGETS
    assert {p['mod_id'] for p in plan['injections']} == {'minimap_worldmap_waypoints_reworked'}


FIXTURE_LIB = """
extern crate self as horizon_client;
pub mod extras {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Corner { TopLeft, TopRight, BottomLeft, BottomRight }
}
pub mod hud {
    pub struct HudBatch { pub width: f32, pub height: f32 }
    impl HudBatch {
        pub fn new(size: [u32;2]) -> Self { Self { width: size[0] as f32, height: size[1] as f32 } }
    }
}
pub mod game;
"""

FIXTURE_GAME = """
use std::{cell::Cell, marker::PhantomData, path::PathBuf};
pub(crate) mod native_hooks;
pub struct Config { pub width:u32, pub height:u32 }
pub struct Renderer { pub config: Config }
pub struct Game<'a> {
    pub renderer: Option<Renderer>,
    pub original: [Cell<usize>;5],
    pub enabled: bool,
    pub window: bool,
    pub directory: PathBuf,
    lifetime: PhantomData<&'a ()>,
}
pub struct ActiveEventLoop;
pub trait ApplicationHandler { fn exiting(&mut self, event_loop: &ActiveEventLoop); }
impl ApplicationHandler for Game<'_> {
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.original[4].set(self.original[4].get()+1);
    }
}
impl Game<'_> {
    pub(super) fn native_window(&self) -> bool { self.window }
    pub(super) fn native_on(&self, _id: &str) -> bool { self.enabled }
    pub(super) fn native_data_dir(&self, _id: &str) -> PathBuf { self.directory.clone() }
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::minimap_worldmap_waypoints_reworked as runtime;
    use native_hooks::NativeMinimap;
    use crate::extras::Corner;
    #[test]
    fn horizon_contract() {
        let directory = PathBuf::from(std::env::var("MINIMAP_FIXTURE_DATA").unwrap());
        let mut game = Game {
            renderer: Some(Renderer { config: Config { width: 320, height: 240 } }),
            original: std::array::from_fn(|_| Cell::new(0)),
            enabled: true, window: true, directory: directory.clone(), lifetime: PhantomData,
        };
        let mut mods = Vec::new();
        game.native_mods(&mut mods);
        game.native_mods(&mut mods);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].id, runtime::MOD_ID);
        let mut buttons = Vec::new();
        game.native_settings("foreign", &mut buttons);
        assert!(buttons.is_empty());
        game.native_settings(runtime::MOD_ID, &mut buttons);
        assert_eq!(buttons.len(), 7);
        let mut map = None;
        game.native_minimap(&mut map);
        let baseline = map.unwrap();
        assert_eq!(baseline.size, 128.0);
        assert_eq!(baseline.corner, Corner::TopRight);
        assert!(!baseline.round);
        game.native_settings_click("foreign", 3);
        game.native_minimap(&mut map);
        assert_eq!(map, Some(baseline));
        game.native_settings_click(runtime::MOD_ID, 3);
        game.native_settings_click(runtime::MOD_ID, 1);
        map = None;
        game.native_minimap(&mut map);
        assert!(map.unwrap().round);
        assert_eq!(map.unwrap().corner, Corner::BottomLeft);
        let occupied = NativeMinimap { size: 11.0, ..baseline };
        map = Some(occupied);
        game.native_minimap(&mut map);
        assert_eq!(map, Some(occupied));
        game.enabled = false;
        map = None;
        game.native_minimap(&mut map);
        assert!(map.is_none());
        game.enabled = true;
        game.window = false;
        game.native_minimap(&mut map);
        assert!(map.is_none());
        game.native_mods(&mut mods);
        assert_eq!(mods.len(), 1);
        game.window = true;
        game.native_settings_click(runtime::MOD_ID, 0);
        game.native_minimap(&mut map);
        assert!(map.is_none());
        game.native_settings_click(runtime::MOD_ID, 0);
        game.renderer.as_mut().unwrap().config = Config { width: 100, height: 100 };
        game.native_minimap(&mut map);
        assert_eq!(map.unwrap().size, 36.0);
        game.renderer.as_mut().unwrap().config = Config { width: 8, height: 8 };
        map = None;
        game.native_minimap(&mut map);
        assert!(map.is_none());
        game.exiting(&ActiveEventLoop);
        let saved = runtime::minimap::Settings::decode(&std::fs::read_to_string(directory.join(runtime::SETTINGS_FILE)).unwrap());
        assert!(saved.enabled && saved.round);
        assert_eq!(saved.corner, runtime::minimap::Corner::BottomLeft);
        assert_eq!(game.original.map(|v| v.get()), [3,2,5,9,1]);
    }
}
"""

FIXTURE_HOOKS = """
use super::Game;
use crate::extras::Corner;
#[derive(Debug, Clone, Default)]
pub(crate) struct NativeMod {
    pub id: String, pub title: String, pub description: String, pub keys: Vec<[String;3]>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NativeMinimap {
    pub corner: Corner, pub size: f32, pub blocks_per_pixel: f32, pub rotate: bool, pub round: bool,
}
impl Game<'_> {
    pub(super) fn native_mods(&mut self, mods: &mut Vec<NativeMod>) {
        self.original[0].set(self.original[0].get()+1); let _ = mods;
    }
    pub(super) fn native_settings(&mut self, mod_id: &str, buttons: &mut Vec<String>) {
        self.original[1].set(self.original[1].get()+1); let _ = (mod_id, buttons);
    }
    pub(super) fn native_settings_click(&mut self, mod_id: &str, button: usize) {
        self.original[2].set(self.original[2].get()+1); let _ = (mod_id, button);
    }
    pub(super) fn native_minimap(&self, minimap: &mut Option<NativeMinimap>) {
        self.original[3].set(self.original[3].get()+1); let _ = minimap;
    }
}
"""


def main():
    root = horizon.ROOT
    (root/'target').mkdir(exist_ok=True)
    revision = run(['git', 'rev-parse', 'HEAD'], cwd=horizon.MIXINS).stdout.strip()
    assert revision == horizon.COMPAT['horizon_mixins_revision'], 'wrong mixin transformer revision'
    run(['git', 'diff', '--exit-code', 'HEAD', '--', 'Cargo.toml', 'Cargo.lock', 'crates'], cwd=horizon.MIXINS)
    run(['cargo', 'build', '--locked', '--offline', '-p', 'horizon-mixins-cli'], cwd=horizon.MIXINS)
    cli = horizon.MIXINS/'target/debug/horizon-mixins'
    with tempfile.TemporaryDirectory(prefix='minimap-test-', dir=root/'target') as scratch:
        scratch = Path(scratch)
        original, stage = scratch/'original', scratch/'stage'
        src = original/'crates/horizon-client/src'
        (src/'game').mkdir(parents=True)
        (original/'Cargo.toml').write_text('[workspace]\nmembers=["crates/horizon-client"]\nresolver="2"\n')
        (original/'crates/horizon-client/Cargo.toml').write_text('[package]\nname="horizon-client"\nversion="0.1.0"\nedition="2024"\n')
        (src/'lib.rs').write_text(FIXTURE_LIB)
        (src/'game.rs').write_text(FIXTURE_GAME)
        (src/'game/native_hooks.rs').write_text(FIXTURE_HOOKS)
        (original/'rust-toolchain.toml').write_bytes((root/'rust-toolchain.toml').read_bytes())
        (original/'.horizon-revision').write_text(horizon.PIN+'\n')
        run(['cargo', 'generate-lockfile', '--offline'], cwd=original)
        before = snapshot(original)
        os.environ['MINIMAP_FIXTURE_DATA'] = str(scratch/'player')
        base = ['--workspace', str(original), '--mods', str(root/'mixins')]
        report = json.loads(run([str(cli), 'check', *base, '--out', str(stage), '--trust-native-mods']).stdout)
        assert report['injections'] == 5 and report['modules'] == 1
        assert 'game::tests::horizon_contract ... ok' in report['cargo_stdout']
        first = snapshot(stage)
        os.environ['MINIMAP_FIXTURE_DATA'] = str(scratch/'player-repeat')
        second = json.loads(run([str(cli), 'check', *base, '--out', str(stage), '--trust-native-mods']).stdout)
        assert not second['stage_changed'] and snapshot(stage) == first
        assert snapshot(original) == before
        plan_path = scratch/'plan.json'
        run([str(cli), 'plan', *base, '--out', str(plan_path)])
        plan = json.loads(plan_path.read_text())
        assert plan['schema'] == horizon.COMPAT['mixin_schema']
        assert_plan(plan)
        audit(original, stage, plan_path)
        (original/'.horizon-revision').write_text('incompatible\n')
        bad = subprocess.run([str(cli), 'apply', *base, '--out', str(stage)], capture_output=True, text=True)
        assert bad.returncode and json.loads(bad.stderr)['code'] == 'E_REVISION'
        assert snapshot(stage) == first
        (original/'.horizon-revision').write_text(horizon.PIN+'\n')
        for method in ['native_mods', 'native_settings', 'native_settings_click', 'native_minimap', 'exiting']:
            source = src/('game.rs' if method == 'exiting' else 'game/native_hooks.rs')
            saved = source.read_text()
            source.write_text(saved.replace('    fn exiting(', '    fn missing_exiting(', 1) if method == 'exiting'
                              else saved.replace(f'fn {method}(', f'fn missing_{method}(', 1))
            bad = subprocess.run([str(cli), 'apply', *base, '--out', str(stage)], capture_output=True, text=True)
            assert bad.returncode and json.loads(bad.stderr)['code'] == 'E_TARGET_NOT_FOUND', method
            assert snapshot(stage) == first
            source.write_text(saved)
    print(json.dumps({'status': 'ok', 'native_registration_settings_minimap_exit': True,
                      'all_original_sources_preserved': True, 'deterministic_bytes_and_mtimes': True,
                      'incompatible_revision_and_each_target_rejected': True}, indent=2))


if __name__ == '__main__':
    main()
