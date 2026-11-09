// GPL-3.0-only; see LICENSE and NOTICE.
fn register(
    game: &mut crate::game::Game<'_>,
    mods: &mut Vec<crate::game::native_hooks::NativeMod>,
) {
    use crate::game::minimap_worldmap_waypoints_reworked as runtime;
    use runtime::tr;
    runtime::set_language(&horizon_client::lang::code());
    if !game.native_window() || mods.iter().any(|m| m.id == runtime::MOD_ID) {
        return;
    }
    runtime::initialize(game.native_data_dir(runtime::MOD_ID));
    mods.push(crate::game::native_hooks::NativeMod {
        id: runtime::MOD_ID.into(),
        title: format!("{} 0.4.1", runtime::MOD_TITLE),
        description: tr("Adaptive minimap. Shape, size, position, scale and rotation.", "Адаптивная миникарта. Форма, размер, положение, масштаб и вращение.", "Minicarte adaptative. Forme, taille, position, échelle et rotation.", "Minimapa adaptable. Forma, tamaño, posición, escala y rotación.").into(),
        keys: vec![
            [
                runtime::WORLD_MAP_KEY.into(),
                tr("Open world map", "Открыть большую карту", "Ouvrir la carte du monde", "Abrir mapa del mundo").into(),
                runtime::WORLD_MAP_DEFAULT.into(),
            ],
            [
                runtime::WAYPOINTS_KEY.into(),
                tr("Waypoints", "Вейпоинты", "Repères", "Marcadores").into(),
                runtime::WAYPOINTS_DEFAULT.into(),
            ],
        ],
    });
}
