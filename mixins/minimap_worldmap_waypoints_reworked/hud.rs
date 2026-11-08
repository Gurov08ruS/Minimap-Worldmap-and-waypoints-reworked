// SPDX-License-Identifier: GPL-3.0-only
fn draw_minimap(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {
    crate::game::reworked_maps::hud(game, batch, corners);
}
