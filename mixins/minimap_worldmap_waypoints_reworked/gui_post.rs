// SPDX-License-Identifier: GPL-3.0-only
// Draw world markers before corner overlays, so the minimap covers them.
fn draw_waypoints(game: &crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::reworked_maps::gui_render_post(
        game,
        batch,
        game.ui.hide_gui,
        game.focused || game.control.is_some(),
    );
}
