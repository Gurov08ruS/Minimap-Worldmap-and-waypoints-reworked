// SPDX-License-Identifier: GPL-3.0-only
fn update_maps(game: &mut crate::game::Game<'_>, elapsed: std::time::Duration) {
    crate::game::reworked_maps::frame(game, elapsed);
}
