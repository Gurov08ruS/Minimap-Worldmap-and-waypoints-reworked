// SPDX-License-Identifier: GPL-3.0-only
fn cache_pad_focus(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<Vec<Target>> {
    match crate::game::reworked_maps::cache_pad_targets(game) {
        Some(targets) => core::ops::ControlFlow::Break(targets),
        None => core::ops::ControlFlow::Continue(()),
    }
}
