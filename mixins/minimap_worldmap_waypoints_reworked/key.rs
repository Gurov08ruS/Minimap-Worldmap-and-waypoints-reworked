// SPDX-License-Identifier: GPL-3.0-only
fn on_map_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    use crate::game::minimap_worldmap_waypoints_reworked as runtime;
    if !pressed || !game.native_window() || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let map = game.native_mod_key(
        runtime::MOD_ID,
        runtime::WORLD_MAP_KEY,
        runtime::WORLD_MAP_DEFAULT,
    );
    let waypoints = game.native_mod_key(
        runtime::MOD_ID,
        runtime::WAYPOINTS_KEY,
        runtime::WAYPOINTS_DEFAULT,
    );
    let chosen = if key == map {
        map.as_str()
    } else if key == waypoints {
        waypoints.as_str()
    } else {
        return;
    };
    if chosen == "key.keyboard.unknown" {
        return;
    }
    if game.native_mod_screen() == Some(runtime::MOD_ID) {
        game.native_mod_screen_close(runtime::MOD_ID);
        return;
    }
    if !game.native_playing() {
        return;
    }
    runtime::request_screen(if key == map {
        runtime::ScreenRequest::Map
    } else {
        runtime::ScreenRequest::Waypoints
    });
    game.native_mod_screen_open(runtime::MOD_ID);
}
