// SPDX-License-Identifier: GPL-3.0-only
// Shared standalone/SDK tests, expanded in the runtime's private scope.

#[cfg(test)]
macro_rules! define_tests {
    () => {
        mod tests {
            use super::*;
            use std::sync::atomic::{AtomicU64, Ordering};

            #[test]
            fn public_sdk_bindings_keep_physical_keys_modifiers_and_unbinding() {
                let map = Some(("key.keyboard.m", 2));
                let waypoints = Some(("key.mouse.right", 0));
                assert_eq!(bound_screen_request("key.keyboard.m", 2, map, waypoints), Some(ScreenRequest::Map));
                assert_eq!(bound_screen_request("key.keyboard.m", 0, map, waypoints), None);
                assert_eq!(bound_screen_request("key.keyboard.m", 2 | 16, map, waypoints), Some(ScreenRequest::Map));
                assert_eq!(bound_screen_request("key.keyboard.n", 2, map, waypoints), None);
                assert_eq!(bound_screen_request("key.mouse.right", 0, map, waypoints), Some(ScreenRequest::Waypoints));
                assert_eq!(bound_screen_request("key.keyboard.m", 2, None, waypoints), None);
                assert_eq!(bound_screen_request("key.mouse.right", 0, map, None), None);
                assert_eq!(bound_screen_request("key.keyboard.m", 2, map, map), Some(ScreenRequest::Map));
            }

            #[test]
            fn public_sdk_lifecycle_coalesces_both_invalidations_without_retaining_hosts() {
                let events = std::sync::Arc::new(ClientEvents::new());
                let other = events.clone();
                std::thread::spawn(move || {
                    other.resources_reloaded();
                    other.disconnected();
                    other.resources_reloaded();
                }).join().unwrap();
                assert_eq!(events.take(), PendingClientEvents { disconnected: true, resources_reloaded: true });
                assert_eq!(events.take(), PendingClientEvents::default());
                events.resources_reloaded();
                assert_eq!(events.take(), PendingClientEvents { disconnected: false, resources_reloaded: true });
            }

            #[test]
            fn cache_scrollbar_drag_keeps_grab_point_and_reaches_both_ends() {
                let bar = ListScrollbar::new(42., 238., 5, 100, 37);
                for grab in [0., 3., bar.thumb_height - 1.] {
                    assert_eq!(bar.row_at(bar.thumb_top + grab, grab), 37);
                    assert_eq!(bar.row_at(-100., grab), 0);
                    assert_eq!(bar.row_at(1000., grab), 95);
                }
                let resized = ListScrollbar::new(42., 46., 1, 4, 95);
                assert_eq!(resized.row_at(resized.thumb_top + 2., 2.), 3);
                for count in [0, 3, 5] {
                    let bar = ListScrollbar::new(42., 238., 5, count, 37);
                    assert_eq!(bar.thumb_height, 238.);
                    assert_eq!(bar.thumb_top, 42.);
                    assert_eq!(bar.row_at(1000., 0.), 0);
                }
            }

            #[test]
            fn smooth_list_scrolling_accumulates_sub_row_events_and_clamps() {
                let mut scroll = ListScroll::default();
                for _ in 0..9 { scroll.scroll(-0.1, 11); }
                assert_eq!(scroll.offset(11), 0);
                scroll.scroll(-0.1, 11);
                assert_eq!(scroll.offset(11), 1);
                for _ in 0..10 { scroll.scroll(0.1, 11); }
                assert_eq!(scroll.offset(11), 0);
                scroll.scroll(1., 11);
                scroll.scroll(-0.5, 11);
                assert_eq!(scroll.offset(11), 0, "outward wheel motion must not accumulate at the edge");
                scroll.scroll(-0.5, 11);
                assert_eq!(scroll.offset(11), 1);
                scroll.scroll(f64::NEG_INFINITY, 11);
                scroll.scroll(f64::NAN, 11);
                assert_eq!(scroll.offset(11), 1);
                scroll.scroll(-f64::MAX, 11);
                assert_eq!(scroll.offset(11), 11);
                scroll.scroll(f64::MAX, 11);
                assert_eq!(scroll.offset(11), 0);
                scroll.page(true, 3, 11);
                scroll.page(true, 3, 11);
                assert_eq!(scroll.offset(11), 6);
                assert_eq!(scroll.offset(2), 2, "a refreshed or resized list clamps its view");
                scroll.page(false, 3, 2);
                assert_eq!(scroll.offset(2), 0);
                scroll.jump(usize::MAX, 11);
                assert_eq!(scroll.offset(11), 11);
                scroll.scroll(-1., 0);
                assert_eq!(scroll.offset(0), 0);
            }

            #[test]
            fn worker_diagnostics_translate_all_supported_languages_without_losing_error_identity() {
                let mut keys = std::collections::BTreeSet::new();
                for (english, translations) in MESSAGES {
                    assert!(keys.insert(*english), "duplicate message: {english}");
                    assert_eq!(message_for(Language::English, english), *english);
                    for (language, expected) in [Language::Russian, Language::French, Language::Spanish, Language::German].into_iter().zip(translations) {
                        assert!(!expected.is_empty());
                        assert_ne!(expected, english);
                        assert_eq!(message_for(language, english), *expected);
                    }
                }
                let raw = "Cannot open cache; sampling paused: unsupported container version";
                for language in [Language::Russian, Language::French, Language::Spanish, Language::German] {
                    let message = message_for(language, raw);
                    assert!(!message.contains("unsupported"));
                    assert!(!message.contains("Cannot open"));
                    assert!(!message_for(language, "Permission denied (os error 13)").contains("Permission"));
                    assert!(!message_for(language, "unknown platform diagnostic").contains("platform"));
                }
                assert_eq!(message_for(Language::English, raw), raw, "stored diagnostics remain stable for format detection");
            }

            fn read_profile(path: &Path) -> io::Result<(Settings, Vec<Waypoint>)> {
                read_profile_migration(path).map(|(settings, points, _)| (settings, points))
            }

            fn read_settings(path: &Path) -> io::Result<Settings> {
                read_profile(path).map(|profile| profile.0)
            }
            #[test]
            fn sdk_choices_cover_saved_preferences_and_reject_actions_and_bad_indices() {
                let mut keys=std::collections::BTreeSet::new();
                for (key,button) in SDK_SETTING_ROWS {
                    assert!(keys.insert(key));
                    let options=Settings::options(button);assert!(!options.is_empty()&&options.len()<=32);
                    for (index,option) in options.iter().enumerate() {
                        assert_eq!(option.option_index(button),Some(index));
                        assert_eq!(Settings::decode(&option.encode()),*option);
                        assert!(!option.option_label(button).unwrap().is_empty());
                        let mut selected=Settings{cache_ram_mib:173,..Settings::default()};
                        assert!(selected.select_option(button,index));
                        assert_eq!(selected.option_index(button),Some(index));assert_eq!(selected.cache_ram_mib,173);
                        let before=selected;assert!(!selected.select_option(button,options.len()));assert_eq!(selected,before);
                    }
                }
                for action in [6,18,19,21,usize::MAX] {assert!(Settings::options(action).is_empty());assert_eq!(Settings::default().option_index(action),None);}
            }
            #[test]
            fn sdk_choice_during_initial_load_preserves_other_preferences() {
                let directory=scratch();fs::create_dir_all(&directory).unwrap();
                fs::write(directory.join(SETTINGS_FILE),"size=256\ncache_ram_mib=173\nwaypoint_letters=one\n").unwrap();
                let controller=Controller::open(directory.clone());
                assert!(controller.set_option(20,1));assert!(!controller.set_option(20,3));assert!(!controller.set_option(19,0));
                controller.shutdown();
                let saved=read_settings(&directory.join(SETTINGS_FILE)).unwrap();
                assert_eq!(saved.size_px,256);assert_eq!(saved.cache_ram_mib,173);assert_eq!(saved.waypoint_letters,minimap::WaypointLetters::AnyColors);
                fs::remove_dir_all(directory).unwrap();
            }
            #[test]
            fn sdk_explicit_defaults_override_nondefault_preferences_during_initial_load() {
                let directory=scratch();fs::create_dir_all(&directory).unwrap();
                fs::write(directory.join(SETTINGS_FILE),"size=256\ncache_ram_mib=256\nwaypoint_letters=one\n").unwrap();
                let controller=Controller::open(directory.clone());
                controller.set_cache_ram_mib(64);assert!(controller.set_option(20,0));controller.shutdown();
                let saved=read_settings(&directory.join(SETTINGS_FILE)).unwrap();
                assert_eq!(saved.size_px,256);assert_eq!(saved.cache_ram_mib,64);assert_eq!(saved.waypoint_letters,minimap::WaypointLetters::SimilarColors);
                fs::remove_dir_all(directory).unwrap();
            }
            #[test]
            fn waypoint_initials_expand_for_collisions_in_the_selected_colour_scope() {
                use minimap::WaypointLetters::*;
                let a=point("save:a","  Ёлка");let mut b=point("save:a","ёжик");b.color=a.color^0x000504;
                assert_eq!(waypoint_labels(&[a.clone(),b.clone()],SimilarColors),vec!["ЁЛ","ЁЖ"]);
                assert_eq!(waypoint_labels(&[a.clone(),b.clone()],One),vec!["Ё","Ё"]);
                b.color=0xff0000;assert_eq!(waypoint_labels(&[a.clone(),b.clone()],SimilarColors),vec!["Ё","Ё"]);
                assert_eq!(waypoint_labels(&[a.clone(),b.clone()],AnyColors),vec!["ЁЛ","ЁЖ"]);
                b.world="save:b".into();assert_eq!(waypoint_labels(&[a.clone(),b.clone()],AnyColors),vec!["Ё","Ё"]);
                b.world=a.world.clone();b.dimension=Dimension::Nether;assert_eq!(waypoint_labels(&[a.clone(),b.clone()],AnyColors),vec!["Ё","Ё"]);
                b.dimension=a.dimension;b.icon=WaypointIcon::Skull;assert_eq!(waypoint_labels(&[a.clone(),b.clone()],AnyColors),vec!["Ё","Ё"]);
                b.icon=WaypointIcon::Letter;b.name="Ё".into();assert_eq!(waypoint_labels(&[a,b],AnyColors),vec!["ЁЛ","Ё"]);
                assert!(similar_waypoint_colors(0xff101020,0x00101020));
                assert!(similar_waypoint_colors(0,48));assert!(!similar_waypoint_colors(0,49));
            }
            #[test]
            fn waypoint_initials_setting_roundtrips_merges_and_resets() {
                use minimap::WaypointLetters;
                for mode in WaypointLetters::ALL {
                    let settings=Settings{waypoint_letters:mode,..Settings::default()};assert_eq!(Settings::decode(&settings.encode()),settings);
                }
                assert_eq!(Settings::decode("waypoint_letters=invalid").waypoint_letters,WaypointLetters::SimilarColors);
                let mut early=Settings::default();let mask=minimap::apply(&mut early,20);
                assert_eq!(early.waypoint_letters,WaypointLetters::AnyColors);
                let mut loaded=Settings::decode("size=256\ncache_ram_mib=128\nwaypoint_letters=one");minimap::merge(&mut loaded,early,mask);
                assert_eq!(loaded.waypoint_letters,WaypointLetters::AnyColors);assert_eq!(loaded.size_px,256);assert_eq!(loaded.cache_ram_mib,128);
                let reset=minimap::apply(&mut early,6);minimap::merge(&mut loaded,early,reset);assert_eq!(loaded,Settings::default());
            }

            static NEXT: AtomicU64 = AtomicU64::new(0);
            fn scratch() -> PathBuf {
                std::env::temp_dir().join(format!(
                    "minimap-test-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ))
            }

            #[test]
            fn settings_reject_invalid_sizes_zooms_and_boolean_values() {
                let settings = Settings::decode(
                    "size=65535\nzoom=NaN\nrotate=invalid\nshape=triangle\ncorner=unknown\nenabled=false",
                );
                assert_eq!(
                    settings,
                    Settings {
                        enabled: false,
                        ..Settings::default()
                    }
                );
                assert_eq!(Settings::decode(&settings.encode()), settings);
            }

            #[test]
            fn early_edits_merge_with_loaded_preferences_instead_of_resetting_them() {
                let mut loaded = Settings::decode("size=128\nshape=round\ncorner=bottom_right");
                let mut early = Settings::default();
                let mask = minimap::apply(&mut early, 0);
                minimap::merge(&mut loaded, early, mask);
                assert!(!loaded.enabled);
                assert!(loaded.round);
                assert_eq!(loaded.size_px, 128);
                assert_eq!(loaded.corner, minimap::Corner::BottomRight);
            }

            #[test]
            fn controller_flushes_queued_edits_and_reloads_them() {
                let directory = scratch();
                let controller = Controller::open(directory.clone());
                controller.click(3);
                controller.click(4);
                controller.click(1);
                controller.shutdown();
                let saved = read_settings(&directory.join(SETTINGS_FILE)).unwrap();
                assert!(saved.round);
                assert_eq!(saved.blocks_per_pixel, 2.0);
                assert_eq!(saved.corner, minimap::Corner::BottomLeft);
                assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
                let reopened = Controller::open(directory.clone());
                reopened.shutdown();
                assert_eq!(reopened.settings(), saved);
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn cache_ram_slider_roundtrips_and_merges_early_edits() {
                let value = Settings::decode("cache_ram_mib=128");
                assert_eq!(Settings::decode(&value.encode()).cache_ram_mib, 128);
                assert_eq!(Settings::decode("cache_ram_mib=0").cache_ram_mib, 64);
                let mut loaded = Settings::decode("cache_ram_mib=256");
                let edited = Settings {
                    cache_ram_mib: 32,
                    ..Settings::default()
                };
                minimap::merge(&mut loaded, edited, 1 << 18);
                assert_eq!(loaded.cache_ram_mib, 32);
            }

            fn point(world: &str, name: &str) -> Waypoint {
                Waypoint {
                    world: world.into(),
                    dimension: Dimension::Overworld,
                    x: 10,
                    y: 64,
                    z: -5,
                    name: name.into(),
                    color: DEFAULT_WAYPOINT_COLOR,
                    icon: WaypointIcon::Letter,
                    biome: None,
                    show_3d: true,
                }
            }

            #[test]
            fn world_and_server_waypoints_do_not_collide_and_survive_reconnect() {
                let directory = scratch();
                let a = waypoint_world(true, "World A").unwrap();
                let b = waypoint_world(true, "World B").unwrap();
                let server = waypoint_world(false, "World A").unwrap();
                let controller = Controller::open(directory.clone());
                for (world, name) in [(&a, "A"), (&b, "B"), (&server, "Server")] {
                    assert!(controller.add_waypoint(point(world, name)));
                }
                assert_eq!(controller.waypoints(&a), vec![point(&a, "A")]);
                assert_eq!(controller.waypoints(&b), vec![point(&b, "B")]);
                assert!(controller.update_waypoint(
                    &b,
                    Dimension::Overworld,
                    10,
                    -5,
                    "B renamed",
                    DEFAULT_WAYPOINT_COLOR
                ));
                assert!(controller.remove_waypoint(&server, Dimension::Overworld, 10, -5));
                assert_eq!(controller.waypoints(&a), vec![point(&a, "A")]);
                assert!(controller.waypoints("").is_empty());
                assert!(!controller.add_waypoint(point("", "unknown")));
                controller.shutdown();
                let reconnected = Controller::open(directory.clone());
                reconnected.shutdown();
                assert_eq!(reconnected.waypoints(&a), vec![point(&a, "A")]);
                assert_eq!(reconnected.waypoints(&b), vec![point(&b, "B renamed")]);
                assert!(reconnected.waypoints(&server).is_empty());
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn waypoint_limits_never_evict_other_worlds() {
                let mut points = vec![point("save:A", "A")];
                for x in 0..100 {
                    push_waypoint(
                        &mut points,
                        Waypoint {
                            x,
                            ..point("save:B", "B")
                        },
                    );
                }
                assert_eq!(points[0], point("save:A", "A"));
                assert_eq!(points.len(), MAX_WAYPOINTS + 1);
                for i in 0..MAX_TOTAL_WAYPOINTS {
                    push_waypoint(&mut points, point(&format!("save:{i}"), "other"));
                }
                assert_eq!(points.len(), MAX_TOTAL_WAYPOINTS);
                assert!(!push_waypoint(&mut points, point("save:overflow", "no")));
                assert_eq!(points[0], point("save:A", "A"));
            }

            #[test]
            fn scoped_identity_roundtrips_delimiters_and_rejects_unknown_kinds() {
                let world = waypoint_world(false, "[::1]:25565% Мир").unwrap();
                let point = point(&world, "Home: 100%");
                assert_eq!(Waypoint::decode(point.encode().trim()), Some(point));
                assert_ne!(waypoint_world(true, "same"), waypoint_world(false, "same"));
                assert!(waypoint_world(false, "").is_none());
                assert!(waypoint_world(false, "bad\naddress").is_none());
                assert!(Waypoint::decode("waypoint_v2=unknown:overworld:1:2:3:name").is_none());
            }

            #[test]
            fn legacy_waypoints_are_deleted_but_preferences_and_scoped_points_survive() {
                let directory = scratch();
                fs::create_dir_all(&directory).unwrap();
                let scoped = point("server:example.org:25565", "Scoped");
                fs::write(
                    directory.join(SETTINGS_FILE),
                    format!(
                        "size=192\nsmooth_worldmap=false\nwaypoint=overworld:10:64:-5:Old\n{}",
                        scoped.encode()
                    ),
                )
                .unwrap();
                let controller = Controller::open(directory.clone());
                // Migration must persist even without an edit or a world being joined.
                controller.shutdown();
                assert_eq!(controller.settings().size_px, 192);
                assert!(!controller.settings().encode().contains("smooth_worldmap"));
                assert_eq!(controller.waypoints(&scoped.world), vec![scoped.clone()]);
                assert!(controller.waypoints("save:A").is_empty());
                let file = fs::read_to_string(directory.join(SETTINGS_FILE)).unwrap();
                assert!(!file.lines().any(|l| l.starts_with("waypoint=")));
                assert!(file.contains("waypoint_v5="));
                let reopened = Controller::open(directory.clone());
                reopened.shutdown();
                assert_eq!(reopened.waypoints(&scoped.world), vec![scoped]);
                assert_eq!(
                    fs::read_to_string(directory.join(SETTINGS_FILE)).unwrap(),
                    file
                );
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn scoped_v2_records_gain_the_default_colour_and_migrate_to_v5() {
                let directory = scratch();
                fs::create_dir_all(&directory).unwrap();
                fs::write(
                    directory.join(SETTINGS_FILE),
                    "waypoint_v2=save%3Atest:overworld:1:2:3:Старый\n",
                )
                .unwrap();
                let controller = Controller::open(directory.clone());
                controller.shutdown();
                let file = fs::read_to_string(directory.join(SETTINGS_FILE)).unwrap();
                assert!(file.contains("waypoint_v5="));
                assert!(!file.contains("waypoint_v2="));
                let reopened = Controller::open(directory.clone());
                reopened.shutdown();
                let waypoint = reopened.waypoints("save:test")[0].clone();
                assert_eq!(waypoint.name, "Старый");
                assert_eq!(waypoint.color, DEFAULT_WAYPOINT_COLOR);
                assert_eq!(waypoint.icon, WaypointIcon::Letter);
                assert_eq!(waypoint.biome, None);
                assert!(waypoint.show_3d);
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn waypoints_persist_deduplicate_and_stay_bounded() {
                let directory = scratch();
                let controller = Controller::open(directory.clone());
                let point = Waypoint {
                    world: "save:test".into(),
                    dimension: Dimension::Nether,
                    x: 10,
                    y: 64,
                    z: -5,
                    name: "База".into(),
                    color: DEFAULT_WAYPOINT_COLOR,
                    icon: WaypointIcon::Letter,
                    biome: None,
                    show_3d: true,
                };
                assert!(controller.add_waypoint(point.clone()));
                assert!(!controller.add_waypoint(point.clone()));
                for x in 0..(MAX_WAYPOINTS as i32 + 1) {
                    assert!(controller.add_waypoint(Waypoint {
                        world: "save:test".into(),
                        dimension: Dimension::Overworld,
                        x,
                        y: 64,
                        z: 0,
                        name: format!("Точка {x}"),
                        color: WAYPOINT_COLORS[(x as usize) % WAYPOINT_COLORS.len()],
                        icon: WaypointIcon::Letter,
                        biome: None,
                        show_3d: true,
                    }));
                }
                controller.shutdown();
                let (_, saved) = read_profile(&directory.join(SETTINGS_FILE)).unwrap();
                assert_eq!(saved.len(), MAX_WAYPOINTS);
                assert!(!saved.contains(&point));
                assert_eq!(Waypoint::decode("waypoint=player:1:2:3"), None);
                let reopened = Controller::open(directory.clone());
                reopened.shutdown();
                assert_eq!(reopened.waypoints("save:test"), saved);
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn death_waypoint_settings_roundtrip_toggle_and_reset() {
                let mut settings = Settings::default();
                assert_eq!(minimap::apply(&mut settings, 11), 1 << 11);
                assert!(!settings.death_waypoints);
                assert_eq!(minimap::apply(&mut settings, 12), 1 << 12);
                assert!(!settings.death_waypoint_auto_remove);
                assert_eq!(Settings::decode(&settings.encode()), settings);
                let mask = minimap::apply(&mut settings, 6);
                assert_eq!(mask & (1 << 11), 1 << 11);
                assert_eq!(mask & (1 << 12), 1 << 12);
                assert!(settings.death_waypoints && settings.death_waypoint_auto_remove);
                assert!(
                    Settings::decode("death_waypoints=false\ndeath_waypoint_auto_remove=false")
                        .encode()
                        .contains("death_waypoints=false")
                );
            }

            #[test]
            fn world_waypoint_settings_roundtrip_toggle_and_reset() {
                let mut settings = Settings::default();
                assert!(settings.world_waypoints);
                assert_eq!(
                    settings.world_waypoint_style,
                    minimap::WorldMarkerStyle::LetterDistance
                );
                assert_eq!(minimap::apply(&mut settings, 13), 1 << 13);
                assert!(!settings.world_waypoints);
                assert_eq!(minimap::apply(&mut settings, 14), 1 << 14);
                assert_eq!(
                    settings.world_waypoint_style,
                    minimap::WorldMarkerStyle::Letter
                );
                assert_eq!(Settings::decode(&settings.encode()), settings);
                let mask = minimap::apply(&mut settings, 6);
                assert_eq!(mask & (1 << 13), 1 << 13);
                assert_eq!(mask & (1 << 14), 1 << 14);
                assert!(settings.world_waypoints);
                assert_eq!(
                    settings.world_waypoint_style,
                    minimap::WorldMarkerStyle::LetterDistance
                );
                assert!(
                    Settings::decode("world_waypoints=false\nworld_waypoint_style=name_distance")
                        .encode()
                        .contains("world_waypoints=false")
                );
            }

            #[test]
            fn direction_settings_roundtrip_toggle_and_reset() {
                let mut settings = Settings::default();
                assert!(!settings.directions);
                assert_eq!(settings.direction_style, minimap::DirectionStyle::Compass);
                assert_eq!(minimap::apply(&mut settings, 15), 1 << 15);
                assert!(settings.directions);
                assert_eq!(minimap::apply(&mut settings, 16), 1 << 16);
                assert_eq!(settings.direction_style, minimap::DirectionStyle::Axes);
                assert_eq!(Settings::decode(&settings.encode()), settings);
                let mask = minimap::apply(&mut settings, 6);
                assert_eq!(mask & (1 << 15), 1 << 15);
                assert_eq!(mask & (1 << 16), 1 << 16);
                assert!(!settings.directions);
                assert_eq!(settings.direction_style, minimap::DirectionStyle::Compass);
                assert!(
                    Settings::decode("directions=true\ndirection_style=axes")
                        .encode()
                        .contains("directions=true")
                );
            }

            #[test]
            fn labels_on_hover_setting_roundtrips_toggle_and_reset() {
                let mut settings = Settings::default();
                assert!(settings.labels_on_hover);
                assert_eq!(minimap::apply(&mut settings, 17), 1 << 17);
                assert!(!settings.labels_on_hover);
                assert_eq!(Settings::decode(&settings.encode()), settings);
                let mask = minimap::apply(&mut settings, 6);
                assert_eq!(mask & (1 << 17), 1 << 17);
                assert!(settings.labels_on_hover);
                assert!(
                    Settings::decode("labels_on_hover=false")
                        .encode()
                        .contains("labels_on_hover=false")
                );
            }

            #[test]
            fn waypoint_3d_flag_roundtrips_and_legacy_records_default_on() {
                let mut point = point("save:test", "Home");
                point.show_3d = false;
                let decoded = Waypoint::decode(point.encode().trim()).unwrap();
                assert!(!decoded.show_3d);
                assert_eq!(decoded, point);
                let legacy =
                    Waypoint::decode("waypoint_v4=save%3Atest:overworld:1:64:2:ff6b6b:letter::Old")
                        .unwrap();
                assert!(legacy.show_3d);
            }

            #[test]
            fn toggling_3d_visibility_persists() {
                let directory = scratch();
                let controller = Controller::open(directory.clone());
                assert!(controller.add_waypoint(point("save:test", "Home")));
                assert!(controller.set_waypoint_3d("save:test", Dimension::Overworld, 10, -5, false));
                assert!(!controller.set_waypoint_3d("save:test", Dimension::Overworld, 10, -5, false));
                assert!(!controller.waypoints("save:test")[0].show_3d);
                controller.shutdown();
                let reopened = Controller::open(directory.clone());
                reopened.shutdown();
                assert!(!reopened.waypoints("save:test")[0].show_3d);
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn death_waypoints_are_numbered_black_skulls_that_persist_their_biome() {
                let directory = scratch();
                let world = "save:death";
                let controller = Controller::open(directory.clone());
                assert!(controller.add_death_waypoint(
                    world,
                    Dimension::Overworld,
                    4,
                    64,
                    8,
                    Some("minecraft:dappled_forest")
                ));
                assert!(controller.add_death_waypoint(world, Dimension::Overworld, 12, 70, 16, None));
                assert!(!controller.add_death_waypoint(world, Dimension::Overworld, 4, 64, 8, None));
                let points = controller.waypoints(world);
                assert_eq!(points.len(), 2);
                assert_eq!(points[0].name, "Death");
                assert_eq!(points[1].name, "Death 2");
                assert!(points.iter().all(|point| {
                    point.icon == WaypointIcon::Skull && point.color == DEATH_WAYPOINT_COLOR
                }));
                assert_eq!(points[0].biome.as_deref(), Some("minecraft:dappled_forest"));
                assert!(controller.update_waypoint_biome(
                    world,
                    Dimension::Overworld,
                    12,
                    16,
                    "minecraft:plains"
                ));
                assert!(!controller.update_waypoint_biome(
                    world,
                    Dimension::Overworld,
                    12,
                    16,
                    "minecraft:plains"
                ));
                controller.shutdown();
                let reopened = Controller::open(directory.clone());
                reopened.shutdown();
                let saved = reopened.waypoints(world);
                assert_eq!(saved[0].biome.as_deref(), Some("minecraft:dappled_forest"));
                assert_eq!(saved[1].biome.as_deref(), Some("minecraft:plains"));
                assert_eq!(saved[0].icon, WaypointIcon::Skull);
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn reaching_death_waypoints_uses_three_dimensional_distance_and_skips_letters() {
                let mut death = point("save:death", "Death");
                death.icon = WaypointIcon::Skull;
                assert!(death_waypoint_reached([10.5, 64.0, -4.5], &death));
                assert!(!death_waypoint_reached([10.5, 64.0, 0.0], &death));
                assert!(!death_waypoint_reached([10.5, 72.0, -4.5], &death));
                assert!(!death_waypoint_reached(
                    [10.5, 64.0, -4.5],
                    &point("save:home", "Home")
                ));
            }

            #[test]
            fn v3_records_default_to_letters_without_a_biome() {
                let legacy =
                    Waypoint::decode("waypoint_v3=save%3Atest:overworld:1:2:3:ff6b6b:Old").unwrap();
                assert_eq!(legacy.icon, WaypointIcon::Letter);
                assert_eq!(legacy.biome, None);
                assert_eq!(legacy.color, 0xff6b6b);
                let v4 = Waypoint::decode(
                    "waypoint_v4=save%3Atest:overworld:1:64:2:000000:skull:minecraft%3Adappled_forest:Death",
                )
                .unwrap();
                assert_eq!(v4.icon, WaypointIcon::Skull);
                assert_eq!(v4.color, DEATH_WAYPOINT_COLOR);
                assert_eq!(v4.biome.as_deref(), Some("minecraft:dappled_forest"));
                assert_eq!(
                    Waypoint::decode(v4.encode().trim()).unwrap().biome,
                    v4.biome
                );
            }

            #[test]
            fn waypoint_names_roundtrip_and_waypoints_can_be_removed() {
                let directory = scratch();
                let waypoint = Waypoint {
                    world: "save:test".into(),
                    dimension: Dimension::Overworld,
                    x: 1,
                    y: 2,
                    z: 3,
                    name: "Дом: у % реки".into(),
                    color: 0x5aa9ff,
                    icon: WaypointIcon::Letter,
                    biome: Some("minecraft:plains".into()),
                    show_3d: true,
                };
                assert_eq!(
                    Waypoint::decode(waypoint.encode().trim()).unwrap(),
                    waypoint
                );
                // 0.5.0 wrote v2 lines without a colour; they keep the default marker.
                let legacy = Waypoint::decode("waypoint_v2=save%3Atest:overworld:1:2:3:Старое").unwrap();
                assert_eq!(legacy.name, "Старое");
                assert_eq!(legacy.color, DEFAULT_WAYPOINT_COLOR);
                let controller = Controller::open(directory.clone());
                assert!(controller.add_waypoint(waypoint));
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while controller.waypoints("save:test").is_empty() {
                    assert!(std::time::Instant::now() < until);
                    thread::sleep(std::time::Duration::from_millis(1));
                }
                assert!(controller.update_waypoint(
                    "save:test",
                    Dimension::Overworld,
                    1,
                    3,
                    "Новое имя",
                    0xff6b6b
                ));
                controller.shutdown();
                let reopened = Controller::open(directory.clone());
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while reopened.waypoints("save:test").is_empty() {
                    assert!(std::time::Instant::now() < until);
                    thread::sleep(std::time::Duration::from_millis(1));
                }
                let saved_waypoint = reopened.waypoints("save:test")[0].clone();
                assert_eq!(saved_waypoint.name, "Новое имя");
                assert_eq!(saved_waypoint.color, 0xff6b6b);
                assert!(reopened.remove_waypoint("save:test", Dimension::Overworld, 1, 3));
                reopened.shutdown();
                let (_, saved) = read_profile(&directory.join(SETTINGS_FILE)).unwrap();
                assert!(saved.is_empty());
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn bounded_notification_queue_persists_latest_edit_after_a_burst() {
                let directory = scratch();
                let controller = Controller::open(directory.clone());
                for _ in 0..1001 {
                    controller.click(0);
                }
                controller.click(usize::MAX);
                controller.shutdown();
                assert!(
                    !read_settings(&directory.join(SETTINGS_FILE))
                        .unwrap()
                        .enabled
                );
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn failed_save_is_visible_and_keeps_the_previous_file_intact() {
                let directory = scratch();
                fs::write(&directory, b"not a directory").unwrap();
                let controller = Controller::open(directory.clone());
                controller.click(0);
                controller.shutdown();
                assert!(controller.labels().last().unwrap().contains(tr(
                    "Failed",
                    "Ошибка",
                    "Échec",
                    "Error",
                    "Fehler"
                )));
                assert_eq!(fs::read(&directory).unwrap(), b"not a directory");
                fs::remove_file(directory).unwrap();
            }

            #[test]
            fn shutdown_retries_unsaved_preferences_after_a_transient_error() {
                let directory = scratch();
                fs::write(&directory, b"temporarily blocked").unwrap();
                let controller = Controller::open(directory.clone());
                controller.click(0);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    let failed = controller
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .error
                        .as_deref()
                        .is_some_and(|e| e.starts_with("Save:"));
                    if failed {
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "worker never reported the failed save"
                    );
                    thread::sleep(std::time::Duration::from_millis(1));
                }
                fs::remove_file(&directory).unwrap();
                fs::create_dir(&directory).unwrap();
                controller.shutdown();
                assert!(
                    !read_settings(&directory.join(SETTINGS_FILE))
                        .unwrap()
                        .enabled
                );
                assert!(controller.state.lock().unwrap().error.is_none());
                fs::remove_dir_all(directory).unwrap();
            }

            #[test]
            fn map_size_fits_gui_viewport_without_overwriting_preference() {
                let settings = Settings {
                    size_px: 128,
                    ..Settings::default()
                };
                assert_eq!(settings.fitted_size(320.0, 240.0), Some(128.0));
                assert_eq!(settings.fitted_size(100.0, 100.0), Some(36.0));
                assert_eq!(settings.fitted_size(100.0, 80.0), Some(16.0));
                assert_eq!(settings.fitted_size(1.0, 1.0), None);
                assert_eq!(settings.size_px, 128);
            }
        }
    };
}

#[cfg(test)]
pub(crate) use define_tests;
