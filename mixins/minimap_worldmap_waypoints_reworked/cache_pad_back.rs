// SPDX-License-Identifier: GPL-3.0-only
fn cache_pad_cancel(game: &crate::game::Game<'_>) -> core::ops::ControlFlow<Result<()>> {
    if crate::game::reworked_maps::cache_pad_back(game) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
