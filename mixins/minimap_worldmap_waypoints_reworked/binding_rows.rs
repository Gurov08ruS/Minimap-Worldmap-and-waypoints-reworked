// SPDX-License-Identifier: GPL-3.0-only
// See LICENSE and NOTICE.
fn binding_rows(
    ui: &crate::options_ui::OptionsUi,
    settings: &crate::settings::Settings,
    index: usize,
) -> core::ops::ControlFlow<Vec<Row>> {
    core::ops::ControlFlow::Break(crate::options_ui::minimap_binding_ui::native_rows(
        ui, settings, index,
    ))
}
