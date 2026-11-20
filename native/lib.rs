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

    fn input_key(&self, host: &mut dyn ClientHost, event: &mut InputKeyEvent<'_>) {
        if event.action != KeyAction::Press
            || !host.on(runtime::MOD_ID)
            || runtime::text_input_active()
        {
            return;
        }
        let map = host.mod_key(runtime::MOD_ID, runtime::WORLD_MAP_KEY);
        let waypoints = host.mod_key(runtime::MOD_ID, runtime::WAYPOINTS_KEY);
        fn binding(value: &Option<(String, ClientModifiers)>) -> Option<(&str, u32)> {
            value
                .as_ref()
                .map(|(key, modifiers)| (key.as_str(), modifiers.bits))
        }
        let Some(request) = runtime::bound_screen_request(
            event.key,
            event.modifiers,
            binding(&map),
            binding(&waypoints),
        ) else {
            return;
        };
        if host.mod_screen() == Some(runtime::MOD_ID) {
            host.mod_screen_close(runtime::MOD_ID);
        } else if host.playing() && host.mod_screen_open(runtime::MOD_ID) {
            runtime::request_screen(request);
        }
    }

    fn render_frame_pre(&self, host: &mut dyn ClientHost, _event: &mut RenderFramePreEvent) {
        if !host.on(runtime::MOD_ID) {
            return;
        }
        let playing = host.playing();
        if playing || host.mod_screen() == Some(runtime::MOD_ID) {
            if let Some(code) = host.client_session().client_language() {
                runtime::set_language(&code);
            }
            // Mouse mappings are counted by KeyMapping, not InputKeyEvent.
            // Drain both queues even behind our screen, where the SDK releases
            // mappings, so closing it cannot replay an earlier press.
            let mut requested = None;
            for (id, screen) in [
                (runtime::WORLD_MAP_KEY, runtime::ScreenRequest::Map),
                (runtime::WAYPOINTS_KEY, runtime::ScreenRequest::Waypoints),
            ] {
                let clicked = host.consume_click(runtime::MOD_ID, id);
                if clicked
                    && requested.is_none()
                    && host
                        .mod_key(runtime::MOD_ID, id)
                        .is_some_and(|(key, _)| key.starts_with("key.mouse."))
                {
                    requested = Some(screen);
                }
            }
            if playing
                && !runtime::text_input_active()
                && let Some(request) = requested
                && host.mod_screen_open(runtime::MOD_ID)
            {
                runtime::request_screen(request);
            }
        }
    }

    // Drawing still needs the native adapters until the SDK exposes map textures.
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
        if host.on(runtime::MOD_ID)
            && host.mod_screen() == Some(runtime::MOD_ID)
            && runtime::waypoint_background_transparent()
        {
            *event.radius = None;
        }
    }
    fn client_disconnecting(&self, host: &dyn ClientHost, event: &mut ClientDisconnectingEvent) {
        let _ = host;
        let _ = event;
        runtime::CLIENT_EVENTS.disconnected();
    }
    fn client_resources_reloaded(
        &self,
        host: &mut dyn ClientHost,
        event: &mut ClientResourcesReloadedEvent,
    ) {
        let _ = host;
        let _ = event;
        runtime::CLIENT_EVENTS.resources_reloaded();
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
