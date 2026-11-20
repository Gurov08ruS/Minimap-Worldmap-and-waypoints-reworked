// SPDX-License-Identifier: GPL-3.0-only
fn cache_pad_actions(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<Vec<DeclaredAction>> {
    if crate::game::reworked_maps::title_cache_open(game) {
        core::ops::ControlFlow::Break(Vec::new())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
