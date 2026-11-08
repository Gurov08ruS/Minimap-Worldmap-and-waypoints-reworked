// SPDX-License-Identifier: GPL-3.0-only
// See LICENSE and NOTICE.
fn binding_tooltip(
    settings: &crate::settings::Settings,
    widget: &crate::options_ui::Widget,
) -> core::ops::ControlFlow<Option<Text>> {
    match crate::options_ui::minimap_binding_ui::tooltip(settings, widget) {
        Some(tooltip) => core::ops::ControlFlow::Break(Some(tooltip)),
        None => core::ops::ControlFlow::Continue(()),
    }
}
