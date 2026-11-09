// SPDX-License-Identifier: GPL-3.0-only
// Only replace presentation; Game's update/network/streaming loop still runs.
fn render_map_only(game: &mut crate::game::Game<'_>) -> core::ops::ControlFlow<Result<bool>> {
    use core::ops::ControlFlow::{Break, Continue};
    if !crate::game::reworked_maps::covers_world(game)
        || game.integrated_starting.is_some()
        || game.pending_entry.is_some()
        || game.pack_reload.is_some()
        || game.screenshot_sequence.is_some()
        || game.remote_loading()
    {
        return Continue(());
    }
    let Some(size) = game
        .renderer
        .as_ref()
        .map(|r| [r.config.width, r.config.height])
    else {
        return Continue(());
    };
    game.gui.atlas.pack_pixels();
    game.pack.render.pixels.pack();
    game.gui.atlas.font.begin_frame();
    if game.clock.still() {
        game.gui.atlas.font.reseed(game.visual_ticks() as i64);
    }
    game.glint_frame = game.capture_glint_frame();
    if game.renderer.as_mut().is_some_and(|r| r.display_changed()) {
        game.apply_ui_settings();
    }
    game.observe_screen();
    game.drop_left_screen_focus();
    // Keep positional audio current even though no world geometry is prepared.
    if let Some((camera, yaw, pitch)) = game.camera_view(game.clock.camera_fraction() as f32) {
        game.audio.set_listener(camera.to_array(), yaw, pitch);
    }
    // Do not reuse the previous world's entity draw commands or projection.
    game.skin_frame.get_mut().clear_draws();
    game.native_camera_frame = None;
    game.locator_camera = None;
    game.posed_rotation = None;
    game.screen_blur_requested.set(false);
    let mut hud = game.hud(size);
    game.flush_ime(&mut hud);
    game.native_gui_render_post(&mut hud, game.ui.hide_gui, game.focused);
    game.resolve_inline_profiles(&hud.inline_profiles);
    // Map tiles and glyphs produced this frame must reach the GPU this frame.
    hud.patches.append(&mut game.map_patches);
    let mut frame = crate::game::loading_frame(&hud);
    frame.ticks = game.texture_clock();
    let result = game.renderer.as_mut().unwrap().render(&frame);
    game.frame_drawn = matches!(result, Ok(true));
    Break(result)
}
