// SPDX-License-Identifier: GPL-3.0-only
fn cache_slider_click(
    ui: &mut crate::options_ui::OptionsUi,
    settings: &crate::settings::Settings,
    x: f32,
    y: f32,
    size: (f32, f32),
    drag: bool,
    right: bool,
) -> core::ops::ControlFlow<Vec<Event>> {
    match crate::options_ui::minimap_binding_ui::cache_click(ui, settings, x, y, size, drag, right)
    {
        Some(events) => core::ops::ControlFlow::Break(events),
        None => core::ops::ControlFlow::Continue(()),
    }
}
