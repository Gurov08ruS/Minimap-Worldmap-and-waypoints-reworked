// GPL-3.0-only; see LICENSE and NOTICE.
fn click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    use crate::game::minimap_worldmap_waypoints_reworked as runtime;
    if mod_id == runtime::MOD_ID && game.native_window() {
        runtime::initialize(game.native_data_dir(runtime::MOD_ID)).click(button);
    }
}
