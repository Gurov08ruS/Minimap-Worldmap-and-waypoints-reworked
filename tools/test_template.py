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


# The full source audit uses the exact production manifest. This focused fixture
# exercises actual registration, persisted settings and key handling without a GPU.
def assert_plan(plan):
    manifest = tomllib.loads((horizon.ROOT/'mixins/minimap_worldmap_waypoints_reworked/horizon-mixin.toml').read_text())
    assert {x['target'] for x in plan['injections']} == {x['target'] for x in manifest['injection']}
    assert {x['name'] for x in plan['modules']} == {x['name'] for x in manifest['module']}
    assert all(x['mod_id'] == manifest['mod']['id'] for x in plan['injections'])

FIXTURE_LIB = 'extern crate self as horizon_client;\npub mod lang { pub fn code() -> String { std::env::var("MINIMAP_FIXTURE_LANGUAGE").unwrap_or_else(|_| "en_us".into()) } }\npub mod game;\n'
FIXTURE_GAME = """
use std::{cell::Cell, marker::PhantomData, path::PathBuf};
pub(crate) mod native_hooks;
pub(crate) mod window_handler;
pub struct Game<'a> {
    pub original: [Cell<usize>;5], pub enabled:bool, pub window:bool,
    pub directory:PathBuf, pub open:bool, pub playing:bool, pub binding:String,
    lifetime:PhantomData<&'a ()>,
}
pub struct ActiveEventLoop;
pub trait ApplicationHandler { fn exiting(&mut self,event_loop:&ActiveEventLoop); }
impl Game<'_> {
    pub(super) fn native_window(&self)->bool {self.window}
    pub(super) fn native_on(&self,_:&str)->bool {self.enabled}
    pub(super) fn native_data_dir(&self,_:&str)->PathBuf {self.directory.clone()}
    pub(super) fn native_mod_key(&self,_:&str,_:&str,_:&str)->String {self.binding.clone()}
    pub(super) fn native_playing(&self)->bool {self.playing&&!self.open}
    pub(super) fn native_mod_screen(&self)->Option<&str> {self.open.then_some(minimap_worldmap_waypoints_reworked::MOD_ID)}
    pub(super) fn native_mod_screen_open(&mut self,_:&str)->bool {self.open=true;true}
    pub(super) fn native_mod_screen_close(&mut self,_:&str) {self.open=false;}
}
#[cfg(test)] mod tests {
    use super::*;
    use super::minimap_worldmap_waypoints_reworked as runtime;
    #[test] fn horizon_contract() {
        let directory=PathBuf::from(std::env::var("MINIMAP_FIXTURE_DATA").unwrap());
        let mut game=Game{original:std::array::from_fn(|_|Cell::new(0)),enabled:true,window:true,directory:directory.clone(),open:false,playing:true,binding:"key.keyboard.m".into(),lifetime:PhantomData};
        let mut mods=Vec::new();game.native_mods(&mut mods);game.native_mods(&mut mods);
        assert_eq!(mods.len(),1);assert_eq!(mods[0].keys[0],[runtime::WORLD_MAP_KEY,runtime::tr("Open world map", "Открыть большую карту", "Ouvrir la carte du monde", "Abrir mapa del mundo", "Weltkarte öffnen"),runtime::WORLD_MAP_DEFAULT].map(str::to_owned));
        let mut buttons=Vec::new();game.native_settings("foreign",&mut buttons);assert!(buttons.is_empty());
        game.native_settings(runtime::MOD_ID,&mut buttons);assert_eq!(buttons.len(),14);
        // The same controller changes labels without altering saved preferences.
        let original=runtime::initialize(directory.clone()).settings();
        runtime::set_language("ru_ru");
        assert_eq!(runtime::Dimension::Overworld.label(), "Верхний мир");
        assert!(runtime::initialize(directory.clone()).labels()[0].starts_with("Миникарта:"));
        runtime::set_language("en_us");
        assert_eq!(runtime::Dimension::Overworld.label(), "Overworld");
        assert!(runtime::initialize(directory.clone()).labels()[0].starts_with("Minimap:"));
        runtime::set_language("en_ca");
        assert_eq!(runtime::tr("Waypoint", "Вейпоинт", "Repère", "Marcador", "Wegpunkt"), "Waypoint");
        for code in ["en_us", "en_ca", "en_gb", "en_au", "EN-NZ"] {
            runtime::set_language(code);
            assert_eq!(runtime::Dimension::Player.label(), "Player");
        }
        for (code, label, setting) in [("fr_fr", "Joueur", "Minicarte :"), ("fr_ca", "Joueur", "Minicarte :"), ("es_es", "Jugador", "Minimapa:"), ("es_mx", "Jugador", "Minimapa:"), ("de_de", "Spieler", "Minikarte:"), ("de_at", "Spieler", "Minikarte:"), ("de_ch", "Spieler", "Minikarte:")] {
            runtime::set_language(code);
            assert_eq!(runtime::Dimension::Player.label(), label);
            assert!(runtime::initialize(directory.clone()).labels()[0].starts_with(setting));
        }
        runtime::set_language("en_us");
        assert_eq!(runtime::initialize(directory.clone()).settings(), original);
        game.native_key("key.keyboard.m",false);assert!(!game.open);
        game.native_key("key.keyboard.m",true);assert!(game.open);
        game.native_key("key.keyboard.m",false);assert!(game.open);
        game.native_key("key.keyboard.m",true);assert!(!game.open);
        game.binding="key.keyboard.b".into();game.native_key("key.keyboard.m",true);assert!(!game.open);
        game.native_key("key.keyboard.b",true);assert!(game.open);game.native_key("key.keyboard.b",true);assert!(!game.open);
        game.playing=false;game.native_key("key.keyboard.b",true);assert!(!game.open);game.playing=true;
        game.enabled=false;game.native_key("key.keyboard.b",true);assert!(!game.open);game.enabled=true;
        game.window=false;game.native_key("key.keyboard.b",true);assert!(!game.open);game.window=true;
        game.binding="key.keyboard.unknown".into();game.native_key("key.keyboard.unknown",true);assert!(!game.open);
        game.native_settings_click("foreign",7);
        game.native_settings_click(runtime::MOD_ID,7);game.native_settings_click(runtime::MOD_ID,8);game.native_settings_click(runtime::MOD_ID,8);
        game.native_settings_click(runtime::MOD_ID,9);
        game.native_settings_click(runtime::MOD_ID,11);game.native_settings_click(runtime::MOD_ID,12);
        game.exiting(&ActiveEventLoop);
        let saved=runtime::minimap::Settings::decode(&std::fs::read_to_string(directory.join(runtime::SETTINGS_FILE)).unwrap());
        assert_eq!(saved.mini_dimension,runtime::Dimension::Overworld);assert_eq!(saved.world_dimension,runtime::Dimension::Nether);
        assert!(!saved.overworld_coordinates);
        assert!(saved.nether_coordinates);
        assert!(!saved.death_waypoints);
        assert!(!saved.death_waypoint_auto_remove);
        assert!(game.original.iter().all(|v|v.get()>0));assert_eq!(game.original[4].get(),1);
    }
}
"""
FIXTURE_WINDOW_HANDLER = """
use super::*;
impl ApplicationHandler for Game<'_> {
    fn exiting(&mut self,_event_loop:&ActiveEventLoop) {self.original[4].set(self.original[4].get()+1);}
}
"""
FIXTURE_HOOKS = """
use super::Game;
#[derive(Clone,Debug,Default)] pub(crate) struct NativeMod {pub id:String,pub title:String,pub description:String,pub keys:Vec<[String;3]>}
impl Game<'_> {
    pub(super) fn native_mods(&mut self,mods:&mut Vec<NativeMod>) {self.original[0].set(self.original[0].get()+1);let _=mods;}
    pub(super) fn native_settings(&mut self,mod_id:&str,buttons:&mut Vec<String>) {self.original[1].set(self.original[1].get()+1);let _=(mod_id,buttons);}
    pub(super) fn native_settings_click(&mut self,mod_id:&str,button:usize) {self.original[2].set(self.original[2].get()+1);let _=(mod_id,button);}
    pub(super) fn native_key(&mut self,key:&str,pressed:bool) {self.original[3].set(self.original[3].get()+1);let _=(key,pressed);}
}
"""

