// SPDX-License-Identifier: GPL-3.0-only
fn map_screen_char(
    game: &crate::game::Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    c: char,
    modifiers: u32,
    canceled: &mut bool,
) {
    let _ = (screen, modifiers);
    crate::game::reworked_maps::screen_char(game, c, canceled);
}
