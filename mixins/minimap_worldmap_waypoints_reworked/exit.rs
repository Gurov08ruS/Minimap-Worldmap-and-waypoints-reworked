// GPL-3.0-only; see LICENSE and NOTICE.
fn flush_settings(game: &mut crate::game::Game<'_>) {
    if game.native_window() {
        crate::game::minimap_worldmap_waypoints_reworked::shutdown();
    }
}
