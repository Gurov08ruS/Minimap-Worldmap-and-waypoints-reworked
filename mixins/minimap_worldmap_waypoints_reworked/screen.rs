// SPDX-License-Identifier: GPL-3.0-only
fn draw_worldmap(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    screen: &mod_api::ScreenView<'_>,
) {
    crate::game::reworked_maps::screen(game, batch, screen);
}
