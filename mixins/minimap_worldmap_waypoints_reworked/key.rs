// SPDX-License-Identifier: GPL-3.0-only
fn on_map_key(
    game: &mut crate::game::Game<'_>,
    key: &str,
    action: mod_api::KeyAction,
    modifiers: u32,
) {
    use crate::game::minimap_compat::MapAccess;
    use ::minimap_worldmap_waypoints_reworked::runtime;
    if action != mod_api::KeyAction::Press
        || !game.native_window()
        || !game.map_on(runtime::MOD_ID)
        || runtime::text_input_active()
    {
        return;
    }
    let chosen = [
        (runtime::WORLD_MAP_KEY, runtime::ScreenRequest::Map),
        (runtime::WAYPOINTS_KEY, runtime::ScreenRequest::Waypoints),
    ]
    .into_iter()
    .find(|(id, _)| {
        game.mod_key(runtime::MOD_ID, id)
            .is_some_and(|(bound, mods)| bound == key && mods.bits & 7 == modifiers & 7)
    });
    let Some((_, request)) = chosen else {
        return;
    };
    if game.mod_screen() == Some(runtime::MOD_ID) {
        game.mod_screen_close(runtime::MOD_ID);
        return;
    }
    if !game.playing() {
        return;
    }
    if game.mod_screen_open(runtime::MOD_ID) {
        runtime::request_screen(request);
    }
}
