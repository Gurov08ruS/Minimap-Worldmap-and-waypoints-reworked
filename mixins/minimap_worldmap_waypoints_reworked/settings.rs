// GPL-3.0-only; see LICENSE and NOTICE.
fn settings(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    use crate::game::minimap_worldmap_waypoints_reworked as runtime;
    if mod_id == runtime::MOD_ID && game.native_window() {
        buttons.extend(runtime::initialize(game.native_data_dir(runtime::MOD_ID)).labels());
    }
}
