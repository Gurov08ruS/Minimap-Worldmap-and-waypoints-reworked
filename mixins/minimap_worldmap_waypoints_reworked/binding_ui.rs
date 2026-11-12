// SPDX-License-Identifier: GPL-3.0-only
// See LICENSE and NOTICE for this project's terms.
// Native row rendering is adapted from Horizon (MIT), revision
// 2ebf00a0281c592f79191e76fce0e7d522c6f425, options_ui.rs::native_rows.
// Copyright (c) 2026 Ludovic Marie and the Horizon contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

// Inject as options_ui::minimap_binding_ui in horizon-client-render. This module
// deliberately depends only on that crate, not on the gameplay runtime.
use super::{Act, Event, OptionsUi, Page, Row, WKind, Widget, button, inactive};
use crate::{
    keybinds::{self, all_mappings},
    lang,
    settings::Settings,
    text::{self, Text},
    ui,
};

fn bound(key: &str) -> bool {
    !key.is_empty() && key != "key.keyboard.unknown"
}

fn native_enabled(s: &Settings, i: usize) -> bool {
    s.mods.installed.get(i).is_some_and(|m| {
        m.native.is_some() && m.problem.is_none() && !s.mods.disabled.contains(&m.id)
    })
}

fn effective_key<'a>(s: &'a Settings, id: &str, name: &str, default: &'a str) -> &'a str {
    s.mods
        .keys
        .get(id)
        .and_then(|keys| keys.get(name))
        .map_or(default, String::as_str)
}

fn native_matches(s: &Settings, key: &str, exclude: Option<(&str, &str)>) -> Vec<String> {
    let mut names = Vec::new();
    if !bound(key) {
        return names;
    }
    for (i, m) in s.mods.installed.iter().enumerate() {
        if !native_enabled(s, i) {
            continue;
        }
        let Some(page) = &m.native else {
            continue;
        };
        for [name, label, default] in &page.keys {
            if exclude == Some((m.id.as_str(), name.as_str())) {
                continue;
            }
            if effective_key(s, &m.id, name, default) == key {
                names.push(format!("{}: {}", m.title, label));
            }
        }
    }
    names
}

fn native_conflicts(s: &Settings, i: usize, k: usize) -> Vec<String> {
    if !native_enabled(s, i) {
        return Vec::new();
    }
    let m = &s.mods.installed[i];
    let Some([name, _, default]) = m.native.as_ref().and_then(|page| page.keys.get(k)) else {
        return Vec::new();
    };
    let key = effective_key(s, &m.id, name, default);
    if !bound(key) {
        return Vec::new();
    }
    // Independent mods can have conflicting defaults; the built-in check's
    // suppression of two unchanged defaults must not hide these collisions.
    let mut names: Vec<String> = all_mappings()
        .filter(|(name, _, _)| s.keys.get(name) == key)
        .map(|(name, _, _)| lang::get(name))
        .collect();
    names.extend(native_matches(s, key, Some((&m.id, name))));
    names
}

pub(super) fn native_rows(ui: &OptionsUi, s: &Settings, i: usize) -> Vec<Row> {
    let Some(m) = s.mods.installed.get(i) else {
        return Vec::new();
    };
    let mut rows = vec![Row::Note(m.title.clone())];
    let Some(page) = &m.native else {
        return rows;
    };
    for (k, [name, label, default]) in page.keys.iter().enumerate() {
        let key = effective_key(s, &m.id, name, default);
        let mut message = keybinds::display_name(key);
        if ui.selected_native_key == Some((i, k)) {
            message = Text::literal("> ")
                .append(message.with_color(text::WHITE))
                .append(" <")
                .with_color(text::YELLOW);
        } else if !native_conflicts(s, i, k).is_empty() {
            message = Text::literal("[ ")
                .append(message.with_color(text::WHITE))
                .append(" ]")
                .with_color(text::YELLOW);
        }
        let caption = button(0.0, 0.0, 150.0, Text::literal(label.clone()), Act::None);
        let change = button(0.0, 0.0, 150.0, message, Act::NativeKey(i, k));
        rows.push(Row::Buttons(inactive(caption), Some(change)));
    }
    for (b, label) in page.buttons.iter().enumerate() {
        let widget = button(
            0.0,
            0.0,
            310.0,
            Text::literal(if m.id == CACHE_MOD && b == CACHE_BUTTON {
                format!("\u{2063}\u{2063}{label}")
            } else {
                label.clone()
            }),
            Act::NativeButton(i, b),
        );
        rows.push(Row::Buttons(widget, None));
    }
    rows
}

