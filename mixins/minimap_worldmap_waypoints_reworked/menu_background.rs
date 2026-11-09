// SPDX-License-Identifier: GPL-3.0-only
fn waypoint_background(
    game: &crate::game::Game<'_>,
    radius: &mut Option<f32>,
) -> core::ops::ControlFlow<()> {
    if crate::game::reworked_maps::transparent_waypoints(game) {
        *radius = None;
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