def focused_bundle(destination):
    import shutil
    source=horizon.ROOT/'mixins/minimap_worldmap_waypoints_reworked'
    manifest=tomllib.loads((source/'horizon-mixin.toml').read_text())
    bundle=destination/manifest['mod']['id'];bundle.mkdir(parents=True)
    selected=[x for x in manifest['injection'] if x['id'] in {'register','settings','settings-click','settings-flush','map-key'}]
    assert len(selected)==5
    text='[mod]\n'+''.join(f'{k} = {json.dumps(v)}\n' for k,v in manifest['mod'].items())
    text+='\n[compatibility]\nhorizon_revision = '+json.dumps(horizon.PIN)+'\n'
    module=next(m for m in manifest['module'] if m['id']=='runtime')
    text+='\n[[module]]\n'+''.join(f'{k} = {json.dumps(v)}\n' for k,v in module.items())
    shutil.copyfile(source/module['source'],bundle/module['source'])
    for injection in selected:
        text+='\n[[injection]]\n'+''.join(f'{k} = {json.dumps(v)}\n' for k,v in injection.items() if k!='bindings')
        text+='bindings = ['+', '.join('{ '+', '.join(f'{k} = {json.dumps(v)}' for k,v in binding.items())+' }' for binding in injection['bindings'])+']\n'
        shutil.copyfile(source/injection['helper'],bundle/injection['helper'])
    (bundle/'horizon-mixin.toml').write_text(text)


