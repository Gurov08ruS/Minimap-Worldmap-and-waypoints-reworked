// SPDX-License-Identifier: GPL-3.0-only
fn map_screen_key(
    game: &crate::game::Game<'_>,
    screen: &crate::mod_api::NativeScreenView<'_>,
    key: &str,
    repeat: bool,
    modifiers: u32,
    canceled: &mut bool,
) {
    let _ = (screen, repeat, modifiers);
    crate::game::reworked_maps::screen_key(game, key, canceled);
}
