// SPDX-License-Identifier: GPL-3.0-only
fn map_drag(game: &crate::game::Game<'_>, delta: [f32; 2], button: i32, canceled: &mut bool) {
    crate::game::reworked_maps::drag(game, delta, button, canceled);
}
