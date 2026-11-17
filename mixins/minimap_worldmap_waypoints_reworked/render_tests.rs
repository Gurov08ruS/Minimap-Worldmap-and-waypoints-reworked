// SPDX-License-Identifier: GPL-3.0-only
// Expand at the original scope to retain private access and test names.
// Mixins requires self-contained payloads; external mod/include files are unsupported.

#[cfg(test)]
macro_rules! define_tests {
    () => {
        mod render_tests {
            #[test]
            fn lod_selection_prefers_visible_overview_before_local_detail() {
                use horizon_client::lod::minimap_lod_adapter::priority;
                assert!(
                    priority(0, 0, 12, [0.; 2], [1024.; 2])
                        < priority(0, 0, 0, [0.; 2], [1024.; 2])
                );
                assert!(
                    priority(0, 0, 0, [0.; 2], [1024.; 2])
                        < priority(100, 100, 12, [0.; 2], [1024.; 2])
                );
            }
            #[test]
            fn lod_dedup_retains_offscreen_sources_without_ram_coverage() {
                let mut state = State::default();
                let columns = Arc::new(Vec::<horizon_client::lod::Column>::new());
                let key = maps::LodKey {
                    x: 2000,
                    z: -4000,
                    level: 12,
                };
                state.lod_sources.insert(
                    (Dimension::Overworld, key),
                    (Arc::downgrade(&columns), Instant::now()),
                );
                assert!(
                    known_lod_sources(&state, Dimension::Overworld)
                        .contains_key(&(2000, -4000, 12))
                );
                assert!(known_lod_sources(&state, Dimension::Nether).is_empty());
                drop(columns);
                assert!(known_lod_sources(&state, Dimension::Overworld).is_empty());
            }
            #[test]
            fn map_pages_use_ordered_picture_runs_and_share_frame_handles() {
                use horizon_client::hud::{Blend, minimap_pages};
                let image = minimap_pages::create(16, 16, vec![255; 1024]);
                let mut batch = HudBatch::new([800, 600]);
                let points = [
                    [0., 0., 0., 0.],
                    [16., 0., 16., 0.],
                    [16., 16., 16., 16.],
                    [0., 16., 0., 16.],
                ];
                minimap_pages::paint(&mut batch, &image, &points);
                minimap_pages::paint(&mut batch, &image, &points);
                assert_eq!(batch.pictures.len(), 1);
                assert!(
                    batch
                        .blends
                        .iter()
                        .any(|(_, b)| matches!(b, Blend::Picture(_)))
                );
                assert_eq!(Arc::strong_count(&image), 2);
                drop(batch);
                assert_eq!(Arc::strong_count(&image), 1);
            }

            use super::*;

            #[test]
            fn showing_waypoint_survives_dimension_transition_and_preserves_zoom() {
                let mut state = State {
                    world: "save:test".into(),
                    mode: Mode::Waypoints,
                    ..State::default()
                };
                assert_eq!(show_selected_waypoint(&mut state), None);
                state.waypoints.push(Waypoint {
                    world: state.world.clone(),
                    dimension: Dimension::Nether,
                    x: -101,
                    y: 64,
                    z: 230,
                    name: "Base".into(),
                    color: 0xff6b6b,
                    icon: runtime::WaypointIcon::Letter,
                    biome: None,
                    show_3d: true,
                });
                state.waypoint_selected = Some(0);
                let zoom = state.camera.zoom;
                assert_eq!(show_selected_waypoint(&mut state), Some(Dimension::Nether));
                assert!(state.mode == Mode::Map);
                state.camera.snap([1000.0, 1000.0]); // normal mode/dimension transition
                apply_map_focus(&mut state, true, Dimension::Nether);
                assert_eq!(state.camera.center, [-100.5, 230.5]);
                assert_eq!(state.camera.target, state.camera.center);
                assert_eq!(state.camera.zoom, zoom);
                assert!(state.map_focus.is_none());
                state.camera.snap([1.0, 2.0]);
                apply_map_focus(&mut state, true, Dimension::Nether);
                assert_eq!(state.camera.center, [1.0, 2.0]); // one-shot, panning remains possible
                state.world = "save:other".into();
                assert_eq!(show_selected_waypoint(&mut state), None);
            }

            #[test]
            fn waypoint_actions_fit_small_viewports_and_do_not_overlap_rows() {
                for width in [320.0, 640.0] {
                    for height in [240.0, 480.0] {
                        let actions = waypoint_actions(width, height);
                        assert!(actions[2][0] + actions[2][2] <= width - 12.0 + 0.01);
                        assert!(actions[0][0] + actions[0][2] < actions[1][0]);
                        let state = State::default();
                        let rows = waypoint_visible_rows(height);
                        let row = waypoint_row_at(&state, width, height, rows - 1).unwrap();
                        assert!(row[1] + row[3] < actions[0][1]);
                        assert!(waypoint_row_at(&state, width, height, rows).is_none());
                    }
                }
            }

            #[test]
            fn leaf_colours_multiply_the_texture_by_the_game_tint() {
                let birch = LeafColor {
                    rgb: 0x808080,
                    tint: LeafTint::Fixed(0x80a755),
                };
                assert_eq!(birch.map_color(None, None), Some(0x40532a));
                let oak = LeafColor {
                    rgb: 0x808080,
                    tint: LeafTint::Foliage,
                };
                assert_eq!(oak.map_color(None, Some(0x77ab2f)), Some(0x3b5517));
                assert_eq!(oak.map_color(None, None), None);
            }

            #[test]
            fn a_waypoint_in_front_projects_onto_the_screen() {
                let camera = horizon_client::mod_api::NativeCamera {
                    position: [0.0; 3],
                    eye_height: 1.62,
                    partial_tick: 1.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    fov_degrees: 70.0,
                    near: 0.05,
                    far: 2048.0,
                    view: horizon_client::camera::view_matrix(0.0, 0.0).to_cols_array(),
                    projection: horizon_client::camera::perspective(
                        70.0, 1920.0, 1080.0, 2048.0, 0.05, true,
                    ),
                    dimension: "minecraft:overworld".into(),
                };
                let at = project(&camera, [0.0, 1.5, 10.0], [1920.0, 1080.0]).unwrap();
                assert!((at[0] - 960.0).abs() < 1.0, "{at:?}");
                assert!(at[1] < 540.0, "{at:?}");
            }

            #[test]
            fn a_narrower_fov_spreads_waypoints_further_from_the_centre() {
                let camera = |fov: f32| horizon_client::mod_api::NativeCamera {
                    position: [0.0; 3],
                    eye_height: 1.62,
                    partial_tick: 1.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    fov_degrees: fov,
                    near: 0.05,
                    far: 2048.0,
                    view: horizon_client::camera::view_matrix(0.0, 0.0).to_cols_array(),
                    projection: horizon_client::camera::perspective(
                        fov, 1920.0, 1080.0, 2048.0, 0.05, true,
                    ),
                    dimension: "minecraft:overworld".into(),
                };
                let wide = project(&camera(110.0), [5.0, 0.0, 10.0], [1920.0, 1080.0]).unwrap();
                let narrow = project(&camera(30.0), [5.0, 0.0, 10.0], [1920.0, 1080.0]).unwrap();
                // A narrower field of view magnifies: the same point moves away from centre.
                assert!(
                    (narrow[0] - 960.0).abs() > (wide[0] - 960.0).abs(),
                    "wide {wide:?}, narrow {narrow:?}"
                );
            }

            #[test]
            fn the_projection_is_independent_of_pixel_resolution() {
                let camera = |width: f32, height: f32| horizon_client::mod_api::NativeCamera {
                    position: [0.0; 3],
                    eye_height: 1.62,
                    partial_tick: 1.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    fov_degrees: 70.0,
                    near: 0.05,
                    far: 2048.0,
                    view: horizon_client::camera::view_matrix(0.0, 0.0).to_cols_array(),
                    projection: horizon_client::camera::perspective(
                        70.0, width, height, 2048.0, 0.05, true,
                    ),
                    dimension: "minecraft:overworld".into(),
                };
                let hd =
                    project(&camera(1920.0, 1080.0), [5.0, 0.0, 10.0], [1920.0, 1080.0]).unwrap();
                let uhd =
                    project(&camera(3840.0, 2160.0), [5.0, 0.0, 10.0], [3840.0, 2160.0]).unwrap();
                // The same world point lands at the same relative screen position.
                assert!(
                    (hd[0] / 1920.0 - uhd[0] / 3840.0).abs() < 1e-3,
                    "{hd:?} {uhd:?}"
                );
                assert!(
                    (hd[1] / 1080.0 - uhd[1] / 2160.0).abs() < 1e-3,
                    "{hd:?} {uhd:?}"
                );
            }

            #[test]
            fn fluid_hover_shows_the_surface_and_bottom_y() {
                let menu = Menu {
                    height_approximate: false,
                    block: [4, -7],
                    height: Some(62),
                    fluid: Some(FluidColumn {
                        kind: FluidKind::Water,
                        bottom: 40,
                    }),
                    dimension: Dimension::Overworld,
                    at: [0.0; 2],
                    portal: None,
                    waypoint: None,
                };
                let title = menu_title(menu);
                assert!(title.contains("Y: 62"), "{title}");
                assert!(title.contains("40"), "{title}");
            }

            #[test]
            fn biome_tint_average_keeps_each_channel() {
                assert_eq!(average_color([255 * 4, 128 * 4, 0], 4), Some(0xff8000));
                assert_eq!(average_color([0; 3], 0), None);
            }

            #[test]
            fn waypoint_colour_swatches_fit_the_name_form() {
                for width in [320.0, 640.0] {
                    for height in [240.0, 480.0] {
                        let rect = name_form_rect(width, height);
                        let last = color_swatch_at(rect, runtime::WAYPOINT_COLORS.len() - 1);
                        assert!(last[0] + last[2] <= rect[0] + rect[2] - 10.0);
                        assert!(last[1] + last[3] < rect[1] + rect[3] - 24.0);
                    }
                }
            }

            #[test]
            fn map_camera_moves_while_wider_terrain_is_still_preparing() {
                let prepared = View {
                    session: 1,
                    dimension: Dimension::Overworld,
                    layer: Layer::Surface,
                    center: [0.0; 2],
                    span: [64.0; 2],
                    width: 64,
                    height: 64,
                    angle: 0.0,
                    round: false,
                    large: true,
                };
                let mut other = prepared.clone();
                other.dimension = Dimension::Nether;
                assert_eq!(complete_map_view(other, &prepared), prepared);
                for center in [[0.0; 2], [2048.0, -4096.0]] {
                    let mut desired = prepared.clone();
                    desired.center = center;
                    desired.span = [1024.0; 2];
                    assert_eq!(complete_map_view(desired.clone(), &prepared), desired);
                }
            }

            #[test]
            fn full_map_uses_the_viewport_with_controls_overlaid_and_hit_tested() {
                for width in [320.0, 640.0] {
                    assert_eq!(rectangle(width, 240.0), [0.0, 0.0, width, 240.0]);
                    assert!(over_map_controls([10.0, 10.0], width, 240.0));
                    assert!(over_map_controls([10.0, 220.0], width, 240.0));
                    assert!(!over_map_controls([width / 2.0, 100.0], width, 240.0));
                    for i in 0..4 {
                        let tab = tab(width, i);
                        assert!(tab[0] >= 0.0 && tab[0] + tab[2] <= width);
                    }
                }
            }

            #[test]
            fn only_map_and_its_prompt_cover_the_world() {
                let mut state = State::default();
                assert!(map_covers_world(&state));
                state.mode = Mode::Waypoints;
                assert!(!map_covers_world(&state));
                state.mode = Mode::Name;
                assert!(!map_covers_world(&state));
                state.name_form = Some(NameForm {
                    text: String::new(),
                    color: runtime::DEFAULT_WAYPOINT_COLOR,
                    target: None,
                    rename: None,
                    background: Mode::Map,
                });
                assert!(map_covers_world(&state));
                state.name_form.as_mut().unwrap().background = Mode::Waypoints;
                assert!(!map_covers_world(&state));
                state.mode = Mode::Map;
                assert!(map_covers_world(&state));
            }
        }
    };
}

#[cfg(test)]
pub(crate) use define_tests;
