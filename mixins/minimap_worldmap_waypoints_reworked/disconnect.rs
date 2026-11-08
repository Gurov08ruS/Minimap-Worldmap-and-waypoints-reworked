// SPDX-License-Identifier: GPL-3.0-only
fn reset_map_session(game: &crate::game::Game<'_>) {
    crate::game::reworked_maps::disconnect(game);
}
