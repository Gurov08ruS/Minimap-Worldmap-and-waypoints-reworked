// SPDX-License-Identifier: GPL-3.0-only
fn draw_effects_below_map(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
) -> core::ops::ControlFlow<()> {
    use core::ops::ControlFlow::{Break, Continue};
    let Some(offset) =
        crate::game::reworked_maps::top_right_map_height(game, batch.width, batch.height)
    else {
        return Continue(());
    };
    if game.potions.effects.list.is_empty() || game.screen_shows_active_effects(batch) {
        return Break(());
    }
    let icons: Vec<horizon_client::hud_overlays::EffectIcon> = game
        .potions
        .effects
        .list
        .iter()
        .map(horizon_client::hud_overlays::EffectIcon::from)
        .collect();
    let draws = horizon_client::hud_overlays::effects(batch.width as i32, false, &icons);
    let start = batch.vertices.len();
    horizon_client::hud_layout::paint(batch, &game.gui.atlas, &draws);
    batch.translate_since(start, 0., offset);
    Break(())
}
