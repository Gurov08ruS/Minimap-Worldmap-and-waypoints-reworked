// GPL-3.0-only; see LICENSE and NOTICE.
fn provide_minimap(
    game: &crate::game::Game<'_>,
    minimap: &mut Option<crate::game::native_hooks::NativeMinimap>,
) {
    use crate::game::minimap_worldmap_waypoints_reworked::{self as runtime, minimap::Corner};
    if minimap.is_some() || !game.native_window() || !game.native_on(runtime::MOD_ID) {
        return;
    }
    let settings = runtime::initialize(game.native_data_dir(runtime::MOD_ID)).settings();
    if !settings.enabled {
        return;
    }
    let Some(renderer) = game.renderer.as_ref() else {
        return;
    };
    let viewport =
        horizon_client::hud::HudBatch::new([renderer.config.width, renderer.config.height]);
    let Some(size) = settings.fitted_size(viewport.width, viewport.height) else {
        return;
    };
    let corner = match settings.corner {
        Corner::TopLeft => horizon_client::extras::Corner::TopLeft,
        Corner::TopRight => horizon_client::extras::Corner::TopRight,
        Corner::BottomLeft => horizon_client::extras::Corner::BottomLeft,
        Corner::BottomRight => horizon_client::extras::Corner::BottomRight,
    };
    *minimap = Some(crate::game::native_hooks::NativeMinimap {
        corner,
        size,
        blocks_per_pixel: settings.blocks_per_pixel,
        rotate: settings.rotate_with_player,
        round: settings.round,
    });
}
