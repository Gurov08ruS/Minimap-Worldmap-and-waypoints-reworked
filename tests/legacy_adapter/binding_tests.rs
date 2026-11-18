// SPDX-License-Identifier: GPL-3.0-only
// Expand at the original scope to retain private access and test names.
// Mixins requires self-contained payloads; external mod/include files are unsupported.

#[cfg(test)]
macro_rules! define_tests {
    () => {
        mod tests {
            use super::*;
            use crate::mod_list::{Installed, NativePage};

            fn install(s: &mut Settings, id: &str, title: &str, keys: &[(&str, &str, &str)]) -> usize {
                let i = s.mods.installed.len();
                s.mods.installed.push(Installed {
                    id: id.into(),
                    title: title.into(),
                    description: String::new(),
                    problem: None,
                    dev: false,
                    native: Some(NativePage {
                        keys: keys
                            .iter()
                            .map(|(name, label, key)| [(*name).into(), (*label).into(), (*key).into()])
                            .collect(),
                        buttons: vec!["Settings".into()],
                    }),
                });
                i
            }

            #[test]
            fn cache_slider_drag_uses_native_setting_events_and_release_stops_dragging() {
                let mut s = Settings::default();
                let index = install(&mut s, CACHE_MOD, "Map", &[]);
                s.mods.installed[index].native.as_mut().unwrap().buttons = vec!["Other".into(); 19];
                s.mods.installed[index].native.as_mut().unwrap().buttons[18] = "Map RAM: 64 MiB".into();
                let mut ui = OptionsUi::default();
                ui.open_at(true, Page::NativeMod(index));
                // Scroll this lower row into view, just as the options list does.
                ui.scroll = 10000.0;
                let (_, wd) = cache_widget(&mut ui, &s, (800.0, 600.0)).unwrap();
                assert!(
                    matches!(&wd.kind,WKind::Button {label,..} if label.plain().starts_with("\u{2063}\u{2063}"))
                );
                assert_eq!(
                    cache_click(
                        &mut ui,
                        &s,
                        wd.x + 4.0,
                        wd.y + 5.0,
                        (800.0, 600.0),
                        false,
                        false
                    ),
                    Some(vec![Event::NativeButton(index, 0x10000 | 16)])
                );
                assert_eq!(
                    cache_click(
                        &mut ui,
                        &s,
                        wd.x + wd.w + 100.0,
                        wd.y - 100.0,
                        (800.0, 600.0),
                        true,
                        false
                    ),
                    Some(vec![Event::NativeButton(index, 0x10000 | 512)])
                );
                cache_release();
                assert_eq!(
                    cache_click(&mut ui, &s, wd.x + wd.w, wd.y, (800.0, 600.0), true, false),
                    None
                );
            }

            fn choose(s: &mut Settings, id: &str, name: &str, key: &str) {
                s.mods
                    .keys
                    .entry(id.into())
                    .or_default()
                    .insert(name.into(), key.into());
            }

            fn translated(test: impl FnOnce()) {
                struct Strings;
                impl lang::ResourceSource for Strings {
                    fn read(&self, path: &str) -> Option<Vec<u8>> {
                        (path == "assets/minecraft/lang/en_us.json").then(|| {
                            br#"{"key.inventory":"Inventory","key.jump":"Jump","key.attack":"Attack","key.emote":"Emotes","controls.keybinds.duplicateKeybinds":"Konflikte: %s"}"#.to_vec()
                        })
                    }
                }
                lang::scoped(lang::Language::load(&[&Strings], "en_us"), test);
            }

            #[test]
            fn default_native_bindings_conflict_across_mods_but_not_with_themselves() {
                let mut s = Settings::default();
                let map = install(
                    &mut s,
                    "map",
                    "Map",
                    &[("open", "World map", "key.keyboard.m")],
                );
                assert!(native_conflicts(&s, map, 0).is_empty());
                let other = install(
                    &mut s,
                    "other",
                    "Other",
                    &[("open", "Menu", "key.keyboard.m")],
                );
                assert_eq!(native_conflicts(&s, map, 0), ["Other: Menu"]);
                assert_eq!(native_conflicts(&s, other, 0), ["Map: World map"]);
            }

            #[test]
            fn separate_actions_within_one_mod_can_conflict() {
                let mut s = Settings::default();
                let map = install(
                    &mut s,
                    "map",
                    "Map",
                    &[
                        ("open", "World map", "key.keyboard.m"),
                        ("mark", "Waypoint", "key.keyboard.m"),
                    ],
                );
                assert_eq!(native_conflicts(&s, map, 0), ["Map: Waypoint"]);
                assert_eq!(native_conflicts(&s, map, 1), ["Map: World map"]);
            }

            #[test]
            fn saved_native_and_builtin_choices_replace_defaults_immediately() {
                translated(|| {
                    let mut s = Settings::default();
                    let map = install(
                        &mut s,
                        "map",
                        "Map",
                        &[("open", "World map", "key.keyboard.m")],
                    );
                    install(
                        &mut s,
                        "other",
                        "Other",
                        &[("open", "Menu", "key.keyboard.e")],
                    );
                    choose(&mut s, "map", "open", "key.keyboard.e");
                    assert_eq!(native_conflicts(&s, map, 0), ["Inventory", "Other: Menu"]);
                    s.keys.set("key.inventory", "key.keyboard.m");
                    assert_eq!(native_conflicts(&s, map, 0), ["Other: Menu"]);
                    choose(&mut s, "other", "open", "key.keyboard.unknown");
                    assert!(native_conflicts(&s, map, 0).is_empty());
                });
            }

            #[test]
            fn disabled_and_unavailable_native_mods_never_warn_on_either_side() {
                let mut s = Settings::default();
                let map = install(
                    &mut s,
                    "map",
                    "Map",
                    &[("open", "World map", "key.keyboard.m")],
                );
                let other = install(
                    &mut s,
                    "other",
                    "Other",
                    &[("open", "Menu", "key.keyboard.m")],
                );
                s.mods.disabled.insert("map".into());
                assert!(native_conflicts(&s, map, 0).is_empty());
                assert!(native_conflicts(&s, other, 0).is_empty());
                s.mods.disabled.clear();
                s.mods.installed[other].problem = Some("Unavailable".into());
                assert!(native_conflicts(&s, map, 0).is_empty());
                assert!(native_conflicts(&s, other, 0).is_empty());
                choose(&mut s, "removed", "open", "key.keyboard.m");
                assert!(native_conflicts(&s, map, 0).is_empty());
            }

            #[test]
            fn unbound_and_missing_native_bindings_are_not_conflicts() {
                let mut s = Settings::default();
                let map = install(
                    &mut s,
                    "map",
                    "Map",
                    &[("open", "World map", "key.keyboard.unknown")],
                );
                install(
                    &mut s,
                    "other",
                    "Other",
                    &[("open", "Menu", "key.keyboard.unknown")],
                );
                assert!(native_conflicts(&s, map, 0).is_empty());
                assert!(native_conflicts(&s, map, 99).is_empty());
                assert!(native_conflicts(&s, 99, 0).is_empty());
                choose(&mut s, "map", "open", "");
                choose(&mut s, "other", "open", "");
                assert!(native_conflicts(&s, map, 0).is_empty());
            }

            #[test]
            fn mouse_bindings_and_horizons_own_mappings_are_checked() {
                translated(|| {
                    let mut s = Settings::default();
                    let map = install(
                        &mut s,
                        "map",
                        "Map",
                        &[("open", "World map", "key.mouse.left")],
                    );
                    assert_eq!(native_conflicts(&s, map, 0), ["Attack"]);
                    s.keys.set("key.emote", "key.keyboard.m");
                    choose(&mut s, "map", "open", "key.keyboard.m");
                    assert_eq!(native_conflicts(&s, map, 0), ["Emotes"]);
                });
            }

            fn change_button(rows: &[Row], row: usize) -> (&Text, &Act) {
                let Row::Buttons(caption, Some(change)) = &rows[row] else {
                    panic!("a native key row must retain both buttons");
                };
                assert!(matches!(
                    caption.kind,
                    WKind::Button {
                        active: false,
                        act: Act::None,
                        ..
                    }
                ));
                let WKind::Button {
                    label,
                    active: true,
                    act,
                } = &change.kind
                else {
                    panic!("the rebind button must remain active");
                };
                assert_eq!((caption.w, change.w), (150.0, 150.0));
                (label, act)
            }

            #[test]
            fn rows_preserve_rebinding_selection_actions_and_settings_buttons() {
                let mut s = Settings::default();
                let map = install(
                    &mut s,
                    "map",
                    "Map",
                    &[("open", "World map", "key.keyboard.m")],
                );
                install(
                    &mut s,
                    "other",
                    "Other",
                    &[("open", "Menu", "key.keyboard.m")],
                );
                let mut ui = OptionsUi::default();
                let rows = native_rows(&ui, &s, map);
                assert_eq!(rows.len(), 3);
                assert!(matches!(&rows[0], Row::Note(title) if title == "Map"));
                let (label, act) = change_button(&rows, 1);
                assert_eq!(label.plain(), "[ M ]");
                assert_eq!(*act, Act::NativeKey(map, 0));
                assert!(
                    matches!(&rows[2], Row::Buttons(Widget { w: 310.0, kind: WKind::Button { act: Act::NativeButton(i, 0), .. }, .. }, None) if *i == map)
                );
                ui.selected_native_key = Some((map, 0));
                assert_eq!(
                    change_button(&native_rows(&ui, &s, map), 1).0.plain(),
                    "> M <"
                );
                ui.selected_native_key = None;
                s.mods.disabled.insert("map".into());
                assert_eq!(change_button(&native_rows(&ui, &s, map), 1).0.plain(), "M");
                assert!(native_rows(&ui, &s, 99).is_empty());
            }

            #[test]
            fn tooltips_translate_and_merge_native_and_existing_builtin_conflicts() {
                translated(|| {
                    let mut s = Settings::default();
                    let map = install(
                        &mut s,
                        "map",
                        "Map",
                        &[("open", "World map", "key.keyboard.e")],
                    );
                    let native = button(0.0, 0.0, 150.0, Text::empty(), Act::NativeKey(map, 0));
                    assert_eq!(
                        tooltip(&s, &native).unwrap().plain(),
                        "Konflikte: Inventory"
                    );
                    let vanilla = button(0.0, 0.0, 75.0, Text::empty(), Act::Key("key.inventory"));
                    s.keys.set("key.jump", "key.keyboard.e");
                    assert_eq!(
                        tooltip(&s, &vanilla).unwrap().plain(),
                        "Konflikte: Jump, Map: World map"
                    );
                    s.mods.disabled.insert("map".into());
                    assert!(tooltip(&s, &native).is_none());
                    // Continue to Horizon's original tooltip, which still reports Jump.
                    assert!(tooltip(&s, &vanilla).is_none());
                    assert_eq!(s.keys.conflicts("key.inventory"), ["key.jump"]);
                    let settings = button(0.0, 0.0, 310.0, Text::empty(), Act::NativeButton(map, 0));
                    assert!(tooltip(&s, &settings).is_none());
                });
            }
        }
    };
}

#[cfg(test)]
pub(crate) use define_tests;
