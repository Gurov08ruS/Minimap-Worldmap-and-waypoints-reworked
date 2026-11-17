// SPDX-License-Identifier: GPL-3.0-only
// Expand at the original scope to retain private access and test names.
// Mixins requires self-contained payloads; external mod/include files are unsupported.

#[cfg(test)]
macro_rules! define_tests {
    () => {
        mod map_regressions {
            use super::*;
            use maps::{Cache, Frame, Layer, Service, Tile, View};
            fn view(session: u64) -> View {
                View {
                    session,
                    dimension: Dimension::Overworld,
                    layer: Layer::Surface,
                    center: [8.0, 8.0],
                    span: [16.0; 2],
                    width: 16,
                    height: 16,
                    angle: 0.0,
                    round: false,
                    large: true,
                }
            }
            fn wait(service: &Service, predicate: impl Fn(&Frame) -> bool) -> Arc<Frame> {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    if let Some(frame) = service.frame()
                        && predicate(&frame)
                    {
                        return frame;
                    }
                    assert!(
                        std::time::Instant::now() < until,
                        "terrain worker did not publish expected frame"
                    );
                    thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            #[test]
            fn biome_pictograms_cover_distinct_vanilla_biomes_and_explicit_unknowns() {
                assert_eq!(BIOME_PICTOGRAMS.len(), 67);
                for (i, (name, icon)) in BIOME_PICTOGRAMS.iter().enumerate() {
                    let id = format!("minecraft:{name}");
                    assert!(std::ptr::eq(biome_pictogram(Some(&id)), icon));
                    assert_ne!(icon, &UNKNOWN_BIOME_PICTOGRAM);
                    assert!(icon.rows.iter().any(|row| *row != 0));
                    assert!(icon.colors.iter().all(|c| c >> 24 == 255));
                    for (other_name, other) in &BIOME_PICTOGRAMS[..i] {
                        assert_ne!(name, other_name);
                        assert_ne!(icon, other, "{name} duplicates {other_name}");
                    }
                }
                for id in [
                    None,
                    Some("custom:forest"),
                    Some("minecraft:unknown_forest"),
                    Some("forest"),
                ] {
                    assert_eq!(biome_pictogram(id), &UNKNOWN_BIOME_PICTOGRAM);
                }
            }

            #[test]
            fn coordinate_toggles_persist_merge_and_reset_independently() {
                let mut settings = Settings::default();
                let mut edited = settings;
                let mask = minimap::apply(&mut edited, 9);
                minimap::merge(&mut settings, edited, mask);
                assert!(!settings.overworld_coordinates && settings.nether_coordinates);
                minimap::apply(&mut settings, 10);
                assert_eq!(Settings::decode(&settings.encode()), settings);
                assert!(coordinate_lines(settings, Dimension::Overworld, [0.0; 3]).is_empty());
                let mask = minimap::apply(&mut edited, 6);
                minimap::merge(&mut settings, edited, mask);
                assert!(settings.overworld_coordinates && settings.nether_coordinates);
                assert_eq!(
                    coordinate_lines(settings, Dimension::Nether, [0.0; 3]).len(),
                    2
                );
            }
            #[test]
            fn portal_coordinates_convert_before_flooring_and_never_scale_height() {
                assert_eq!(
                    portal_coordinates(Dimension::Overworld, Dimension::Nether, [-0.5, 63.9, -8.1]),
                    Some([-1, 63, -2])
                );
                assert_eq!(
                    portal_coordinates(Dimension::Nether, Dimension::Overworld, [-0.5, -0.5, 1.25]),
                    Some([-4, -1, 10])
                );
                assert_eq!(
                    portal_coordinates(
                        Dimension::Overworld,
                        Dimension::Overworld,
                        [1.9, 64.0, -1.1]
                    ),
                    Some([1, 64, -2])
                );
                assert_eq!(
                    portal_coordinates(Dimension::End, Dimension::Overworld, [0.0; 3]),
                    None
                );
                assert_eq!(
                    portal_coordinates(Dimension::Overworld, Dimension::Nether, [f64::NAN; 3]),
                    None
                );
            }

            #[test]
            fn legacy_smoothing_is_ignored_without_losing_other_preferences() {
                for old in ["true", "false", "invalid"] {
                    let settings = Settings::decode(&format!(
                        "smooth_worldmap={old}\nsize=192\nworld_dimension=nether\nnether_coordinates=false"
                    ));
                    assert_eq!(settings.size_px, 192);
                    assert_eq!(settings.world_dimension, Dimension::Nether);
                    assert!(!settings.nether_coordinates);
                    assert!(!settings.encode().contains("smooth_worldmap"));
                    assert_eq!(Settings::decode(&settings.encode()), settings);
                    assert_eq!(settings.labels().len(), 18);
                }
            }
            #[test]
            fn dimension_preferences_roundtrip_and_are_independent() {
                let settings = Settings::decode("mini_dimension=nether\nworld_dimension=end");
                assert_eq!(settings.mini_dimension, Dimension::Nether);
                assert_eq!(settings.world_dimension, Dimension::End);
                assert_eq!(Settings::decode(&settings.encode()), settings);
                assert_eq!(Dimension::Player.resolve(Dimension::End), Dimension::End);
                assert_eq!(Dimension::Nether.resolve(Dimension::End), Dimension::Nether);
            }
            #[test]
            fn nether_scale_converts_both_ways_for_every_dimension() {
                let position = [800.0, -1200.0];
                assert_eq!(
                    Dimension::Overworld.convert(Dimension::Nether, position),
                    [100.0, -150.0]
                );
                assert_eq!(
                    Dimension::Nether.convert(Dimension::Overworld, position),
                    [6400.0, -9600.0]
                );
                assert_eq!(
                    Dimension::Nether.convert(Dimension::End, position),
                    [6400.0, -9600.0]
                );
                assert_eq!(
                    Dimension::End.convert(Dimension::Nether, position),
                    [100.0, -150.0]
                );
                assert_eq!(
                    Dimension::End.convert(Dimension::Overworld, position),
                    position
                );
                assert_eq!(
                    Dimension::Player.convert(Dimension::Nether, position),
                    position
                );
            }
            #[test]
            fn a_stale_connection_job_cannot_repopulate_a_new_session() {
                let service = Service::new();
                service.reset(1);
                let (started_tx, started_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                assert!(
                    service.submit(1, Dimension::Overworld, Layer::Surface, [0, 0], move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Tile {
                            pixels: [255, 0, 0, 255].repeat(256),
                            ..Tile::default()
                        }
                    })
                );
                started_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                service.reset(2);
                service.request(view(2));
                release_tx.send(()).unwrap();
                let frame = wait(&service, |f| f.view.session == 2);
                assert_eq!(frame.known, 0);
                assert!(!service.contains(Dimension::Overworld, Layer::Surface, [0, 0]));
                assert!(
                    service.submit(2, Dimension::Overworld, Layer::Surface, [0, 0], || Tile {
                        pixels: [0, 255, 0, 255].repeat(256),
                        ..Tile::default()
                    })
                );
                let frame = wait(&service, |f| f.atlas.as_ref().is_some_and(|a| a.chunks.contains(&[0,0])));
                let atlas = frame.atlas.as_ref().unwrap();
                assert_eq!(&atlas.pixels[..4], &[0, 255, 0, 255]);
                let mut moved = view(2);
                moved.center = [-31.5, 17.75];
                moved.span = [128.0; 2];
                service.request(moved.clone());
                let next = wait(&service, |f| f.view == moved);
                assert!(next.atlas.is_some());
                assert!(next.pixels.is_empty());
                service.request(view(2));
                let frame=wait(&service,|f|f.view==view(2)&&f.atlas.as_ref().is_some_and(|a|a.chunks.contains(&[0,0])));
                let atlas=frame.atlas.as_ref().unwrap();
                assert!(
                    service.submit(2, Dimension::Overworld, Layer::Surface, [0, 0], || Tile {
                        pixels: [255, 0, 0, 255].repeat(256),
                        ..Tile::default()
                    })
                );
                let changed = wait(&service, |f| {
                    f.atlas.as_ref().is_some_and(|a| !Arc::ptr_eq(a, atlas))
                });
                assert_eq!(
                    &changed.atlas.as_ref().unwrap().pixels[..4],
                    &[255, 0, 0, 255]
                );
                service.shutdown();
            }
            #[test]
            fn circle_pixels_are_transparent_and_loaded_void_is_known() {
                let mut cache = Cache::default();
                cache.insert(
                    Dimension::Overworld,
                    Layer::Surface,
                    [0, 0],
                    Tile {
                        pixels: vec![0; 1024],
                        ..Tile::default()
                    },
                );
                let mut request = view(1);
                request.round = true;
                let frame = cache.render(request, 1);
                assert_eq!(frame.pixels[3], 0);
                assert!(frame.known > 0);
            }
            #[test]
            fn terrain_beyond_the_old_budget_is_retained() {
                let mut cache = Cache::default();
                for x in 0..4097 {
                    cache.insert(
                        Dimension::Overworld,
                        Layer::Surface,
                        [x, 0],
                        Tile {
                            pixels: [1, 2, 3, 255].repeat(256),
                            ..Tile::default()
                        },
                    );
                }
                assert_eq!(cache.render(view(1), 1).known, 256);
                cache.insert(
                    Dimension::Overworld,
                    Layer::Surface,
                    [0, 0],
                    Tile {
                        pixels: [1, 2, 3, 255].repeat(256),
                        ..Tile::default()
                    },
                );
                assert_eq!(cache.render(view(1), 2).known, 256);
            }
            #[test]
            fn relief_shading_reads_the_neighbour_chunk_without_a_seam() {
                let mut cache = Cache::default();
                let mut south = Tile {
                    pixels: [200, 200, 200, 255].repeat(256),
                    ..Tile::default()
                };
                south.heights = [10; 256];
                cache.insert(Dimension::Overworld, Layer::Surface, [0, 0], south);
                let mut north = Tile {
                    pixels: [200, 200, 200, 255].repeat(256),
                    ..Tile::default()
                };
                north.heights = [13; 256];
                cache.insert(Dimension::Overworld, Layer::Surface, [0, -1], north);
                let mut request = view(1);
                request.center = [8.0, 0.0];
                request.span = [16.0, 32.0];
                request.height = 32;
                let frame = cache.render(request, 1);
                // Block (8, 0) is three blocks below (8, -1), in the neighbouring tile.
                let shaded = (16 * 16 + 8) * 4;
                assert_eq!(&frame.pixels[shaded..shaded + 3], &[167, 167, 167]);
                // Inside the north chunk, equal heights keep the base brightness.
                let plain = (15 * 16 + 8) * 4;
                assert_eq!(&frame.pixels[plain..plain + 3], &[218, 218, 218]);
            }
            #[test]
            fn unknown_terrain_is_transparent_instead_of_a_screen_checkerboard() {
                let cache = Cache::default();
                let frame = cache.render(view(1), 1);
                assert_eq!(&frame.pixels[..4], &[0, 0, 0, 0]);
                assert_eq!(frame.known, 0);
            }
        }
    };
}

#[cfg(test)]
pub(crate) use define_tests;
