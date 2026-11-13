// GPL-3.0-only; see LICENSE and NOTICE.
fn click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    use crate::game::minimap_worldmap_waypoints_reworked as runtime;
    if mod_id == runtime::MOD_ID && game.native_window() {
        if button & 0xffff0000 == 0x10000 {
            runtime::initialize(game.native_data_dir(runtime::MOD_ID))
                .set_cache_ram_mib((button & 0xffff) as u16);
            return;
        }
        if button == 15 {
            let controller = runtime::initialize(game.native_data_dir(runtime::MOD_ID));
            let value = controller.settings().cache_ram_mib;
            controller.set_cache_ram_mib(if value >= 512 { 16 } else { value + 16 });
            return;
        }
        runtime::initialize(game.native_data_dir(runtime::MOD_ID)).click(button);
    }
}
