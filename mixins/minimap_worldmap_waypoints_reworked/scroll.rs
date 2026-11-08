// SPDX-License-Identifier: GPL-3.0-only
fn map_scroll(
    game: &crate::game::Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    crate::game::reworked_maps::scroll(game, screen, scroll, canceled);
}