// None leaves the original tooltip handler in charge, including its existing
// built-in duplicate rules and all non-binding option descriptions.
pub(super) fn tooltip(s: &Settings, wd: &Widget) -> Option<Text> {
    let WKind::Button { act, .. } = &wd.kind else {
        return None;
    };
    let names = match act {
        Act::NativeKey(i, k) => native_conflicts(s, *i, *k),
        Act::Key(name) => {
            let native = native_matches(s, s.keys.get(name), None);
            if native.is_empty() {
                return None;
            }
            let mut names: Vec<String> =
                s.keys.conflicts(name).into_iter().map(lang::get).collect();
            names.extend(native);
            names
        }
        _ => return None,
    };
    if names.is_empty() {
        return None;
    }
    Some(Text::translate(
        "controls.keybinds.duplicateKeybinds",
        &[Text::literal(names.join(", "))],
    ))
}

#[cfg(test)]
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
        s.mods.installed[index].native.as_mut().unwrap().buttons = vec!["Other".into(); 14];
        s.mods.installed[index].native.as_mut().unwrap().buttons[13] = "Map RAM: 64 MiB".into();
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

// Native mod buttons have no built-in slider ABI. Keep their existing action
// for keyboard activation and draw/handle the RAM control on that same row.
const CACHE_MOD: &str = "minimap_worldmap_waypoints_reworked";
const CACHE_BUTTON: usize = 13;
static CACHE_DRAGS: std::sync::Mutex<std::collections::BTreeMap<usize, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

fn cache_widget(ui: &mut OptionsUi, s: &Settings, size: (f32, f32)) -> Option<(usize, Widget)> {
    if ui.resource_error.is_some() || ui.selected_key.is_some() || ui.selected_native_key.is_some()
    {
        return None;
    }
    let Page::NativeMod(index) = ui.page() else {
        return None;
    };
    if s.mods.installed.get(index)?.id != CACHE_MOD {
        return None;
    }
    let (widgets, top, bottom, _) = ui.layout(s, size.0, size.1);
    let widget = widgets.into_iter().find(|w| {
        matches!(&w.kind,
        WKind::Button {act: Act::NativeButton(i, CACHE_BUTTON), ..} if *i == index)
    })?;
    if widget.y < top || widget.y + widget.h > bottom {
        return None;
    }
    Some((index, widget))
}
pub(crate) fn cache_draw(
    batch: &mut crate::hud::HudBatch,
    atlas: &crate::hud::HudAtlas,
    rect: [f32; 4],
    label: &Text,
    active: bool,
    hover: bool,
) -> bool {
    let plain = label.plain();
    let Some(caption) = plain.strip_prefix("\u{2063}\u{2063}") else {
        return false;
    };
    let Some(value) = caption
        .split_whitespace()
        .find_map(|w| w.parse::<u16>().ok())
        .filter(|v| (16..=512).contains(v))
    else {
        return false;
    };
    let value = (f32::from(value) - 16.0) / 496.0;
    let dragging = !CACHE_DRAGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty();
    ui::slider_ext(
        batch,
        atlas,
        rect[0],
        rect[1],
        rect[2],
        &Text::literal(caption),
        value,
        hover || dragging,
        false,
        false,
        active,
        dragging,
    );
    true
}
pub(super) fn cache_click(
    ui: &mut OptionsUi,
    s: &Settings,
    x: f32,
    y: f32,
    size: (f32, f32),
    drag: bool,
    right: bool,
) -> Option<Vec<Event>> {
    let owner = s as *const Settings as usize;
    let mut drags = CACHE_DRAGS.lock().unwrap_or_else(|e| e.into_inner());
    if !drag {
        drags.remove(&owner);
    }
    if right {
        return None;
    }
    let Some((index, wd)) = cache_widget(ui, s, size) else {
        drags.remove(&owner);
        return None;
    };
    if drag {
        if drags.get(&owner) != Some(&index) {
            return None;
        }
    } else {
        if !ui::hovered((x, y), wd.x, wd.y, wd.w, wd.h) {
            return None;
        }
        drags.insert(owner, index);
    }
    let value = 16 + (ui::slider_value(wd.x, wd.w, x).clamp(0.0, 1.0) * 31.0).round() as usize * 16;
    Some(vec![Event::NativeButton(index, 0x10000 | value)])
}
pub(super) fn cache_release() {
    CACHE_DRAGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}
