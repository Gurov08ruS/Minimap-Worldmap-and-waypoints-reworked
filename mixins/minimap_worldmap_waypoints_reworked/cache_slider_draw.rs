// SPDX-License-Identifier: GPL-3.0-only
fn cache_slider_draw(
    batch: &mut crate::hud::HudBatch,
    atlas: &crate::hud::HudAtlas,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &crate::text::Text,
    active: bool,
    hover: bool,
) -> core::ops::ControlFlow<()> {
    if crate::options_ui::minimap_binding_ui::cache_draw(
        batch,
        atlas,
        [x, y, w, h],
        label,
        active,
        hover,
    ) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
