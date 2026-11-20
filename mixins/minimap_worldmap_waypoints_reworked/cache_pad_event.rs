// SPDX-License-Identifier: GPL-3.0-only
fn cache_pad_pointer(
    game: &crate::game::Game<'_>,
    event: &WindowEvent,
) -> core::ops::ControlFlow<()> {
    if crate::game::reworked_maps::cache_pad_event(game, event) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
