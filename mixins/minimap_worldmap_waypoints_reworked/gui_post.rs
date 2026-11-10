// SPDX-License-Identifier: GPL-3.0-only
fn draw_waypoints(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    gui_hidden: bool,
    focused: bool,
) {
    crate::game::reworked_maps::gui_render_post(game, batch, gui_hidden, focused);
}