def main():
    root=horizon.ROOT
    revision=run(['git','rev-parse','HEAD'],cwd=horizon.MIXINS).stdout.strip()
    assert revision==horizon.COMPAT['horizon_mixins_revision']
    run(['git','diff','--exit-code','HEAD','--','Cargo.toml','Cargo.lock','crates'],cwd=horizon.MIXINS)
    run(['cargo','build','--locked','--offline','--target-dir',str(horizon.MIXINS_TARGET),'-p','horizon-mixins-cli'],cwd=horizon.MIXINS)
    cli=horizon.MIXINS_TARGET/'debug/horizon-mixins'
    with tempfile.TemporaryDirectory(prefix='mwwr-fixture-') as directory:
        scratch=Path(directory);original=scratch/'original';stage=scratch/'stage';src=original/'crates/horizon-client/src'
        (src/'game').mkdir(parents=True)
        (original/'Cargo.toml').write_text('[workspace]\nmembers=["crates/horizon-client"]\nresolver="2"\n')
        (src.parent/'Cargo.toml').write_text('[package]\nname="horizon-client"\nversion="0.1.0"\nedition="2024"\n')
        (src/'lib.rs').write_text(FIXTURE_LIB);(src/'game.rs').write_text(FIXTURE_GAME);(src/'game/native_hooks.rs').write_text(FIXTURE_HOOKS);(src/'game/window_handler.rs').write_text(FIXTURE_WINDOW_HANDLER)
        (original/'rust-toolchain.toml').write_bytes((root/'rust-toolchain.toml').read_bytes());(original/'.horizon-revision').write_text(horizon.PIN+'\n')
        run(['cargo','generate-lockfile','--offline'],cwd=original)
        focused_bundle(scratch/'mods');base=['--workspace',str(original),'--mods',str(scratch/'mods')]
        before=snapshot(original);os.environ['MINIMAP_FIXTURE_DATA']=str(scratch/'player')
        report=json.loads(run([str(cli),'check',*base,'--out',str(stage),'--trust-native-mods']).stdout)
        assert report['injections']==5 and report['modules']==1
        assert 'game::tests::horizon_contract ... ok' in report['cargo_stdout']
        first=snapshot(stage);os.environ['MINIMAP_FIXTURE_DATA']=str(scratch/'player-2')
        second=json.loads(run([str(cli),'check',*base,'--out',str(stage),'--trust-native-mods']).stdout)
        assert not second['stage_changed'] and snapshot(stage)==first and snapshot(original)==before
        plan=scratch/'plan.json';run([str(cli),'plan',*base,'--out',str(plan)]);audit(original,stage,plan)
        (original/'.horizon-revision').write_text('incompatible\n')
        bad=subprocess.run([str(cli),'apply',*base,'--out',str(stage)],capture_output=True,text=True)
        assert bad.returncode and json.loads(bad.stderr)['code']=='E_REVISION';assert snapshot(stage)==first
        (original/'.horizon-revision').write_text(horizon.PIN+'\n')
        for method in ['native_mods','native_settings','native_settings_click','native_key','exiting']:
            source=src/('game/window_handler.rs' if method=='exiting' else 'game/native_hooks.rs');saved=source.read_text()
            source.write_text(saved.replace('    fn exiting(', '    fn missing_exiting(',1) if method=='exiting' else saved.replace(f'fn {method}(',f'fn missing_{method}(',1))
            bad=subprocess.run([str(cli),'apply',*base,'--out',str(stage)],capture_output=True,text=True)
            assert bad.returncode and json.loads(bad.stderr)['code']=='E_TARGET_NOT_FOUND',method
            assert snapshot(stage)==first;source.write_text(saved)
    print(json.dumps({'status':'ok','focused_registration_rebinding_dimension_settings_exit':True,'original_sources_preserved':True,'deterministic_stage':True},indent=2))

if __name__=='__main__':main()
