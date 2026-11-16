// SPDX-License-Identifier: GPL-3.0-only
// See LICENSE and NOTICE for this project's terms.
// Native row rendering is adapted from Horizon (MIT), revision
// 9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0, options_ui_pages.rs::native_rows.
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
super::minimap_binding_tests::define_tests!();

// Native mod buttons have no built-in slider ABI. Keep their existing action
// for keyboard activation and draw/handle the RAM control on that same row.
const CACHE_MOD: &str = "minimap_worldmap_waypoints_reworked";
const CACHE_BUTTON: usize = 18;
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
