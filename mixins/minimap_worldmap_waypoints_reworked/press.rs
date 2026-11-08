// SPDX-License-Identifier: GPL-3.0-only
fn map_press(
    game: &crate::game::Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    crate::game::reworked_maps::press(game, screen, button, canceled);
}
