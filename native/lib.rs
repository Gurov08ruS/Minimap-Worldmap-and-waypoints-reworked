// SPDX-License-Identifier: GPL-3.0-only
use mod_api::{self, Hooks, KeyRegistration, Registrar, RegistrationContext, SettingSpec};
pub mod runtime;
use mod_api::*;

#[derive(Default)]
struct Mod;
static INITIAL: std::sync::OnceLock<runtime::minimap::Settings> = std::sync::OnceLock::new();
pub fn initial_settings() -> runtime::minimap::Settings {
    INITIAL.get().copied().unwrap_or_default()
}

fn legacy_preferences(cx: &RegistrationContext) -> runtime::minimap::Settings {
    use std::io::Read;
    let Some(root) = cx.instance_dir().ok().and_then(native_path) else {
        return runtime::minimap::Settings::default();
    };
    let path = root
        .join("horizon-mods/.horizon-mod-data")
        .join(runtime::MOD_ID)
        .join("settings.ini");
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(65536).read_to_string(&mut text);
    }
    runtime::minimap::Settings::decode(&text)
}
fn native_path(directory: &mod_api::InstanceDirectory) -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    if let mod_api::InstanceDirectory::Unix(bytes) = directory {
        use std::os::unix::ffi::OsStringExt;
        return Some(std::ffi::OsString::from_vec(bytes.clone()).into());
    }
    #[cfg(windows)]
    if let mod_api::InstanceDirectory::Windows(units) = directory {
        use std::os::windows::ffi::OsStringExt;
        return Some(std::ffi::OsString::from_wide(units).into());
    }
    let _ = directory;
    None
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, r: &mut dyn Registrar, cx: &RegistrationContext) {
        let mut preferences = legacy_preferences(cx);
        for (key, button) in runtime::SDK_SETTING_ROWS {
            if let Ok(Some(raw)) = cx.setting(runtime::MOD_ID, key) {
                if let Ok(index) = raw.parse() {
                    preferences.select_option(button, index);
                }
            }
        }
        if let Ok(Some(raw)) = cx.setting(runtime::MOD_ID, "cache_ram_mib") {
            if let Ok(value) = raw.parse::<f64>() {
                if value.is_finite() && (16. ..=512.).contains(&value) {
                    preferences.cache_ram_mib = value.round() as u16;
                }
            }
        }
        let _ = INITIAL.set(preferences);
        for (id, default) in [
            (runtime::WORLD_MAP_KEY, runtime::WORLD_MAP_DEFAULT),
            (runtime::WAYPOINTS_KEY, runtime::WAYPOINTS_DEFAULT),
        ] {
            r.register_key_mapping(
                KeyRegistration::new(id, default).label(&format!("mwwr.key.{id}")),
            );
        }
        for (key, button) in runtime::SDK_SETTING_ROWS {
            let options = runtime::minimap::Settings::options(button)
                .iter()
                .enumerate()
                .map(|(index, _)| format!("mwwr.value.{key}.{index}"))
                .collect::<Vec<_>>();
            r.register_setting(SettingSpec::choice(
                key,
                format!("mwwr.setting.{key}"),
                options,
                preferences.option_index(button).unwrap_or(0),
            ))
            .expect("valid minimap setting");
        }
        r.register_setting(SettingSpec::range(
            "cache_ram_mib",
            "mwwr.setting.cache_ram_mib",
            16.,
            512.,
            1.,
            f64::from(preferences.cache_ram_mib),
        ))
        .expect("valid RAM setting");
        for key in ["cache_manager", "reset"] {
            r.register_setting(SettingSpec::choice(
                key,
                format!("mwwr.setting.{key}"),
                [format!("mwwr.action.{key}"), format!("mwwr.action.{key}")],
                0,
            ))
            .expect("valid minimap action");
        }
    }

    // The engine skips these entry points unless an SDK listener subscribes.
    fn render_gui_post(&self, host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
        let _ = host;
        let _ = event;
    }
    fn gui_render_post(&self, host: &dyn ClientHost, event: &mut GuiRenderPostEvent<'_>) {
        let _ = host;
        let _ = event;
    }
    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        let _ = host;
        let _ = event;
    }
    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        let _ = host;
        let _ = event;
        Flow::Continue
    }
    fn screen_mouse_dragged_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseDraggedPreEvent<'_>,
    ) -> Flow {
        let _ = host;
        let _ = event;
        Flow::Continue
    }
    fn screen_mouse_scrolled_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseScrolledPreEvent<'_>,
    ) -> Flow {
        let _ = host;
        let _ = event;
        Flow::Continue
    }
    fn screen_key_pressed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenKeyPressedPreEvent<'_>,
    ) -> Flow {
        let _ = host;
        let _ = event;
        Flow::Continue
    }
    fn screen_char_typed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenCharTypedPreEvent<'_>,
    ) -> Flow {
        let _ = host;
        let _ = event;
        Flow::Continue
    }
    fn menu_background(&self, host: &dyn ClientHost, event: &mut MenuBackgroundEvent<'_>) {
        let _ = host;
        let _ = event;
    }
    fn client_disconnecting(&self, host: &dyn ClientHost, event: &mut ClientDisconnectingEvent) {
        let _ = host;
        let _ = event;
    }
    fn client_resources_reloaded(
        &self,
        host: &mut dyn ClientHost,
        event: &mut ClientResourcesReloadedEvent,
    ) {
        let _ = host;
        let _ = event;
    }
    fn client_stopping(
        &self,
        _host: &mut dyn mod_api::ClientHost,
        _event: &mut mod_api::ClientStoppingEvent,
    ) {
        runtime::shutdown();
    }
}

#[cfg(test)]
mod minimap_container_tests;
#[cfg(test)]
mod minimap_map_regression_tests;
#[cfg(test)]
mod minimap_maps_tests;
#[cfg(test)]
mod minimap_settings_tests;
