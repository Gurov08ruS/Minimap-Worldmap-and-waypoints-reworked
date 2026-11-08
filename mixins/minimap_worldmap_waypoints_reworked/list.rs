// GPL-3.0-only; see LICENSE and NOTICE.
fn register(
    game: &mut crate::game::Game<'_>,
    mods: &mut Vec<crate::game::native_hooks::NativeMod>,
) {
    use crate::game::minimap_worldmap_waypoints_reworked as runtime;
    if !game.native_window() || mods.iter().any(|m| m.id == runtime::MOD_ID) {
        return;
    }
    runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    mods.push(crate::game::native_hooks::NativeMod {
        id: runtime::MOD_ID.into(),
        title: format!("{} 0.3.0", runtime::MOD_TITLE),
        description: "Адаптивная миникарта. Форма, размер, положение, масштаб и вращение.".into(),
        keys: vec![
            [
                runtime::WORLD_MAP_KEY.into(),
                "Открыть большую карту".into(),
                runtime::WORLD_MAP_DEFAULT.into(),
            ],
            [
                runtime::WAYPOINTS_KEY.into(),
                "Вейпоинты".into(),
                runtime::WAYPOINTS_DEFAULT.into(),
            ],
        ],
    });
}
