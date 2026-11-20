// SPDX-License-Identifier: GPL-3.0-only
fn reserve_map_and_effects(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<f32> {
    use core::ops::ControlFlow::{Break, Continue};
    let (width, height) = game.gui_size();
    let Some(map_height) = crate::game::reworked_maps::top_right_map_height(game, width, height)
    else {
        return Continue(());
    };
    // Preserve vanilla row allocation below the map, including PvP's override.
    let effects = &game.potions.effects.list;
    let rows = if effects.is_empty()
        || game.screen != crate::game::Screen::Playing
        || game.pvp_effects_shown()
    {
        0.
    } else if effects.iter().any(|e| {
        horizon_client::brewing::effect(e.id)
            .is_some_and(|d| d.category != horizon_client::brewing::Category::Beneficial)
    }) {
        50.
    } else {
        25.
    };
    Break(map_height + rows)
}
